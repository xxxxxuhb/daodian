//! 把一块屏幕抓成自上而下的 BGRA。
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
        if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) == 0 {
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
fn monitor_dc(x: i32, y: i32, width: i32, height: i32) -> Option<windows_sys::Win32::Graphics::Gdi::HDC> {
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
pub(super) fn capture_bgra(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    include_cursor: bool,
) -> Result<BgraImage, String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        SRCCOPY, SelectObject,
    };

    if width <= 0 || height <= 0 || width > 16_000 || height > 16_000 {
        return Err("截图区域无效".into());
    }
    let _dpi = ThreadDpi::per_monitor_v2();
    let bytes = width as usize * height as usize * 4;
    let image_size = u32::try_from(bytes).map_err(|_| "截图区域无效")?;
    unsafe {
        let owned = monitor_dc(x, y, width, height);
        let screen = match owned {
            Some(dc) => dc,
            None => GetDC(std::ptr::null_mut()),
        };
        let blit_x = if owned.is_some() { 0 } else { x };
        let blit_y = if owned.is_some() { 0 } else { y };
        let release = |dc| {
            if owned.is_some() {
                let _ = DeleteDC(dc);
            } else {
                let _ = windows_sys::Win32::Graphics::Gdi::ReleaseDC(std::ptr::null_mut(), dc);
            }
        };
        if screen.is_null() {
            return Err("拿不到屏幕".into());
        }
        let memory = CreateCompatibleDC(screen);
        if memory.is_null() {
            release(screen);
            return Err("创建截图设备失败".into());
        }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap.is_null() {
            let _ = DeleteDC(memory);
            release(screen);
            return Err("创建截图位图失败".into());
        }
        let previous = SelectObject(memory, bitmap as _);
        let copied = BitBlt(memory, 0, 0, width, height, screen, blit_x, blit_y, SRCCOPY);
        if copied == 0 {
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap as _);
            let _ = DeleteDC(memory);
            release(screen);
            return Err("复制屏幕失败".into());
        }
        if include_cursor {
            draw_cursor(memory, x, y);
        }
        let read = read_top_down(memory, bitmap, width, height, image_size);
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap as _);
        let _ = DeleteDC(memory);
        release(screen);
        let mut pixels = read?;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(BgraImage {
            width: width as u32,
            height: height as u32,
            pixels,
        })
    }
}

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

#[cfg(not(windows))]
pub(super) fn capture_bgra(
    _x: i32,
    _y: i32,
    _width: i32,
    _height: i32,
    _include_cursor: bool,
) -> Result<BgraImage, String> {
    Err("当前只支持在 Windows 上截图".into())
}
