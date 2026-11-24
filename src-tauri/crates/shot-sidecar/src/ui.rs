use std::fs::File;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use shot_core::{Hotkey, Message, write_frame};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG,
    PM_NOREMOVE, WM_HOTKEY,
};

use crate::log;

const WM_APP_RELOAD: u32 = 0x8000 + 1;
const WM_APP_QUIT: u32 = 0x8000 + 2;
pub(crate) const WM_APP_CAPTURE: u32 = 0x8000 + 3;

static PIPE: OnceLock<Arc<Mutex<Option<File>>>> = OnceLock::new();
const HOTKEY_ID: i32 = 1;

struct Reload {
    hotkey: Option<Hotkey>,
    reply: Sender<Result<(), String>>,
}

pub struct Ui {
    thread_id: u32,
    inbox: Arc<Mutex<Option<Reload>>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Ui {
    pub fn start(
        writer: Arc<Mutex<Option<File>>>,
        session: Arc<AtomicU64>,
        on_capture: Arc<dyn Fn() + Send + Sync>,
        on_dismiss: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let _ = PIPE.set(Arc::clone(&writer));
        let inbox = Arc::new(Mutex::new(None));
        let (ready_tx, ready_rx) = mpsc::channel();
        let inbox_thread = Arc::clone(&inbox);
        let join = std::thread::Builder::new()
            .name("shot-ui".into())
            .spawn(move || ui_loop(inbox_thread, writer, session, ready_tx, on_capture, on_dismiss))
            .map_err(|err| format!("截图界面线程启动失败: {err}"))?;
        let thread_id = ready_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "截图界面线程没有就绪".to_string())?;
        Ok(Self {
            thread_id,
            inbox,
            join: Some(join),
        })
    }

    pub fn thread_id(&self) -> u32 {
        self.thread_id
    }

    pub fn reload(&self, hotkey: Option<Hotkey>) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        {
            let mut inbox = self.inbox.lock().unwrap_or_else(|err| err.into_inner());
            *inbox = Some(Reload {
                hotkey,
                reply: reply_tx,
            });
        }
        let posted = unsafe { PostThreadMessageW(self.thread_id, WM_APP_RELOAD, 0, 0) };
        if posted == 0 {
            return Err(format!(
                "通知截图界面线程失败: {}",
                std::io::Error::last_os_error()
            ));
        }
        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "截图界面线程没有响应".to_string())?
    }

    pub fn quit(mut self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_APP_QUIT, 0, 0) };
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_APP_QUIT, 0, 0) };
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn ui_loop(
    inbox: Arc<Mutex<Option<Reload>>>,
    _writer: Arc<Mutex<Option<File>>>,
    session: Arc<AtomicU64>,
    ready: Sender<u32>,
    on_capture: Arc<dyn Fn() + Send + Sync>,
    on_dismiss: Arc<dyn Fn() + Send + Sync>,
) {
    unsafe {
        let mut wake: MSG = std::mem::zeroed();
        PeekMessageW(&mut wake, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
        let thread_id = windows_sys::Win32::System::Threading::GetCurrentThreadId();
        let _ = ready.send(thread_id);
        let mut message: MSG = std::mem::zeroed();
        loop {
            let pulled = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
            if pulled == 0 || pulled == -1 {
                break;
            }
            if message.message == WM_APP_CAPTURE && message.wParam == 1 {
                on_dismiss();
                continue;
            }
            if message.message == WM_HOTKEY || message.message == WM_APP_CAPTURE {
                start_session(&session, &on_capture);
                continue;
            }
            if message.message == WM_APP_RELOAD {
                let request = inbox.lock().unwrap_or_else(|err| err.into_inner()).take();
                if let Some(request) = request {
                    let result = apply_hotkey(request.hotkey);
                    let _ = request.reply.send(result);
                }
                continue;
            }
            if message.message == WM_APP_QUIT {
                let _ = UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID);
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

pub(crate) fn pipe() -> Option<Arc<Mutex<Option<File>>>> {
    PIPE.get().cloned()
}

fn start_session(session: &AtomicU64, on_capture: &Arc<dyn Fn() + Send + Sync>) {
    let id = session.fetch_add(1, Ordering::SeqCst) + 1;
    on_capture();
    if let Some(slot) = pipe() {
        let mut file = slot.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(file) = file.as_mut() {
            if let Err(err) = write_frame(file, &Message::Hotkey { session: id }) {
                log::warn(&format!("发送热键事件失败: {err}"));
            } else {
                log::info(&format!("hotkey session={id}"));
            }
        }
    }
}

fn apply_hotkey(hotkey: Option<Hotkey>) -> Result<(), String> {
    unsafe { UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID) };
    let Some(hotkey) = hotkey else {
        log::info("screenshot hotkey cleared");
        return Ok(());
    };
    let mut mods = MOD_NOREPEAT;
    if hotkey.alt {
        mods |= MOD_ALT;
    }
    if hotkey.control {
        mods |= MOD_CONTROL;
    }
    if hotkey.shift {
        mods |= MOD_SHIFT;
    }
    if hotkey.win {
        mods |= MOD_WIN;
    }
    let ok = unsafe { RegisterHotKey(std::ptr::null_mut(), HOTKEY_ID, mods, hotkey.vk.into()) };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        log::warn(&format!("RegisterHotKey failed: {err}"));
        return Err(if err.raw_os_error() == Some(1409) {
            "快捷键已被其他程序占用".into()
        } else {
            format!("注册快捷键失败: {err}")
        });
    }
    log::info(&format!(
        "screenshot hotkey registered vk={:#x} mods={mods:#x}",
        hotkey.vk
    ));
    Ok(())
}
