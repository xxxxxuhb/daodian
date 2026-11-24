//! 抓屏和选区裁切。大图像素留在进程里，管道只传结果文件路径。

#[derive(Clone, Debug)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn crop_bgra(
    frame_w: u32,
    frame_h: u32,
    pixels: &[u8],
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if frame_w == 0 || frame_h == 0 || width == 0 || height == 0 {
        return Err("截图区域无效".into());
    }
    let expected = (frame_w as usize)
        .checked_mul(frame_h as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    if pixels.len() != expected || x < 0 || y < 0 {
        return Err("截图区域无效".into());
    }
    let x = x as u32;
    let y = y as u32;
    if x.saturating_add(width) > frame_w || y.saturating_add(height) > frame_h {
        return Err("截图区域超出屏幕".into());
    }
    let mut out = vec![0u8; width as usize * height as usize * 4];
    let stride = frame_w as usize * 4;
    let row = width as usize * 4;
    for line in 0..height as usize {
        let src = (y as usize + line) * stride + x as usize * 4;
        let dst = line * row;
        out[dst..dst + row].copy_from_slice(&pixels[src..src + row]);
    }
    Ok(out)
}

pub fn encode_png(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    use std::io::Cursor;

    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    if width == 0 || height == 0 || bgra.len() != expected {
        return Err("截图区域无效".into());
    }
    let mut rgba = bgra.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(
        Cursor::new(&mut bytes),
        CompressionType::Fast,
        FilterType::NoFilter,
    )
    .write_image(
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|err| format!("保存截图失败: {err}"))?;
    Ok(bytes)
}

#[cfg(windows)]
pub fn capture_monitors() -> Result<Vec<Frame>, String> {
    let rects = monitor_rects()?;
    let mut frames = Vec::with_capacity(rects.len());
    for (x, y, width, height) in rects {
        frames.push(capture_bgra(x, y, width, height)?);
    }
    Ok(frames)
}

#[cfg(windows)]
fn monitor_rects() -> Result<Vec<(i32, i32, u32, u32)>, String> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
    };

    struct List(Vec<(i32, i32, u32, u32)>);

    unsafe extern "system" fn each(
        _monitor: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> windows_sys::core::BOOL {
        let list = unsafe { &mut *(data as *mut List) };
        if list.0.len() >= 8 {
            return 0;
        }
        let rect = unsafe { &*rect };
        let width = rect.right.saturating_sub(rect.left);
        let height = rect.bottom.saturating_sub(rect.top);
        if width > 0 && height > 0 {
            list.0
                .push((rect.left, rect.top, width as u32, height as u32));
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

#[cfg(windows)]
fn capture_bgra(x: i32, y: i32, width: u32, height: u32) -> Result<Frame, String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, SRCCOPY,
        SelectObject,
    };

    let width_i = i32::try_from(width).map_err(|_| "截图区域无效")?;
    let height_i = i32::try_from(height).map_err(|_| "截图区域无效")?;
    if width_i <= 0 || height_i <= 0 || width > 16_000 || height > 16_000 {
        return Err("截图区域无效".into());
    }
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or("截图区域无效")?;
    let image_size = u32::try_from(bytes).map_err(|_| "截图区域无效")?;
    unsafe {
        let owned = monitor_dc(x, y, width_i, height_i);
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
        let bitmap = CreateCompatibleBitmap(screen, width_i, height_i);
        if bitmap.is_null() {
            let _ = DeleteDC(memory);
            release(screen);
            return Err("创建截图位图失败".into());
        }
        let previous = SelectObject(memory, bitmap as _);
        let copied = BitBlt(
            memory, 0, 0, width_i, height_i, screen, blit_x, blit_y, SRCCOPY,
        );
        if copied == 0 {
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap as _);
            let _ = DeleteDC(memory);
            release(screen);
            return Err("复制屏幕失败".into());
        }
        let pixels = read_top_down(memory, bitmap, width_i, height_i, image_size);
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap as _);
        let _ = DeleteDC(memory);
        release(screen);
        let mut pixels = pixels?;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(Frame {
            x,
            y,
            width,
            height,
            pixels,
        })
    }
}

#[cfg(windows)]
fn monitor_dc(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Option<windows_sys::Win32::Graphics::Gdi::HDC> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
        MONITORINFOEXW,
    };

    struct Hit {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        device: [u16; 32],
        found: bool,
    }

    unsafe extern "system" fn each(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> windows_sys::core::BOOL {
        unsafe {
            let hit = &mut *(data as *mut Hit);
            let mut info: MONITORINFOEXW = std::mem::zeroed();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) == 0
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

    let mut hit = Hit {
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
            Some(each),
            &mut hit as *mut Hit as LPARAM,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_keeps_the_chosen_pixel() {
        let mut pixels = vec![0u8; 8 * 4];
        pixels[4 * 4] = 7;
        pixels[4 * 4 + 1] = 8;
        pixels[4 * 4 + 2] = 9;
        let cropped = crop_bgra(4, 2, &pixels, 0, 1, 1, 1).unwrap();
        assert_eq!(cropped, vec![7, 8, 9, 0]);
    }

    #[test]
    fn png_roundtrip_swaps_channels_back() {
        let bgra = vec![10, 20, 30, 1, 40, 50, 60, 2];
        let bytes = encode_png(2, 1, &bgra).unwrap();
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(image.as_raw(), &[30, 20, 10, 255, 60, 50, 40, 255]);
    }
}
