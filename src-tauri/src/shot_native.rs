//! 方案 D：每屏一个 Win32 窗口，直接画已抓到的 raw 帧。不经过 WebView。
//! 选区、复制、另存留在这个窗口里。标注把同一帧交给原来的编辑层。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreatePen, DeleteDC, DeleteObject,
    EndPaint, GetStockObject, InvalidateRect, Rectangle, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    DIB_RGB_COLORS, DEFAULT_GUI_FONT, HDC, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT, PS_SOLID,
    SRCCOPY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, ReleaseCapture, SetCapture, VK_ESCAPE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, MessageBoxW, PostMessageW,
    RegisterClassExW, SendMessageW, SetWindowPos, ShowWindow, ShowWindowAsync,
    BS_PUSHBUTTON, CS_HREDRAW, CS_VREDRAW, HMENU, HWND_TOPMOST,
    IDC_CROSS, MB_ICONWARNING, MB_OK, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONUP, WM_SETFONT, WNDCLASSEXW,
    WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

static CLASS_NAME: [u16; 32] = utf16("DaodianShotNative");
const BTN_COPY: usize = 101;
const BTN_SAVE: usize = 102;
const BTN_MARK: usize = 103;
const BTN_AGAIN: usize = 104;
const BTN_EXIT: usize = 105;
const BUTTONS: [(usize, &str); 5] = [
    (BTN_COPY, "复制"),
    (BTN_SAVE, "另存"),
    (BTN_MARK, "标注"),
    (BTN_AGAIN, "重选"),
    (BTN_EXIT, "退出"),
];

static EPOCH: AtomicU64 = AtomicU64::new(0);
static OPEN: AtomicBool = AtomicBool::new(false);
static HANDING_OFF: AtomicBool = AtomicBool::new(false);
static NATIVE: OnceLock<Mutex<NativeState>> = OnceLock::new();

struct NativeState {
    epoch: u64,
    app: Option<AppHandle>,
    screens: Vec<NativeScreen>,
    gesture: Option<Gesture>,
}

struct NativeScreen {
    epoch: u64,
    index: u32,
    hwnd: isize,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    mem_dc: isize,
    dib: isize,
    old_bitmap: isize,
    buttons: [isize; 5],
    selection: Option<PixelRect>,
    interactive: bool,
}

struct Gesture {
    index: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

#[derive(Clone, Copy)]
struct PixelRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

const fn utf16(text: &str) -> [u16; 32] {
    let bytes = text.as_bytes();
    let mut out = [0u16; 32];
    let mut index = 0;
    while index < bytes.len() && index < 31 {
        out[index] = bytes[index] as u16;
        index += 1;
    }
    out
}

fn state() -> std::sync::MutexGuard<'static, NativeState> {
    NATIVE
        .get_or_init(|| {
            Mutex::new(NativeState {
                epoch: 0,
                app: None,
                screens: Vec::new(),
                gesture: None,
            })
        })
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

pub(crate) fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

pub(crate) fn begin(app: AppHandle) {
    let epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    let stale = {
        let mut native = state();
        native.epoch = epoch;
        native.app = Some(app);
        native.gesture = None;
        native.screens.iter().map(|screen| screen.hwnd).collect::<Vec<_>>()
    };
    post_close(&stale);
}

pub(crate) fn begin_handoff() {
    HANDING_OFF.store(true, Ordering::SeqCst);
}

pub(crate) fn end_handoff() {
    HANDING_OFF.store(false, Ordering::SeqCst);
}

pub(crate) fn request_close() {
    let hwnds = state().screens.iter().map(|screen| screen.hwnd).collect::<Vec<_>>();
    post_close(&hwnds);
}

pub(crate) fn destroy_windows() {
    let hwnds = state().screens.iter().map(|screen| screen.hwnd).collect::<Vec<_>>();
    for hwnd in hwnds {
        if hwnd != 0 {
            unsafe {
                DestroyWindow(hwnd as HWND);
            }
        }
    }
}

pub(crate) fn set_visible(show: bool) {
    let hwnds = state().screens.iter().map(|screen| screen.hwnd).collect::<Vec<_>>();
    for hwnd in hwnds {
        if hwnd == 0 {
            continue;
        }
        unsafe {
            ShowWindowAsync(
                hwnd as HWND,
                if show { SW_SHOWNOACTIVATE } else { SW_HIDE },
            );
        }
    }
}

pub(crate) fn present(
    index: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    interactive: bool,
) -> Result<(), String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    if width == 0 || height == 0 || pixels.len() != expected {
        return Err("截图区域无效".into());
    }
    ensure_class()?;
    let drawn = draw_buffer(width, height, &pixels)?;
    let hwnd = unsafe {
        let mut ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        if !interactive {
            ex_style |= WS_EX_TRANSPARENT;
        }
        CreateWindowExW(
            ex_style,
            CLASS_NAME.as_ptr(),
            CLASS_NAME.as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
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
        discard_buffer(drawn);
        return Err("打不开原生截图窗口".into());
    }
    let epoch = state().epoch;
    state().screens.push(NativeScreen {
        epoch,
        index,
        hwnd: hwnd as isize,
        width,
        height,
        pixels,
        mem_dc: drawn.0,
        dib: drawn.1,
        old_bitmap: drawn.2,
        buttons: [0; 5],
        selection: None,
        interactive,
    });
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            width as i32,
            height as i32,
            SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    if interactive {
        OPEN.store(true, Ordering::SeqCst);
    }
    Ok(())
}

const HOST_SUBCLASS: usize = 0xD40D_B61D;

struct HostFrame {
    top: isize,
    host: isize,
    width: u32,
    height: u32,
    mem_dc: isize,
    dib: isize,
    old_bitmap: isize,
    hooked: bool,
}

fn host_frames() -> std::sync::MutexGuard<'static, Vec<HostFrame>> {
    static HOSTS: OnceLock<Mutex<Vec<HostFrame>>> = OnceLock::new();
    HOSTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

/// 把冻结帧画在网页宿主上，按物理像素 1:1。PixPin 也是把图直接画在窗口上，不另开一层透明窗。
pub(crate) fn bind_host(
    top: isize,
    host: isize,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), String> {
    if top == 0 || host == 0 {
        return Err("没有截图窗口".into());
    }
    let drawn = draw_buffer(width, height, pixels)?;
    allow_paint_under_children(host);
    set_desktop_bleed(top, false);
    let mut frames = host_frames();
    if let Some(slot) = frames.iter_mut().find(|item| item.host == host) {
        discard_buffer(Drawn(slot.mem_dc, slot.dib, slot.old_bitmap));
        slot.top = top;
        slot.width = width;
        slot.height = height;
        slot.mem_dc = drawn.0;
        slot.dib = drawn.1;
        slot.old_bitmap = drawn.2;
    } else {
        frames.push(HostFrame {
            top,
            host,
            width,
            height,
            mem_dc: drawn.0,
            dib: drawn.1,
            old_bitmap: drawn.2,
            hooked: false,
        });
    }
    let needs_hook = frames
        .iter()
        .find(|item| item.host == host)
        .map(|item| !item.hooked)
        .unwrap_or(false);
    if needs_hook {
        let hooked = unsafe {
            windows_sys::Win32::UI::Shell::SetWindowSubclass(
                host as HWND,
                Some(host_wndproc),
                HOST_SUBCLASS,
                0,
            )
        };
        if let Some(slot) = frames.iter_mut().find(|item| item.host == host) {
            slot.hooked = hooked != 0;
        }
        if hooked == 0 {
            return Err("挂不上冻结画面".into());
        }
    }
    drop(frames);
    unsafe {
        InvalidateRect(host as HWND, std::ptr::null(), 1);
    }
    Ok(())
}

pub(crate) fn release_hosts() {
    let frames = std::mem::take(&mut *host_frames());
    for frame in frames {
        set_desktop_bleed(frame.top, true);
        if frame.hooked {
            unsafe {
                windows_sys::Win32::UI::Shell::RemoveWindowSubclass(
                    frame.host as HWND,
                    Some(host_wndproc),
                    HOST_SUBCLASS,
                );
            }
        }
        discard_buffer(Drawn(frame.mem_dc, frame.dib, frame.old_bitmap));
        unsafe {
            InvalidateRect(frame.host as HWND, std::ptr::null(), 1);
        }
    }
}

fn allow_paint_under_children(host: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOZORDER,
    };
    unsafe {
        let hwnd = host as HWND;
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let next = style & !(WS_CLIPCHILDREN as isize);
        if next == style {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, next);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

fn set_desktop_bleed(top: isize, enable: bool) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmEnableBlurBehindWindow, DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND,
    };
    use windows_sys::Win32::Graphics::Gdi::CreateRectRgn;
    if top == 0 {
        return;
    }
    unsafe {
        let region = if enable {
            CreateRectRgn(0, 0, -1, -1)
        } else {
            std::ptr::null_mut()
        };
        let blur = DWM_BLURBEHIND {
            dwFlags: if enable {
                DWM_BB_ENABLE | DWM_BB_BLURREGION
            } else {
                DWM_BB_ENABLE
            },
            fEnable: if enable { 1 } else { 0 },
            hRgnBlur: region,
            fTransitionOnMaximized: 0,
        };
        let _ = DwmEnableBlurBehindWindow(top as HWND, &blur);
        if !region.is_null() {
            let _ = DeleteObject(region as HGDIOBJ);
        }
    }
}

unsafe extern "system" fn host_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;
    match msg {
        WM_ERASEBKGND => {
            blit_host(hwnd, wparam as HDC);
            1
        }
        WM_PAINT => {
            unsafe {
                let mut paint: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut paint);
                blit_host(hwnd, hdc);
                EndPaint(hwnd, &paint);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
        }
        WM_NCDESTROY => {
            release_one(hwnd as isize);
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

fn blit_host(hwnd: HWND, hdc: HDC) {
    let frame = {
        let frames = host_frames();
        frames
            .iter()
            .find(|item| item.host == hwnd as isize)
            .map(|item| (item.mem_dc, item.width, item.height))
    };
    let Some((mem, width, height)) = frame else {
        return;
    };
    if mem == 0 || hdc.is_null() {
        return;
    }
    unsafe {
        BitBlt(
            hdc,
            0,
            0,
            width as i32,
            height as i32,
            mem as HDC,
            0,
            0,
            SRCCOPY,
        );
    }
}

fn release_one(host: isize) {
    let mut frames = host_frames();
    let Some(index) = frames.iter().position(|item| item.host == host) else {
        return;
    };
    let frame = frames.remove(index);
    drop(frames);
    discard_buffer(Drawn(frame.mem_dc, frame.dib, frame.old_bitmap));
}

fn post_close(hwnds: &[isize]) {
    for hwnd in hwnds {
        if *hwnd != 0 {
            unsafe {
                PostMessageW(*hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
    }
}

fn ensure_class() -> Result<(), String> {
    static READY: OnceLock<Result<(), String>> = OnceLock::new();
    READY
        .get_or_init(|| {
            let cursor = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_CROSS) };
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(native_wndproc),
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
                Err("注册原生截图窗口失败".into())
            } else {
                Ok(())
            }
        })
        .clone()
}

fn screen_interactive(hwnd: HWND) -> bool {
    state()
        .screens
        .iter()
        .find(|screen| screen.hwnd == hwnd as isize)
        .map(|screen| screen.interactive)
        .unwrap_or(false)
}

fn instance() -> windows_sys::Win32::Foundation::HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null()) as _ }
}

struct Drawn(isize, isize, isize);

fn draw_buffer(width: u32, height: u32, pixels: &[u8]) -> Result<Drawn, String> {
    unsafe {
        let screen = windows_sys::Win32::Graphics::Gdi::GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return Err("拿不到屏幕".into());
        }
        let memory = CreateCompatibleDC(screen);
        windows_sys::Win32::Graphics::Gdi::ReleaseDC(std::ptr::null_mut(), screen);
        if memory.is_null() {
            return Err("创建截图设备失败".into());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [windows_sys::Win32::Graphics::Gdi::RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(
            memory,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if dib.is_null() || bits.is_null() {
            let _ = DeleteDC(memory);
            return Err("创建截图位图失败".into());
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast(), pixels.len());
        let old = SelectObject(memory, dib as _);
        Ok(Drawn(memory as isize, dib as isize, old as isize))
    }
}

fn discard_buffer(drawn: Drawn) {
    unsafe {
        if drawn.2 != 0 {
            SelectObject(drawn.0 as HDC, drawn.2 as HGDIOBJ);
        }
        if drawn.1 != 0 {
            let _ = DeleteObject(drawn.1 as HGDIOBJ);
        }
        if drawn.0 != 0 {
            let _ = DeleteDC(drawn.0 as HDC);
        }
    }
}

unsafe extern "system" fn native_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => paint(hwnd),
        WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_LBUTTONUP | WM_RBUTTONUP | WM_KEYDOWN | WM_COMMAND
            if !screen_interactive(hwnd) =>
        {
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point_of(lparam);
            {
                let mut native = state();
                if let Some(screen) = native.screens.iter_mut().find(|screen| screen.hwnd == hwnd as isize)
                {
                    let index = screen.index;
                    native.gesture = Some(Gesture {
                        index,
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                    });
                }
            }
            unsafe {
                SetCapture(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            if unsafe { GetCapture() } != hwnd {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            let (x, y) = point_of(lparam);
            let mut native = state();
            if let Some(gesture) = native.gesture.as_mut() {
                gesture.x1 = x;
                gesture.y1 = y;
            }
            drop(native);
            unsafe {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            unsafe {
                ReleaseCapture();
            }
            finish_gesture(hwnd);
            0
        }
        WM_RBUTTONUP => {
            close_active();
            0
        }
        WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
            close_active();
            0
        }
        WM_COMMAND => {
            run_button(hwnd, (wparam & 0xffff) as usize);
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

fn paint(hwnd: HWND) -> LRESULT {
    let frame = {
        let native = state();
        let Some(screen) = native.screens.iter().find(|screen| screen.hwnd == hwnd as isize) else {
            return 0;
        };
        let rect = native
            .gesture
            .as_ref()
            .filter(|gesture| gesture.index == screen.index)
            .map(|gesture| norm_rect(gesture.x0, gesture.y0, gesture.x1, gesture.y1))
            .or_else(|| {
                screen.selection.map(|rect| {
                    (
                        rect.x,
                        rect.y,
                        rect.x + rect.w as i32,
                        rect.y + rect.h as i32,
                    )
                })
            });
        (screen.width, screen.height, screen.mem_dc, rect)
    };
    unsafe {
        let mut paint: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut paint);
        if frame.2 != 0 {
            BitBlt(
                hdc,
                0,
                0,
                frame.0 as i32,
                frame.1 as i32,
                frame.2 as HDC,
                0,
                0,
                SRCCOPY,
            );
        }
        if let Some((left, top, right, bottom)) = frame.3 {
            if right - left >= 3 && bottom - top >= 3 {
                draw_frame(hdc, left, top, right, bottom);
            }
        }
        EndPaint(hwnd, &paint);
    }
    0
}

fn draw_frame(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, 2, rgb(143, 208, 234));
        let old_pen = SelectObject(hdc, pen as _);
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        Rectangle(hdc, left, top, right, bottom);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen as _);
    }
}

fn finish_gesture(hwnd: HWND) {
    let mut native = state();
    let Some(gesture) = native.gesture.take() else {
        return;
    };
    let Some(screen) = native
        .screens
        .iter_mut()
        .find(|screen| screen.hwnd == hwnd as isize && screen.index == gesture.index)
    else {
        return;
    };
    let (left, top, right, bottom) = norm_rect(gesture.x0, gesture.y0, gesture.x1, gesture.y1);
    let width = (right - left).max(0) as u32;
    let height = (bottom - top).max(0) as u32;
    if width < 3 || height < 3 {
        screen.selection = None;
        let buttons = screen.buttons;
        screen.buttons = [0; 5];
        drop(native);
        destroy_buttons(&buttons);
    } else {
        let clamped = clamp_rect(screen.width, screen.height, left, top, width, height);
        screen.selection = clamped;
        drop(native);
        place_buttons(hwnd);
    }
    unsafe {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

fn clamp_rect(width: u32, height: u32, x: i32, y: i32, w: u32, h: u32) -> Option<PixelRect> {
    let x = x.max(0).min(width as i32);
    let y = y.max(0).min(height as i32);
    let w = (w as i32).min(width as i32 - x).max(0) as u32;
    let h = (h as i32).min(height as i32 - y).max(0) as u32;
    (w >= 3 && h >= 3).then_some(PixelRect { x, y, w, h })
}

fn place_buttons(hwnd: HWND) {
    let layout = {
        let native = state();
        native.screens.iter().find(|screen| screen.hwnd == hwnd as isize).and_then(|screen| {
            let rect = screen.selection?;
            Some((screen.buttons, rect, screen.width, screen.height))
        })
    };
    let Some((existing, rect, width, height)) = layout else {
        return;
    };
    let button_w = 72i32;
    let button_h = 28i32;
    let gap = 6i32;
    let total = BUTTONS.len() as i32 * button_w + (BUTTONS.len() as i32 - 1) * gap;
    let mut left = rect.x;
    if left + total > width as i32 {
        left = (width as i32 - total).max(0);
    }
    let mut top = rect.y + rect.h as i32 + 8;
    if top + button_h > height as i32 {
        top = (rect.y - button_h - 8).max(0);
    }
    let mut made = existing;
    for (index, (id, _label)) in BUTTONS.iter().enumerate() {
        let x = left + index as i32 * (button_w + gap);
        if made[index] == 0 {
            made[index] = create_button(hwnd, *id, BUTTONS[index].1, x, top, button_w, button_h);
        } else {
            unsafe {
                SetWindowPos(
                    made[index] as HWND,
                    std::ptr::null_mut(),
                    x,
                    top,
                    button_w,
                    button_h,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }
    if let Some(screen) = state()
        .screens
        .iter_mut()
        .find(|screen| screen.hwnd == hwnd as isize)
    {
        screen.buttons = made;
    }
}

fn create_button(parent: HWND, id: usize, label: &str, x: i32, y: i32, w: i32, h: i32) -> isize {
    let class = utf16("BUTTON");
    let text = utf16_text(label);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | (BS_PUSHBUTTON as u32),
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            instance(),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return 0;
    }
    unsafe {
        let font = GetStockObject(DEFAULT_GUI_FONT);
        SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1);
    }
    hwnd as isize
}

fn destroy_buttons(buttons: &[isize; 5]) {
    for hwnd in buttons {
        if *hwnd != 0 {
            unsafe {
                DestroyWindow(*hwnd as HWND);
            }
        }
    }
}

fn run_button(hwnd: HWND, id: usize) {
    match id {
        BTN_COPY => copy_selection(hwnd),
        BTN_SAVE => save_selection(hwnd),
        BTN_MARK => mark_selection(),
        BTN_AGAIN => clear_selection(hwnd),
        BTN_EXIT => close_active(),
        _ => {}
    }
}

fn copy_selection(hwnd: HWND) {
    let copied = selection_pixels(hwnd);
    let Some((_app, _index, width, height, pixels)) = copied else {
        tell(hwnd, "先拖一块区域");
        return;
    };
    match crate::shot::copy_bgra(width, height, &pixels) {
        Ok(()) => close_active(),
        Err(err) => tell(hwnd, &err),
    }
}

fn save_selection(hwnd: HWND) {
    let Some((app, _index, width, height, pixels)) = selection_pixels(hwnd) else {
        tell(hwnd, "先拖一块区域");
        return;
    };
    match crate::shot::png_bytes_from_bgra(width, height, &pixels) {
        Ok(bytes) => crate::shot::save_native_async(app, bytes),
        Err(err) => tell(hwnd, &err),
    }
}

fn mark_selection() {
    let app = state().app.clone();
    if let Some(app) = app {
        crate::shot::annotate_from_native(&app);
    }
}

fn clear_selection(hwnd: HWND) {
    let buttons = {
        let mut native = state();
        let Some(screen) = native.screens.iter_mut().find(|screen| screen.hwnd == hwnd as isize) else {
            return;
        };
        screen.selection = None;
        let buttons = screen.buttons;
        screen.buttons = [0; 5];
        buttons
    };
    destroy_buttons(&buttons);
    unsafe {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

fn selection_pixels(hwnd: HWND) -> Option<(AppHandle, u32, u32, u32, Vec<u8>)> {
    let native = state();
    let app = native.app.clone()?;
    let screen = native.screens.iter().find(|screen| screen.hwnd == hwnd as isize)?;
    let rect = screen.selection?;
    let pixels = crate::shot::crop_bgra(
        screen.width,
        screen.height,
        &screen.pixels,
        rect.x,
        rect.y,
        rect.w,
        rect.h,
    )
    .ok()?;
    Some((app, screen.index, rect.w, rect.h, pixels))
}

fn close_active() {
    request_close();
}

fn on_destroy(hwnd: HWND) {
    let (app, close_session) = {
        let mut native = state();
        let epoch = native
            .screens
            .iter()
            .find(|screen| screen.hwnd == hwnd as isize)
            .map(|screen| screen.epoch);
        if let Some(screen) = native.screens.iter().find(|screen| screen.hwnd == hwnd as isize) {
            discard_buffer(Drawn(screen.mem_dc, screen.dib, screen.old_bitmap));
        }
        native.screens.retain(|screen| screen.hwnd != hwnd as isize);
        let empty = native.screens.is_empty();
        if empty {
            OPEN.store(false, Ordering::SeqCst);
            native.gesture = None;
        }
        let close_session = empty
            && epoch == Some(native.epoch)
            && !HANDING_OFF.load(Ordering::SeqCst);
        (native.app.clone(), close_session)
    };
    if close_session {
        if let Some(app) = app {
            crate::shot::close_from_native(&app);
        }
    }
}

fn tell(hwnd: HWND, message: &str) {
    let text = utf16_text(message);
    let title = utf16_text("截图");
    unsafe {
        MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING);
    }
}

fn point_of(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam as u32;
    let x = (packed & 0xffff) as i16 as i32;
    let y = ((packed >> 16) & 0xffff) as i16 as i32;
    (x, y)
}

fn norm_rect(x0: i32, y0: i32, x1: i32, y1: i32) -> (i32, i32, i32, i32) {
    (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

fn utf16_text(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

