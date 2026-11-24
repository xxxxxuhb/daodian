//! 把一块屏幕抓成自上而下的 BGRA。
use std::cell::RefCell;

use super::BgraImage;

#[cfg(windows)]
struct ThreadDpi(windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);

#[cfg(windows)]
impl ThreadDpi {
    fn per_monitor_v2() -> Self {
        use windows_sys::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
        };
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        Self(previous)
    }
}

#[cfg(windows)]
impl Drop for ThreadDpi {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}

#[cfg(windows)]
struct MonitorHit {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    device: [u16; 32],
    found: bool,
}

#[cfg(windows)]
unsafe extern "system" fn on_capture_monitor(
    monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR,
    _hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    _rect: *mut windows_sys::Win32::Foundation::RECT,
    data: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::core::BOOL {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITORINFOEXW};

    unsafe {
        let hit = &mut *(data as *mut MonitorHit);
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(
            monitor,
            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
        ) == 0
        {
            return 1;
        }
        let rc = info.monitorInfo.rcMonitor;
        if rc.left == hit.x
            && rc.top == hit.y
            && rc.right - rc.left == hit.width
            && rc.bottom - rc.top == hit.height
        {
            hit.device = info.szDevice;
            hit.found = true;
            return 0;
        }
        1
    }
}

/// 从这块屏自己的设备上下文抓 (0,0)。虚拟桌面坐标在混合 DPI 下会偏。
#[cfg(windows)]
fn monitor_dc(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Option<windows_sys::Win32::Graphics::Gdi::HDC> {
    use windows_sys::Win32::Graphics::Gdi::{CreateDCW, EnumDisplayMonitors};

    let mut hit = MonitorHit {
        x,
        y,
        width,
        height,
        device: [0; 32],
        found: false,
    };
    unsafe {
        let _ = EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(on_capture_monitor),
            &mut hit as *mut MonitorHit as isize,
        );
        if !hit.found || hit.device[0] == 0 {
            return None;
        }
        let dc = CreateDCW(
            hit.device.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        );
        if dc.is_null() { None } else { Some(dc) }
    }
}

#[cfg(windows)]
struct CachedDc {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    owned: bool,
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
}

#[cfg(windows)]
impl CachedDc {
    fn same(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        self.x == x
            && self.y == y
            && self.width == width
            && self.height == height
            && !self.dc.is_null()
    }
}

#[cfg(windows)]
impl Drop for CachedDc {
    fn drop(&mut self) {
        if self.dc.is_null() {
            return;
        }
        unsafe {
            if self.owned {
                let _ = windows_sys::Win32::Graphics::Gdi::DeleteDC(self.dc);
            } else {
                let _ = windows_sys::Win32::Graphics::Gdi::ReleaseDC(std::ptr::null_mut(), self.dc);
            }
        }
    }
}

#[cfg(windows)]
thread_local! {
    static MONITOR_DC: RefCell<Option<CachedDc>> = const { RefCell::new(None) };
}

#[cfg(windows)]
fn clear_cached_dc() {
    MONITOR_DC.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(windows)]
fn open_monitor_dc(x: i32, y: i32, width: i32, height: i32) -> Result<CachedDc, String> {
    use windows_sys::Win32::Graphics::Gdi::GetDC;

    if let Some(dc) = monitor_dc(x, y, width, height) {
        return Ok(CachedDc {
            x,
            y,
            width,
            height,
            owned: true,
            dc,
        });
    }
    let dc = unsafe { GetDC(std::ptr::null_mut()) };
    if dc.is_null() {
        return Err("拿不到屏幕".into());
    }
    Ok(CachedDc {
        x,
        y,
        width,
        height,
        owned: false,
        dc,
    })
}

#[cfg(windows)]
pub(in crate::shot) fn capture_bgra(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    include_cursor: bool,
) -> Result<BgraImage, String> {
    if width <= 0 || height <= 0 || width > 16_000 || height > 16_000 {
        return Err("截图区域无效".into());
    }
    let bytes = width as usize * height as usize * 4;
    let image_size = u32::try_from(bytes).map_err(|_| "截图区域无效")?;
    let _dpi = ThreadDpi::per_monitor_v2();
    match blit_dib(x, y, width, height, image_size, include_cursor) {
        Ok(image) => Ok(image),
        Err(err) if err == "复制屏幕失败" || err == "创建截图位图失败" => {
            clear_cached_dc();
            blit_dib(x, y, width, height, image_size, include_cursor)
        }
        Err(err) => Err(err),
    }
}

/// BitBlt 直接写进 top-down 32bpp DIB，再按行拷出。不再 GetDIBits，也不再失败后整图翻转。
#[cfg(windows)]
fn blit_dib(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    image_size: u32,
    include_cursor: bool,
) -> Result<BgraImage, String> {
    MONITOR_DC.with(|slot| {
        let mut cache = slot.borrow_mut();
        let matches = cache
            .as_ref()
            .is_some_and(|dc| dc.same(x, y, width, height));
        if !matches {
            *cache = Some(open_monitor_dc(x, y, width, height)?);
        }
        let Some(cached) = cache.as_ref() else {
            return Err("拿不到屏幕".into());
        };
        let blit_x = if cached.owned { 0 } else { x };
        let blit_y = if cached.owned { 0 } else { y };
        unsafe {
            copy_dib(
                cached.dc,
                blit_x,
                blit_y,
                x,
                y,
                width,
                height,
                image_size,
                include_cursor,
            )
        }
    })
}

#[cfg(windows)]
unsafe fn copy_dib(
    screen: windows_sys::Win32::Graphics::Gdi::HDC,
    blit_x: i32,
    blit_y: i32,
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
    image_size: u32,
    include_cursor: bool,
) -> Result<BgraImage, String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, SRCCOPY,
        SelectObject,
    };

    unsafe {
        let memory = CreateCompatibleDC(screen);
        if memory.is_null() {
            return Err("创建截图设备失败".into());
        }
        let info = dib_info(width, -height, image_size);
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            screen,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if bitmap.is_null() || bits.is_null() {
            let _ = DeleteDC(memory);
            return Err("创建截图位图失败".into());
        }
        let previous = SelectObject(memory, bitmap as _);
        let copied = BitBlt(memory, 0, 0, width, height, screen, blit_x, blit_y, SRCCOPY);
        if copied == 0 {
            release_dib(memory, bitmap, previous);
            return Err("复制屏幕失败".into());
        }
        if include_cursor {
            draw_cursor(memory, origin_x, origin_y);
        }
        let pixels = match copy_dib_rows(bits, width, height) {
            Ok(pixels) => pixels,
            Err(err) => {
                release_dib(memory, bitmap, previous);
                return Err(err);
            }
        };
        release_dib(memory, bitmap, previous);
        Ok(BgraImage {
            width: width as u32,
            height: height as u32,
            pixels,
        })
    }
}

#[cfg(windows)]
unsafe fn release_dib(
    memory: windows_sys::Win32::Graphics::Gdi::HDC,
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    previous: windows_sys::Win32::Graphics::Gdi::HGDIOBJ,
) {
    use windows_sys::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, SelectObject};

    unsafe {
        let failed = -1isize as windows_sys::Win32::Graphics::Gdi::HGDIOBJ;
        if !previous.is_null() && previous != failed {
            let _ = SelectObject(memory, previous);
        }
        let _ = DeleteObject(bitmap as _);
        let _ = DeleteDC(memory);
    }
}

#[cfg(windows)]
unsafe fn copy_dib_rows(
    bits: *mut core::ffi::c_void,
    width: i32,
    height: i32,
) -> Result<Vec<u8>, String> {
    let row_bytes = width as usize * 4;
    let stride = row_bytes;
    let rows = height as usize;
    let bytes = row_bytes.checked_mul(rows).ok_or("截图区域无效")?;
    let total = stride.checked_mul(rows).ok_or("截图区域无效")?;
    let mut pixels = vec![0u8; bytes];
    unsafe {
        let src = std::slice::from_raw_parts(bits.cast::<u8>(), total);
        for row in 0..rows {
            let from = row * stride;
            let to = row * row_bytes;
            pixels[to..to + row_bytes].copy_from_slice(&src[from..from + row_bytes]);
            for pixel in pixels[to..to + row_bytes].chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
    }
    Ok(pixels)
}

/// PrintWindow 写的是兼容位图，仍然要用 GetDIBits 读出来。
#[cfg(windows)]
unsafe fn read_top_down(
    memory: windows_sys::Win32::Graphics::Gdi::HDC,
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    width: i32,
    height: i32,
    image_size: u32,
) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Graphics::Gdi::{DIB_RGB_COLORS, GetDIBits};

    unsafe {
        let mut pixels = vec![0u8; image_size as usize];
        let mut info = dib_info(width, -height, image_size);
        let rows = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        if rows == height {
            return Ok(pixels);
        }
        pixels.fill(0);
        info = dib_info(width, height, image_size);
        let rows = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        if rows != height {
            return Err(format!("读取屏幕像素失败: {rows}/{height}"));
        }
        let stride = width as usize * 4;
        let mut flipped = vec![0u8; pixels.len()];
        for row in 0..height as usize {
            let src = (height as usize - 1 - row) * stride;
            let dst = row * stride;
            flipped[dst..dst + stride].copy_from_slice(&pixels[src..src + stride]);
        }
        Ok(flipped)
    }
}

#[cfg(windows)]
fn dib_info(
    width: i32,
    height: i32,
    image_size: u32,
) -> windows_sys::Win32::Graphics::Gdi::BITMAPINFO {
    use windows_sys::Win32::Graphics::Gdi::{BITMAPINFO, BITMAPINFOHEADER, RGBQUAD};
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: image_size,
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

pub(super) fn capture_rect(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    include_cursor: bool,
) -> Result<image::RgbaImage, String> {
    let mut image = capture_bgra(x, y, width, height, include_cursor)?;
    for pixel in image.pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image::RgbaImage::from_raw(image.width, image.height, image.pixels)
        .ok_or_else(|| "屏幕像素尺寸不匹配".into())
}

#[cfg(windows)]
unsafe fn draw_cursor(hdc: windows_sys::Win32::Graphics::Gdi::HDC, origin_x: i32, origin_y: i32) {
    use windows_sys::Win32::Graphics::Gdi::DeleteObject;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, ICONINFO,
    };

    unsafe {
        let mut cursor: CURSORINFO = std::mem::zeroed();
        cursor.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
        if GetCursorInfo(&mut cursor) == 0 || cursor.flags != CURSOR_SHOWING {
            return;
        }
        let mut icon: ICONINFO = std::mem::zeroed();
        if GetIconInfo(cursor.hCursor as _, &mut icon) == 0 {
            return;
        }
        let x = cursor.ptScreenPos.x - origin_x - icon.xHotspot as i32;
        let y = cursor.ptScreenPos.y - origin_y - icon.yHotspot as i32;
        let _ = DrawIconEx(
            hdc,
            x,
            y,
            cursor.hCursor as _,
            0,
            0,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        );
        if !icon.hbmMask.is_null() {
            let _ = DeleteObject(icon.hbmMask as _);
        }
        if !icon.hbmColor.is_null() {
            let _ = DeleteObject(icon.hbmColor as _);
        }
    }
}

/// 长截图一次会话只用一个采集源。默认屏幕复制；屏幕几乎全黑才改用窗口绘制。
#[cfg(windows)]
#[derive(Clone, Copy)]
pub(in crate::shot) enum PageSource {
    Screen,
    Print(isize),
}

#[cfg(windows)]
impl PageSource {
    fn label(self) -> String {
        match self {
            Self::Screen => "screen".to_string(),
            Self::Print(hwnd) => format!("print hwnd={hwnd:#x} class={}", window_class(hwnd)),
        }
    }

    pub(in crate::shot) fn hwnd_bits(self) -> isize {
        match self {
            Self::Screen => 0,
            Self::Print(hwnd) => hwnd,
        }
    }
}

#[cfg(windows)]
pub(in crate::shot) fn page_source_from_bits(bits: isize) -> PageSource {
    if bits == 0 {
        PageSource::Screen
    } else {
        PageSource::Print(bits)
    }
}

#[cfg(windows)]
pub(in crate::shot) fn choose_page_source(x: i32, y: i32, width: i32, height: i32) -> PageSource {
    let source = match capture_bgra(x, y, width, height, false) {
        Ok(image) if !mostly_blank(&image) => PageSource::Screen,
        Ok(_) => foreign_hwnd(x, y, width, height)
            .map(PageSource::Print)
            .unwrap_or(PageSource::Screen),
        Err(err) => {
            crate::logging::warn(format_args!("longshot screen grab failed: {err}"));
            foreign_hwnd(x, y, width, height)
                .map(PageSource::Print)
                .unwrap_or(PageSource::Screen)
        }
    };
    crate::logging::info(format_args!("longshot source={}", source.label()));
    source
}

/// 丢掉第一帧，再连续两帧足够接近才当作首帧。来源在这次调用里保持不变。
#[cfg(windows)]
pub(in crate::shot) fn capture_opening(
    source: PageSource,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<BgraImage, String> {
    const STABLE_SAD: u32 = 2;
    let started = std::time::Instant::now();
    let mut dumped = 0u32;
    for attempt in 1..=5 {
        dwm_flush_bounded();
        std::thread::sleep(std::time::Duration::from_millis(32));
        let discarded = grab_page(source, x, y, width, height)?;
        dump_opening_frame(dumped, &discarded);
        dumped = dumped.saturating_add(1);
        std::thread::sleep(std::time::Duration::from_millis(16));
        dwm_flush_bounded();
        let second = grab_page(source, x, y, width, height)?;
        let third = grab_page(source, x, y, width, height)?;
        if dumped < 3 {
            dump_opening_frame(dumped, &second);
            dumped = dumped.saturating_add(1);
        }
        if dumped < 3 {
            dump_opening_frame(dumped, &third);
            dumped = dumped.saturating_add(1);
        }
        let sad = super::stitch::sample_sad(second.width, second.height, &second.pixels, &third.pixels)
            .unwrap_or(u32::MAX);
        crate::logging::info(format_args!(
            "longshot open source={} attempt={attempt} sad={sad} discarded=1 elapsed={}ms",
            source.label(),
            started.elapsed().as_millis()
        ));
        if sad <= STABLE_SAD {
            return Ok(third);
        }
    }
    Err("页面还在动，稍后再试".into())
}

#[cfg(windows)]
pub(in crate::shot) fn grab_page(
    source: PageSource,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<BgraImage, String> {
    match source {
        PageSource::Screen => capture_bgra(x, y, width, height, false),
        PageSource::Print(hwnd) => print_one(hwnd as _, x, y, width, height)
            .filter(|image| !mostly_blank(image))
            .ok_or_else(|| "窗口画面拿不到".to_string()),
    }
}

#[cfg(windows)]
fn dwm_flush_bounded() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("dwm-flush".into())
        .spawn(move || {
            unsafe {
                let _ = windows_sys::Win32::Graphics::Dwm::DwmFlush();
            }
            let _ = tx.send(());
        });
    if spawned.is_err() {
        BUSY.store(false, Ordering::SeqCst);
        return;
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(80) {
        if rx.try_recv().is_ok() {
            BUSY.store(false, Ordering::SeqCst);
            return;
        }
        pump_once();
        std::thread::sleep(Duration::from_millis(2));
    }
    BUSY.store(false, Ordering::SeqCst);
    crate::logging::warn(format_args!("longshot dwm flush timed out"));
}

#[cfg(windows)]
fn pump_once() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };

    unsafe {
        let mut message: MSG = std::mem::zeroed();
        if PeekMessageW(
            &mut message,
            std::ptr::null_mut(),
            0,
            0,
            PM_REMOVE,
        ) != 0
        {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[cfg(windows)]
fn foreign_hwnd(x: i32, y: i32, width: i32, height: i32) -> Option<isize> {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetWindowThreadProcessId, IsWindow, WindowFromPoint,
    };

    if width <= 0 || height <= 0 {
        return None;
    }
    let _dpi = ThreadDpi::per_monitor_v2();
    let center = windows_sys::Win32::Foundation::POINT {
        x: x + width / 2,
        y: y + height / 2,
    };
    let mut hwnd = unsafe { WindowFromPoint(center) };
    let self_pid = unsafe { GetCurrentProcessId() };
    for _ in 0..8 {
        if unsafe { IsWindow(hwnd) } == 0 {
            return None;
        }
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid != 0 && pid != self_pid {
            return Some(hwnd as isize);
        }
        let parent = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if parent.is_null() || parent == hwnd {
            return None;
        }
        hwnd = parent;
    }
    None
}

#[cfg(windows)]
fn window_class(hwnd: isize) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut buffer = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd as _, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..len as usize])
}

#[cfg(all(windows, debug_assertions))]
fn dump_opening_frame(index: u32, image: &BgraImage) {
    let mut rgba = Vec::with_capacity(image.pixels.len());
    for pixel in image.pixels.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let path = std::env::temp_dir().join(format!("daodian-longshot-open-{index}.png"));
    match image::save_buffer(
        &path,
        &rgba,
        image.width,
        image.height,
        image::ExtendedColorType::Rgba8,
    ) {
        Ok(()) => crate::logging::info(format_args!("longshot open frame {} {}", index, path.display())),
        Err(err) => crate::logging::warn(format_args!("longshot open frame dump failed: {err}")),
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn dump_opening_frame(_index: u32, _image: &BgraImage) {}

#[cfg(windows)]
fn print_one(
    hwnd: windows_sys::Win32::Foundation::HWND,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Option<BgraImage> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        BLACK_BRUSH, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, FillRect,
        GetDC, GetStockObject, ReleaseDC, SelectObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

    const PW_RENDERFULLCONTENT: u32 = 2;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn PrintWindow(
            hwnd: windows_sys::Win32::Foundation::HWND,
            hdc: windows_sys::Win32::Graphics::Gdi::HDC,
            flags: u32,
        ) -> windows_sys::core::BOOL;
    }
    let mut window: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut window) } == 0 {
        return None;
    }
    if x < window.left || y < window.top || x + width > window.right || y + height > window.bottom
    {
        return None;
    }
    let win_w = window.right - window.left;
    let win_h = window.bottom - window.top;
    if win_w <= 0 || win_h <= 0 || win_w > 16_000 || win_h > 16_000 {
        return None;
    }
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let memory = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, win_w, win_h);
        if memory.is_null() || bitmap.is_null() {
            if !bitmap.is_null() {
                let _ = DeleteObject(bitmap as _);
            }
            if !memory.is_null() {
                let _ = DeleteDC(memory);
            }
            let _ = ReleaseDC(std::ptr::null_mut(), screen);
            return None;
        }
        let previous = SelectObject(memory, bitmap as _);
        let bounds = RECT {
            left: 0,
            top: 0,
            right: win_w,
            bottom: win_h,
        };
        let brush = GetStockObject(BLACK_BRUSH);
        FillRect(memory, &bounds, brush);
        let printed = PrintWindow(hwnd, memory, PW_RENDERFULLCONTENT);
        let image_size = u32::try_from(win_w as usize * win_h as usize * 4).ok();
        let pixels = if printed != 0 {
            image_size.and_then(|size| read_top_down(memory, bitmap, win_w, win_h, size).ok())
        } else {
            None
        };
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap as _);
        let _ = DeleteDC(memory);
        let _ = ReleaseDC(std::ptr::null_mut(), screen);
        let pixels = pixels?;
        let left = (x - window.left) as usize;
        let top = (y - window.top) as usize;
        let src_stride = win_w as usize * 4;
        let dst_stride = width as usize * 4;
        let mut cropped = vec![0u8; dst_stride * height as usize];
        for row in 0..height as usize {
            let start = (top + row) * src_stride + left * 4;
            let end = start + dst_stride;
            let dest = row * dst_stride;
            cropped[dest..dest + dst_stride].copy_from_slice(&pixels[start..end]);
        }
        for pixel in cropped.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Some(BgraImage {
            width: width as u32,
            height: height as u32,
            pixels: cropped,
        })
    }
}

#[cfg(windows)]
fn mostly_blank(image: &BgraImage) -> bool {
    let stride = image.width as usize * 4;
    if image.pixels.len() < stride || image.height == 0 {
        return true;
    }
    let mut seen = 0u32;
    let mut dark = 0u32;
    let step = (image.height / 16).max(1);
    let mut y = 0u32;
    while y < image.height {
        let row = y as usize * stride;
        let mut x = 0usize;
        while x + 2 < stride {
            let sum = u32::from(image.pixels[row + x])
                + u32::from(image.pixels[row + x + 1])
                + u32::from(image.pixels[row + x + 2]);
            seen += 1;
            if sum < 12 {
                dark += 1;
            }
            x += 64;
        }
        y += step;
    }
    seen > 0 && dark * 100 / seen >= 96
}

#[cfg(not(windows))]
pub(in crate::shot) fn capture_bgra(
    _x: i32,
    _y: i32,
    _width: i32,
    _height: i32,
    _include_cursor: bool,
) -> Result<BgraImage, String> {
    Err("当前只支持在 Windows 上截图".into())
}
