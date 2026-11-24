//! 长截图：选区外的自绘控制条、穿透的边框。
//! 滚动过程中只留最后一帧，停稳并再确认一帧后才拼一次。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    CombineRgn, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen, CreateRectRgn,
    CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH,
    DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC,
    DeleteObject, DrawTextW, Ellipse, EndPaint, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, FillRect,
    COLORONCOLOR, GM_ADVANCED, GetStockObject, HDC, HOLLOW_BRUSH, InvalidateRect, LineTo, MoveToEx,
    NULL_PEN, SetGraphicsMode, SetWorldTransform, XFORM,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RGBQUAD, RGN_DIFF, Rectangle, RoundRect, SRCCOPY, SelectObject,
    SetBkMode, SetStretchBltMode, SetTextColor, SetWindowRgn, StretchDIBits, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, ReleaseCapture, SendInput,
    SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
    VK_NEXT, VK_PRIOR, VK_SPACE, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GetClientRect, GetCursorPos, HWND_TOPMOST, IDC_ARROW, KBDLLHOOKSTRUCT, LoadCursorW,
    MB_ICONWARNING, MB_OK, MSLLHOOKSTRUCT, MessageBoxW, PostMessageW, RegisterClassExW, SW_HIDE,
    SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetCursorPos, SetWindowDisplayAffinity, SetWindowPos, SetWindowTextW, SetWindowsHookExW,
    ShowWindow, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WHEEL_DELTA, WM_CLOSE,
    WM_COMMAND, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_PAINT, WM_SIZE, WM_SYSKEYDOWN, WNDCLASSEXW, WS_CLIPCHILDREN,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::capture::{
    capture_opening, choose_page_source, grab_page, page_source_from_bits,
};
use super::stitch::{
    BAR_H, BAR_W, Frame, LiveLayout, MAX_STITCH_FRAMES, MAX_STITCH_HEIGHT, Push, ScreenRect,
    Stitcher, crop_vertical, live_layout_bar, sample_sad,
};

const CLASS_NAME: [u16; 32] = utf16_fixed("DaodianLongShot");
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
const WM_APP_FRAME: u32 = 0x8000 + 1;
const WM_APP_CANCEL: u32 = 0x8000 + 2;
const WM_APP_SAVED: u32 = 0x8000 + 3;
const CMD_UNDO: usize = 301;
const CMD_DONE: usize = 302;
const CMD_CANCEL: usize = 303;
const CMD_AUTO: usize = 304;
const CMD_COPY: usize = 311;
const CMD_SAVE: usize = 312;
const CMD_CROP_TOP: usize = 313;
const CMD_CROP_BOTTOM: usize = 314;
const CMD_RESTART: usize = 315;
const CMD_CLOSE: usize = 316;
const PREVIEW_COMMANDS: [(usize, &str, bool); 6] = [
    (CMD_COPY, "复制", true),
    (CMD_SAVE, "保存", false),
    (CMD_CROP_TOP, "裁顶", false),
    (CMD_CROP_BOTTOM, "裁底", false),
    (CMD_RESTART, "重拼", false),
    (CMD_CLOSE, "关闭", false),
];
const WM_MOUSELEAVE: u32 = 675;

const MOTION_SAD: u32 = 6;
const SETTLE_MS: u64 = 150;
const STILL_NEED: u8 = 2;
const NO_MOTION_MS: u64 = 400;

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
static SCROLL_SEQ: AtomicU64 = AtomicU64::new(0);
static SCROLL_DIR: AtomicI32 = AtomicI32::new(-1);
static CAPTURE_MS: AtomicU64 = AtomicU64::new(200);
static PRESS_Y: AtomicI32 = AtomicI32::new(i32::MIN);
static HOOK_BAR: AtomicIsize = AtomicIsize::new(0);
static HOLE_X: AtomicI32 = AtomicI32::new(0);
static HOLE_Y: AtomicI32 = AtomicI32::new(0);
static HOLE_W: AtomicI32 = AtomicI32::new(0);
static HOLE_H: AtomicI32 = AtomicI32::new(0);
static SAVE_ERROR: Mutex<String> = Mutex::new(String::new());

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Waiting,
    Scrolling,
    Settling,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScrollDir {
    Up,
    Down,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    Watch,
    Stitch,
    NoMotion,
}

#[derive(Clone, Copy, Debug)]
struct Decision {
    act: Act,
    downward: bool,
    phase_changed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dot {
    Wait,
    Scroll,
    Error,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BarCmd {
    Manual,
    Auto,
    Undo,
    Done,
    Close,
    Retry,
    Restitch,
}

struct BarSlot {
    cmd: BarCmd,
    rect: RECT,
}

struct CaptureClock {
    phase: Phase,
    still: u8,
    motion_at: Option<Instant>,
    awaiting_pixels: bool,
    gesture_down: bool,
    unsettled_since: Option<Instant>,
}

struct Tick {
    bar: isize,
    inject: Option<(ScreenRect, u32)>,
    repaint: bool,
}

struct BarModel {
    title: String,
    sub: String,
    dot: Dot,
    auto: bool,
    error: bool,
    capped: bool,
    height: u32,
    hot: i32,
    down: i32,
}

impl CaptureClock {
    fn new() -> Self {
        Self {
            phase: Phase::Waiting,
            still: 0,
            motion_at: None,
            awaiting_pixels: false,
            gesture_down: false,
            unsettled_since: None,
        }
    }

    fn fail(&mut self) {
        self.phase = Phase::Error;
        self.still = 0;
        self.motion_at = None;
        self.awaiting_pixels = false;
        self.gesture_down = false;
        self.unsettled_since = None;
    }

    fn clear_error(&mut self) {
        if self.phase != Phase::Error {
            return;
        }
        self.phase = Phase::Waiting;
        self.still = 0;
        self.motion_at = None;
        self.awaiting_pixels = false;
        self.gesture_down = false;
        self.unsettled_since = None;
    }

    fn note(&mut self, changed: bool, input: Option<ScrollDir>, now: Instant) -> Decision {
        let before = self.phase;
        if let Some(dir) = input {
            if self.phase != Phase::Scrolling {
                self.unsettled_since = Some(now);
            }
            self.gesture_down = dir == ScrollDir::Down;
            self.phase = Phase::Scrolling;
            self.awaiting_pixels = true;
            self.still = 0;
            self.motion_at = Some(now);
        }
        if self.phase == Phase::Error {
            return self.decision(before, Act::Watch, false);
        }
        if changed {
            if self.unsettled_since.is_none() {
                self.unsettled_since = Some(now);
            }
            self.phase = Phase::Scrolling;
            self.awaiting_pixels = false;
            self.still = 0;
            self.motion_at = Some(now);
            return self.decision(before, Act::Watch, false);
        }
        if self.phase == Phase::Waiting && !self.awaiting_pixels {
            return self.decision(before, Act::Watch, false);
        }
        self.still = self.still.saturating_add(1);
        let elapsed = self
            .motion_at
            .map(|start| now.saturating_duration_since(start))
            .unwrap_or_default();
        if self.awaiting_pixels {
            if self.still >= STILL_NEED && elapsed >= Duration::from_millis(NO_MOTION_MS) {
                let downward = self.gesture_down;
                self.finish_gesture(Phase::Waiting);
                return self.decision(before, Act::NoMotion, downward);
            }
            return self.decision(before, Act::Watch, false);
        }
        if self.phase == Phase::Scrolling {
            if self.still >= STILL_NEED && elapsed >= Duration::from_millis(SETTLE_MS) {
                self.phase = Phase::Settling;
            }
            return self.decision(before, Act::Watch, false);
        }
        if self.phase == Phase::Settling {
            let downward = self.gesture_down;
            self.finish_gesture(Phase::Waiting);
            return self.decision(before, Act::Stitch, downward);
        }
        self.decision(before, Act::Watch, false)
    }

    fn finish_gesture(&mut self, phase: Phase) {
        self.phase = phase;
        self.still = 0;
        self.motion_at = None;
        self.awaiting_pixels = false;
        self.gesture_down = false;
        self.unsettled_since = None;
    }

    fn content_busy(&self, now: Instant) -> bool {
        self.phase == Phase::Scrolling
            && !self.awaiting_pixels
            && self.unsettled_since.is_some_and(|start| {
                now.saturating_duration_since(start) >= Duration::from_millis(2_000)
            })
    }

    fn decision(&self, before: Phase, act: Act, downward: bool) -> Decision {
        Decision {
            act,
            downward,
            phase_changed: self.phase != before,
        }
    }
}

struct Mail {
    stop: AtomicBool,
    generation: AtomicU64,
    frame: Mutex<Option<Vec<u8>>>,
    grab_hwnd: AtomicIsize,
}

struct Session {
    app: AppHandle,
    layout: LiveLayout,
    monitor: ScreenRect,
    border: isize,
    bar: isize,
    preview: isize,
    preview_hot: i32,
    preview_down: i32,
    stitcher: Stitcher,
    last: Vec<u8>,
    clock: CaptureClock,
    scroll_seen: u64,
    auto: bool,
    px_per_notch: u32,
    armed_notches: u32,
    content_busy: bool,
    bottom_hits: u32,
    bar_hot: i32,
    bar_down: i32,
    bar_scale: f32,
    capped: bool,
    crop_top: u32,
    crop_bottom: u32,
    scroll: u32,
    mode_preview: bool,
    ending: bool,
    close_session: bool,
    saving: bool,
    mail: Arc<Mail>,
    hook_mouse: isize,
    hook_key: isize,
}

pub(crate) struct LiveGrab {
    pub selection_x: i32,
    pub selection_y: i32,
    pub selection_w: u32,
    pub selection_h: u32,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_w: u32,
    pub monitor_h: u32,
}

pub(crate) fn is_active() -> bool {
    session().as_ref().is_some_and(|live| !live.ending)
}

pub(crate) fn shutdown() {
    request_end(false);
}

pub(crate) fn start(app: AppHandle, grab: LiveGrab) -> Result<(), String> {
    if session().is_some() {
        return Err("长截图还开着".into());
    }
    if super::gifshot::is_active() {
        return Err("GIF 还开着".into());
    }
    let selection = ScreenRect {
        x: grab.selection_x,
        y: grab.selection_y,
        width: grab.selection_w,
        height: grab.selection_h,
    };
    let monitor = ScreenRect {
        x: grab.monitor_x,
        y: grab.monitor_y,
        width: grab.monitor_w,
        height: grab.monitor_h,
    };
    let bar_scale = dpi_scale(selection.x.saturating_add(selection.width as i32 / 2), selection.y);
    let bar_w = (BAR_W as f32 * bar_scale).round().max(1.0) as i32;
    let bar_h = (BAR_H as f32 * bar_scale).round().max(1.0) as i32;
    let layout = live_layout_bar(selection, monitor, bar_w, bar_h).ok_or("选区太小，或贴着屏幕边")?;
    let source = choose_page_source(
        layout.capture.x,
        layout.capture.y,
        layout.capture.width as i32,
        layout.capture.height as i32,
    );
    let captured = capture_opening(
        source,
        layout.capture.x,
        layout.capture.y,
        layout.capture.width as i32,
        layout.capture.height as i32,
    )?;
    let stitcher = Stitcher::start(Frame {
        width: captured.width,
        height: captured.height,
        pixels: captured.pixels.clone(),
    })
    .ok_or("这一块没拼上")?;
    let border = create_chrome_window(
        layout.border.x,
        layout.border.y,
        layout.border.width,
        layout.border.height,
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
        false,
    )?;
    let bar = match create_chrome_window(
        layout.bar.x,
        layout.bar.y,
        layout.bar.width,
        layout.bar.height,
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        false,
    ) {
        Ok(hwnd) => hwnd,
        Err(err) => {
            unsafe {
                DestroyWindow(border);
            }
            return Err(err);
        }
    };
    if let Err(err) = punch_hole(border, &layout) {
        unsafe {
            DestroyWindow(bar);
            DestroyWindow(border);
        }
        return Err(err);
    }
    exclude_from_capture(border);
    exclude_from_capture(bar);
    round_window(bar, (12.0 * bar_scale).round().max(12.0) as i32);
    let mail = Arc::new(Mail {
        stop: AtomicBool::new(false),
        generation: AtomicU64::new(1),
        frame: Mutex::new(None),
        grab_hwnd: AtomicIsize::new(source.hwnd_bits()),
    });
    let hooks = install_hooks();
    remember_hole(&layout.capture);
    HOOK_BAR.store(bar as isize, Ordering::SeqCst);
    CAPTURE_MS.store(200, Ordering::SeqCst);
    let scroll_seen = SCROLL_SEQ.load(Ordering::SeqCst);
    {
        let mut guard = session();
        *guard = Some(Session {
            app,
            layout,
            monitor,
            border: border as isize,
            bar: bar as isize,
            preview: 0,
            preview_hot: -1,
            preview_down: -1,
            stitcher,
            last: captured.pixels,
            clock: CaptureClock::new(),
            scroll_seen,
            auto: false,
            px_per_notch: 0,
            armed_notches: 0,
            content_busy: false,
            bottom_hits: 0,
            bar_hot: -1,
            bar_down: -1,
            bar_scale,
            capped: false,
            crop_top: 0,
            crop_bottom: 0,
            scroll: 0,
            mode_preview: false,
            ending: false,
            close_session: false,
            saving: false,
            mail: Arc::clone(&mail),
            hook_mouse: hooks.0,
            hook_key: hooks.1,
        });
    }
    show_top(border);
    show_top(bar);
    spawn_capture(
        mail,
        session().as_ref().expect("长截图刚打开").layout.capture,
        bar as isize,
    );
    crate::logging::info(format_args!(
        "longshot started {}x{}",
        grab.selection_w, grab.selection_h
    ));
    Ok(())
}

fn request_end(close_session: bool) {
    let hwnds = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.saving {
            return;
        }
        if live.ending {
            live.close_session |= close_session;
            return;
        }
        live.ending = true;
        live.close_session = close_session;
        live.mail.stop.store(true, Ordering::SeqCst);
        HOOK_BAR.store(0, Ordering::SeqCst);
        [live.border, live.bar, live.preview]
    };
    for hwnd in hwnds {
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
    }
}

fn on_frame() {
    let pixels = {
        let guard = session();
        let Some(live) = guard.as_ref() else {
            return;
        };
        if live.ending || live.mode_preview {
            return;
        }
        let mut slot = live
            .mail
            .frame
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        slot.take()
    };
    let Some(pixels) = pixels else {
        return;
    };
    let tick = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.ending || live.mode_preview || live.capped {
            return;
        }
        let width = live.layout.capture.width;
        let height = live.layout.capture.height;
        if pixels.len() != width as usize * height as usize * 4 {
            return;
        }
        on_captured(live, width, height, pixels)
    };
    if let Some((hole, notches)) = tick.inject {
        inject_wheel(hole, notches);
    }
    if tick.repaint {
        repaint(tick.bar);
    }
}

fn on_captured(live: &mut Session, width: u32, height: u32, pixels: Vec<u8>) -> Tick {
    let now = Instant::now();
    let input = poll_scroll(&mut live.scroll_seen);
    let changed = sample_sad(width, height, &live.last, &pixels).unwrap_or(255) > MOTION_SAD;
    let decision = live.clock.note(changed, input, now);
    live.last = pixels;
    let busy = live.clock.content_busy(now);
    let busy_changed = busy != live.content_busy;
    live.content_busy = busy;
    let mut tick = match decision.act {
        Act::Watch => Tick {
            bar: live.bar,
            inject: None,
            repaint: decision.phase_changed,
        },
        Act::NoMotion => on_no_motion(live, decision.downward),
        Act::Stitch => on_stitch(live, width, height, decision.downward),
    };
    if busy_changed {
        tick.repaint = true;
    }
    set_pace(live);
    tick
}

fn on_no_motion(live: &mut Session, downward: bool) -> Tick {
    if downward {
        live.bottom_hits = live.bottom_hits.saturating_add(1);
    } else {
        live.bottom_hits = 0;
    }
    if live.bottom_hits >= 2 {
        live.auto = false;
    } else if live.auto {
        live.armed_notches = 0;
        return arm_scroll(live);
    }
    Tick {
        bar: live.bar,
        inject: None,
        repaint: true,
    }
}

fn on_stitch(live: &mut Session, width: u32, height: u32, downward: bool) -> Tick {
    let frame = Frame {
        width,
        height,
        pixels: live.last.clone(),
    };
    let pushed = if live.auto && live.px_per_notch > 0 && live.armed_notches > 0 {
        let expected = live.px_per_notch.saturating_mul(live.armed_notches);
        let slack = (expected / 5).max(12);
        live.stitcher.push_near(frame, expected, slack)
    } else {
        live.stitcher.push_step(frame)
    };
    match pushed {
        Push::Added { .. } => {
            let scrolled = live.stitcher.last_scroll();
            if live.armed_notches > 0 && scrolled > 0 {
                let measured = (scrolled / live.armed_notches).max(1);
                live.px_per_notch = if live.px_per_notch == 0 {
                    measured
                } else {
                    live.px_per_notch.saturating_mul(3).saturating_add(measured) / 4
                };
                crate::logging::info(format_args!(
                    "longshot notch {}px over {} notches",
                    live.px_per_notch, live.armed_notches
                ));
            }
            live.armed_notches = 0;
            live.bottom_hits = 0;
            if live.auto {
                arm_scroll(live)
            } else {
                Tick {
                    bar: live.bar,
                    inject: None,
                    repaint: true,
                }
            }
        }
        Push::Short => {
            if live.auto {
                arm_scroll(live)
            } else {
                Tick {
                    bar: live.bar,
                    inject: None,
                    repaint: false,
                }
            }
        }
        Push::Still => {
            if downward || live.auto {
                live.bottom_hits = live.bottom_hits.saturating_add(1);
            }
            if live.bottom_hits >= 2 {
                live.auto = false;
                live.armed_notches = 0;
                Tick {
                    bar: live.bar,
                    inject: None,
                    repaint: true,
                }
            } else if live.auto {
                live.armed_notches = 0;
                arm_scroll(live)
            } else {
                Tick {
                    bar: live.bar,
                    inject: None,
                    repaint: false,
                }
            }
        }
        Push::Weak => {
            live.clock.fail();
            live.auto = false;
            live.armed_notches = 0;
            crate::logging::info(format_args!("longshot seam rejected"));
            Tick {
                bar: live.bar,
                inject: None,
                repaint: true,
            }
        }
        Push::Limit => {
            live.capped = true;
            live.auto = false;
            Tick {
                bar: live.bar,
                inject: None,
                repaint: true,
            }
        }
    }
}

fn arm_scroll(live: &mut Session) -> Tick {
    if live.capped || !live.auto || live.clock.phase == Phase::Error {
        return Tick {
            bar: live.bar,
            inject: None,
            repaint: true,
        };
    }
    let height = live.layout.capture.height.max(1);
    let notches = if live.px_per_notch == 0 {
        1
    } else {
        let target = height * 55 / 100;
        target.div_ceil(live.px_per_notch).clamp(1, 3)
    };
    live.armed_notches = live.armed_notches.saturating_add(notches).min(6);
    Tick {
        bar: live.bar,
        inject: Some((live.layout.capture, notches)),
        repaint: true,
    }
}

fn toggle_auto() {
    let tick = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.ending || live.mode_preview {
            return;
        }
        if live.auto {
            live.auto = false;
            set_pace(live);
            Some(Tick {
                bar: live.bar,
                inject: None,
                repaint: true,
            })
        } else if live.capped || live.clock.phase == Phase::Error {
            None
        } else {
            live.auto = true;
            live.bottom_hits = 0;
            set_pace(live);
            crate::logging::info(format_args!("longshot auto"));
            Some(arm_scroll(live))
        }
    };
    let Some(tick) = tick else {
        return;
    };
    apply_tick(tick);
}

fn ensure_manual() {
    let tick = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if !live.auto || live.ending || live.mode_preview {
            return;
        }
        live.auto = false;
        set_pace(live);
        Tick {
            bar: live.bar,
            inject: None,
            repaint: true,
        }
    };
    apply_tick(tick);
}

fn retry_seam() {
    let tick = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.ending || live.mode_preview || live.clock.phase != Phase::Error {
            return;
        }
        let width = live.layout.capture.width;
        let height = live.layout.capture.height;
        live.clock.clear_error();
        let tick = on_stitch(live, width, height, false);
        set_pace(live);
        tick
    };
    apply_tick(tick);
}

fn apply_tick(tick: Tick) {
    if let Some((hole, notches)) = tick.inject {
        inject_wheel(hole, notches);
    }
    if tick.repaint {
        repaint(tick.bar);
    }
}

fn poll_scroll(seen: &mut u64) -> Option<ScrollDir> {
    let seq = SCROLL_SEQ.load(Ordering::SeqCst);
    if seq == *seen {
        return None;
    }
    *seen = seq;
    if SCROLL_DIR.load(Ordering::SeqCst) < 0 {
        Some(ScrollDir::Down)
    } else {
        Some(ScrollDir::Up)
    }
}

fn mark_scroll(down: bool) {
    SCROLL_DIR.store(if down { -1 } else { 1 }, Ordering::SeqCst);
    SCROLL_SEQ.fetch_add(1, Ordering::SeqCst);
}

fn set_pace(live: &Session) {
    let busy = live.auto || matches!(live.clock.phase, Phase::Scrolling | Phase::Settling);
    CAPTURE_MS.store(if busy { 100 } else { 200 }, Ordering::SeqCst);
}

fn inject_wheel(hole: ScreenRect, notches: u32) {
    if notches == 0 || hole.width < 32 || hole.height < 32 {
        return;
    }
    mark_scroll(true);
    let notches = notches.min(6);
    unsafe {
        let mut saved = POINT { x: 0, y: 0 };
        let had = GetCursorPos(&mut saved) != 0;
        let inside = had && point_in_hole(saved, hole);
        let cx = hole.x + hole.width as i32 / 2;
        let cy = hole.y + hole.height as i32 / 2;
        if !inside {
            SetCursorPos(cx, cy);
        }
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: (notches as i32)
                .wrapping_neg()
                .wrapping_mul(WHEEL_DELTA as i32) as u32,
            dwFlags: MOUSEEVENTF_WHEEL,
            time: 0,
            dwExtraInfo: 0,
        };
        let sent = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
        if !inside && had {
            SetCursorPos(saved.x, saved.y);
        }
        if sent == 0 {
            crate::logging::warn(format_args!("长截图滚轮没有发出"));
        }
    }
}

fn point_in_hole(point: POINT, hole: ScreenRect) -> bool {
    point.x >= hole.x
        && point.y >= hole.y
        && point.x < hole.x.saturating_add(hole.width as i32)
        && point.y < hole.y.saturating_add(hole.height as i32)
}

fn undo() {
    let bar = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.mode_preview || !live.stitcher.undo() {
            return;
        }
        live.capped = false;
        live.bottom_hits = 0;
        live.clock.clear_error();
        set_pace(live);
        live.bar
    };
    repaint(bar);
}

fn finish() {
    let (monitor, border, bar, existing) = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.ending || live.mode_preview {
            return;
        }
        live.auto = false;
        set_pace(live);
        live.mail.generation.fetch_add(1, Ordering::SeqCst);
        live.mode_preview = true;
        (live.monitor, live.border, live.bar, live.preview)
    };
    let preview = if existing != 0 {
        existing
    } else {
        match create_preview(monitor) {
            Ok(created) => created,
            Err(err) => {
                resume_live();
                tell(bar, &err);
                return;
            }
        }
    };
    let title = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            drop(guard);
            unsafe {
                DestroyWindow(preview as HWND);
            }
            return;
        };
        if live.preview == 0 {
            live.preview = preview;
        }
        title_text(live)
    };
    let wide = utf16(&title);
    unsafe {
        SetWindowTextW(preview as HWND, wide.as_ptr());
        ShowWindow(border as HWND, SW_HIDE);
        ShowWindow(bar as HWND, SW_HIDE);
        show_top(preview as HWND);
    }
    repaint(preview);
}

fn resume_live() {
    let mut guard = session();
    if let Some(live) = guard.as_mut() {
        live.mode_preview = false;
        live.mail.generation.fetch_add(1, Ordering::SeqCst);
        let mail = Arc::clone(&live.mail);
        let hole = live.layout.capture;
        let bar = live.bar;
        drop(guard);
        spawn_capture(mail, hole, bar);
    }
}

fn restart() {
    let (hole, mail, border, bar, preview) = {
        let guard = session();
        let Some(live) = guard.as_ref() else {
            return;
        };
        if live.ending {
            return;
        }
        (
            live.layout.capture,
            Arc::clone(&live.mail),
            live.border,
            live.bar,
            live.preview,
        )
    };
    mail.generation.fetch_add(1, Ordering::SeqCst);
    let source = page_source_from_bits(mail.grab_hwnd.load(Ordering::SeqCst));
    let captured = match capture_opening(source, hole.x, hole.y, hole.width as i32, hole.height as i32)
    {
        Ok(image) => image,
        Err(err) => {
            tell(preview, &err);
            return;
        }
    };
    let Some(stitcher) = Stitcher::start(Frame {
        width: captured.width,
        height: captured.height,
        pixels: captured.pixels.clone(),
    }) else {
        tell(preview, "这一块没拼上");
        return;
    };
    {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        live.stitcher = stitcher;
        live.last = captured.pixels;
        live.clock = CaptureClock::new();
        live.scroll_seen = SCROLL_SEQ.load(Ordering::SeqCst);
        live.auto = false;
        live.px_per_notch = 0;
        live.armed_notches = 0;
        live.content_busy = false;
        live.bottom_hits = 0;
        live.bar_hot = -1;
        live.bar_down = -1;
        live.capped = false;
        live.crop_top = 0;
        live.crop_bottom = 0;
        live.scroll = 0;
        live.mode_preview = false;
        set_pace(live);
    }
    unsafe {
        if preview != 0 {
            ShowWindow(preview as HWND, SW_HIDE);
        }
        ShowWindow(border as HWND, SW_SHOWNOACTIVATE);
        ShowWindow(bar as HWND, SW_SHOWNOACTIVATE);
    }
    repaint(bar);
    spawn_capture(mail, hole, bar);
}

fn copy_result() {
    let exported = {
        let guard = session();
        let Some(live) = guard.as_ref() else {
            return;
        };
        export(live)
    };
    let Some((width, height, pixels)) = exported else {
        return;
    };
    match super::copy_bgra(width, height, &pixels) {
        Ok(()) => request_end(true),
        Err(err) => tell(0, &err),
    }
}

fn save_result() {
    let (app, preview, exported) = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.saving {
            return;
        }
        let Some(exported) = export(live) else {
            return;
        };
        live.saving = true;
        (live.app.clone(), live.preview, exported)
    };
    unsafe {
        ShowWindow(preview as HWND, SW_HIDE);
    }
    let name = super::save::long_screenshot_name();
    let spawned = thread::Builder::new()
        .name("longshot-save".into())
        .spawn(move || {
            let (width, height, pixels) = exported;
            let result = super::png_bytes_from_bgra(width, height, &pixels)
                .and_then(|bytes| super::save::save_png_dialog(&app, &bytes, &name));
            let code = match result {
                Ok(()) => 0,
                Err(err) => {
                    *SAVE_ERROR.lock().unwrap_or_else(|err| err.into_inner()) = err;
                    1
                }
            };
            unsafe {
                PostMessageW(preview as HWND, WM_APP_SAVED, code, 0);
            }
        });
    if spawned.is_err() {
        if let Some(live) = session().as_mut() {
            live.saving = false;
        }
        unsafe {
            ShowWindow(preview as HWND, SW_SHOW);
        }
        tell(preview, "保存没有启动");
    }
}

fn on_saved(code: usize) {
    let (preview, message) = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        live.saving = false;
        let message = (code == 1).then(|| {
            std::mem::take(&mut *SAVE_ERROR.lock().unwrap_or_else(|err| err.into_inner()))
        });
        (live.preview, message)
    };
    if preview != 0 {
        unsafe {
            ShowWindow(preview as HWND, SW_SHOW);
        }
    }
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        tell(preview, &message);
    }
}

fn crop(top: bool) {
    let (preview, title) = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        let height = live.stitcher.image().height;
        let room = height.saturating_sub(live.crop_top + live.crop_bottom + 32);
        let step = room.min(48);
        if step == 0 {
            return;
        }
        if top {
            live.crop_top += step;
        } else {
            live.crop_bottom += step;
        }
        live.scroll = 0;
        (live.preview, title_text(live))
    };
    let wide = utf16(&title);
    unsafe {
        SetWindowTextW(preview as HWND, wide.as_ptr());
    }
    repaint(preview);
}

fn export(live: &Session) -> Option<(u32, u32, Vec<u8>)> {
    let cropped = crop_vertical(live.stitcher.image(), live.crop_top, live.crop_bottom)?;
    Some((cropped.width, cropped.height, cropped.pixels))
}

fn title_text(live: &Session) -> String {
    let image = live.stitcher.image();
    let height = image
        .height
        .saturating_sub(live.crop_top + live.crop_bottom);
    format!(
        "长截图 {}×{height} · {} 段",
        image.width,
        live.stitcher.frames()
    )
}

fn spawn_capture(mail: Arc<Mail>, hole: ScreenRect, bar: isize) {
    let generation = mail.generation.load(Ordering::SeqCst);
    if thread::Builder::new()
        .name("longshot".into())
        .spawn(move || capture_loop(mail, generation, hole, bar))
        .is_err()
    {
        crate::logging::warn(format_args!("长截图抓帧线程没有启动"));
    }
}

fn capture_loop(mail: Arc<Mail>, generation: u64, hole: ScreenRect, bar: isize) {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    loop {
        if mail.stop.load(Ordering::SeqCst) || mail.generation.load(Ordering::SeqCst) != generation
        {
            break;
        }
        let wait = CAPTURE_MS.load(Ordering::SeqCst).clamp(50, 500);
        thread::sleep(Duration::from_millis(wait));
        if mail.stop.load(Ordering::SeqCst) || mail.generation.load(Ordering::SeqCst) != generation
        {
            break;
        }
        if HOOK_BAR.load(Ordering::SeqCst) == 0 && escape_down() {
            unsafe {
                PostMessageW(bar as HWND, WM_APP_CANCEL, 0, 0);
            }
            break;
        }
        let source = page_source_from_bits(mail.grab_hwnd.load(Ordering::SeqCst));
        match grab_page(source, hole.x, hole.y, hole.width as i32, hole.height as i32) {
            Ok(image) => {
                if mail.stop.load(Ordering::SeqCst)
                    || mail.generation.load(Ordering::SeqCst) != generation
                {
                    break;
                }
                *mail.frame.lock().unwrap_or_else(|err| err.into_inner()) = Some(image.pixels);
                unsafe {
                    PostMessageW(bar as HWND, WM_APP_FRAME, 0, 0);
                }
            }
            Err(err) => {
                crate::logging::warn(format_args!("长截图抓帧失败: {err}"));
            }
        }
    }
}

fn escape_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_ESCAPE as i32) as u16) & 0x8000 != 0 }
}

fn create_preview(monitor: ScreenRect) -> Result<isize, String> {
    let width = (monitor.width as i32 - 48).clamp(460, 1100);
    let height = (monitor.height as i32 - 80).clamp(360, 860);
    let x = monitor.x + (monitor.width as i32 - width) / 2;
    let y = monitor.y + (monitor.height as i32 - height) / 2;
    let hwnd = create_chrome_window(
        x,
        y,
        width as u32,
        height as u32,
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        true,
    )?;
    Ok(hwnd as isize)
}

fn create_chrome_window(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    ex_style: u32,
    clip_children: bool,
) -> Result<HWND, String> {
    ensure_class()?;
    let mut style = WS_POPUP;
    if clip_children {
        style |= WS_CLIPCHILDREN;
    }
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            CLASS_NAME.as_ptr(),
            CLASS_NAME.as_ptr(),
            style,
            x,
            y,
            width as i32,
            height as i32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance(),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        Err("长截图窗口打不开".into())
    } else {
        Ok(hwnd)
    }
}

fn punch_hole(border: HWND, layout: &LiveLayout) -> Result<(), String> {
    unsafe {
        let outer = CreateRectRgn(
            0,
            0,
            layout.border.width as i32,
            layout.border.height as i32,
        );
        let hx = (layout.capture.x - layout.border.x).max(0);
        let hy = (layout.capture.y - layout.border.y).max(0);
        let inner = CreateRectRgn(
            hx,
            hy,
            hx + layout.capture.width as i32,
            hy + layout.capture.height as i32,
        );
        let diff = CreateRectRgn(0, 0, 1, 1);
        let combined = CombineRgn(diff, outer, inner, RGN_DIFF);
        DeleteObject(outer as _);
        DeleteObject(inner as _);
        if combined == 0 || SetWindowRgn(border, diff, 1) == 0 {
            DeleteObject(diff as _);
            return Err("长截图边框打不开".into());
        }
        Ok(())
    }
}

fn exclude_from_capture(hwnd: HWND) {
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }
}

fn round_window(hwnd: HWND, radius: i32) {
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetClientRect(hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width < 8 || height < 8 {
            return;
        }
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
        if region.is_null() {
            return;
        }
        if SetWindowRgn(hwnd, region, 0) == 0 {
            DeleteObject(region as _);
        }
    }
}

fn show_top(hwnd: HWND) {
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOSIZE | SWP_NOMOVE,
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

fn paint(hwnd: HWND) {
    let kind = {
        let guard = session();
        guard.as_ref().map(|live| {
            if live.border == hwnd as isize {
                1
            } else if live.bar == hwnd as isize {
                2
            } else if live.preview == hwnd as isize {
                3
            } else {
                0
            }
        })
    };
    match kind.unwrap_or(0) {
        1 => paint_border(hwnd),
        2 => paint_bar(hwnd),
        3 => paint_preview(hwnd),
        _ => {}
    }
}

fn paint_border(hwnd: HWND) {
    unsafe {
        let mut paint = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut paint);
        fill_client(hwnd, hdc, rgb(121, 198, 175));
        let (width, height) = client_size(hwnd);
        let pen = CreatePen(PS_SOLID, 1, rgb(255, 255, 255));
        let old_pen = SelectObject(hdc, pen as _);
        let old_brush = SelectObject(hdc, GetStockObject(HOLLOW_BRUSH));
        Rectangle(hdc, 0, 0, width, height);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(pen as _);
        EndPaint(hwnd, &paint);
    }
}

fn paint_bar(hwnd: HWND) {
    let model = session().as_ref().map(bar_model);
    let scale = session()
        .as_ref()
        .map(|live| live.bar_scale)
        .unwrap_or(1.0);
    unsafe {
        let mut paint = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut paint);
        if let Some(model) = model {
            let (width, height) = client_size(hwnd);
            let mem = CreateCompatibleDC(hdc);
            let bitmap = CreateCompatibleBitmap(hdc, width.max(1), height.max(1));
            if !mem.is_null() && !bitmap.is_null() {
                let old = SelectObject(mem, bitmap as _);
                scale_bar_dc(mem, scale);
                paint_bar_dc(mem, &model);
                reset_bar_dc(mem);
                BitBlt(hdc, 0, 0, width, height, mem, 0, 0, SRCCOPY);
                SelectObject(mem, old);
            } else {
                scale_bar_dc(hdc, scale);
                paint_bar_dc(hdc, &model);
            }
            if !bitmap.is_null() {
                DeleteObject(bitmap as _);
            }
            if !mem.is_null() {
                DeleteDC(mem);
            }
        }
        EndPaint(hwnd, &paint);
    }
}

fn bar_model(live: &Session) -> BarModel {
    let frames = live.stitcher.frames();
    let height = live.stitcher.image().height;
    let error = live.clock.phase == Phase::Error;
    let scrolling = matches!(live.clock.phase, Phase::Scrolling | Phase::Settling);
    let bottom = live.bottom_hits >= 2 && !error && !scrolling;
    let (title, sub, dot) = if live.capped {
        (
            format!("到 {MAX_STITCH_FRAMES} 段上限了"),
            "先完成或重拼".to_string(),
            Dot::Error,
        )
    } else if error {
        (
            "这段没接上".to_string(),
            "往回滚一点".to_string(),
            Dot::Error,
        )
    } else if bottom && frames <= 1 {
        (
            "这里滚不动".to_string(),
            "换个页面再试".to_string(),
            Dot::Error,
        )
    } else if bottom {
        ("已经到底".to_string(), "点完成收尾".to_string(), Dot::Done)
    } else if live.auto {
        let sub = if live.px_per_notch == 0 {
            "先量一下这一页滚多远".to_string()
        } else {
            format!("每格约 {}px · 按半屏来滚", live.px_per_notch)
        };
        (
            format!("自动滚动中 · 已接 {frames} 段"),
            sub,
            Dot::Scroll,
        )
    } else if live.content_busy {
        (
            format!("正在接第 {} 段…", frames + 1),
            "内容在动，建议暂停动画".to_string(),
            Dot::Scroll,
        )
    } else if scrolling {
        (
            format!("正在接第 {} 段…", frames + 1),
            "停稳后自动接上".to_string(),
            Dot::Scroll,
        )
    } else {
        (
            format!("已接 {frames} 段 · {height}px"),
            "滚动页面，停一下自动接上".to_string(),
            Dot::Wait,
        )
    };
    BarModel {
        title,
        sub,
        dot,
        auto: live.auto,
        error,
        capped: live.capped,
        height,
        hot: live.bar_hot,
        down: live.bar_down,
    }
}

fn bar_slots(live: &Session) -> Vec<BarSlot> {
    bar_layout(live.clock.phase == Phase::Error, live.capped)
}

fn bar_layout(error: bool, capped: bool) -> Vec<BarSlot> {
    let top = 6i32;
    let height = 26i32;
    let gap = 6i32;
    let mut edge = BAR_W - 8;
    let mut slots = Vec::new();
    let mut push = |cmd: BarCmd, width: i32| {
        let rect = RECT {
            left: edge - width,
            top,
            right: edge,
            bottom: top + height,
        };
        edge = rect.left - gap;
        slots.push(BarSlot { cmd, rect });
    };
    push(BarCmd::Close, 28);
    push(BarCmd::Done, 52);
    push(BarCmd::Undo, 28);
    if error {
        push(BarCmd::Retry, 48);
    }
    if capped {
        push(BarCmd::Restitch, 48);
    }
    let seg_right = edge;
    let seg_left = seg_right - 96;
    let mid = seg_left + 48;
    slots.push(BarSlot {
        cmd: BarCmd::Auto,
        rect: RECT {
            left: mid,
            top,
            right: seg_right,
            bottom: top + height,
        },
    });
    slots.push(BarSlot {
        cmd: BarCmd::Manual,
        rect: RECT {
            left: seg_left,
            top,
            right: mid,
            bottom: top + height,
        },
    });
    slots
}

fn paint_bar_dc(hdc: HDC, model: &BarModel) {
    fill_round(
        hdc,
        RECT {
            left: 0,
            top: 0,
            right: BAR_W,
            bottom: BAR_H,
        },
        24,
        rgb(12, 31, 26),
    );
    stroke_round(
        hdc,
        RECT {
            left: 1,
            top: 1,
            right: BAR_W - 1,
            bottom: BAR_H - 1,
        },
        20,
        rgb(40, 58, 52),
    );
    let slots = bar_layout(model.error, model.capped);
    let text_right = slots
        .iter()
        .map(|slot| slot.rect.left)
        .min()
        .unwrap_or(BAR_W)
        - 8;
    let dot = match model.dot {
        Dot::Wait => rgb(130, 148, 140),
        Dot::Scroll | Dot::Done => rgb(121, 198, 175),
        Dot::Error => rgb(233, 100, 107),
    };
    fill_ellipse(hdc, 12, 9, 8, dot);
    if text_right > 28 {
        let mut title = RECT {
            left: 26,
            top: 2,
            right: text_right,
            bottom: 22,
        };
        let mut sub = RECT {
            left: 26,
            top: 21,
            right: text_right,
            bottom: 40,
        };
        draw_line(
            hdc,
            &model.title,
            &mut title,
            title_font(),
            rgb(233, 241, 236),
            false,
        );
        draw_line(
            hdc,
            &model.sub,
            &mut sub,
            sub_font(),
            rgb(149, 161, 156),
            false,
        );
    }
    paint_segment(hdc, &slots, model.auto);
    for (index, slot) in slots.iter().enumerate() {
        paint_slot(hdc, slot, index as i32, model);
    }
    paint_progress(hdc, model.height, model.dot == Dot::Error);
}

fn paint_segment(hdc: HDC, slots: &[BarSlot], auto: bool) {
    let Some(manual) = slots.iter().find(|slot| slot.cmd == BarCmd::Manual) else {
        return;
    };
    let Some(auto_slot) = slots.iter().find(|slot| slot.cmd == BarCmd::Auto) else {
        return;
    };
    fill_round(
        hdc,
        RECT {
            left: manual.rect.left,
            top: manual.rect.top,
            right: auto_slot.rect.right,
            bottom: manual.rect.bottom,
        },
        16,
        rgb(23, 48, 42),
    );
    let active = if auto { auto_slot.rect } else { manual.rect };
    fill_round(hdc, inset_rect(active, 2), 12, rgb(121, 198, 175));
}

fn paint_slot(hdc: HDC, slot: &BarSlot, index: i32, model: &BarModel) {
    let hot = model.hot == index;
    let pressed = model.down == index;
    match slot.cmd {
        BarCmd::Manual | BarCmd::Auto => {
            let active = (slot.cmd == BarCmd::Auto && model.auto)
                || (slot.cmd == BarCmd::Manual && !model.auto);
            let label = match slot.cmd {
                BarCmd::Manual => "手动",
                BarCmd::Auto if model.auto => "停止",
                _ => "自动",
            };
            let color = if active {
                rgb(12, 31, 26)
            } else {
                rgb(233, 241, 236)
            };
            let mut bounds = slot.rect;
            draw_line(hdc, label, &mut bounds, title_font(), color, true);
        }
        BarCmd::Done => {
            let fill = if pressed {
                rgb(210, 222, 216)
            } else {
                rgb(233, 241, 236)
            };
            fill_round(hdc, slot.rect, 12, fill);
            let mut bounds = slot.rect;
            draw_line(
                hdc,
                "完成",
                &mut bounds,
                title_font(),
                rgb(12, 31, 26),
                true,
            );
        }
        BarCmd::Undo | BarCmd::Close => {
            if pressed {
                fill_round(hdc, slot.rect, 10, rgb(36, 68, 60));
            } else if hot {
                fill_round(hdc, slot.rect, 10, rgb(28, 54, 48));
            }
            draw_icon(hdc, slot.cmd, slot.rect, rgb(233, 241, 236));
        }
        BarCmd::Retry | BarCmd::Restitch => {
            if pressed {
                fill_round(hdc, slot.rect, 10, rgb(36, 68, 60));
            } else if hot {
                fill_round(hdc, slot.rect, 10, rgb(28, 54, 48));
            }
            let label = if slot.cmd == BarCmd::Retry {
                "重试"
            } else {
                "重拼"
            };
            let mut bounds = slot.rect;
            draw_line(
                hdc,
                label,
                &mut bounds,
                title_font(),
                rgb(233, 241, 236),
                true,
            );
        }
    }
}

fn paint_progress(hdc: HDC, height: u32, alert: bool) {
    let track = RECT {
        left: 14,
        top: BAR_H - 3,
        right: BAR_W - 14,
        bottom: BAR_H - 1,
    };
    fill_rect_color(hdc, track, rgb(23, 48, 42));
    let span = (track.right - track.left) as u64;
    let width = (span * u64::from(height) / u64::from(MAX_STITCH_HEIGHT)).min(span) as i32;
    if width > 0 {
        let color = if alert {
            rgb(233, 100, 107)
        } else {
            rgb(121, 198, 175)
        };
        fill_rect_color(
            hdc,
            RECT {
                left: track.left,
                top: track.top,
                right: track.left + width,
                bottom: track.bottom,
            },
            color,
        );
    }
}

fn draw_line(hdc: HDC, text: &str, rect: &mut RECT, font: isize, color: COLORREF, center: bool) {
    unsafe {
        let old = SelectObject(hdc, font as _);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, color);
        let wide = utf16(text);
        let align = if center { DT_CENTER } else { DT_LEFT };
        DrawTextW(
            hdc,
            wide.as_ptr(),
            -1,
            rect,
            align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old);
    }
}

fn draw_icon(hdc: HDC, cmd: BarCmd, rect: RECT, color: COLORREF) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    unsafe {
        let pen = CreatePen(PS_SOLID, 2, color);
        let old = SelectObject(hdc, pen as _);
        match cmd {
            BarCmd::Close => {
                stroke(hdc, cx - 4, cy - 4, cx + 4, cy + 4);
                stroke(hdc, cx + 4, cy - 4, cx - 4, cy + 4);
            }
            BarCmd::Undo => {
                stroke(hdc, cx + 5, cy, cx - 3, cy);
                stroke(hdc, cx - 3, cy, cx, cy - 3);
                stroke(hdc, cx - 3, cy, cx, cy + 3);
            }
            _ => {}
        }
        SelectObject(hdc, old);
        DeleteObject(pen as _);
    }
}

fn stroke(hdc: HDC, x0: i32, y0: i32, x1: i32, y1: i32) {
    unsafe {
        MoveToEx(hdc, x0, y0, std::ptr::null_mut());
        LineTo(hdc, x1, y1);
    }
}

fn stroke_round(hdc: HDC, rect: RECT, radius: i32, color: COLORREF) {
    unsafe {
        let pen = CreatePen(PS_SOLID, 1, color);
        let old_pen = SelectObject(hdc, pen as _);
        let old_brush = SelectObject(hdc, GetStockObject(HOLLOW_BRUSH));
        RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius,
            radius,
        );
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(pen as _);
    }
}

fn fill_ellipse(hdc: HDC, x: i32, y: i32, diameter: i32, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush as _);
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        Ellipse(hdc, x, y, x + diameter, y + diameter);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(brush as _);
    }
}

fn fill_rect_color(hdc: HDC, rect: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &rect, brush);
        DeleteObject(brush as _);
    }
}

fn inset_rect(rect: RECT, pad: i32) -> RECT {
    RECT {
        left: rect.left + pad,
        top: rect.top + pad,
        right: rect.right - pad,
        bottom: rect.bottom - pad,
    }
}

fn title_font() -> isize {
    cached_font(true)
}

fn sub_font() -> isize {
    cached_font(false)
}

fn cached_font(title: bool) -> isize {
    static TITLE: OnceLock<isize> = OnceLock::new();
    static SUB: OnceLock<isize> = OnceLock::new();
    let slot = if title { &TITLE } else { &SUB };
    *slot.get_or_init(|| {
        make_font(
            if title { -15 } else { -13 },
            if title {
                FW_SEMIBOLD as i32
            } else {
                FW_NORMAL as i32
            },
        )
    })
}

fn make_font(size: i32, weight: i32) -> isize {
    let face = utf16("Microsoft YaHei UI");
    let font = unsafe {
        CreateFontW(
            size,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_DONTCARE) as u32,
            face.as_ptr(),
        )
    };
    if font.is_null() {
        unsafe { GetStockObject(DEFAULT_GUI_FONT) as isize }
    } else {
        font as isize
    }
}

fn scale_bar_dc(hdc: HDC, scale: f32) {
    if scale <= 1.01 {
        return;
    }
    unsafe {
        SetGraphicsMode(hdc, GM_ADVANCED as i32);
        let _ = SetWorldTransform(hdc, &bar_transform(scale));
    }
}

fn reset_bar_dc(hdc: HDC) {
    unsafe {
        let _ = SetWorldTransform(hdc, &bar_transform(1.0));
    }
}

fn bar_transform(scale: f32) -> XFORM {
    XFORM {
        eM11: scale,
        eM12: 0.0,
        eM21: 0.0,
        eM22: scale,
        eDx: 0.0,
        eDy: 0.0,
    }
}

fn bar_local(live: &Session, x: i32, y: i32) -> (i32, i32) {
    let scale = live.bar_scale.max(1.0);
    (
        (x as f32 / scale).round() as i32,
        (y as f32 / scale).round() as i32,
    )
}

fn dpi_scale(x: i32, y: i32) -> f32 {
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
        SetThreadDpiAwarenessContext,
    };

    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        let status = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        if !previous.is_null() {
            SetThreadDpiAwarenessContext(previous);
        }
        if status != 0 {
            return 1.0;
        }
        (dpi_x.max(dpi_y).max(96) as f32 / 96.0).clamp(1.0, 3.0)
    }
}

fn bar_move(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = point_lparam(lparam);
    let changed = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.bar != hwnd as isize {
            return;
        }
        let (x, y) = bar_local(live, x, y);
        let hit = slot_at(&bar_slots(live), x, y);
        if live.bar_hot == hit {
            false
        } else {
            live.bar_hot = hit;
            true
        }
    };
    track_leave(hwnd);
    if changed {
        repaint(hwnd as isize);
    }
}

fn bar_down(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = point_lparam(lparam);
    let armed = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.bar != hwnd as isize {
            return;
        }
        let (x, y) = bar_local(live, x, y);
        let hit = slot_at(&bar_slots(live), x, y);
        if hit < 0 {
            false
        } else {
            live.bar_hot = hit;
            live.bar_down = hit;
            true
        }
    };
    if !armed {
        return;
    }
    unsafe {
        SetCapture(hwnd);
    }
    repaint(hwnd as isize);
}

fn bar_up(hwnd: HWND, lparam: LPARAM) -> Option<BarCmd> {
    let pressed = {
        let mut guard = session();
        let live = guard.as_mut()?;
        if live.bar != hwnd as isize {
            return None;
        }
        let pressed = live.bar_down;
        live.bar_down = -1;
        pressed
    };
    unsafe {
        ReleaseCapture();
    }
    let (x, y) = point_lparam(lparam);
    let cmd = {
        let mut guard = session();
        let live = guard.as_mut()?;
        let (x, y) = bar_local(live, x, y);
        let slots = bar_slots(live);
        let hit = slot_at(&slots, x, y);
        live.bar_hot = hit;
        if pressed >= 0 && pressed == hit {
            slots.get(pressed as usize).map(|slot| slot.cmd)
        } else {
            None
        }
    };
    repaint(hwnd as isize);
    cmd
}

fn bar_leave(hwnd: HWND) {
    let pressed = session().as_mut().is_some_and(|live| {
        if live.bar != hwnd as isize || live.bar_down >= 0 {
            return live.bar == hwnd as isize && live.bar_down >= 0;
        }
        live.bar_hot = -1;
        false
    });
    if !pressed {
        repaint(hwnd as isize);
    }
}

fn dispatch_bar(cmd: BarCmd) {
    match cmd {
        BarCmd::Manual => ensure_manual(),
        BarCmd::Auto => toggle_auto(),
        BarCmd::Undo => undo(),
        BarCmd::Done => finish(),
        BarCmd::Close => request_end(true),
        BarCmd::Retry => retry_seam(),
        BarCmd::Restitch => restart(),
    }
}

fn slot_at(slots: &[BarSlot], x: i32, y: i32) -> i32 {
    slots
        .iter()
        .position(|slot| contains(slot.rect, x, y))
        .map(|index| index as i32)
        .unwrap_or(-1)
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn is_bar(hwnd: HWND) -> bool {
    session()
        .as_ref()
        .is_some_and(|live| live.bar == hwnd as isize)
}

struct Dock {
    bar: RECT,
    slots: [(usize, RECT); 6],
}

fn dock_at(client_w: i32, client_h: i32) -> Dock {
    let button_w = 56i32;
    let button_h = 30i32;
    let gap = 4i32;
    let pad = 7i32;
    let count = PREVIEW_COMMANDS.len() as i32;
    let dock_w = count * button_w + (count - 1) * gap + pad * 2;
    let dock_h = button_h + pad * 2;
    let left = ((client_w - dock_w) / 2).max(8);
    let top = (client_h - dock_h - 10).max(8);
    let mut slots = [(
        0,
        RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
    ); 6];
    let mut x = left + pad;
    let y = top + pad;
    for (index, (id, _, _)) in PREVIEW_COMMANDS.iter().enumerate() {
        slots[index] = (
            *id,
            RECT {
                left: x,
                top: y,
                right: x + button_w,
                bottom: y + button_h,
            },
        );
        x += button_w + gap;
    }
    Dock {
        bar: RECT {
            left,
            top,
            right: left + dock_w,
            bottom: top + dock_h,
        },
        slots,
    }
}

fn paint_dock(hwnd: HWND, hdc: HDC) {
    let (hot, down) = {
        let guard = session();
        let Some(live) = guard.as_ref() else {
            return;
        };
        if live.preview != hwnd as isize {
            return;
        }
        (live.preview_hot, live.preview_down)
    };
    let mut client: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        GetClientRect(hwnd, &mut client);
    }
    let dock = dock_at(client.right - client.left, client.bottom - client.top);
    fill_round(hdc, dock.bar, 16, rgb(12, 31, 26));
    let font = ui_font();
    unsafe {
        let old = SelectObject(hdc, font as _);
        SetBkMode(hdc, TRANSPARENT as i32);
        for (index, (_, rect)) in dock.slots.iter().enumerate() {
            let primary = PREVIEW_COMMANDS[index].2;
            let pressed = down == index as i32;
            let hover = hot == index as i32;
            let (fill, text) = if primary {
                (
                    Some(if pressed {
                        rgb(210, 222, 216)
                    } else {
                        rgb(233, 241, 236)
                    }),
                    rgb(12, 31, 26),
                )
            } else if pressed {
                (Some(rgb(36, 68, 60)), rgb(233, 241, 236))
            } else if hover {
                (Some(rgb(28, 54, 48)), rgb(233, 241, 236))
            } else {
                (None, rgb(233, 241, 236))
            };
            if let Some(color) = fill {
                fill_round(hdc, *rect, 10, color);
            }
            SetTextColor(hdc, text);
            let mut bounds = *rect;
            let wide = utf16(PREVIEW_COMMANDS[index].1);
            DrawTextW(
                hdc,
                wide.as_ptr(),
                -1,
                &mut bounds,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
        SelectObject(hdc, old);
    }
}

fn fill_round(hdc: HDC, rect: RECT, radius: i32, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush as _);
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius,
            radius,
        );
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(brush as _);
    }
}

fn ui_font() -> isize {
    static FONT: OnceLock<isize> = OnceLock::new();
    *FONT.get_or_init(|| {
        let face = utf16("Microsoft YaHei UI");
        let font = unsafe {
            CreateFontW(
                -14,
                0,
                0,
                0,
                FW_SEMIBOLD as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                (DEFAULT_PITCH | FF_DONTCARE) as u32,
                face.as_ptr(),
            )
        };
        if font.is_null() {
            unsafe { GetStockObject(DEFAULT_GUI_FONT) as isize }
        } else {
            font as isize
        }
    })
}

fn preview_move(hwnd: HWND, lparam: LPARAM) {
    if !is_preview(hwnd) {
        return;
    }
    let (x, y) = point_lparam(lparam);
    let (width, height) = client_size(hwnd);
    let hit = dock_index(width, height, x, y);
    let (changed, entered) = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        let entered = live.preview_hot < 0 && hit >= 0;
        if live.preview_hot == hit {
            (false, entered)
        } else {
            live.preview_hot = hit;
            (true, entered)
        }
    };
    if entered {
        track_leave(hwnd);
    }
    if changed {
        invalidate_dock(hwnd);
    }
}

fn preview_down(hwnd: HWND, lparam: LPARAM) {
    if !is_preview(hwnd) {
        return;
    }
    let (x, y) = point_lparam(lparam);
    let (width, height) = client_size(hwnd);
    let hit = dock_index(width, height, x, y);
    if hit < 0 {
        return;
    }
    if let Some(live) = session().as_mut() {
        live.preview_hot = hit;
        live.preview_down = hit;
    }
    unsafe {
        SetCapture(hwnd);
    }
    invalidate_dock(hwnd);
}

fn preview_up(hwnd: HWND, lparam: LPARAM) -> Option<usize> {
    if !is_preview(hwnd) {
        return None;
    }
    let down = session().as_mut().map(|live| {
        let down = live.preview_down;
        live.preview_down = -1;
        down
    });
    unsafe {
        ReleaseCapture();
    }
    let (x, y) = point_lparam(lparam);
    let (width, height) = client_size(hwnd);
    let hit = dock_index(width, height, x, y);
    if let Some(live) = session().as_mut() {
        live.preview_hot = hit;
    }
    invalidate_dock(hwnd);
    let down = down?;
    if down >= 0 && down == hit {
        Some(PREVIEW_COMMANDS[down as usize].0)
    } else {
        None
    }
}

fn preview_leave(hwnd: HWND) {
    if !is_preview(hwnd) {
        return;
    }
    let pressed = session().as_mut().is_some_and(|live| {
        if live.preview_down >= 0 {
            return true;
        }
        let was = live.preview_hot;
        live.preview_hot = -1;
        was < 0
    });
    if !pressed {
        invalidate_dock(hwnd);
    }
}

fn dock_index(width: i32, height: i32, x: i32, y: i32) -> i32 {
    let dock = dock_at(width, height);
    dock.slots
        .iter()
        .position(|(_, rect)| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom)
        .map(|index| index as i32)
        .unwrap_or(-1)
}

fn is_preview(hwnd: HWND) -> bool {
    session()
        .as_ref()
        .is_some_and(|live| live.preview == hwnd as isize)
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut client: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        GetClientRect(hwnd, &mut client);
    }
    (client.right - client.left, client.bottom - client.top)
}

fn point_lparam(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam as u32;
    let x = (packed & 0xffff) as i16 as i32;
    let y = (packed >> 16) as i16 as i32;
    (x, y)
}

fn track_leave(hwnd: HWND) {
    let mut event = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        TrackMouseEvent(&mut event);
    }
}

fn invalidate_dock(hwnd: HWND) {
    let (width, height) = client_size(hwnd);
    let dock = dock_at(width, height);
    unsafe {
        InvalidateRect(hwnd, &dock.bar, 0);
    }
}

fn rect_contains(outer: RECT, inner: RECT) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

fn paint_preview(hwnd: HWND) {
    unsafe {
        let mut paint: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut paint);
        let (width, height) = client_size(hwnd);
        if width > 0 && height > 0 {
            let dock = dock_at(width, height);
            let only_dock = rect_contains(dock.bar, paint.rcPaint);
            let mem = CreateCompatibleDC(hdc);
            let bitmap = CreateCompatibleBitmap(hdc, width, height);
            if !mem.is_null() && !bitmap.is_null() {
                let old = SelectObject(mem, bitmap as _);
                if only_dock {
                    BitBlt(mem, 0, 0, width, height, hdc, 0, 0, SRCCOPY);
                } else {
                    fill_client(hwnd, mem, rgb(24, 24, 24));
                    draw_long_image(hwnd, mem);
                }
                paint_dock(hwnd, mem);
                if only_dock {
                    let dock_w = dock.bar.right - dock.bar.left;
                    let dock_h = dock.bar.bottom - dock.bar.top;
                    BitBlt(
                        hdc,
                        dock.bar.left,
                        dock.bar.top,
                        dock_w,
                        dock_h,
                        mem,
                        dock.bar.left,
                        dock.bar.top,
                        SRCCOPY,
                    );
                } else {
                    BitBlt(hdc, 0, 0, width, height, mem, 0, 0, SRCCOPY);
                }
                SelectObject(mem, old);
            } else {
                fill_client(hwnd, hdc, rgb(24, 24, 24));
                draw_long_image(hwnd, hdc);
                paint_dock(hwnd, hdc);
            }
            if !bitmap.is_null() {
                DeleteObject(bitmap as _);
            }
            if !mem.is_null() {
                DeleteDC(mem);
            }
        }
        EndPaint(hwnd, &paint);
    }
}

fn draw_long_image(hwnd: HWND, hdc: HDC) {
    let guard = session();
    let Some(live) = guard.as_ref() else {
        return;
    };
    let image = live.stitcher.image();
    if live.crop_top + live.crop_bottom >= image.height {
        return;
    }
    let height = image.height - live.crop_top - live.crop_bottom;
    let width = image.width;
    if width == 0 || height < 32 {
        return;
    }
    let mut client: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        GetClientRect(hwnd, &mut client);
    }
    let client_w = (client.right - client.left).max(1);
    let client_h = (client.bottom - client.top - BAR_H).max(1);
    let visible = ((client_h as u64 * width as u64) / client_w as u64)
        .max(1)
        .min(height as u64) as u32;
    let max_scroll = height.saturating_sub(visible);
    let scroll = live.scroll.min(max_scroll);
    let stride = width as usize * 4;
    let row = (live.crop_top + scroll) as usize * stride;
    if row >= image.pixels.len() {
        return;
    }
    let info = dib_top_down(width as i32, visible as i32);
    unsafe {
        SetStretchBltMode(hdc, COLORONCOLOR);
        let dest_h = ((visible as u64 * client_w as u64) / width as u64).max(1) as i32;
        StretchDIBits(
            hdc,
            0,
            0,
            client_w,
            dest_h,
            0,
            0,
            width as i32,
            visible as i32,
            image.pixels[row..].as_ptr().cast(),
            &info,
            0,
            SRCCOPY,
        );
    }
}

fn dib_top_down(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    }
}

fn fill_client(hwnd: HWND, hdc: HDC, color: COLORREF) {
    unsafe {
        let mut client: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut client);
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &client, brush);
        DeleteObject(brush as _);
    }
}

fn on_wheel(hwnd: HWND, wparam: WPARAM) {
    let delta = (wparam >> 16) as i16;
    let mut guard = session();
    let Some(live) = guard.as_mut() else {
        return;
    };
    if live.preview != hwnd as isize {
        return;
    }
    let limit = live
        .stitcher
        .image()
        .height
        .saturating_sub(live.crop_top + live.crop_bottom);
    if delta > 0 {
        live.scroll = live.scroll.saturating_sub(80);
    } else {
        live.scroll = live.scroll.saturating_add(80).min(limit);
    }
    drop(guard);
    repaint(hwnd as isize);
}

fn on_command(id: usize) {
    match id {
        CMD_UNDO => undo(),
        CMD_DONE => finish(),
        CMD_CANCEL | CMD_CLOSE => request_end(true),
        CMD_COPY => copy_result(),
        CMD_SAVE => save_result(),
        CMD_CROP_TOP => crop(true),
        CMD_CROP_BOTTOM => crop(false),
        CMD_RESTART => restart(),
        CMD_AUTO => toggle_auto(),
        _ => {}
    }
}

fn on_destroy(hwnd: HWND) {
    let finished = {
        let mut guard = session();
        let Some(live) = guard.as_mut() else {
            return;
        };
        if live.border == hwnd as isize {
            live.border = 0;
        }
        if live.bar == hwnd as isize {
            live.bar = 0;
        }
        if live.preview == hwnd as isize {
            live.preview = 0;
        }
        if live.border == 0 && live.bar == 0 && live.preview == 0 {
            guard.take()
        } else {
            None
        }
    };
    let Some(live) = finished else {
        return;
    };
    unhook(live.hook_mouse, live.hook_key);
    clear_hole();
    if live.close_session {
        super::close_from_native(&live.app);
    }
}

fn install_hooks() -> (isize, isize) {
    unsafe {
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), instance(), 0);
        let key = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), instance(), 0);
        (mouse as isize, key as isize)
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        match wparam as u32 {
            WM_MOUSEWHEEL => {
                let delta = (info.mouseData >> 16) as i16;
                if delta != 0 && cursor_over_capture() {
                    mark_scroll(delta < 0);
                }
            }
            WM_LBUTTONDOWN => {
                if in_scrollbar_strip(info.pt) {
                    PRESS_Y.store(info.pt.y, Ordering::SeqCst);
                } else {
                    PRESS_Y.store(i32::MIN, Ordering::SeqCst);
                }
            }
            WM_MOUSEMOVE => {
                let origin = PRESS_Y.load(Ordering::SeqCst);
                if origin != i32::MIN {
                    let dy = info.pt.y - origin;
                    if dy.abs() >= 12 {
                        mark_scroll(dy > 0);
                        PRESS_Y.store(info.pt.y, Ordering::SeqCst);
                    }
                }
            }
            WM_LBUTTONUP => PRESS_Y.store(i32::MIN, Ordering::SeqCst),
            _ => {}
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && (wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize) {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if info.vkCode == VK_ESCAPE as u32 {
            let hwnd = HOOK_BAR.load(Ordering::SeqCst) as HWND;
            if !hwnd.is_null() {
                unsafe {
                    PostMessageW(hwnd, WM_APP_CANCEL, 0, 0);
                }
            }
        } else if HOOK_BAR.load(Ordering::SeqCst) != 0 && cursor_over_capture() {
            let down = matches!(
                info.vkCode,
                code if code == VK_DOWN as u32
                    || code == VK_NEXT as u32
                    || code == VK_SPACE as u32
                    || code == VK_END as u32
            );
            let up = matches!(
                info.vkCode,
                code if code == VK_UP as u32 || code == VK_PRIOR as u32 || code == VK_HOME as u32
            );
            if down || up {
                mark_scroll(down);
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn cursor_over_capture() -> bool {
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return false;
    }
    let x = HOLE_X.load(Ordering::Relaxed);
    let y = HOLE_Y.load(Ordering::Relaxed);
    let right = x.saturating_add(HOLE_W.load(Ordering::Relaxed));
    let bottom = y.saturating_add(HOLE_H.load(Ordering::Relaxed));
    point.x >= x && point.y >= y && point.x < right && point.y < bottom
}

fn remember_hole(hole: &ScreenRect) {
    HOLE_X.store(hole.x, Ordering::SeqCst);
    HOLE_Y.store(hole.y, Ordering::SeqCst);
    HOLE_W.store(hole.width as i32, Ordering::SeqCst);
    HOLE_H.store(hole.height as i32, Ordering::SeqCst);
}

fn in_scrollbar_strip(point: POINT) -> bool {
    let x = HOLE_X.load(Ordering::Relaxed);
    let y = HOLE_Y.load(Ordering::Relaxed);
    let width = HOLE_W.load(Ordering::Relaxed);
    let height = HOLE_H.load(Ordering::Relaxed);
    let right = x.saturating_add(width);
    let bottom = y.saturating_add(height);
    point.y >= y && point.y < bottom && point.x >= right.saturating_sub(18) && point.x < right
}

fn clear_hole() {
    HOLE_X.store(0, Ordering::SeqCst);
    HOLE_Y.store(0, Ordering::SeqCst);
    HOLE_W.store(0, Ordering::SeqCst);
    HOLE_H.store(0, Ordering::SeqCst);
    PRESS_Y.store(i32::MIN, Ordering::SeqCst);
}

fn unhook(mouse: isize, key: isize) {
    unsafe {
        if mouse != 0 {
            UnhookWindowsHookEx(mouse as _);
        }
        if key != 0 {
            UnhookWindowsHookEx(key as _);
        }
    }
    HOOK_BAR.store(0, Ordering::SeqCst);
}

fn ensure_class() -> Result<(), String> {
    static READY: OnceLock<Result<(), String>> = OnceLock::new();
    READY
        .get_or_init(|| {
            let cursor = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) };
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance(),
                hIcon: std::ptr::null_mut(),
                hCursor: cursor,
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: CLASS_NAME.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            let atom = unsafe { RegisterClassExW(&class) };
            if atom == 0 {
                Err("注册长截图窗口失败".into())
            } else {
                Ok(())
            }
        })
        .clone()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_COMMAND => {
            on_command((wparam & 0xffff) as usize);
            0
        }
        WM_MOUSEMOVE => {
            if is_bar(hwnd) {
                bar_move(hwnd, lparam);
            } else {
                preview_move(hwnd, lparam);
            }
            0
        }
        WM_LBUTTONDOWN => {
            if is_bar(hwnd) {
                bar_down(hwnd, lparam);
            } else {
                preview_down(hwnd, lparam);
            }
            0
        }
        WM_LBUTTONUP => {
            if is_bar(hwnd) {
                if let Some(cmd) = bar_up(hwnd, lparam) {
                    dispatch_bar(cmd);
                }
            } else if let Some(id) = preview_up(hwnd, lparam) {
                on_command(id);
            }
            0
        }
        msg if msg == WM_MOUSELEAVE => {
            if is_bar(hwnd) {
                bar_leave(hwnd);
            } else {
                preview_leave(hwnd);
            }
            0
        }
        WM_MOUSEWHEEL => {
            on_wheel(hwnd, wparam);
            0
        }
        WM_SIZE => 0,
        WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
            request_end(true);
            0
        }
        msg if msg == WM_APP_FRAME => {
            on_frame();
            0
        }
        msg if msg == WM_APP_CANCEL => {
            request_end(true);
            0
        }
        msg if msg == WM_APP_SAVED => {
            on_saved(wparam);
            0
        }
        WM_CLOSE => {
            unsafe {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            on_destroy(hwnd);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn session() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

fn repaint(hwnd: isize) {
    if hwnd != 0 {
        unsafe {
            InvalidateRect(hwnd as HWND, std::ptr::null(), 0);
        }
    }
}

fn tell(owner: isize, message: &str) {
    let text = utf16(message);
    let title = utf16("长截图");
    unsafe {
        MessageBoxW(
            owner as HWND,
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONWARNING,
        );
    }
}

fn instance() -> windows_sys::Win32::Foundation::HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null()) as _ }
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{Act, CaptureClock, Phase, ScrollDir};
    use std::time::{Duration, Instant};

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn motion_waits_until_it_settles_and_one_more_frame_confirms() {
        let mut clock = CaptureClock::new();
        let start = Instant::now();
        let frames = [
            (0, true, None),
            (100, true, None),
            (200, false, None),
            (360, false, None),
            (460, false, None),
        ];
        let acts: Vec<_> = frames
            .into_iter()
            .map(|(ms, changed, input)| clock.note(changed, input, at(start, ms)).act)
            .collect();
        assert_eq!(
            acts,
            [Act::Watch, Act::Watch, Act::Watch, Act::Watch, Act::Stitch]
        );
        assert_eq!(clock.phase, Phase::Waiting);
    }

    #[test]
    fn one_still_frame_does_not_settle() {
        let mut clock = CaptureClock::new();
        let start = Instant::now();
        assert_eq!(clock.note(true, None, start).act, Act::Watch);
        let later = clock.note(false, None, at(start, 400));
        assert_eq!(later.act, Act::Watch);
        assert_eq!(clock.phase, Phase::Scrolling);
    }

    #[test]
    fn two_early_still_frames_stay_in_motion() {
        let mut clock = CaptureClock::new();
        let start = Instant::now();
        clock.note(true, None, start);
        clock.note(false, None, at(start, 40));
        let early = clock.note(false, None, at(start, 80));
        assert_eq!(early.act, Act::Watch);
        assert_eq!(clock.phase, Phase::Scrolling);
    }

    #[test]
    fn continuous_motion_never_stitches() {
        let mut clock = CaptureClock::new();
        let start = Instant::now();
        for step in 0..20 {
            assert_eq!(
                clock.note(true, None, at(start, step * 100)).act,
                Act::Watch
            );
        }
        assert_eq!(clock.phase, Phase::Scrolling);
    }

    #[test]
    fn downward_scroll_without_new_pixels_counts_as_no_motion() {
        let mut clock = CaptureClock::new();
        let start = Instant::now();
        assert_eq!(
            clock.note(false, Some(ScrollDir::Down), start).act,
            Act::Watch
        );
        assert_eq!(clock.note(false, None, at(start, 200)).act, Act::Watch);
        let done = clock.note(false, None, at(start, 450));
        assert_eq!(done.act, Act::NoMotion);
        assert!(done.downward);
        assert_eq!(clock.phase, Phase::Waiting);
    }

    #[test]
    fn upward_scroll_without_new_pixels_is_not_downward() {
        let mut clock = CaptureClock::new();
        let start = Instant::now();
        clock.note(false, Some(ScrollDir::Up), start);
        clock.note(false, None, at(start, 200));
        let done = clock.note(false, None, at(start, 450));
        assert_eq!(done.act, Act::NoMotion);
        assert!(!done.downward);
    }

    #[test]
    fn error_holds_until_the_user_scrolls() {
        let mut clock = CaptureClock::new();
        clock.fail();
        let start = Instant::now();
        let held = clock.note(true, None, at(start, 500));
        assert_eq!(held.act, Act::Watch);
        assert_eq!(clock.phase, Phase::Error);
        let scrolled = clock.note(false, Some(ScrollDir::Down), at(start, 600));
        assert_eq!(scrolled.act, Act::Watch);
        assert_eq!(clock.phase, Phase::Scrolling);
    }

    #[test]
    fn a_new_change_cancels_the_confirm_frame() {
        let mut clock = CaptureClock::new();
        let start = Instant::now();
        clock.note(true, None, start);
        clock.note(false, None, at(start, 80));
        let settling = clock.note(false, None, at(start, 200));
        assert_eq!(settling.act, Act::Watch);
        assert_eq!(clock.phase, Phase::Settling);
        let again = clock.note(true, None, at(start, 280));
        assert_eq!(again.act, Act::Watch);
        assert_eq!(clock.phase, Phase::Scrolling);
    }
}

const fn utf16_fixed(text: &str) -> [u16; 32] {
    let bytes = text.as_bytes();
    let mut out = [0u16; 32];
    let mut index = 0;
    while index < bytes.len() && index < 31 {
        out[index] = bytes[index] as u16;
        index += 1;
    }
    out
}
