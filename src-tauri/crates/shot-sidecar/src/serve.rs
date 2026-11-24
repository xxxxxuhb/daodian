use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use shot_core::{
    parse_shortcut, read_frame, write_frame, Decoder, Message, ShotConfig, CODE_BAD_MESSAGE,
    CODE_HOTKEY, CODE_UNSUPPORTED, CODE_VERSION, PROTOCOL_VERSION,
};

use crate::host::{self, OwnedHandle};
use crate::log;
use crate::ui::Ui;

pub struct ServeOptions {
    pub parent: u32,
    pub pipe: String,
    pub mutex: String,
    pub on_capture: Arc<dyn Fn() + Send + Sync>,
    pub on_dismiss: Arc<dyn Fn() + Send + Sync>,
    pub on_config: Arc<dyn Fn(ShotConfig) + Send + Sync>,
}

struct Pipe {
    read: File,
    write: Arc<Mutex<Option<File>>>,
}

static OUT: OnceLock<Arc<Mutex<Option<File>>>> = OnceLock::new();
static TEXT_WAIT: Mutex<Option<(u64, SyncSender<Result<String, String>>)>> = Mutex::new(None);
static TEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn send_text(kind: &str, text: &str) -> Result<String, String> {
    let id = TEXT_ID.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::sync_channel(1);
    {
        let mut wait = TEXT_WAIT.lock().unwrap_or_else(|err| err.into_inner());
        if wait.is_some() {
            return Err("上一段文字还在交给主进程".into());
        }
        *wait = Some((id, tx));
    }
    let written = (|| -> Result<(), String> {
        let out = OUT.get().ok_or("截图管道还没准备好")?;
        let mut slot = out.lock().unwrap_or_else(|err| err.into_inner());
        let file = slot.as_mut().ok_or("截图管道还没连上")?;
        write_frame(
            file,
            &Message::Text {
                id,
                kind: kind.to_string(),
                text: text.to_string(),
            },
        )
        .map_err(|err| err.to_string())
    })();
    if let Err(err) = written {
        clear_text_wait(id);
        return Err(err);
    }
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result,
        Err(_) => {
            clear_text_wait(id);
            Err("主进程没有收下这段文字".into())
        }
    }
}

fn clear_text_wait(id: u64) {
    let mut wait = TEXT_WAIT.lock().unwrap_or_else(|err| err.into_inner());
    if matches!(wait.as_ref(), Some((pending, _)) if *pending == id) {
        *wait = None;
    }
}

fn complete_text(id: u64, result: Result<String, String>) -> bool {
    let mut wait = TEXT_WAIT.lock().unwrap_or_else(|err| err.into_inner());
    match wait.as_ref() {
        Some((pending, _)) if *pending == id => {
            let (_, tx) = wait.take().expect("waiter");
            let _ = tx.send(result);
            true
        }
        _ => false,
    }
}

pub fn serve(options: ServeOptions) -> Result<(), String> {
    let _instance = host::lock_single_instance(&options.mutex)?;
    let parent = host::open_parent(options.parent)?;
    let stop = host::manual_event()?;
    let write_slot = Arc::new(Mutex::new(None));
    let _ = OUT.set(Arc::clone(&write_slot));
    let session = Arc::new(AtomicU64::new(0));
    let ui = Ui::start(
        Arc::clone(&write_slot),
        Arc::clone(&session),
        Arc::clone(&options.on_capture),
        Arc::clone(&options.on_dismiss),
    )?;
    let ipc_thread = current_thread_id();
    let stopping = Arc::new(AtomicBool::new(false));
    let watch = spawn_parent_watch(
        parent.0 as isize,
        stop.0 as isize,
        ui.thread_id(),
        ipc_thread,
        Arc::clone(&stopping),
    );
    let pipe = match host::create_pipe(&options.pipe) {
        Ok(pipe) => pipe,
        Err(err) => {
            stopping.store(true, Ordering::SeqCst);
            finish(ui, stop, watch);
            return Err(err);
        }
    };
    log::info(&format!("pipe listening {}", options.pipe));
    let connected = host::connect_pipe(pipe);
    if connected.is_err() || stopping.load(Ordering::SeqCst) {
        let _ = connected;
        finish(ui, stop, watch);
        return Err("主进程还没连上，截图进程就退出了".into());
    }
    let write_handle = match host::duplicate(pipe) {
        Ok(handle) => handle,
        Err(err) => {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(pipe) };
            stopping.store(true, Ordering::SeqCst);
            finish(ui, stop, watch);
            return Err(err);
        }
    };
    let pipe = Pipe {
        read: host::file_from(pipe),
        write: Arc::clone(&write_slot),
    };
    {
        let mut slot = write_slot.lock().unwrap_or_else(|err| err.into_inner());
        *slot = Some(host::file_from(write_handle));
    }
    let result = command_loop(&pipe, &ui, &stopping, &options);
    stopping.store(true, Ordering::SeqCst);
    host::signal(stop.0);
    finish(ui, stop, watch);
    result
}

fn finish(ui: Ui, stop: OwnedHandle, watch: JoinHandle<()>) {
    host::signal(stop.0);
    ui.quit();
    let _ = watch.join();
}

fn spawn_parent_watch(
    parent: isize,
    stop: isize,
    ui_thread: u32,
    ipc_thread: u32,
    stopping: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("shot-parent".into())
        .spawn(move || {
            let parent = parent as windows_sys::Win32::Foundation::HANDLE;
            let stop = stop as windows_sys::Win32::Foundation::HANDLE;
            if host::wait_parent_or_stop(parent, stop) {
                log::warn("parent exited; shutting down sidecar");
                stopping.store(true, Ordering::SeqCst);
                host::cancel_thread_io(ipc_thread);
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                        ui_thread,
                        0x8000 + 2,
                        0,
                        0,
                    );
                }
            }
        })
        .expect("父进程监视线程启动失败")
}

fn current_thread_id() -> u32 {
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

fn command_loop(
    pipe: &Pipe,
    ui: &Ui,
    stopping: &AtomicBool,
    options: &ServeOptions,
) -> Result<(), String> {
    let mut decoder = Decoder::default();
    let mut reader = &pipe.read;
    let hello = read_frame(&mut reader, &mut decoder).map_err(|err| err.to_string())?;
    let Message::Hello { id, version } = hello else {
        reply(
            &pipe.write,
            &Message::Error {
                id: hello.id().unwrap_or(0),
                session: 0,
                code: CODE_BAD_MESSAGE.into(),
                message: "第一条消息必须是 hello".into(),
            },
        )?;
        return Err("协议握手失败".into());
    };
    if version != PROTOCOL_VERSION {
        reply(
            &pipe.write,
            &Message::Error {
                id,
                session: 0,
                code: CODE_VERSION.into(),
                message: format!("截图协议版本不一致：主进程 {version}，截图进程 {PROTOCOL_VERSION}"),
            },
        )?;
        return Err("协议版本不一致".into());
    }
    reply(
        &pipe.write,
        &Message::HelloOk {
            id,
            version: PROTOCOL_VERSION,
        },
    )?;
    log::info("hello ok");

    while !stopping.load(Ordering::SeqCst) {
        let message = match read_frame(&mut reader, &mut decoder) {
            Ok(message) => message,
            Err(err) => {
                if stopping.load(Ordering::SeqCst) {
                    return Ok(());
                }
                return Err(err.to_string());
            }
        };
        match handle(pipe, ui, message, options) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(err) => {
                log::warn(&format!("command failed: {err}"));
                return Err(err);
            }
        }
    }
    Ok(())
}

/// `Ok(true)` 表示这条命令之后进程该退出，而且这是正常退出。
fn handle(
    pipe: &Pipe,
    ui: &Ui,
    message: Message,
    options: &ServeOptions,
) -> Result<bool, String> {
    match message {
        Message::ReloadConfig { id, config } => {
            reload(pipe, ui, id, config, &options.on_config).map(|_| false)
        }
        Message::Ping { id } => reply(&pipe.write, &Message::Pong { id }).map(|_| false),
        Message::Shutdown { id: _ } => {
            reply(&pipe.write, &Message::Exit { reason: "shutdown".into() })?;
            Ok(true)
        }
        Message::Ack { id, session } => {
            log::info(&format!("ack session={session}"));
            reply(&pipe.write, &Message::Ok { id }).map(|_| false)
        }
        Message::TextOk { id, message } => {
            complete_text(id, Ok(message));
            Ok(false)
        }
        Message::Error { id, message, .. } if complete_text(id, Err(message.clone())) => Ok(false),
        Message::Capture { id, .. } => {
            (options.on_capture)();
            reply(&pipe.write, &Message::Ok { id }).map(|_| false)
        }
        Message::Cancel { id, .. } => {
            (options.on_dismiss)();
            reply(&pipe.write, &Message::Ok { id }).map(|_| false)
        }
        Message::Longshot { id, session } | Message::Gif { id, session } => reply(
            &pipe.write,
            &Message::Error {
                id,
                session,
                code: CODE_UNSUPPORTED.into(),
                message: "长截图和 GIF 从截图选区里开始，还在接到这个进程".into(),
            },
        )
        .map(|_| false),
        Message::Pin { id, session, .. } => reply(
            &pipe.write,
            &Message::Error {
                id,
                session,
                code: CODE_UNSUPPORTED.into(),
                message: "截图画面还在主进程里，这一阶段只接管热键".into(),
            },
        )
        .map(|_| false),
        other => reply(
            &pipe.write,
            &Message::Error {
                id: other.id().unwrap_or(0),
                session: 0,
                code: CODE_BAD_MESSAGE.into(),
                message: "截图进程不接受这条消息".into(),
            },
        )
        .map(|_| false),
    }
}

fn reload(
    pipe: &Pipe,
    ui: &Ui,
    id: u64,
    config: ShotConfig,
    on_config: &Arc<dyn Fn(ShotConfig) + Send + Sync>,
) -> Result<(), String> {
    if config.gif_fps == 0 || config.gif_fps > 60 {
        return reply(
            &pipe.write,
            &Message::Error {
                id,
                session: 0,
                code: CODE_BAD_MESSAGE.into(),
                message: "GIF 帧率要在 1 到 60 之间".into(),
            },
        );
    }
    let hotkey = if config.enabled {
        match parse_shortcut(&config.shortcut) {
            Ok(hotkey) => Some(hotkey),
            Err(err) => {
                return reply(
                    &pipe.write,
                    &Message::Error {
                        id,
                        session: 0,
                        code: CODE_HOTKEY.into(),
                        message: err,
                    },
                );
            }
        }
    } else {
        None
    };
    if let Err(err) = ui.reload(hotkey) {
        return reply(
            &pipe.write,
            &Message::Error {
                id,
                session: 0,
                code: CODE_HOTKEY.into(),
                message: err,
            },
        );
    }
    log::info(&format!(
        "config applied enabled={} shortcut={} mode={}",
        config.enabled, config.shortcut, config.capture_mode
    ));
    on_config(config);
    reply(
        &pipe.write,
        &Message::Ready {
            id,
            version: PROTOCOL_VERSION,
            hotkey: hotkey.is_some(),
        },
    )
}

fn reply(writer: &Mutex<Option<File>>, message: &Message) -> Result<(), String> {
    let mut slot = writer.lock().unwrap_or_else(|err| err.into_inner());
    let Some(file) = slot.as_mut() else {
        return Err("管道还没连上".into());
    };
    write_frame(file, message).map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())
}
