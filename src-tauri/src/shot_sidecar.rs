//! 主进程里的截屏进程客户端。
//!
//! 截图进程是另一个 `daodian.exe --shot-service`。它用原来的网页选区，
//! 不另开原生选区窗口。主进程只负责拉起、下发配置和在它退出时关掉它。

use std::fs::File;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use shot_core::{
    read_frame, write_frame, Decoder, Message, ShotConfig, CODE_CLOSED, CODE_VERSION, PIPE_NAME,
    PROTOCOL_VERSION,
};
use tauri::{AppHandle, Manager};

use crate::settings::{ScreenshotSettings, SharedSettings};

static WORKER: OnceLock<Sender<CommandMsg>> = OnceLock::new();
static LIVE: AtomicU64 = AtomicU64::new(0);
static TOOLTIP: Mutex<String> = Mutex::new(String::new());

const BACKOFF: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
];

#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    pub hotkey: bool,
    pub status: String,
    pub generation: u64,
}

enum CommandMsg {
    Apply {
        app: AppHandle,
        config: ShotConfig,
        reply: Sender<Result<Outcome, String>>,
    },
    Stop {
        reply: Sender<()>,
        exit: bool,
    },
    Down {
        generation: u64,
    },
    Capture {
        reply: Sender<Result<(), String>>,
    },
}

struct BootError {
    message: String,
    fatal: bool,
}

struct Session {
    generation: u64,
    child: Child,
    io: Sender<IoCmd>,
    stopping: Arc<AtomicBool>,
    next_id: AtomicU64,
    thread: Option<std::thread::JoinHandle<()>>,
}

enum IoCmd {
    Call {
        message: Message,
        reply: Sender<Message>,
    },
    Stop,
}

impl Session {
    fn request(&self, message: Message, timeout: Duration) -> Result<Message, String> {
        let (tx, rx) = mpsc::channel();
        self.io
            .send(IoCmd::Call { message, reply: tx })
            .map_err(|_| "截图管道已经断开".to_string())?;
        rx.recv_timeout(timeout)
            .map_err(|_| "截图进程没有响应".to_string())
    }

    fn reload(&self, config: &ShotConfig) -> Result<Outcome, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let message = self.request(
            Message::ReloadConfig {
                id,
                config: config.clone(),
            },
            Duration::from_secs(2),
        )?;
        outcome_from(message, self.generation, config)
    }

    fn ping(&self) -> Result<(), String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        match self.request(Message::Ping { id }, Duration::from_secs(2))? {
            Message::Pong { .. } => Ok(()),
            other => Err(other
                .id()
                .map(|_| "截图进程没有回应心跳".to_string())
                .unwrap_or_else(|| "截图进程没有回应心跳".into())),
        }
    }

    fn shutdown(self) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.request(Message::Shutdown { id }, Duration::from_millis(800));
        self.kill();
    }

    fn kill(mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.io.send(IoCmd::Stop);
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) fn capture() -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    worker()
        .send(CommandMsg::Capture { reply: tx })
        .map_err(|_| "截图服务已经退出".to_string())?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "截图进程没有响应".to_string())?
}

pub(crate) fn apply(app: &AppHandle, settings: &ScreenshotSettings) -> Result<Outcome, String> {
    let (tx, rx) = mpsc::channel();
    worker()
        .send(CommandMsg::Apply {
            app: app.clone(),
            config: shot_config(settings),
            reply: tx,
        })
        .map_err(|_| "截图服务已经退出".to_string())?;
    rx.recv_timeout(Duration::from_secs(8))
        .map_err(|_| "截图进程没有响应".to_string())?
}

pub(crate) fn shutdown() {
    signal_stop(true);
}

pub(crate) fn note_ready(app: &AppHandle) {
    set_tooltip(app, "到点 · 截图已就绪");
}

pub(crate) fn note_unavailable(app: &AppHandle) {
    set_tooltip(app, "到点 · 截图服务不可用，可从托盘重试");
}

pub(crate) fn note_restarting(app: &AppHandle) {
    set_tooltip(app, "到点 · 正在重启截图服务");
}

pub(crate) fn note_idle(app: &AppHandle) {
    set_tooltip(app, "到点");
}

pub(crate) fn apply_tooltip(app: &AppHandle) {
    let text = TOOLTIP
        .lock()
        .map(|slot| {
            if slot.is_empty() {
                "到点".to_string()
            } else {
                slot.clone()
            }
        })
        .unwrap_or_else(|_| "到点".to_string());
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(text));
    }
}

fn set_tooltip(app: &AppHandle, text: &str) {
    if let Ok(mut slot) = TOOLTIP.lock() {
        *slot = text.to_string();
    }
    apply_tooltip(app);
}

fn signal_stop(exit: bool) {
    LIVE.store(0, Ordering::SeqCst);
    let Some(tx) = WORKER.get() else {
        return;
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    let _ = tx.send(CommandMsg::Stop {
        reply: reply_tx,
        exit,
    });
    let _ = reply_rx.recv_timeout(Duration::from_secs(2));
}

fn worker() -> Sender<CommandMsg> {
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            let loop_tx = tx.clone();
            std::thread::Builder::new()
                .name("shot-sidecar".into())
                .spawn(move || run(loop_tx, rx))
                .expect("截图服务线程启动失败");
            tx
        })
        .clone()
}

struct Supervisor {
    app: Option<AppHandle>,
    wanted: Option<ShotConfig>,
    session: Option<Session>,
    failures: u32,
    given_up: bool,
    pending: Option<Sender<Result<Outcome, String>>>,
}

fn run(tx: Sender<CommandMsg>, rx: Receiver<CommandMsg>) {
    let mut state = Supervisor {
        app: None,
        wanted: None,
        session: None,
        failures: 0,
        given_up: false,
        pending: None,
    };
    loop {
        if state.wanted.is_some() && state.session.is_none() && !state.given_up {
            let config = state.wanted.clone().expect("已检查");
            match boot(&tx, state.app.clone(), &config) {
                Ok((session, outcome)) => {
                    state.failures = 0;
                    state.session = Some(session);
                    deliver(&mut state, Ok(outcome));
                }
                Err(err) => {
                    state.failures += 1;
                    let terminal = err.fatal || state.failures >= 3;
                    let waiting = state.pending.is_some();
                    deliver(&mut state, Err(err.message));
                    if !waiting {
                        if let Some(app) = state.app.clone() {
                            post_unavailable(app, LIVE.load(Ordering::SeqCst), terminal);
                        }
                    }
                    if terminal {
                        state.given_up = true;
                    } else if let Some(cmd) =
                        interruptible(&rx, BACKOFF[(state.failures as usize - 1).min(2)])
                    {
                        if handle(&mut state, cmd) {
                            break;
                        }
                    }
                }
            }
            continue;
        }

        let idle = if state.session.is_some() {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(idle) {
            Ok(cmd) => {
                if handle(&mut state, cmd) {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let failed = state.session.as_ref().is_some_and(|session| session.ping().is_err());
                if failed {
                    let generation = state.session.as_ref().map(|session| session.generation);
                    if let Some(session) = state.session.take() {
                        session.kill();
                    }
                    state.failures += 1;
                    let terminal = state.failures >= 3;
                    if let Some(app) = state.app.clone() {
                        post_unavailable(app, generation.unwrap_or(0), terminal);
                    }
                    if terminal {
                        state.given_up = true;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if let Some(session) = state.session.take() {
        session.shutdown();
    }
}

fn handle(state: &mut Supervisor, cmd: CommandMsg) -> bool {
    match cmd {
        CommandMsg::Apply { app, config, reply } => {
            state.app = Some(app);
            state.wanted = Some(config.clone());
            state.given_up = false;
            state.failures = 0;
            if let Some(session) = state.session.as_ref() {
                match session.reload(&config) {
                    Ok(outcome) => {
                        let _ = reply.send(Ok(outcome));
                    }
                    Err(_) => {
                        if let Some(session) = state.session.take() {
                            session.kill();
                        }
                        if let Some(previous) = state.pending.replace(reply) {
                            let _ = previous.send(Err("截图配置已被更新的请求取代".into()));
                        }
                    }
                }
            } else if let Some(previous) = state.pending.replace(reply) {
                let _ = previous.send(Err("截图配置已被更新的请求取代".into()));
            }
            false
        }
        CommandMsg::Stop { reply, exit } => {
            state.wanted = None;
            state.given_up = false;
            state.failures = 0;
            LIVE.store(0, Ordering::SeqCst);
            if let Some(session) = state.session.take() {
                session.shutdown();
            }
            if let Some(pending) = state.pending.take() {
                let _ = pending.send(Err("截图进程已停止".into()));
            }
            let _ = reply.send(());
            exit
        }
        CommandMsg::Capture { reply } => {
            let result = match state.session.as_ref() {
                Some(session) => {
                    let id = session.next_id.fetch_add(1, Ordering::SeqCst) + 1;
                    match session.request(
                        Message::Capture { id, session: 0 },
                        Duration::from_secs(2),
                    ) {
                        Ok(Message::Ok { .. }) => Ok(()),
                        Ok(Message::Error { message, .. }) => Err(message),
                        Ok(_) => Err("截图进程没有开始".into()),
                        Err(err) => Err(err),
                    }
                }
                None => Err("截图进程还没就绪".into()),
            };
            let _ = reply.send(result);
            false
        }
        CommandMsg::Down { generation } => {
            let current = state.session.as_ref().map(|session| session.generation);
            if current == Some(generation) {
                if let Some(session) = state.session.take() {
                    session.kill();
                }
                state.failures += 1;
                let terminal = state.failures >= 3;
                if let Some(app) = state.app.clone() {
                    post_unavailable(app, generation, terminal);
                }
                if terminal {
                    state.given_up = true;
                }
            }
            false
        }
    }
}

fn deliver(state: &mut Supervisor, result: Result<Outcome, String>) {
    if let Some(reply) = state.pending.take() {
        let _ = reply.send(result);
        return;
    }
    let Ok(outcome) = result else {
        return;
    };
    let Some(app) = state.app.clone() else {
        return;
    };
    let shortcut = state
        .wanted
        .as_ref()
        .map(|config| config.shortcut.clone())
        .unwrap_or_default();
    post_ready(app, outcome, shortcut);
}

fn boot(
    tx: &Sender<CommandMsg>,
    app: Option<AppHandle>,
    config: &ShotConfig,
) -> Result<(Session, Outcome), BootError> {
    let Some(app) = app else {
        return Err(BootError {
            message: "截图服务还没有主进程句柄".into(),
            fatal: true,
        });
    };
    let exe = std::env::current_exe().map_err(|err| BootError {
        message: format!("找不到当前程序: {err}"),
        fatal: true,
    })?;
    crate::logging::info(format_args!("starting shot service {}", exe.display()));
    let mut child = Command::new(&exe)
        .arg("--shot-service")
        .arg("--parent")
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| BootError {
            message: format!("启动截图进程失败: {err}"),
            fatal: false,
        })?;
    let file = match connect_child(&mut child) {
        Ok(file) => file,
        Err(err) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }
    };
    let generation = LIVE.fetch_add(1, Ordering::SeqCst) + 1;
    LIVE.store(generation, Ordering::SeqCst);
    let stopping = Arc::new(AtomicBool::new(false));
    let (io_tx, io_rx) = mpsc::channel();
    let thread = spawn_io(
        file,
        io_rx,
        tx.clone(),
        app,
        generation,
        Arc::clone(&stopping),
    );
    let session = Session {
        generation,
        child,
        io: io_tx,
        stopping,
        next_id: AtomicU64::new(1),
        thread: Some(thread),
    };
    let hello = session.request(
        Message::Hello {
            id: 1,
            version: PROTOCOL_VERSION,
        },
        Duration::from_secs(2),
    );
    let hello = match hello {
        Ok(message) => message,
        Err(message) => {
            session.kill();
            return Err(BootError {
                message,
                fatal: false,
            });
        }
    };
    if let Err(err) = accept_hello(hello) {
        session.kill();
        return Err(err);
    }
    match session.reload(config) {
        Ok(outcome) => Ok((session, outcome)),
        Err(message) => {
            session.kill();
            Err(BootError {
                message,
                fatal: false,
            })
        }
    }
}

fn accept_hello(message: Message) -> Result<(), BootError> {
    match message {
        Message::HelloOk { version, .. } if version == PROTOCOL_VERSION => Ok(()),
        Message::Error { code, message, .. } if code == CODE_VERSION => Err(BootError {
            message,
            fatal: true,
        }),
        Message::Error { message, .. } => Err(BootError {
            message,
            fatal: false,
        }),
        _ => Err(BootError {
            message: "截图进程握手失败".into(),
            fatal: false,
        }),
    }
}

fn outcome_from(message: Message, generation: u64, config: &ShotConfig) -> Result<Outcome, String> {
    match message {
        Message::Ready { hotkey, .. } => Ok(Outcome {
            hotkey,
            status: if hotkey {
                format!("已启用：{}", config.shortcut)
            } else {
                "已关闭".into()
            },
            generation,
        }),
        Message::Error { code, message, .. } if code == CODE_CLOSED => Err(message),
        Message::Error { message, .. } => Ok(Outcome {
            hotkey: false,
            status: format!("注册失败：{message}"),
            generation,
        }),
        _ => Err("截图进程没有确认配置".into()),
    }
}

/// 管道只能由这一条线程按顺序读写。
/// 同步管道上同时 ReadFile 和 WriteFile 会卡死：读线程占着句柄，写线程发不出配置。
fn spawn_io(
    mut file: File,
    rx: Receiver<IoCmd>,
    tx: Sender<CommandMsg>,
    app: AppHandle,
    generation: u64,
    stopping: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("shot-sidecar-io".into())
        .spawn(move || {
            let mut decoder = Decoder::default();
            loop {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                if drain_ready(&mut file, &mut decoder, &app).is_err() {
                    break;
                }
                match rx.recv_timeout(Duration::from_millis(30)) {
                    Ok(IoCmd::Stop) => break,
                    Ok(IoCmd::Call { message, reply }) => {
                        let id = message.id().unwrap_or(0);
                        if write_frame(&mut file, &message).is_err() {
                            let _ = reply.send(closed_message());
                            break;
                        }
                        match read_reply(&mut file, &mut decoder, id, &app) {
                            Ok(message) => {
                                let exit = matches!(message, Message::Exit { .. });
                                let _ = reply.send(message);
                                if exit {
                                    break;
                                }
                            }
                            Err(()) => {
                                let _ = reply.send(closed_message());
                                break;
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            if !stopping.load(Ordering::SeqCst) {
                let _ = tx.send(CommandMsg::Down { generation });
            }
        })
        .expect("截图管道线程启动失败")
}

fn drain_ready(file: &mut File, decoder: &mut Decoder, app: &AppHandle) -> Result<(), ()> {
    loop {
        match pipe_available(file) {
            Some(0) => return Ok(()),
            Some(_) => {
                dispatch_or_skip(file, decoder, app, None)?;
            }
            None => return Err(()),
        }
    }
}

fn read_reply(
    file: &mut File,
    decoder: &mut Decoder,
    id: u64,
    app: &AppHandle,
) -> Result<Message, ()> {
    loop {
        if let Some(message) = dispatch_or_skip(file, decoder, app, Some(id))? {
            return Ok(message);
        }
    }
}

/// `want` 有值时，碰到对应 id 或 Exit 就把消息交回去。热键当场转给截图。
fn dispatch_or_skip(
    file: &mut File,
    decoder: &mut Decoder,
    app: &AppHandle,
    want: Option<u64>,
) -> Result<Option<Message>, ()> {
    match read_frame(file, decoder) {
        Ok(Message::Text { id, kind, text }) => {
            let result = crate::shot::land_text(app, &kind, &text);
            let reply = match result {
                Ok(message) => Message::TextOk { id, message },
                Err(message) => Message::Error {
                    id,
                    session: 0,
                    code: shot_core::CODE_CAPTURE.into(),
                    message,
                },
            };
            if write_frame(file, &reply).is_err() {
                return Err(());
            }
            Ok(None)
        }
        Ok(Message::Hotkey { session }) => {
            crate::logging::info(format_args!("shot sidecar hotkey session={session}"));
            Ok(None)
        }
        Ok(Message::Result {
            id,
            session,
            path,
            mode,
            ..
        }) => {
            store_result(file, app, id, session, &path, &mode);
            Ok(None)
        }
        Ok(Message::Error { id, message, .. }) if want != Some(id) => {
            note_shot_status(app, &message);
            Ok(None)
        }
        Ok(message) if message.id() == want || matches!(message, Message::Exit { .. }) => {
            Ok(Some(message))
        }
        Ok(_) => Ok(None),
        Err(_) => Err(()),
    }
}

fn store_result(
    file: &mut File,
    app: &AppHandle,
    id: u64,
    session: u64,
    path: &str,
    mode: &str,
) {
    match copy_result(app, path, mode) {
        Ok(dest) => {
            crate::logging::info(format_args!("screenshot stored {}", dest.display()));
            let text = format!("已保存：{}", dest.display());
            let parent = dest
                .parent()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default();
            let app = app.clone();
            let _ = app.clone().run_on_main_thread(move || {
                if let Ok(mut store) = app.state::<SharedSettings>().lock() {
                    let _ = store.set_screenshot_status(&text);
                    if !parent.is_empty() {
                        let _ = store.set_last_shot_dir(&parent);
                    }
                }
            });
            let _ = write_frame(file, &Message::Ack { id, session });
        }
        Err(err) => crate::logging::warn(format_args!("收下截图失败: {err}")),
    }
}

fn copy_result(app: &AppHandle, path: &str, mode: &str) -> Result<std::path::PathBuf, String> {
    let source = std::path::PathBuf::from(path)
        .canonicalize()
        .map_err(|err| format!("截图文件打不开: {err}"))?;
    let root = std::env::temp_dir()
        .join("daodian-shot")
        .canonicalize()
        .map_err(|err| format!("截图临时目录打不开: {err}"))?;
    if !source.starts_with(&root) {
        return Err("截图文件不在临时目录里".into());
    }
    let settings = app
        .state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .screenshot_settings();
    let dir = history_dir(&settings);
    std::fs::create_dir_all(&dir).map_err(|err| format!("保存目录不可写: {err}"))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = match mode {
        "gif" => format!("到点GIF-{stamp}.gif"),
        "long" => format!("到点长截图-{stamp}.png"),
        _ => format!("到点截图-{stamp}.png"),
    };
    let dest = dir.join(name);
    std::fs::copy(&source, &dest).map_err(|err| format!("保存截图失败: {err}"))?;
    Ok(dest)
}

fn history_dir(settings: &ScreenshotSettings) -> std::path::PathBuf {
    let last = settings.last_save_dir.trim();
    if !last.is_empty() {
        let path = std::path::PathBuf::from(last);
        if path.is_dir() {
            return path;
        }
    }
    let configured = settings.save_dir.trim();
    if !configured.is_empty() {
        return std::path::PathBuf::from(configured);
    }
    std::env::var_os("USERPROFILE")
        .map(|profile| std::path::PathBuf::from(profile).join("Pictures").join("到点截图"))
        .unwrap_or_else(|| std::env::temp_dir().join("daodian-shot-saved"))
}

fn note_shot_status(app: &AppHandle, message: &str) {
    crate::logging::warn(format_args!("shot sidecar: {message}"));
    let message = message.to_string();
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Ok(mut store) = app.state::<SharedSettings>().lock() {
            let _ = store.set_screenshot_status(&message);
        }
    });
}

fn closed_message() -> Message {
    Message::Error {
        id: 0,
        session: 0,
        code: CODE_CLOSED.into(),
        message: "截图进程断开了".into(),
    }
}

fn pipe_available(file: &File) -> Option<u32> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    let mut available = 0u32;
    let ok = unsafe {
        PeekNamedPipe(
            file.as_raw_handle() as _,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 { None } else { Some(available) }
}

fn post_ready(app: AppHandle, outcome: Outcome, shortcut: String) {
    let generation = outcome.generation;
    let hotkey = outcome.hotkey;
    let status = outcome.status;
    let _ = app.clone().run_on_main_thread(move || {
        if LIVE.load(Ordering::SeqCst) != generation {
            return;
        }
        let enabled = app
            .state::<SharedSettings>()
            .lock()
            .map(|store| store.screenshot_settings().enabled)
            .unwrap_or(false);
        if !enabled || !hotkey {
            return;
        }
        crate::shot::release_main_shortcut(&app);
        crate::shot::cache_dismiss(Some(shortcut.as_str()));
        if let Ok(mut store) = app.state::<SharedSettings>().lock() {
            let _ = store.set_screenshot_status(&status);
        }
        note_ready(&app);
    });
}

fn post_unavailable(app: AppHandle, generation: u64, terminal: bool) {
    let _ = app.clone().run_on_main_thread(move || {
        let live = LIVE.load(Ordering::SeqCst);
        if live != 0 && live != generation {
            return;
        }
        crate::shot::release_main_shortcut(&app);
        let status = if terminal {
            "截图服务不可用，可从托盘重试"
        } else {
            "正在重启截图服务"
        };
        if let Ok(mut store) = app.state::<SharedSettings>().lock() {
            let _ = store.set_screenshot_status(status);
        }
        if terminal {
            note_unavailable(&app);
            crate::logging::warn(format_args!("shot service unavailable after retries"));
        } else {
            note_restarting(&app);
            crate::logging::warn(format_args!("shot service restarting"));
        }
    });
}

fn interruptible(rx: &Receiver<CommandMsg>, duration: Duration) -> Option<CommandMsg> {
    match rx.recv_timeout(duration) {
        Ok(cmd) => Some(cmd),
        Err(RecvTimeoutError::Timeout) => None,
        Err(RecvTimeoutError::Disconnected) => Some(CommandMsg::Stop {
            reply: mpsc::channel().0,
            exit: true,
        }),
    }
}

fn shot_config(settings: &ScreenshotSettings) -> ShotConfig {
    ShotConfig {
        enabled: settings.enabled,
        shortcut: settings.shortcut.trim().to_string(),
        save_dir: settings.save_dir.clone(),
        capture_mode: settings.capture_mode.clone(),
        gif_fps: 10,
    }
}

fn connect_child(child: &mut Child) -> Result<File, BootError> {
    let mut last = "截图管道还没打开".to_string();
    for _ in 0..120 {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(BootError {
                message: format!("截图进程退出了：{status}"),
                fatal: false,
            });
        }
        match connect_pipe(PIPE_NAME) {
            Ok(file) => return Ok(file),
            Err(err) => last = err,
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(BootError {
        message: last,
        fatal: false,
    })
}

fn connect_pipe(name: &str) -> Result<File, String> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING,
    };

    let wide = wide(name);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0x8000_0000 | 0x4000_0000,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(unsafe { File::from_raw_handle(handle as _) })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
