use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicIsize, AtomicU8, AtomicU32, AtomicU64, Ordering,
};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use crate::settings::SharedSettings;

mod capture;
mod ocr;
pub(crate) mod gifshot;
#[cfg(windows)]
pub(crate) mod longshot;
mod place;
mod save;
mod stitch;

use capture::{capture_bgra, capture_rect};
use place::place_shot_physical;
pub use save::ShotSaveResult;
pub(crate) use save::{cleanup_leftovers, copy_bgra, png_bytes_from_bgra};
use save::{
    copy_image, decode_png, encode_png, flatten_alpha, remove_tree, reveal_in_folder,
    save_with_dialog, validate_export_bytes, write_png,
};
#[cfg(test)]
use save::{
    ensure_png, format_long_screenshot_name, format_screenshot_name, resolve_save_start,
    unique_png, write_bytes_atomic,
};

const MAX_SCREENS: usize = 8;
static IN_SHOT_SERVICE: AtomicBool = AtomicBool::new(false);
const PIN_LIMIT: usize = 12;
const MAX_PNG_BYTES: usize = 80 * 1024 * 1024;

static CANCELLED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
const PHASE_IDLE: u8 = 0;
const PHASE_CAPTURING: u8 = 1;
const PHASE_OPEN: u8 = 2;
static PHASE: AtomicU8 = AtomicU8::new(PHASE_IDLE);
static LAST_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);
static ESCAPE_TICK: AtomicU64 = AtomicU64::new(0);
static ESCAPE_ACK: AtomicU64 = AtomicU64::new(0);
static SAVING: AtomicBool = AtomicBool::new(false);
static LAST_SAVED: Mutex<String> = Mutex::new(String::new());
static TMP_SEQ: AtomicU32 = AtomicU32::new(0);
static ACTIVE_SCREEN: AtomicU32 = AtomicU32::new(0);
static OPENED_AT: AtomicU64 = AtomicU64::new(0);
static READY_GATE: Mutex<u64> = Mutex::new(0);
static READY_WAKE: Condvar = Condvar::new();
static SPAN: Mutex<ShotSpan> = Mutex::new(ShotSpan {
    generation: 0,
    service_ms: 0,
    capture_ms: 0,
    shown_ms: 0,
    frame_ms: 0,
    bytes_ms: 0,
    pack_ms: 0,
    decode_ms: 0,
    paint_ms: 0,
    extras_ms: 0,
    screens: 0,
    summarized: false,
});
static DISMISS_ARMED: AtomicBool = AtomicBool::new(false);
static HOLD_CLOSED: AtomicBool = AtomicBool::new(false);
static SHOT_HWNDS: [AtomicIsize; MAX_SCREENS] = [const { AtomicIsize::new(0) }; MAX_SCREENS];
static SHOT_SHOWN: [AtomicBool; MAX_SCREENS] = [const { AtomicBool::new(false) }; MAX_SCREENS];
const KIND_FREEZE: u8 = 1;
const KIND_LIVE: u8 = 2;
static SHOT_KIND: [AtomicU8; MAX_SCREENS] = [const { AtomicU8::new(0) }; MAX_SCREENS];
#[cfg(windows)]
static SHOT_LOCK_X: [AtomicI32; MAX_SCREENS] = [const { AtomicI32::new(0) }; MAX_SCREENS];
#[cfg(windows)]
static SHOT_LOCK_Y: [AtomicI32; MAX_SCREENS] = [const { AtomicI32::new(0) }; MAX_SCREENS];
#[cfg(windows)]
static SHOT_LOCK_W: [AtomicI32; MAX_SCREENS] = [const { AtomicI32::new(0) }; MAX_SCREENS];
#[cfg(windows)]
static SHOT_LOCK_H: [AtomicI32; MAX_SCREENS] = [const { AtomicI32::new(0) }; MAX_SCREENS];
#[cfg(windows)]
static SHOT_LOCK_HWND: [AtomicIsize; MAX_SCREENS] = [const { AtomicIsize::new(0) }; MAX_SCREENS];
static PREWARM_STOP: AtomicBool = AtomicBool::new(false);
static PREWARM_STARTED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
const SHOT_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-timer-throttling --disable-backgrounding-occluded-windows --disable-renderer-backgrounding";

#[derive(Clone, Copy)]
struct DismissBinding {
    mods: Modifiers,
    vk: u32,
}

static DISMISS: Mutex<Option<DismissBinding>> = Mutex::new(None);

pub(crate) struct ShotRuntime {
    inner: Mutex<RuntimeInner>,
}

struct RuntimeInner {
    session: Option<Session>,
    opening: bool,
    registered: Option<String>,
    pins: [Option<PinSlot>; PIN_LIMIT],
}

struct Session {
    dir: PathBuf,
    screens: Vec<ScreenFile>,
    frames: Vec<LiveFrame>,
}

#[derive(Clone, Serialize)]
struct ShotLive {
    index: u32,
    active: bool,
}

#[derive(Clone, serde::Serialize)]
struct ShotRefresh {
    index: u32,
    path: String,
    raw: bool,
    active: bool,
    crop: bool,
    /// 旧的原生底图标记。冻结图改走 shot_frame，这里保持 false。
    backdrop: bool,
    generation: u64,
}

#[derive(Clone)]
struct ScreenFile {
    index: u32,
    path: PathBuf,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    monitor_w: u32,
    monitor_h: u32,
}

struct CapturedScreen {
    screen: ScreenFile,
    frame: Option<LiveFrame>,
}

/// 展示用的一帧。像素是自上而下的 BGRA，alpha 已写成 255。
/// `pixels` 用 Arc，shot_frame 只增加引用计数，不在锁里复制整帧。
struct LiveFrame {
    index: u32,
    width: u32,
    height: u32,
    pixels: Arc<Vec<u8>>,
}

struct ShotSpan {
    generation: u64,
    service_ms: u64,
    capture_ms: u64,
    shown_ms: u64,
    frame_ms: u64,
    bytes_ms: u64,
    pack_ms: u64,
    decode_ms: u64,
    paint_ms: u64,
    extras_ms: u64,
    screens: u32,
    summarized: bool,
}

struct BgraImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

#[derive(Clone, Copy)]
struct CaptureRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

struct PinSlot {
    path: PathBuf,
    width: u32,
    height: u32,
}

impl Default for ShotRuntime {
    fn default() -> Self {
        Self {
            inner: Mutex::new(RuntimeInner {
                session: None,
                opening: false,
                registered: None,
                pins: std::array::from_fn(|_| None),
            }),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenPayload {
    index: u32,
    path: String,
    width: u32,
    height: u32,
    raw: bool,
    active: bool,
    live: bool,
    generation: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinPayload {
    id: usize,
    path: String,
    width: u32,
    height: u32,
}

pub(crate) fn matches_configured(app: &AppHandle, shortcut: &Shortcut) -> bool {
    let settings = app.state::<SharedSettings>();
    let Ok(store) = settings.lock() else {
        return false;
    };
    let shot = store.screenshot_settings();
    shot.enabled && same_shortcut(&shot.shortcut, shortcut)
}

pub(crate) fn same_shortcut(configured: &str, incoming: &Shortcut) -> bool {
    configured
        .trim()
        .parse::<Shortcut>()
        .is_ok_and(|parsed| parsed == *incoming)
}

pub(crate) fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

struct TrackedWindow {
    label: String,
    hwnd: isize,
}

static TRACKED: Mutex<Vec<TrackedWindow>> = Mutex::new(Vec::new());

#[cfg(windows)]
fn exclude_from_capture(window: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity;
    const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd.0 as _, WDA_EXCLUDEFROMCAPTURE);
    }
}

/// 热键先把已经预热的窗口亮出来。抓屏在后面走，窗口已排除在抓屏之外。
fn reveal_warm(app: &AppHandle, generation: u64) {
    if !matches!(capture_mode(app), CaptureMode::Freeze) {
        return;
    }
    let Ok(rects) = monitor_rects(app) else {
        return;
    };
    let focus = cursor_screen(&rects);
    let Some(rect) = rects.get(focus as usize).copied() else {
        return;
    };
    ACTIVE_SCREEN.store(focus, Ordering::SeqCst);
    let _ = app.emit("shot-arm", ());
    let shown = app.clone();
    let _ = app.run_on_main_thread(move || {
        if CANCELLED.load(Ordering::SeqCst) || !generation_current(generation) {
            return;
        }
        let Some(window) = shown.get_webview_window(&format!("shot-{focus}")) else {
            return;
        };
        let _ = window.set_ignore_cursor_events(false);
        #[cfg(windows)]
        exclude_from_capture(&window);
        remember_window(&window);
        present_shot(&window, focus, true);
        place_shot_physical(&window, rect.x, rect.y, rect.width, rect.height);
        if CANCELLED.load(Ordering::SeqCst) || !generation_current(generation) {
            conceal_shot_windows();
            return;
        }
        PHASE.store(PHASE_OPEN, Ordering::SeqCst);
        let _ = shown.emit("shot-active", focus);
        crate::logging::info(format_args!(
            "screenshot hotkey shown: {focus} in {}ms",
            elapsed_since_open()
        ));
    });
}

pub(crate) fn remember_window(window: &WebviewWindow) {
    #[cfg(windows)]
    {
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        let label = window.label().to_string();
        let raw = hwnd.0 as isize;
        if let Some(index) = shot_index(&label) {
            SHOT_HWNDS[index].store(raw, Ordering::SeqCst);
        }
        let mut guard = TRACKED.lock().unwrap_or_else(|err| err.into_inner());
        guard.retain(|item| item.label != label);
        guard.push(TrackedWindow { label, hwnd: raw });
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

pub(crate) fn forget_window(label: &str) {
    if let Some(index) = shot_index(label) {
        SHOT_HWNDS[index].store(0, Ordering::SeqCst);
    }
    let mut guard = TRACKED.lock().unwrap_or_else(|err| err.into_inner());
    guard.retain(|item| item.label != label);
}

fn shot_index(label: &str) -> Option<usize> {
    let index = label.strip_prefix("shot-")?.parse::<usize>().ok()?;
    (index < SHOT_HWNDS.len()).then_some(index)
}

/// Esc 从键盘钩子进来。钩子里只投递退出，不调用会等待主线程的接口。
#[cfg(windows)]
pub(crate) fn handle_escape(app: &AppHandle) -> bool {
    if SAVING.load(Ordering::SeqCst) {
        return false;
    }
    let foreground =
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as isize };
    if let Some(hwnd) = foreground_pin(foreground) {
        force_close(hwnd);
        return true;
    }
    if !is_active() {
        return false;
    }
    let tick = ESCAPE_TICK.fetch_add(1, Ordering::SeqCst) + 1;
    let waited = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(120));
        if ESCAPE_TICK.load(Ordering::SeqCst) == tick && ESCAPE_ACK.load(Ordering::SeqCst) != tick {
            crate::logging::info(format_args!("screenshot escape timed out; closing"));
            force_dismiss(&waited);
        }
    });
    // 通知页面做分层退出。这条调用不能和上面的超时放在同一线程：它可能一直等主线程。
    let notified = app.clone();
    let _ = app.run_on_main_thread(move || {
        let label = format!("shot-{}", ACTIVE_SCREEN.load(Ordering::SeqCst));
        let _ = notified.emit_to(&label, "shot-escape", tick);
    });
    true
}

#[cfg(windows)]
fn foreground_pin(foreground: isize) -> Option<isize> {
    let guard = TRACKED.try_lock().ok()?;
    guard.iter().find_map(|item| {
        (item.label.starts_with("pin-") && item.hwnd == foreground).then_some(item.hwnd)
    })
}

fn conceal_shot_windows() {
    #[cfg(windows)]
    {
        longshot::shutdown();
        gifshot::shutdown();
        for slot in &SHOT_HWNDS {
            force_hide(slot.load(Ordering::SeqCst));
        }
    }
}

fn reveal_shot_windows(focus: u32) {
    #[cfg(windows)]
    {
        for (index, slot) in SHOT_HWNDS.iter().enumerate() {
            let raw = slot.load(Ordering::SeqCst);
            if index as u32 == focus {
                force_show(raw);
            } else {
                force_show_inactive(raw);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = focus;
    }
}

#[cfg(windows)]
fn force_hide(raw: isize) {
    if raw == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindowAsync};
        let hwnd = raw as windows_sys::Win32::Foundation::HWND;
        let _ = ShowWindowAsync(hwnd, SW_HIDE);
    }
}

#[cfg(windows)]
fn force_show(raw: isize) {
    show_async(raw, true);
}

#[cfg(windows)]
fn force_show_inactive(raw: isize) {
    show_async(raw, false);
}

#[cfg(windows)]
fn show_async(raw: isize, activate: bool) {
    if raw == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SW_SHOW, SW_SHOWNOACTIVATE, ShowWindowAsync,
        };
        let hwnd = raw as windows_sys::Win32::Foundation::HWND;
        let _ = ShowWindowAsync(hwnd, if activate { SW_SHOW } else { SW_SHOWNOACTIVATE });
    }
}

#[cfg(windows)]
fn activate_window(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{BringWindowToTop, SetForegroundWindow};
        let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(windows)]
fn force_close(raw: isize) {
    if raw == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            PostMessageW, SW_HIDE, ShowWindowAsync, WM_CLOSE,
        };
        let hwnd = raw as windows_sys::Win32::Foundation::HWND;
        let _ = ShowWindowAsync(hwnd, SW_HIDE);
        let _ = PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

fn session_open(app: &AppHandle) -> bool {
    app.state::<ShotRuntime>()
        .inner
        .lock()
        .map(|state| state.session.is_some())
        .unwrap_or(false)
}

enum ShotCommand {
    Open(u64),
    Close,
    Suspend(Sender<()>),
}

static WORKER: OnceLock<Sender<ShotCommand>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn elapsed_since_open() -> u64 {
    unix_ms().saturating_sub(OPENED_AT.load(Ordering::SeqCst))
}

fn shot_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match std::env::var("DAODIAN_SHOT_TRACE") {
        Ok(value) => {
            let value = value.trim();
            value != "0"
                && !value.eq_ignore_ascii_case("false")
                && !value.eq_ignore_ascii_case("off")
        }
        Err(_) => true,
    })
}

fn shot_trace(message: std::fmt::Arguments<'_>) {
    if shot_trace_enabled() {
        crate::logging::info(message);
    }
}

fn span_lock() -> std::sync::MutexGuard<'static, ShotSpan> {
    SPAN.lock().unwrap_or_else(|err| err.into_inner())
}

fn span_begin(generation: u64) {
    let mut span = span_lock();
    *span = ShotSpan {
        generation,
        service_ms: 0,
        capture_ms: 0,
        shown_ms: 0,
        frame_ms: 0,
        bytes_ms: 0,
        pack_ms: 0,
        decode_ms: 0,
        paint_ms: 0,
        extras_ms: 0,
        screens: 0,
        summarized: false,
    };
}

fn span_update(generation: u64, update: impl FnOnce(&mut ShotSpan)) {
    let mut span = span_lock();
    if span.generation == generation {
        update(&mut span);
    }
}

fn ready_lock() -> std::sync::MutexGuard<'static, u64> {
    READY_GATE.lock().unwrap_or_else(|err| err.into_inner())
}

fn note_active_ready(generation: u64) {
    let mut guard = ready_lock();
    *guard = generation;
    READY_WAKE.notify_all();
}

fn wait_active_frame(generation: u64) {
    let guard = ready_lock();
    let (guard, result) = READY_WAKE
        .wait_timeout_while(guard, Duration::from_millis(1200), |ready| {
            *ready != generation
                && generation_current(generation)
                && !CANCELLED.load(Ordering::SeqCst)
        })
        .unwrap_or_else(|err| err.into_inner());
    let matched = *guard == generation;
    drop(guard);
    if !matched && result.timed_out() {
        crate::logging::info(format_args!(
            "screenshot extra released: timeout at {}ms",
            elapsed_since_open()
        ));
    }
}

struct ActionEnd(&'static str, Instant);

impl Drop for ActionEnd {
    fn drop(&mut self) {
        crate::logging::info(format_args!(
            "screenshot {}-end: {}ms",
            self.0,
            self.1.elapsed().as_millis()
        ));
    }
}

fn log_shot_summary(paint_ms: u64) {
    let mut span = span_lock();
    if span.summarized || span.generation == 0 {
        return;
    }
    span.paint_ms = paint_ms;
    span.summarized = true;
    crate::logging::info(format_args!(
        "screenshot summary: gen={} service={}ms capture={}ms shown={}ms frame={}ms ipc={}ms pack={}ms decode={}ms paint={}ms extras={}ms screens={}",
        span.generation,
        span.service_ms,
        span.capture_ms,
        span.shown_ms,
        span.frame_ms,
        span.bytes_ms,
        span.pack_ms,
        span.decode_ms,
        span.paint_ms,
        span.extras_ms,
        span.screens,
    ));
}

fn ensure_worker(app: &AppHandle) -> Sender<ShotCommand> {
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            let app = app.clone();
            std::thread::Builder::new()
                .name("shot".into())
                .spawn(move || {
                    while let Ok(command) = rx.recv() {
                        match command {
                            ShotCommand::Open(generation) => do_open(&app, generation),
                            ShotCommand::Close => do_close(&app),
                            ShotCommand::Suspend(done) => {
                                hide_overlay(&app, false);
                                let _ = done.send(());
                            }
                        }
                    }
                })
                .expect("截图线程启动失败");
            tx
        })
        .clone()
}

fn send_command(app: &AppHandle, command: ShotCommand) {
    let _ = ensure_worker(app).send(command);
}

fn conceal_shots(app: &AppHandle) {
    ACTIVE.store(false, Ordering::SeqCst);
    let (done_tx, done_rx) = mpsc::channel();
    send_command(app, ShotCommand::Suspend(done_tx));
    let _ = done_rx.recv_timeout(Duration::from_secs(5));
}

fn reveal_shots(app: &AppHandle) {
    if !session_open(app) {
        return;
    }
    CANCELLED.store(false, Ordering::SeqCst);
    PHASE.store(PHASE_OPEN, Ordering::SeqCst);
    ACTIVE.store(true, Ordering::SeqCst);
    reveal_shot_windows(ACTIVE_SCREEN.load(Ordering::SeqCst));
}

fn release_overlay(app: &AppHandle) {
    send_command(app, ShotCommand::Close);
}

pub(crate) fn request_cancel(app: &AppHandle) {
    conceal_shot_windows();
    crate::logging::info(format_args!("screenshot cancel requested"));
    enqueue_close(app);
}

/// 从键盘钩子调用。只投稿和异步隐藏，不等主线程，也不写日志。
pub(crate) fn force_dismiss(app: &AppHandle) {
    if SAVING.load(Ordering::SeqCst) {
        return;
    }
    conceal_shot_windows();
    enqueue_close(app);
}

fn enqueue_close(app: &AppHandle) {
    CANCELLED.store(true, Ordering::SeqCst);
    ACTIVE.store(false, Ordering::SeqCst);
    GENERATION.fetch_add(1, Ordering::SeqCst);
    send_command(app, ShotCommand::Close);
}

pub(crate) fn trigger(app: &AppHandle) {
    // 另存对话框还开着时，热键、托盘和命令入口都不再起新会话。
    if SAVING.load(Ordering::SeqCst) {
        return;
    }
    // 预热和截图不能共用一条队列，否则第一次热键要等窗口创建完。
    PREWARM_STOP.store(true, Ordering::SeqCst);
    ensure_worker(app);
    if HOLD_CLOSED.load(Ordering::SeqCst) {
        return;
    }
    let now = unix_ms();
    let previous = LAST_TOGGLE_MS.swap(now, Ordering::Relaxed);
    let phase = PHASE.load(Ordering::SeqCst);
    if phase == PHASE_OPEN {
        // 窗口已经在屏幕上。先藏起来，抓屏线程收尾时才不会再把它打开。
        conceal_shot_windows();
        enqueue_close(app);
        return;
    }
    // 只吞掉按住不放时的连发。已经打开时，再按一次立刻退出。
    if phase == PHASE_CAPTURING || now.saturating_sub(previous) < 120 {
        return;
    }
    if PHASE
        .compare_exchange(
            PHASE_IDLE,
            PHASE_CAPTURING,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }
    CANCELLED.store(false, Ordering::SeqCst);
    ACTIVE.store(true, Ordering::SeqCst);
    DISMISS_ARMED.store(false, Ordering::SeqCst);
    OPENED_AT.store(now, Ordering::SeqCst);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    shot_trace(format_args!("screenshot hotkey gen={generation}"));
    send_command(app, ShotCommand::Open(generation));
    reveal_warm(app, generation);
}

pub(crate) fn dismiss(app: &AppHandle) {
    request_cancel(app);
}

pub(crate) fn release_pin(app: &AppHandle, id: usize) {
    // 同一个编号又开了一张新贴图时，旧窗口的关闭不能把新文件删掉。
    if app.get_webview_window(&format!("pin-{id}")).is_some() {
        return;
    }
    let Some(slot) = app
        .state::<ShotRuntime>()
        .inner
        .lock()
        .ok()
        .and_then(|mut state| state.pins.get_mut(id).and_then(|slot| slot.take()))
    else {
        return;
    };
    if let Err(err) = fs::remove_file(&slot.path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            crate::logging::warn(format_args!("删除贴图失败: {err}"));
        }
    }
}

pub(crate) fn persist_registration_status(app: &AppHandle) -> Result<String, String> {
    let status = sync_registration(app);
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .set_screenshot_status(&status)?;
    Ok(status)
}

fn sync_registration(app: &AppHandle) -> String {
    let configured = match app.state::<SharedSettings>().lock() {
        Ok(store) => store.screenshot_settings(),
        Err(err) => return format!("截图状态读取失败：{err}"),
    };
    #[cfg(windows)]
    {
        return sync_sidecar(app, &configured);
    }
    #[cfg(not(windows))]
    {
        register_on_main(app, &configured)
    }
}

#[cfg(windows)]
fn sync_sidecar(app: &AppHandle, configured: &crate::settings::ScreenshotSettings) -> String {
    let shortcut = configured.shortcut.trim().to_string();
    if !configured.enabled {
        let _ = crate::shot_sidecar::apply(app, configured);
        release_main_shortcut(app);
        cache_dismiss(None);
        crate::shot_sidecar::note_idle(app);
        return "已关闭".into();
    }
    if shortcut.is_empty() || shortcut.parse::<Shortcut>().is_err() || conflicts(app, &shortcut) {
        let mut disabled = configured.clone();
        disabled.enabled = false;
        let _ = crate::shot_sidecar::apply(app, &disabled);
        release_main_shortcut(app);
        cache_dismiss(None);
        crate::shot_sidecar::note_idle(app);
        if shortcut.is_empty() {
            return "注册失败：快捷键不能为空".into();
        }
        if shortcut.parse::<Shortcut>().is_err() {
            return "注册失败：快捷键格式无效".into();
        }
        return "注册失败：和快速记录的快捷键相同".into();
    }
    match crate::shot_sidecar::apply(app, configured) {
        Ok(outcome) if outcome.hotkey => {
            release_main_shortcut(app);
            cache_dismiss(Some(shortcut.as_str()));
            crate::shot_sidecar::note_ready(app);
            outcome.status
        }
        Ok(outcome) => {
            release_main_shortcut(app);
            cache_dismiss(None);
            crate::shot_sidecar::note_unavailable(app);
            outcome.status
        }
        Err(err) => {
            release_main_shortcut(app);
            cache_dismiss(None);
            crate::shot_sidecar::note_unavailable(app);
            err
        }
    }
}

pub(crate) fn release_main_shortcut(app: &AppHandle) {
    let runtime = app.state::<ShotRuntime>();
    let Ok(mut state) = runtime.inner.lock() else {
        return;
    };
    let Some(previous) = state.registered.take() else {
        return;
    };
    if let Err(err) = app.global_shortcut().unregister(previous.as_str()) {
        crate::logging::warn(format_args!("注销主进程截图快捷键失败: {err}"));
    }
}

#[cfg(not(windows))]
fn register_on_main(app: &AppHandle, configured: &crate::settings::ScreenshotSettings) -> String {
    let shortcut = configured.shortcut.trim().to_string();
    let conflict = configured.enabled && conflicts(app, &shortcut);
    let runtime = app.state::<ShotRuntime>();
    let mut state = match runtime.inner.lock() {
        Ok(state) => state,
        Err(err) => return format!("截图状态锁定失败：{err}"),
    };
    let desired = configured.enabled.then_some(shortcut.clone());
    if state.registered == desired && !conflict {
        cache_dismiss(desired.as_deref());
        return if desired.is_some() {
            format!("已启用：{shortcut}")
        } else {
            "已关闭".into()
        };
    }
    if let Some(previous) = state.registered.clone() {
        if let Err(err) = app.global_shortcut().unregister(previous.as_str()) {
            return format!("注销快捷键失败：{err}");
        }
        state.registered = None;
    }
    if !configured.enabled {
        cache_dismiss(None);
        return "已关闭".into();
    }
    if shortcut.is_empty() {
        cache_dismiss(None);
        return "注册失败：快捷键不能为空".into();
    }
    if shortcut.parse::<Shortcut>().is_err() {
        cache_dismiss(None);
        return "注册失败：快捷键格式无效".into();
    }
    if conflict {
        cache_dismiss(None);
        return "注册失败：和快速记录的快捷键相同".into();
    }
    match app.global_shortcut().register(shortcut.as_str()) {
        Ok(()) => {
            state.registered = Some(shortcut.clone());
            cache_dismiss(Some(shortcut.as_str()));
            format!("已启用：{shortcut}")
        }
        Err(err) => {
            cache_dismiss(None);
            format!("注册失败：{err}")
        }
    }
}

pub(crate) fn cache_dismiss(shortcut: Option<&str>) {
    let binding = shortcut.and_then(|value| {
        let parsed = value.parse::<Shortcut>().ok()?;
        Some(DismissBinding {
            mods: parsed.mods,
            vk: code_vk(parsed.key)?,
        })
    });
    let mut slot = DISMISS.lock().unwrap_or_else(|err| err.into_inner());
    *slot = binding;
}

fn dismiss_binding() -> Option<DismissBinding> {
    *DISMISS.try_lock().ok()?
}

fn code_vk(key: Code) -> Option<u32> {
    Some(match key {
        Code::Digit0 => 0x30,
        Code::Digit1 => 0x31,
        Code::Digit2 => 0x32,
        Code::Digit3 => 0x33,
        Code::Digit4 => 0x34,
        Code::Digit5 => 0x35,
        Code::Digit6 => 0x36,
        Code::Digit7 => 0x37,
        Code::Digit8 => 0x38,
        Code::Digit9 => 0x39,
        Code::KeyA => 0x41,
        Code::KeyB => 0x42,
        Code::KeyC => 0x43,
        Code::KeyD => 0x44,
        Code::KeyE => 0x45,
        Code::KeyF => 0x46,
        Code::KeyG => 0x47,
        Code::KeyH => 0x48,
        Code::KeyI => 0x49,
        Code::KeyJ => 0x4A,
        Code::KeyK => 0x4B,
        Code::KeyL => 0x4C,
        Code::KeyM => 0x4D,
        Code::KeyN => 0x4E,
        Code::KeyO => 0x4F,
        Code::KeyP => 0x50,
        Code::KeyQ => 0x51,
        Code::KeyR => 0x52,
        Code::KeyS => 0x53,
        Code::KeyT => 0x54,
        Code::KeyU => 0x55,
        Code::KeyV => 0x56,
        Code::KeyW => 0x57,
        Code::KeyX => 0x58,
        Code::KeyY => 0x59,
        Code::KeyZ => 0x5A,
        Code::F1 => 0x70,
        Code::F2 => 0x71,
        Code::F3 => 0x72,
        Code::F4 => 0x73,
        Code::F5 => 0x74,
        Code::F6 => 0x75,
        Code::F7 => 0x76,
        Code::F8 => 0x77,
        Code::F9 => 0x78,
        Code::F10 => 0x79,
        Code::F11 => 0x7A,
        Code::F12 => 0x7B,
        Code::Space => 0x20,
        Code::Tab => 0x09,
        Code::Enter => 0x0D,
        _ => return None,
    })
}

#[cfg(windows)]
pub(crate) fn hook_shortcut(app: &AppHandle, vk: u32, key_up: bool) -> bool {
    if SAVING.load(Ordering::SeqCst) {
        return false;
    }
    let Some(binding) = dismiss_binding() else {
        return false;
    };
    if vk != binding.vk {
        return false;
    }
    if key_up {
        HOLD_CLOSED.store(false, Ordering::SeqCst);
        let phase = PHASE.load(Ordering::SeqCst);
        if phase == PHASE_OPEN || phase == PHASE_CAPTURING {
            DISMISS_ARMED.store(true, Ordering::SeqCst);
        }
        return false;
    }
    if HOLD_CLOSED.load(Ordering::SeqCst) {
        return true;
    }
    if PHASE.load(Ordering::SeqCst) != PHASE_OPEN || !modifiers_held(binding.mods) {
        return false;
    }
    if !DISMISS_ARMED.swap(false, Ordering::SeqCst) {
        return false;
    }
    HOLD_CLOSED.store(true, Ordering::SeqCst);
    force_dismiss(app);
    true
}

#[cfg(windows)]
fn modifiers_held(mods: Modifiers) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    let down = |vk: u16| unsafe { GetAsyncKeyState(i32::from(vk)) } < 0;
    let want_win = mods.contains(Modifiers::SUPER) || mods.contains(Modifiers::META);
    mods.contains(Modifiers::CONTROL) == down(VK_CONTROL)
        && mods.contains(Modifiers::SHIFT) == down(VK_SHIFT)
        && mods.contains(Modifiers::ALT) == down(VK_MENU)
        && want_win == (down(VK_LWIN) || down(VK_RWIN))
}

fn conflicts(app: &AppHandle, shortcut: &str) -> bool {
    let Ok(parsed) = shortcut.trim().parse::<Shortcut>() else {
        return false;
    };
    let settings = app.state::<SharedSettings>();
    let Ok(store) = settings.lock() else {
        return false;
    };
    let quick = store.quick_capture_settings();
    quick.enabled && same_shortcut(&quick.shortcut, &parsed)
}

fn on_main<T>(app: &AppHandle, task: impl FnOnce() -> T + Send + 'static) -> Result<T, String>
where
    T: Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.send(task());
    })
    .map_err(|err| err.to_string())?;
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(value) => Ok(value),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            conceal_shot_windows();
            crate::logging::warn(format_args!("截图窗口操作超时"));
            Err("截图窗口没有响应".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("截图窗口没有响应".into()),
    }
}

fn do_open(app: &AppHandle, generation: u64) {
    let service_ms = elapsed_since_open();
    span_begin(generation);
    span_update(generation, |span| span.service_ms = service_ms);
    crate::logging::info(format_args!(
        "screenshot service-recv: {service_ms}ms gen={generation}"
    ));
    if !generation_current(generation) {
        settle_idle();
        return;
    }
    let mode = capture_mode(app);
    crate::logging::info(format_args!("screenshot mode: {}", mode.label()));
    let opened = match mode {
        CaptureMode::Live => show_live(app, generation),
        CaptureMode::Freeze => capture_and_show(app, generation),
    };
    match opened {
        Ok(()) if generation_current(generation) && !CANCELLED.load(Ordering::SeqCst) => {
            PHASE.store(PHASE_OPEN, Ordering::SeqCst);
            ACTIVE.store(true, Ordering::SeqCst);
        }
        Ok(()) => {
            hide_overlay(app, true);
            settle_idle();
        }
        Err(err) => {
            if generation_current(generation) {
                CANCELLED.store(true, Ordering::SeqCst);
            }
            if err != "截图已取消" && err != "截图窗口没有响应" {
                crate::logging::warn(format_args!("截图失败: {err}"));
            }
            hide_overlay(app, true);
            if generation_current(generation) {
                settle_idle();
            }
        }
    }
}

fn do_close(app: &AppHandle) {
    let generation = GENERATION.load(Ordering::SeqCst);
    CANCELLED.store(true, Ordering::SeqCst);
    if generation_current(generation) {
        settle_idle();
    }
    hide_overlay(app, true);
    if generation_current(generation) {
        CANCELLED.store(true, Ordering::SeqCst);
        settle_idle();
    }
}

fn settle_idle() {
    PHASE.store(PHASE_IDLE, Ordering::SeqCst);
    ACTIVE.store(false, Ordering::SeqCst);
}

fn generation_current(generation: u64) -> bool {
    GENERATION.load(Ordering::SeqCst) == generation
}

fn hide_overlay(app: &AppHandle, clear_session: bool) {
    conceal_shot_windows();
    if !clear_session {
        return;
    }
    let _ = app.emit("shot-arm", "reset");
    let dir = app
        .state::<ShotRuntime>()
        .inner
        .lock()
        .ok()
        .and_then(|mut state| {
            state.opening = false;
            state.session.take().map(|session| session.dir)
        });
    if let Some(dir) = dir {
        remove_tree(&dir);
        crate::logging::info(format_args!("screenshot session closed"));
    }
}

fn capture_and_show(app: &AppHandle, generation: u64) -> Result<(), String> {
    if !generation_current(generation) {
        return Err("截图已取消".into());
    }
    // 不走 available_monitors。那条要等主线程，而主线程可能正卡在另一块屏的 WebView2 创建里。
    let rects = monitor_rects(app)?;
    let focus = cursor_screen(&rects);
    ACTIVE_SCREEN.store(focus, Ordering::SeqCst);
    let raw = raw_frames_enabled();
    crate::logging::info(format_args!(
        "screenshot capture mode: {}",
        if raw { "raw" } else { "png" }
    ));
    let live = shots_dir(app)?
        .join("live")
        .join(format!("{}-{generation}", unix_ms()));
    if !raw {
        fs::create_dir_all(&live).map_err(|err| format!("创建截图目录失败: {err}"))?;
    }
    {
        let runtime = runtime(app);
        let mut state = runtime.inner.lock().map_err(|err| err.to_string())?;
        state.session = Some(Session {
            dir: live.clone(),
            screens: Vec::new(),
            frames: Vec::new(),
        });
        state.opening = false;
    }
    let _ = app.emit("shot-arm", ());

    // 先只抓鼠标所在屏并显示。其它屏可以并行抓，但窗口要等活动屏的冻结图出来再摆。
    let focus_index = usize::try_from(focus).map_err(|_| "没有可用的显示器")?;
    let focus_rect = rects.get(focus_index).copied().ok_or("没有可用的显示器")?;
    shot_trace(format_args!(
        "screenshot capture-start: {focus} at {}ms",
        elapsed_since_open()
    ));
    let capture_at = elapsed_since_open();
    let captured = {
        let _priority = CapturePriority::boost();
        capture_one(&live, focus, focus_rect, raw)
    }?;
    let capture_ms = elapsed_since_open().saturating_sub(capture_at);
    shot_trace(format_args!(
        "screenshot capture-end: {focus} {capture_ms}ms"
    ));
    if !generation_current(generation) || CANCELLED.load(Ordering::SeqCst) {
        return Err("截图已取消".into());
    }
    let screen = captured.screen;
    let active_ms = elapsed_since_open();
    crate::logging::info(format_args!(
        "screenshot active captured: {} {}x{} in {active_ms}ms",
        screen.index, screen.width, screen.height
    ));
    remember_screen(app, &screen, captured.frame)?;
    show_screen(app, &screen, true, generation)?;
    let shown_ms = elapsed_since_open();
    crate::logging::info(format_args!(
        "screenshot active shown: {} in {shown_ms}ms",
        screen.index
    ));
    span_update(generation, |span| {
        span.capture_ms = capture_ms;
        span.shown_ms = shown_ms;
        span.screens = rects.len() as u32;
    });

    let pending: Vec<(u32, CaptureRect)> = rects
        .iter()
        .copied()
        .enumerate()
        .filter(|(index, _)| *index as u32 != focus)
        .map(|(index, rect)| (index as u32, rect))
        .collect();
    if pending.is_empty() {
        crate::logging::info(format_args!("screenshot session opened: 1 in {shown_ms}ms"));
        return Ok(());
    }
    defer_extra_screens(app, generation, focus, live, pending, raw);
    Ok(())
}

fn defer_extra_screens(
    app: &AppHandle,
    generation: u64,
    focus: u32,
    live: PathBuf,
    pending: Vec<(u32, CaptureRect)>,
    raw: bool,
) {
    let (tx, rx) = mpsc::channel();
    let mut started = 0usize;
    for (index, rect) in pending {
        let report = tx.clone();
        let dir = live.clone();
        let opened = OPENED_AT.load(Ordering::SeqCst);
        match std::thread::Builder::new()
            .name(format!("shot-cap-{index}"))
            .spawn(move || {
                relax_capture_thread();
                let result = capture_one(&dir, index, rect, raw);
                let elapsed = unix_ms().saturating_sub(opened);
                let _ = report.send((result, elapsed));
            }) {
            Ok(_) => started += 1,
            Err(err) => {
                crate::logging::warn(format_args!("启动截图失败: {err}"));
                break;
            }
        }
    }
    drop(tx);
    let app = app.clone();
    let task = move || finish_extra_screens(app, generation, focus, rx, started);
    if let Err(err) = std::thread::Builder::new()
        .name("shot-extra".into())
        .spawn(task)
    {
        crate::logging::warn(format_args!("screenshot extra thread failed: {err}"));
    }
}

fn finish_extra_screens(
    app: AppHandle,
    generation: u64,
    focus: u32,
    rx: mpsc::Receiver<(Result<CapturedScreen, String>, u64)>,
    started: usize,
) {
    let mut screens = Vec::new();
    let mut received = 0usize;
    while received < started {
        let (frame, _elapsed) = match rx.recv() {
            Ok(frame) => frame,
            Err(_) => break,
        };
        received += 1;
        if !generation_current(generation) || CANCELLED.load(Ordering::SeqCst) {
            return;
        }
        let captured = match frame {
            Ok(captured) => captured,
            Err(err) => {
                crate::logging::warn(format_args!("screenshot extra capture failed: {err}"));
                continue;
            }
        };
        let screen = captured.screen;
        if let Err(err) = remember_screen(&app, &screen, captured.frame) {
            crate::logging::warn(format_args!("screenshot extra store failed: {err}"));
            continue;
        }
        screens.push(screen);
    }
    if !generation_current(generation) || CANCELLED.load(Ordering::SeqCst) {
        return;
    }
    if screens.is_empty() {
        crate::logging::info(format_args!(
            "screenshot session opened: 1 in {}ms",
            elapsed_since_open()
        ));
        return;
    }
    wait_active_frame(generation);
    if !generation_current(generation) || CANCELLED.load(Ordering::SeqCst) {
        return;
    }
    for screen in &screens {
        if let Err(err) = show_screen(&app, screen, false, generation) {
            crate::logging::warn(format_args!("screenshot extra show failed: {err}"));
            continue;
        }
        crate::logging::info(format_args!(
            "screenshot extra shown: {} in {}ms",
            screen.index,
            elapsed_since_open()
        ));
    }
    if let Err(err) = focus_shot(&app, focus, generation) {
        crate::logging::warn(format_args!("screenshot focus failed: {err}"));
    }
    let total = screens.len() + 1;
    let extras_ms = elapsed_since_open();
    span_update(generation, |span| span.extras_ms = extras_ms);
    crate::logging::info(format_args!(
        "screenshot session opened: {total} in {extras_ms}ms"
    ));
}

fn show_screen(
    app: &AppHandle,
    screen: &ScreenFile,
    activate: bool,
    generation: u64,
) -> Result<(), String> {
    let screen = screen.clone();
    let shown = app.clone();
    on_main(app, move || -> Result<(), String> {
        if !generation_current(generation) || CANCELLED.load(Ordering::SeqCst) {
            conceal_shot_windows();
            return Err("截图已取消".into());
        }
        ensure_shown(&shown, &screen, activate)
    })
    .and_then(|result| result)
}

fn focus_shot(app: &AppHandle, focus: u32, generation: u64) -> Result<(), String> {
    let shown = app.clone();
    on_main(app, move || -> Result<(), String> {
        if !generation_current(generation) || CANCELLED.load(Ordering::SeqCst) {
            conceal_shot_windows();
            return Err("截图已取消".into());
        }
        let Some(window) = shown.get_webview_window(&format!("shot-{focus}")) else {
            return Ok(());
        };
        #[cfg(windows)]
        activate_window(&window);
        let _ = window.as_ref().set_focus();
        Ok(())
    })
    .and_then(|result| result)
}

fn remember_screen(
    app: &AppHandle,
    screen: &ScreenFile,
    frame: Option<LiveFrame>,
) -> Result<(), String> {
    let runtime = runtime(app);
    let mut state = runtime.inner.lock().map_err(|err| err.to_string())?;
    let session = state.session.as_mut().ok_or("截图已经关掉")?;
    if let Some(slot) = session
        .screens
        .iter_mut()
        .find(|item| item.index == screen.index)
    {
        *slot = screen.clone();
    } else {
        session.screens.push(screen.clone());
    }
    if let Some(frame) = frame {
        session.frames.retain(|item| item.index != frame.index);
        session.frames.push(frame);
    }
    Ok(())
}

fn ensure_window(
    app: &AppHandle,
    screen: &ScreenFile,
    live: bool,
) -> Result<WebviewWindow, String> {
    let label = format!("shot-{}", screen.index);
    let slot = shot_index(&label);
    let kind = if live { KIND_LIVE } else { KIND_FREEZE };
    if let Some(existing) = app.get_webview_window(&label) {
        let current = slot
            .map(|index| SHOT_KIND[index].load(Ordering::SeqCst))
            .unwrap_or(0);
        let reusable = current == kind || (current == 0 && !live);
        if reusable {
            if let Some(index) = slot {
                SHOT_KIND[index].store(kind, Ordering::SeqCst);
            }
            shot_trace(format_args!(
                "screenshot window-ensure: {} reused",
                screen.index
            ));
            #[cfg(windows)]
            exclude_from_capture(&existing);
            place_shot_physical(
                &existing,
                screen.x,
                screen.y,
                screen.monitor_w,
                screen.monitor_h,
            );
            return Ok(existing);
        }
        existing
            .destroy()
            .map_err(|err| format!("切换截图窗口失败: {err}"))?;
        if let Some(index) = slot {
            SHOT_SHOWN[index].store(false, Ordering::SeqCst);
            SHOT_HWNDS[index].store(0, Ordering::SeqCst);
            SHOT_KIND[index].store(0, Ordering::SeqCst);
        }
    }
    if let Some(index) = slot {
        SHOT_SHOWN[index].store(false, Ordering::SeqCst);
    }
    let webview = shot_webview_dir(screen.index)?;
    let url = if live {
        format!("shot.html?screen={}&live=1", screen.index)
    } else {
        format!("shot.html?screen={}", screen.index)
    };
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .data_directory(webview)
        .title("截图")
        .decorations(false)
        .resizable(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false);
    builder = builder
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0));
    #[cfg(windows)]
    {
        builder = builder.additional_browser_args(SHOT_BROWSER_ARGS);
    }
    let window = builder
        .zoom_hotkeys_enabled(false)
        .build()
        .map_err(|err| format!("打开截图窗口失败: {err}"))?;
    #[cfg(windows)]
    exclude_from_capture(&window);
    shot_trace(format_args!(
        "screenshot window-ensure: {} created",
        screen.index
    ));
    if let Some(index) = slot {
        SHOT_KIND[index].store(kind, Ordering::SeqCst);
    }
    place_shot_physical(
        &window,
        screen.x,
        screen.y,
        screen.monitor_w,
        screen.monitor_h,
    );
    Ok(window)
}

fn ensure_shown(
    app: &AppHandle,
    screen: &ScreenFile,
    activate: bool,
) -> Result<(), String> {
    if CANCELLED.load(Ordering::SeqCst) {
        return Err("截图已取消".into());
    }
    let window = ensure_window(app, screen, false)?;
    let _ = window.set_ignore_cursor_events(false);
    remember_window(&window);
    if CANCELLED.load(Ordering::SeqCst) {
        conceal_shot_windows();
        return Err("截图已取消".into());
    }
    present_shot(&window, screen.index, activate);
    if CANCELLED.load(Ordering::SeqCst) {
        conceal_shot_windows();
        return Err("截图已取消".into());
    }
    // 显示后再摆一次。换屏时系统会按 DPI 把窗口尺寸乘一遍，必须在那之后用物理像素盖回去。
    place_shot_physical(
        &window,
        screen.x,
        screen.y,
        screen.monitor_w,
        screen.monitor_h,
    );
    if activate {
        let _ = app.emit("shot-active", screen.index);
    }
    let _ = app.emit_to(
        &format!("shot-{}", screen.index),
        "shot-refresh",
        ShotRefresh {
            index: screen.index,
            path: screen.path.display().to_string(),
            raw: screen.path.as_os_str().is_empty(),
            active: activate,
            crop: false,
            backdrop: false,
            generation: GENERATION.load(Ordering::SeqCst),
        },
    );
    Ok(())
}

fn present_shot(window: &WebviewWindow, index: u32, activate: bool) {
    let slot = usize::try_from(index)
        .ok()
        .filter(|slot| *slot < MAX_SCREENS);
    let first = match slot {
        Some(slot) => !SHOT_SHOWN[slot].load(Ordering::SeqCst),
        None => true,
    };
    let raw = slot
        .map(|slot| SHOT_HWNDS[slot].load(Ordering::SeqCst))
        .unwrap_or(0);
    let visible = if first {
        // 父窗口和网页视图的可见性是分开的。第一次两套都要打开；非活动屏不用会抢焦点的 show。
        reveal_first(window, raw, activate)
    } else {
        reveal_again(window, raw, activate);
        true
    };
    if first && visible {
        if let Some(slot) = slot {
            SHOT_SHOWN[slot].store(true, Ordering::SeqCst);
        }
    }
    if activate {
        #[cfg(windows)]
        activate_window(window);
        let _ = window.as_ref().set_focus();
    }
}

fn reveal_first(window: &WebviewWindow, raw: isize, activate: bool) -> bool {
    let webview: &tauri::Webview<_> = window.as_ref();
    #[cfg(windows)]
    if !activate && raw != 0 {
        // 同步显示，网页视图才能跟在已经可见的父窗口后面；不用 SW_SHOW，避免抢走活动屏焦点。
        show_inactive_now(raw);
        let view = webview.show();
        show_inactive_now(raw);
        return view.is_ok();
    }
    let parent = window.show();
    let view = webview.show();
    #[cfg(windows)]
    if !activate && raw != 0 {
        show_inactive_now(raw);
    }
    let _ = raw;
    parent.is_ok() && view.is_ok()
}

#[cfg(windows)]
fn show_inactive_now(raw: isize) {
    if raw == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};
        let hwnd = raw as windows_sys::Win32::Foundation::HWND;
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

fn reveal_again(window: &WebviewWindow, raw: isize, activate: bool) {
    #[cfg(windows)]
    if raw != 0 {
        if activate {
            force_show(raw);
        } else {
            force_show_inactive(raw);
        }
        return;
    }
    let _ = (raw, activate);
    let _ = window.show();
}

fn shot_webview_dir(index: u32) -> Result<PathBuf, String> {
    // 截图窗口带了单独的浏览器参数，不能跟主窗口共用数据目录。
    // 每块屏再各用一个目录：第二块屏如果复用第一块的环境，创建会卡在主线程的嵌套消息循环里，热键就再也打不开。
    let root = crate::webview_data_dir().map_err(|err| err.to_string())?;
    let dir = root.join("shot").join(index.to_string());
    fs::create_dir_all(&dir).map_err(|err| format!("创建截图窗口目录失败: {err}"))?;
    Ok(dir)
}

pub(crate) fn prewarm(app: &AppHandle) {
    if !in_shot_service() || !screenshot_enabled(app) || PREWARM_STARTED.swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    if std::thread::Builder::new()
        .name("shot-prewarm".into())
        .spawn(move || prewarm_screens(&app))
        .is_err()
    {
        PREWARM_STARTED.store(false, Ordering::SeqCst);
    }
}

fn prewarm_screens(app: &AppHandle) {
    #[cfg(windows)]
    relax_prewarm_thread();
    std::thread::sleep(Duration::from_millis(200));
    if prewarm_blocked() || !screenshot_enabled(app) {
        return;
    }
    let live = matches!(capture_mode(app), CaptureMode::Live);
    // 显示器列表不能等主线程，预热自己就跑在主线程外面。
    let Ok(rects) = monitor_rects(app) else {
        return;
    };
    let focus = usize::try_from(cursor_screen(&rects)).unwrap_or(0);
    let Some(rect) = rects.get(focus).copied() else {
        return;
    };
    // 只预热鼠标所在的那块屏。接着在主线程里创建第二块屏的 WebView2 时，
    // 完成回调进不了正在跑的消息循环，主线程就不再回来，后面的热键全部空转。
    if prewarm_blocked() || !screenshot_enabled(app) {
        crate::logging::info(format_args!("screenshot prewarm stopped"));
        return;
    }
    let screen = ScreenFile {
        index: focus as u32,
        path: PathBuf::new(),
        width: rect.width,
        height: rect.height,
        x: rect.x,
        y: rect.y,
        monitor_w: rect.width,
        monitor_h: rect.height,
    };
    let prepared = app.clone();
    let build_at = unix_ms();
    let created = on_main_prewarm(app, move || -> Result<(), String> {
        if prewarm_blocked() {
            return Ok(());
        }
        ensure_window(&prepared, &screen, live)?;
        Ok(())
    });
    match created {
        Ok(Ok(())) => crate::logging::info(format_args!(
            "screenshot prewarm: screen {focus} build {}ms",
            unix_ms().saturating_sub(build_at)
        )),
        Ok(Err(err)) | Err(err) => {
            crate::logging::warn(format_args!("截图窗口预热失败: {err}"));
            return;
        }
    }
    crate::logging::info(format_args!("screenshot windows ready"));
}

fn on_main_prewarm<T>(
    app: &AppHandle,
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.send(task());
    })
    .map_err(|err| err.to_string())?;
    rx.recv_timeout(Duration::from_secs(8))
        .map_err(|_| "截图窗口没有响应".to_string())
}

fn prewarm_blocked() -> bool {
    PREWARM_STOP.load(Ordering::SeqCst) || PHASE.load(Ordering::SeqCst) != PHASE_IDLE
}

fn screenshot_enabled(app: &AppHandle) -> bool {
    app.state::<SharedSettings>()
        .lock()
        .map(|store| store.screenshot_settings().enabled)
        .unwrap_or(false)
}

#[derive(Clone, Copy)]
enum CaptureMode {
    Freeze,
    Live,
}

impl CaptureMode {
    fn label(self) -> &'static str {
        match self {
            Self::Freeze => "freeze",
            Self::Live => "live",
        }
    }
}

pub(crate) fn mark_shot_service() {
    IN_SHOT_SERVICE.store(true, Ordering::SeqCst);
}

fn in_shot_service() -> bool {
    IN_SHOT_SERVICE.load(Ordering::SeqCst)
}

fn capture_mode(app: &AppHandle) -> CaptureMode {
    let mode = app
        .state::<SharedSettings>()
        .lock()
        .map(|store| store.screenshot_settings().capture_mode)
        .unwrap_or_default();
    match mode.as_str() {
        "live" => CaptureMode::Live,
        _ => CaptureMode::Freeze,
    }
}

fn monitor_rects(app: &AppHandle) -> Result<Vec<CaptureRect>, String> {
    #[cfg(windows)]
    {
        let _ = app;
        return local_monitors();
    }
    #[cfg(not(windows))]
    {
        let monitors = app.available_monitors().map_err(|err| err.to_string())?;
        if monitors.is_empty() {
            return Err("没有可用的显示器".into());
        }
        Ok(monitors
            .into_iter()
            .take(MAX_SCREENS)
            .map(|monitor| {
                let position = monitor.position();
                let size = monitor.size();
                CaptureRect {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                }
            })
            .collect())
    }
}

#[cfg(windows)]
fn local_monitors() -> Result<Vec<CaptureRect>, String> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
    };

    struct List(Vec<CaptureRect>);

    unsafe extern "system" fn each(
        _monitor: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> windows_sys::core::BOOL {
        let list = unsafe { &mut *(data as *mut List) };
        if list.0.len() >= MAX_SCREENS {
            return 0;
        }
        let rect = unsafe { &*rect };
        let width = rect.right.saturating_sub(rect.left);
        let height = rect.bottom.saturating_sub(rect.top);
        if width > 0 && height > 0 {
            list.0.push(CaptureRect {
                x: rect.left,
                y: rect.top,
                width: width as u32,
                height: height as u32,
            });
        }
        1
    }

    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let mut list = List(Vec::new());
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(each),
            &mut list as *mut List as LPARAM,
        );
        if !previous.is_null() {
            SetThreadDpiAwarenessContext(previous);
        }
    }
    if list.0.is_empty() {
        return Err("没有可用的显示器".into());
    }
    Ok(list.0)
}

fn open_empty_session(app: &AppHandle, generation: u64) -> Result<(), String> {
    let dir = shots_dir(app)?
        .join("live")
        .join(format!("{}-{generation}", unix_ms()));
    let runtime = runtime(app);
    let mut state = runtime.inner.lock().map_err(|err| err.to_string())?;
    state.session = Some(Session {
        dir,
        screens: Vec::new(),
        frames: Vec::new(),
    });
    state.opening = false;
    Ok(())
}

fn show_live(app: &AppHandle, generation: u64) -> Result<(), String> {
    let rects = monitor_rects(app)?;
    if !generation_current(generation) || CANCELLED.load(Ordering::SeqCst) {
        return Err("截图已取消".into());
    }
    let focus = cursor_screen(&rects);
    ACTIVE_SCREEN.store(focus, Ordering::SeqCst);
    open_empty_session(app, generation)?;
    let rect = rects
        .get(usize::try_from(focus).unwrap_or(usize::MAX))
        .copied()
        .ok_or("没有可用的显示器")?;
    let screen = ScreenFile {
        index: focus,
        path: PathBuf::new(),
        width: rect.width,
        height: rect.height,
        x: rect.x,
        y: rect.y,
        monitor_w: rect.width,
        monitor_h: rect.height,
    };
    remember_screen(app, &screen, None)?;
    let shown = app.clone();
    on_main(app, move || show_live_screen(&shown, &screen)).and_then(|result| result)?;
    crate::logging::info(format_args!(
        "screenshot live shown: {focus} in {}ms",
        elapsed_since_open()
    ));
    Ok(())
}

fn show_live_screen(app: &AppHandle, screen: &ScreenFile) -> Result<(), String> {
    if CANCELLED.load(Ordering::SeqCst) {
        return Err("截图已取消".into());
    }
    let window = ensure_window(app, screen, true)?;
    remember_window(&window);
    #[cfg(windows)]
    set_no_activate(shot_hwnd(screen.index), true);
    present_shot(&window, screen.index, false);
    place_shot_physical(
        &window,
        screen.x,
        screen.y,
        screen.monitor_w,
        screen.monitor_h,
    );
    let _ = app.emit_to(
        &format!("shot-{}", screen.index),
        "shot-live",
        ShotLive {
            index: screen.index,
            active: true,
        },
    );
    Ok(())
}

pub(crate) fn close_from_native(app: &AppHandle) {
    enqueue_close(app);
}

#[cfg(windows)]
fn shot_hwnd(index: u32) -> isize {
    usize::try_from(index)
        .ok()
        .filter(|slot| *slot < MAX_SCREENS)
        .map(|slot| SHOT_HWNDS[slot].load(Ordering::SeqCst))
        .unwrap_or(0)
}

#[cfg(windows)]
fn set_no_activate(raw: isize, enabled: bool) {
    if raw == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_NOACTIVATE,
        };
        let hwnd = raw as windows_sys::Win32::Foundation::HWND;
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let flag = isize::try_from(WS_EX_NOACTIVATE).unwrap_or(0);
        let next = if enabled { style | flag } else { style & !flag };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
    }
}

#[cfg(windows)]
fn hide_shot_sync(index: u32) {
    let raw = shot_hwnd(index);
    if raw == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
        let hwnd = raw as windows_sys::Win32::Foundation::HWND;
        // 不在主线程 DwmFlush：合成要等这个线程处理窗口消息，会一直卡成未响应。
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

#[cfg(windows)]
fn reveal_shot_sync(app: &AppHandle, index: u32) {
    let raw = shot_hwnd(index);
    set_no_activate(raw, false);
    if raw != 0 {
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{SW_SHOW, ShowWindow};
            let _ = ShowWindow(raw as windows_sys::Win32::Foundation::HWND, SW_SHOW);
        }
    }
    if let Some(window) = app.get_webview_window(&format!("shot-{index}")) {
        activate_window(&window);
        let _ = window.as_ref().set_focus();
    }
}

fn css_to_physical(css: f64, stage: f64, monitor: u32) -> i32 {
    if !css.is_finite() || monitor == 0 {
        return 0;
    }
    if !stage.is_finite() || stage < 1.0 {
        return css.round() as i32;
    }
    (css * f64::from(monitor) / stage).round() as i32
}

fn clamp_grab(
    monitor_w: u32,
    monitor_h: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(i32, i32, u32, u32), String> {
    if monitor_w == 0 || monitor_h == 0 || monitor_w > 16_000 || monitor_h > 16_000 {
        return Err("截图区域无效".into());
    }
    let x = x.max(0);
    let y = y.max(0);
    if x >= monitor_w as i32 || y >= monitor_h as i32 {
        return Err("选区跑出屏幕了".into());
    }
    let width = width.min(monitor_w as i32 - x);
    let height = height.min(monitor_h as i32 - y);
    if width < 3 || height < 3 {
        return Err("选区太小".into());
    }
    Ok((x, y, width as u32, height as u32))
}

#[cfg(test)]
pub(crate) fn crop_bgra(
    width: u32,
    height: u32,
    pixels: &[u8],
    x: i32,
    y: i32,
    crop_w: u32,
    crop_h: u32,
) -> Result<Vec<u8>, String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    if pixels.len() != expected {
        return Err("截图区域无效".into());
    }
    let (x, y, crop_w, crop_h) = clamp_grab(width, height, x, y, crop_w as i32, crop_h as i32)?;
    let mut out = vec![0u8; crop_w as usize * crop_h as usize * 4];
    for row in 0..crop_h {
        let src = ((y as u32 + row) as usize * width as usize + x as usize) * 4;
        let dst = row as usize * crop_w as usize * 4;
        let len = crop_w as usize * 4;
        out[dst..dst + len].copy_from_slice(&pixels[src..src + len]);
    }
    Ok(out)
}

#[cfg(windows)]
fn set_thread_priority(priority: i32) {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority};
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), priority);
    }
}

#[cfg(windows)]
fn relax_prewarm_thread() {
    use windows_sys::Win32::System::Threading::THREAD_PRIORITY_BELOW_NORMAL;
    set_thread_priority(THREAD_PRIORITY_BELOW_NORMAL);
}

fn relax_capture_thread() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::THREAD_PRIORITY_BELOW_NORMAL;
        set_thread_priority(THREAD_PRIORITY_BELOW_NORMAL);
    }
}

struct CapturePriority;

impl CapturePriority {
    fn boost() -> Self {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::THREAD_PRIORITY_ABOVE_NORMAL;
            set_thread_priority(THREAD_PRIORITY_ABOVE_NORMAL);
        }
        Self
    }
}

impl Drop for CapturePriority {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::THREAD_PRIORITY_NORMAL;
            set_thread_priority(THREAD_PRIORITY_NORMAL);
        }
    }
}

fn capture_one(
    live: &Path,
    index: u32,
    rect: CaptureRect,
    raw: bool,
) -> Result<CapturedScreen, String> {
    if CANCELLED.load(Ordering::SeqCst) {
        return Err("截图已取消".into());
    }
    let screen = ScreenFile {
        index,
        path: PathBuf::new(),
        width: 0,
        height: 0,
        x: rect.x,
        y: rect.y,
        monitor_w: rect.width,
        monitor_h: rect.height,
    };
    if raw {
        let image = capture_bgra(rect.x, rect.y, rect.width as i32, rect.height as i32, false)?;
        return Ok(CapturedScreen {
            screen: ScreenFile {
                width: image.width,
                height: image.height,
                ..screen
            },
            frame: Some(LiveFrame {
                index,
                width: image.width,
                height: image.height,
                pixels: Arc::new(image.pixels),
            }),
        });
    }
    let image = capture_rect(rect.x, rect.y, rect.width as i32, rect.height as i32, false)?;
    let path = live.join(format!("{index}.png"));
    write_png(&path, &image)?;
    Ok(CapturedScreen {
        screen: ScreenFile {
            path,
            width: image.width(),
            height: image.height(),
            ..screen
        },
        frame: None,
    })
}

/// 展示默认走内存帧。`DAODIAN_SHOT_PNG=1` 回到 PNG 落盘再读回。
fn raw_frames_enabled() -> bool {
    std::env::var("DAODIAN_SHOT_PNG")
        .ok()
        .map(|value| {
            let value = value.trim();
            !(value == "1" || value.eq_ignore_ascii_case("true"))
        })
        .unwrap_or(true)
}

fn shots_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .directory();
    Ok(base.join("shots"))
}

fn runtime(app: &AppHandle) -> tauri::State<'_, ShotRuntime> {
    app.state()
}

fn cursor_screen(rects: &[CaptureRect]) -> u32 {
    screen_at(rects, cursor_point())
}

fn screen_at(rects: &[CaptureRect], point: Option<(i32, i32)>) -> u32 {
    let Some((x, y)) = point else {
        return 0;
    };
    rects
        .iter()
        .enumerate()
        .find(|(_, rect)| {
            x >= rect.x
                && y >= rect.y
                && x < rect.x.saturating_add(rect.width as i32)
                && y < rect.y.saturating_add(rect.height as i32)
        })
        .map(|(index, _)| index as u32)
        .unwrap_or(0)
}

#[cfg(windows)]
fn cursor_point() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 {
            None
        } else {
            Some((point.x, point.y))
        }
    }
}

#[cfg(not(windows))]
fn cursor_point() -> Option<(i32, i32)> {
    None
}

fn place_pin(
    anchor: &WebviewWindow,
    window: &WebviewWindow,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let origin = anchor.outer_position().map_err(|err| err.to_string())?;
    let mut left = origin.x.saturating_add(x);
    let mut top = origin.y.saturating_add(y);
    let mut w = width.max(1);
    let mut h = height.max(1);
    let monitor = anchor
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| anchor.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor.as_ref() {
        let area = monitor.work_area();
        let margin = 8i32;
        let max_w = (f64::from(area.size.width) * 0.7).max(24.0);
        let max_h = (f64::from(area.size.height) * 0.7).max(24.0);
        let fit = (max_w / f64::from(w)).min(max_h / f64::from(h)).min(1.0);
        w = (f64::from(w) * fit).clamp(24.0, f64::from(area.size.width).max(24.0)) as u32;
        h = (f64::from(h) * fit).clamp(24.0, f64::from(area.size.height).max(24.0)) as u32;
        let min_x = area.position.x + margin;
        let min_y = area.position.y + margin;
        let max_x = area.position.x + area.size.width as i32 - w as i32 - margin;
        let max_y = area.position.y + area.size.height as i32 - h as i32 - margin;
        left = left.clamp(min_x, max_x.max(min_x));
        top = top.clamp(min_y, max_y.max(min_y));
    }
    window
        .set_size(PhysicalSize::new(w, h))
        .map_err(|err| err.to_string())?;
    window
        .set_position(PhysicalPosition::new(left, top))
        .map_err(|err| err.to_string())?;
    crate::logging::info(format_args!(
        "screenshot pin placed: {w}x{h} at {left},{top}"
    ));
    Ok(())
}

#[tauri::command]
pub fn shot_start(app: AppHandle) {
    if !in_shot_service() {
        #[cfg(windows)]
        if let Err(err) = crate::shot_sidecar::capture() {
            crate::logging::warn(format_args!("截图失败: {err}"));
        }
        return;
    }
    trigger(&app);
}

#[tauri::command]
pub fn shot_screen(app: AppHandle, index: u32) -> Result<ScreenPayload, String> {
    let mode_live = matches!(capture_mode(&app), CaptureMode::Live);
    let (path, width, height, live) = {
        let runtime = runtime(&app);
        let state = runtime.inner.lock().map_err(|err| err.to_string())?;
        let session = state.session.as_ref().ok_or("截图已经关掉")?;
        let screen = session
            .screens
            .iter()
            .find(|screen| screen.index == index)
            .ok_or("找不到这一屏")?;
        let has_frame = session
            .frames
            .iter()
            .any(|frame| frame.index == index && !frame.pixels.is_empty());
        (
            screen.path.clone(),
            screen.width,
            screen.height,
            !has_frame && mode_live,
        )
    };
    Ok(ScreenPayload {
        index,
        path: path.display().to_string(),
        width,
        height,
        raw: path.as_os_str().is_empty(),
        active: index == ACTIVE_SCREEN.load(Ordering::SeqCst),
        live,
        generation: GENERATION.load(Ordering::SeqCst),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotGrab {
    index: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    stage_width: f64,
    stage_height: f64,
}

#[tauri::command]
pub fn shot_long(app: AppHandle, grab: ShotGrab) -> Result<(), String> {
    #[cfg(windows)]
    {
        start_live_from_overlay(&app, grab, longshot::start)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, grab);
        Err("当前只支持在 Windows 上长截图".into())
    }
}

#[tauri::command]
pub fn shot_gif(app: AppHandle, grab: ShotGrab) -> Result<(), String> {
    #[cfg(windows)]
    {
        start_live_from_overlay(&app, grab, gifshot::start)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, grab);
        Err("当前只支持在 Windows 上录 GIF".into())
    }
}

#[cfg(windows)]
fn start_live_from_overlay(
    app: &AppHandle,
    grab: ShotGrab,
    launch: impl FnOnce(AppHandle, longshot::LiveGrab) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    if CANCELLED.load(Ordering::SeqCst) {
        return Err("截图已取消".into());
    }
    let screen = {
        let runtime = runtime(app);
        let state = runtime.inner.lock().map_err(|err| err.to_string())?;
        let session = state.session.as_ref().ok_or("截图已经关掉")?;
        session
            .screens
            .iter()
            .find(|screen| screen.index == grab.index)
            .cloned()
            .ok_or("找不到这一屏")?
    };
    let x = css_to_physical(grab.x, grab.stage_width, screen.monitor_w);
    let y = css_to_physical(grab.y, grab.stage_height, screen.monitor_h);
    let width = css_to_physical(grab.width, grab.stage_width, screen.monitor_w);
    let height = css_to_physical(grab.height, grab.stage_height, screen.monitor_h);
    let (x, y, width, height) =
        clamp_grab(screen.monitor_w, screen.monitor_h, x, y, width, height)?;
    let live = longshot::LiveGrab {
        selection_x: screen.x.saturating_add(x),
        selection_y: screen.y.saturating_add(y),
        selection_w: width,
        selection_h: height,
        monitor_x: screen.x,
        monitor_y: screen.y,
        monitor_w: screen.monitor_w,
        monitor_h: screen.monitor_h,
    };
    let owned = app.clone();
    on_main(app, move || {
        for index in 0..MAX_SCREENS as u32 {
            hide_shot_sync(index);
        }
        std::thread::sleep(Duration::from_millis(80));
        match launch(owned.clone(), live) {
            Ok(()) => Ok(()),
            Err(err) => {
                for index in 0..MAX_SCREENS as u32 {
                    if shot_hwnd(index) != 0 {
                        reveal_shot_sync(&owned, index);
                    }
                }
                Err(err)
            }
        }
    })?
}

#[tauri::command]
pub fn shot_grab(app: AppHandle, grab: ShotGrab) -> Result<(), String> {
    if CANCELLED.load(Ordering::SeqCst) {
        return Err("截图已取消".into());
    }
    let screen = {
        let runtime = runtime(&app);
        let state = runtime.inner.lock().map_err(|err| err.to_string())?;
        let session = state.session.as_ref().ok_or("截图已经关掉")?;
        session
            .screens
            .iter()
            .find(|screen| screen.index == grab.index)
            .cloned()
            .ok_or("找不到这一屏")?
    };
    let x = css_to_physical(grab.x, grab.stage_width, screen.monitor_w);
    let y = css_to_physical(grab.y, grab.stage_height, screen.monitor_h);
    let width = css_to_physical(grab.width, grab.stage_width, screen.monitor_w);
    let height = css_to_physical(grab.height, grab.stage_height, screen.monitor_h);
    let (x, y, width, height) =
        clamp_grab(screen.monitor_w, screen.monitor_h, x, y, width, height)?;
    #[cfg(windows)]
    hide_shot_sync(screen.index);
    let image = match capture_bgra(
        screen.x.saturating_add(x),
        screen.y.saturating_add(y),
        width as i32,
        height as i32,
        false,
    ) {
        Ok(image) => image,
        Err(err) => {
            #[cfg(windows)]
            reveal_shot_sync(&app, screen.index);
            return Err(err);
        }
    };
    remember_screen(
        &app,
        &screen,
        Some(LiveFrame {
            index: screen.index,
            width: image.width,
            height: image.height,
            pixels: Arc::new(image.pixels),
        }),
    )?;
    #[cfg(windows)]
    reveal_shot_sync(&app, screen.index);
    let _ = app.emit_to(
        &format!("shot-{}", screen.index),
        "shot-refresh",
        ShotRefresh {
            index: screen.index,
            path: String::new(),
            raw: true,
            active: true,
            crop: true,
            backdrop: false,
            generation: GENERATION.load(Ordering::SeqCst),
        },
    );
    crate::logging::info(format_args!(
        "screenshot selection grabbed: {} {width}x{height}",
        screen.index
    ));
    Ok(())
}

#[tauri::command]
pub fn shot_dismiss(app: AppHandle) {
    dismiss(&app);
}

#[tauri::command]
pub fn shot_backdrop_hide() {}

#[tauri::command]
pub fn shot_escape_ack(tick: u64) {
    ESCAPE_ACK.store(tick, Ordering::SeqCst);
}

#[tauri::command]
pub fn shot_frame(app: AppHandle, index: u32) -> Result<tauri::ipc::Response, String> {
    let requested = elapsed_since_open();
    shot_trace(format_args!(
        "screenshot frame-request: {index} at {requested}ms"
    ));
    let (width, height, pixels) = {
        let runtime = runtime(&app);
        let state = runtime.inner.lock().map_err(|err| err.to_string())?;
        let session = state.session.as_ref().ok_or("截图已经关掉")?;
        let frame = session
            .frames
            .iter()
            .find(|frame| frame.index == index && !frame.pixels.is_empty())
            .ok_or("找不到这一屏")?;
        (frame.width, frame.height, Arc::clone(&frame.pixels))
    };
    shot_trace(format_args!(
        "screenshot frame-bytes: {index} {} at {}ms",
        pixels.len(),
        elapsed_since_open()
    ));
    let pack_at = Instant::now();
    let file = rgba_frame(width, height, &pixels)?;
    let pack_ms = pack_at.elapsed().as_millis() as u64;
    if index == ACTIVE_SCREEN.load(Ordering::SeqCst) {
        let generation = GENERATION.load(Ordering::SeqCst);
        span_update(generation, |span| span.pack_ms = pack_ms);
    }
    shot_trace(format_args!("screenshot frame-pack: {index} {pack_ms}ms"));
    Ok(tauri::ipc::Response::new(file))
}

#[tauri::command]
pub fn shot_focus(window: WebviewWindow, index: u32) {
    ACTIVE_SCREEN.store(index, Ordering::SeqCst);
    let _ = window.app_handle().emit("shot-active", index);
    #[cfg(windows)]
    activate_window(&window);
    let _ = window.as_ref().set_focus();
}

#[tauri::command]
pub fn shot_ready(app: AppHandle, index: u32, generation: u64, bytes_ms: u64, decode_ms: u64) {
    if generation == 0 || generation != GENERATION.load(Ordering::SeqCst) {
        return;
    }
    let frame_ms = elapsed_since_open();
    crate::logging::info(format_args!(
        "screenshot frame ready: {index} in {frame_ms}ms"
    ));
    if index == ACTIVE_SCREEN.load(Ordering::SeqCst) {
        span_update(generation, |span| {
            span.frame_ms = frame_ms;
            span.bytes_ms = bytes_ms;
            span.decode_ms = decode_ms;
        });
    }
    release_frame(&app, index);
}

#[tauri::command]
pub fn shot_painted(index: u32, generation: u64, paint_ms: u64) {
    if generation == 0 || generation != GENERATION.load(Ordering::SeqCst) {
        return;
    }
    if index != ACTIVE_SCREEN.load(Ordering::SeqCst) {
        shot_trace(format_args!("screenshot first-paint: {index} {paint_ms}ms"));
        return;
    }
    note_active_ready(generation);
    log_shot_summary(paint_ms);
}

fn release_frame(app: &AppHandle, index: u32) {
    let runtime = app.state::<ShotRuntime>();
    let Ok(mut state) = runtime.inner.lock() else {
        return;
    };
    let Some(session) = state.session.as_mut() else {
        return;
    };
    if let Some(frame) = session.frames.iter_mut().find(|frame| frame.index == index) {
        frame.pixels = Arc::new(Vec::new());
    }
}

#[tauri::command]
pub async fn shot_ocr(png: String) -> Result<ocr::OcrOutcome, String> {
    let started = Instant::now();
    shot_trace(format_args!("screenshot ocr-start"));
    let result = tauri::async_runtime::spawn_blocking(move || ocr::recognize_png(&png))
        .await
        .map_err(|_| "取字被打断".to_string())?;
    crate::logging::info(format_args!(
        "screenshot ocr-end: {}ms",
        started.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
pub fn shot_copy_text(text: String) -> Result<(), String> {
    ocr::copy_text(&text)
}

#[tauri::command]
pub fn shot_ocr_settings() -> Result<(), String> {
    ocr::open_language_settings()
}

#[tauri::command]
pub fn shot_text_send(app: AppHandle, kind: String, text: String) -> Result<String, String> {
    let text = ocr::clip_text(&text);
    if text.is_empty() {
        return Err("没有可保存的文字".into());
    }
    if kind != "note" && kind != "todo" {
        return Err("不知道要把文字放到哪里".into());
    }
    #[cfg(windows)]
    if in_shot_service() {
        return daodian_shot::send_text(&kind, &text);
    }
    land_text(&app, &kind, &text)
}

pub(crate) fn land_text(app: &AppHandle, kind: &str, text: &str) -> Result<String, String> {
    let text = ocr::clip_text(text);
    if text.is_empty() {
        return Err("没有可保存的文字".into());
    }
    let (id, message) = match kind {
        "note" => crate::notes::from_capture(app, &text)?,
        "todo" => crate::store::todos::from_capture(app, &text)?,
        _ => return Err("不知道要把文字放到哪里".into()),
    };
    crate::logging::info(format_args!(
        "shot text landed kind={kind} chars={}",
        text.chars().count()
    ));
    crate::tray::show_main(app);
    let _ = app.emit(
        "shot-text-landed",
        serde_json::json!({ "kind": kind, "id": id }),
    );
    Ok(message)
}

#[tauri::command]
pub fn shot_copy(png: String) -> Result<(), String> {
    let started = Instant::now();
    shot_trace(format_args!("screenshot copy-start"));
    let result = (|| {
        if SAVING.load(Ordering::SeqCst) {
            return Err("正在保存，请稍等".into());
        }
        copy_image(&decode_png(&png)?)
    })();
    crate::logging::info(format_args!(
        "screenshot copy-end: {}ms",
        started.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
pub async fn shot_save(app: AppHandle, png: String) -> Result<ShotSaveResult, String> {
    let started = Instant::now();
    shot_trace(format_args!("screenshot save-start"));
    let image = decode_png(&png)?;
    let bytes = encode_png(&image)?;
    crate::logging::info(format_args!(
        "screenshot save-prepare: {}ms",
        started.elapsed().as_millis()
    ));
    let result = save_with_dialog(&app, &bytes);
    crate::logging::info(format_args!(
        "screenshot save-end: {}ms",
        started.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
pub async fn shot_save_bytes(
    app: AppHandle,
    request: tauri::ipc::Request<'_>,
) -> Result<ShotSaveResult, String> {
    let started = Instant::now();
    shot_trace(format_args!("screenshot save-start"));
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => bytes.as_slice(),
        tauri::ipc::InvokeBody::Json(_) => return Err("截图数据无效".into()),
    };
    validate_export_bytes(bytes)?;
    crate::logging::info(format_args!(
        "screenshot save-prepare: {}ms",
        started.elapsed().as_millis()
    ));
    let result = save_with_dialog(&app, bytes);
    crate::logging::info(format_args!(
        "screenshot save-end: {}ms",
        started.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
pub fn shot_show_saved(path: String) -> Result<(), String> {
    let saved = LAST_SAVED.lock().map_err(|err| err.to_string())?.clone();
    if path.is_empty() || saved != path {
        return Err("只能打开刚刚保存的文件".into());
    }
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err("找不到这个文件".into());
    }
    reveal_in_folder(&file)
}

#[tauri::command]
pub async fn shot_pin(
    app: AppHandle,
    window: WebviewWindow,
    png: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<usize, String> {
    let started = Instant::now();
    shot_trace(format_args!("screenshot pin-start"));
    let _pin_timer = ActionEnd("pin", started);
    if SAVING.load(Ordering::SeqCst) {
        return Err("正在保存，请稍等".into());
    }
    let mut image = decode_png(&png)?;
    // 截图像素的 alpha 经常是 0。贴图不需要透明，不抹平的话窗口会透出桌面。
    flatten_alpha(&mut image);
    // 先拿掉全屏层，再开置顶贴图。窗口要在异步命令里创建，同步命令里创建会把主线程卡住。
    conceal_shots(&app);
    let id = {
        let free = {
            let runtime = runtime(&app);
            let state = runtime.inner.lock().map_err(|err| err.to_string())?;
            state
                .pins
                .iter()
                .enumerate()
                .filter_map(|(index, slot)| slot.is_none().then_some(index))
                .collect::<Vec<_>>()
        };
        free.into_iter()
            .find(|index| app.get_webview_window(&format!("pin-{index}")).is_none())
            .ok_or_else(|| "贴图太多了，先关掉几张".to_string())?
    };
    let path = shots_dir(&app)?.join("pins").join(format!("{id}.png"));
    if let Err(err) = write_png(&path, &image) {
        reveal_shots(&app);
        return Err(err);
    }
    let image_w = image.width();
    let image_h = image.height();
    {
        let runtime = runtime(&app);
        runtime.inner.lock().map_err(|err| err.to_string())?.pins[id] = Some(PinSlot {
            path: path.clone(),
            width: image_w,
            height: image_h,
        });
    }
    let label = format!("pin-{id}");
    let webview_dir = crate::webview_data_dir().map_err(|err| err.to_string())?;
    let url = format!("index.html?window=pin&id={id}");
    let built = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .data_directory(webview_dir)
        .title("贴图")
        .decorations(false)
        .resizable(true)
        .transparent(false)
        .shadow(true)
        .background_color(tauri::window::Color(16, 24, 22, 255))
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .closable(true)
        .min_inner_size(24.0, 24.0)
        .prevent_overflow()
        .build();
    let pin = match built {
        Ok(pin) => pin,
        Err(err) => {
            clear_pin(&app, id, &path);
            reveal_shots(&app);
            return Err(format!("打开贴图失败: {err}"));
        }
    };
    remember_window(&pin);
    if let Err(err) = place_pin(&window, &pin, x, y, width, height) {
        let _ = pin.destroy();
        clear_pin(&app, id, &path);
        reveal_shots(&app);
        return Err(err);
    }
    if CANCELLED.load(Ordering::SeqCst) {
        let _ = pin.destroy();
        clear_pin(&app, id, &path);
        return Ok(id);
    }
    if let Err(err) = pin.show() {
        let _ = pin.destroy();
        clear_pin(&app, id, &path);
        reveal_shots(&app);
        return Err(err.to_string());
    }
    let pin_view: &tauri::Webview<_> = pin.as_ref();
    let _ = pin_view.show();
    // 隐藏期间改过的尺寸，WebView 不一定跟着变。显示后再摆一次，画面才能铺满窗口。
    if let Err(err) = place_pin(&window, &pin, x, y, width, height) {
        let _ = pin.destroy();
        clear_pin(&app, id, &path);
        reveal_shots(&app);
        return Err(err);
    }
    #[cfg(windows)]
    activate_window(&pin);
    release_overlay(&app);
    crate::logging::info(format_args!(
        "screenshot pin opened: {id} {image_w}x{image_h} file={}",
        path.display()
    ));
    Ok(id)
}

fn clear_pin(app: &AppHandle, id: usize, path: &Path) {
    if let Ok(mut state) = app.state::<ShotRuntime>().inner.lock() {
        if let Some(slot) = state.pins.get_mut(id) {
            *slot = None;
        }
    }
    let _ = fs::remove_file(path);
    forget_window(&format!("pin-{id}"));
}

#[tauri::command]
pub fn shot_pin_info(app: AppHandle, id: usize) -> Result<PinPayload, String> {
    let (path, width, height) = {
        let runtime = runtime(&app);
        let state = runtime.inner.lock().map_err(|err| err.to_string())?;
        let slot = state
            .pins
            .get(id)
            .and_then(|slot| slot.as_ref())
            .ok_or("这张贴图已经关掉")?;
        (slot.path.clone(), slot.width, slot.height)
    };
    Ok(PinPayload {
        id,
        path: path.display().to_string(),
        width,
        height,
    })
}

/// 8 字节宽高加上自上而下的 RGBA。不再包一层 BMP。
fn rgba_frame(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    if pixels == 0 || bgra.len() != pixels || width > 16_000 || height > 16_000 {
        return Err("截图区域无效".into());
    }
    let mut bytes = vec![0u8; 8 + pixels];
    bytes[..4].copy_from_slice(&width.to_le_bytes());
    bytes[4..8].copy_from_slice(&height.to_le_bytes());
    for (src, dst) in bgra.chunks_exact(4).zip(bytes[8..].chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    Ok(bytes)
}

#[cfg(test)]
fn bmp_file(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    if pixels == 0 || bgra.len() != pixels || width > 16_000 || height > 16_000 {
        return Err("截图区域无效".into());
    }
    let bytes_len = 54 + pixels;
    let file_size = u32::try_from(bytes_len).map_err(|_| "截图区域无效")?;
    let image_size = u32::try_from(pixels).map_err(|_| "截图区域无效")?;
    let mut bytes = Vec::with_capacity(bytes_len);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&file_size.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&image_size.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 16]);
    let stride = width as usize * 4;
    for row in (0..height as usize).rev() {
        let start = row * stride;
        bytes.extend_from_slice(&bgra[start..start + stride]);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
