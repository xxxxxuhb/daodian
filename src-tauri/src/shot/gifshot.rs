//! GIF：选区外的控制条、穿透的边框。倒计时后按 10/15 帧抓选区，停下来再编码。

use std::time::Duration;

const MAX_WIDTH: u32 = 720;
const MAX_RECORD: Duration = Duration::from_secs(15);
const MAX_BYTES: usize = 150 * 1024 * 1024;
const SIZE_WARN: usize = 12 * 1024 * 1024;
const COUNTDOWN: u32 = 3;

#[derive(Clone)]
struct Scaled {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

pub(crate) fn is_active() -> bool {
    #[cfg(windows)]
    {
        win::is_open()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(crate) fn shutdown() {
    #[cfg(windows)]
    win::shutdown();
}

#[cfg(windows)]
pub(crate) fn start(app: tauri::AppHandle, grab: super::longshot::LiveGrab) -> Result<(), String> {
    win::start(app, grab)
}

fn scale_bgra(src_w: u32, src_h: u32, src: &[u8], max_w: u32) -> Option<Scaled> {
    if src_w == 0 || src_h == 0 || max_w == 0 {
        return None;
    }
    let expect = src_w as usize * src_h as usize * 4;
    if src.len() < expect {
        return None;
    }
    if src_w <= max_w {
        return Some(Scaled {
            width: src_w,
            height: src_h,
            pixels: src[..expect].to_vec(),
        });
    }
    let dst_w = max_w;
    let dst_h = ((u64::from(src_h) * u64::from(dst_w)) / u64::from(src_w)).max(1) as u32;
    let src_w = src_w as usize;
    let src_h = src_h as usize;
    let dst_w_px = dst_w as usize;
    let dst_h_px = dst_h as usize;
    let mut pixels = vec![0u8; dst_w_px * dst_h_px * 4];
    for y in 0..dst_h_px {
        let sy = y * src_h / dst_h_px;
        let sy2 = (sy + 1).min(src_h - 1);
        for x in 0..dst_w_px {
            let sx = x * src_w / dst_w_px;
            let sx2 = (sx + 1).min(src_w - 1);
            let samples = [
                (sy * src_w + sx) * 4,
                (sy * src_w + sx2) * 4,
                (sy2 * src_w + sx) * 4,
                (sy2 * src_w + sx2) * 4,
            ];
            let dest = (y * dst_w_px + x) * 4;
            for channel in 0..4 {
                let sum = samples
                    .iter()
                    .fold(0u32, |sum, index| sum + u32::from(src[index + channel]));
                pixels[dest + channel] = (sum / 4) as u8;
            }
        }
    }
    Some(Scaled {
        width: dst_w,
        height: dst_h,
        pixels,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn frame_limit(width: u32, height: u32) -> usize {
    let frame = width as usize * height as usize * 4;
    if frame == 0 {
        return 0;
    }
    MAX_BYTES / frame
}

fn format_clock(ms: u64) -> String {
    let total = ms / 1000;
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn delay_cs(fps: u32) -> u16 {
    let fps = fps.clamp(1, 60);
    ((100 + fps / 2) / fps).max(1) as u16
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{}KB", bytes.div_ceil(1024))
    }
}

fn encode_gif(
    frames: &[Scaled],
    fps: u32,
    mut cancel: impl FnMut() -> bool,
    mut progress: impl FnMut(u32, u32),
) -> Result<Vec<u8>, String> {
    if frames.is_empty() {
        return Err("还没有帧".into());
    }
    let width = u16::try_from(frames[0].width).map_err(|_| "画面太宽".to_string())?;
    let height = u16::try_from(frames[0].height).map_err(|_| "画面太高".to_string())?;
    let total = frames.len() as u32;
    progress(0, total);
    if cancel() {
        return Err("已取消".into());
    }
    let palette = shared_palette(frames);
    let cube = palette_cube(&palette);
    let mut output = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut output, width, height, &palette)
            .map_err(|err| format!("GIF 编码器打不开: {err}"))?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|err| format!("GIF 循环设置失败: {err}"))?;
        let delay = delay_cs(fps);
        for (index, frame) in frames.iter().enumerate() {
            if cancel() {
                return Err("已取消".into());
            }
            if frame.width != frames[0].width || frame.height != frames[0].height {
                return Err("帧尺寸不一致".into());
            }
            let indices = dither(
                &frame.pixels,
                frame.width,
                frame.height,
                &palette,
                &cube,
                &mut cancel,
            )?;
            let mut gif_frame = gif::Frame::default();
            gif_frame.delay = delay;
            gif_frame.width = width;
            gif_frame.height = height;
            gif_frame.buffer = std::borrow::Cow::Owned(indices);
            encoder
                .write_frame(&gif_frame)
                .map_err(|err| format!("GIF 写入失败: {err}"))?;
            progress(index as u32 + 1, total);
        }
    }
    Ok(output)
}

fn shared_palette(frames: &[Scaled]) -> Vec<u8> {
    let mut indexes = Vec::with_capacity(3);
    let last = frames.len() - 1;
    for index in [0, frames.len() / 2, last] {
        if !indexes.contains(&index) {
            indexes.push(index);
        }
    }
    let budget = 4096 / indexes.len().max(1);
    let mut sample = Vec::with_capacity(indexes.len() * budget * 4);
    for index in indexes {
        let frame = &frames[index];
        let pixels = (frame.width as usize).saturating_mul(frame.height as usize);
        if pixels == 0 {
            continue;
        }
        let take = budget.min(pixels);
        let step = (pixels / take).max(1);
        let mut got = 0usize;
        for pixel in frame.pixels.chunks_exact(4).step_by(step) {
            sample.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
            got += 1;
            if got >= take {
                break;
            }
        }
    }
    if sample.len() < 64 * 4 {
        let pixel = sample.first_chunk::<4>().copied().unwrap_or([0, 0, 0, 255]);
        while sample.len() < 64 * 4 {
            sample.extend_from_slice(&pixel);
        }
    }
    color_quant::NeuQuant::new(1, 256, &sample).color_map_rgb()
}

fn dither(
    bgra: &[u8],
    width: u32,
    height: u32,
    palette: &[u8],
    cube: &[u8],
    cancel: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>, String> {
    let width = width as usize;
    let height = height as usize;
    let mut work = vec![0i16; width * height * 3];
    for (index, pixel) in bgra.chunks_exact(4).take(width * height).enumerate() {
        work[index * 3] = i16::from(pixel[2]);
        work[index * 3 + 1] = i16::from(pixel[1]);
        work[index * 3 + 2] = i16::from(pixel[0]);
    }
    let mut indices = vec![0u8; width * height];
    for y in 0..height {
        if y % 16 == 0 && cancel() {
            return Err("已取消".into());
        }
        for x in 0..width {
            let at = (y * width + x) * 3;
            let red = work[at].clamp(0, 255);
            let green = work[at + 1].clamp(0, 255);
            let blue = work[at + 2].clamp(0, 255);
            let chosen = cube_index(cube, red, green, blue);
            indices[y * width + x] = chosen;
            let base = usize::from(chosen) * 3;
            let err_r = red - i16::from(palette[base]);
            let err_g = green - i16::from(palette[base + 1]);
            let err_b = blue - i16::from(palette[base + 2]);
            spread(&mut work, width, height, x, y, err_r, err_g, err_b);
        }
    }
    Ok(indices)
}

fn palette_cube(palette: &[u8]) -> Vec<u8> {
    let mut cube = vec![0u8; 4096];
    for red in 0..16usize {
        for green in 0..16usize {
            for blue in 0..16usize {
                cube[(red << 8) | (green << 4) | blue] = nearest(
                    palette,
                    (red as u8).saturating_mul(17),
                    (green as u8).saturating_mul(17),
                    (blue as u8).saturating_mul(17),
                );
            }
        }
    }
    cube
}

fn nearest(palette: &[u8], red: u8, green: u8, blue: u8) -> u8 {
    let mut best = 0u8;
    let mut best_distance = i32::MAX;
    for (index, color) in palette.chunks_exact(3).enumerate() {
        let dr = i32::from(red) - i32::from(color[0]);
        let dg = i32::from(green) - i32::from(color[1]);
        let db = i32::from(blue) - i32::from(color[2]);
        let distance = dr * dr + dg * dg + db * db;
        if distance < best_distance {
            best_distance = distance;
            best = index as u8;
        }
    }
    best
}

fn cube_index(cube: &[u8], red: i16, green: i16, blue: i16) -> u8 {
    let red = usize::from(red.clamp(0, 255) as u8) >> 4;
    let green = usize::from(green.clamp(0, 255) as u8) >> 4;
    let blue = usize::from(blue.clamp(0, 255) as u8) >> 4;
    cube[(red << 8) | (green << 4) | blue]
}

fn spread(
    work: &mut [i16],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    err_r: i16,
    err_g: i16,
    err_b: i16,
) {
    let apply = |work: &mut [i16], x: isize, y: isize, weight: i16| {
        if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
            return;
        }
        let at = (y as usize * width + x as usize) * 3;
        work[at] = work[at].saturating_add(err_r.saturating_mul(weight) / 16);
        work[at + 1] = work[at + 1].saturating_add(err_g.saturating_mul(weight) / 16);
        work[at + 2] = work[at + 2].saturating_add(err_b.saturating_mul(weight) / 16);
    };
    apply(work, x as isize + 1, y as isize, 7);
    apply(work, x as isize - 1, y as isize + 1, 3);
    apply(work, x as isize, y as isize + 1, 5);
    apply(work, x as isize + 1, y as isize + 1, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_and_delay_follow_the_recording_limits() {
        assert_eq!(format_clock(0), "00:00");
        assert_eq!(format_clock(3_000), "00:03");
        assert_eq!(format_clock(15_000), "00:15");
        assert_eq!(delay_cs(10), 10);
        assert_eq!(delay_cs(15), 7);
        assert!(frame_limit(720, 405) >= 100);
        assert!(frame_limit(720, 405) < 200);
    }

    #[test]
    fn scale_keeps_a_narrow_frame_and_halves_a_wide_one() {
        let wide = vec![10u8; 16 * 8 * 4];
        let scaled = scale_bgra(16, 8, &wide, 8).expect("wide");
        assert_eq!((scaled.width, scaled.height), (8, 4));
        let narrow = vec![10u8; 8 * 4 * 4];
        let same = scale_bgra(8, 4, &narrow, MAX_WIDTH).expect("narrow");
        assert_eq!((same.width, same.height), (8, 4));
    }

    #[test]
    fn encoded_gif_loops_the_frames() {
        let pixels = vec![0, 0, 220, 255].repeat(8 * 8);
        let frame = Scaled {
            width: 8,
            height: 8,
            pixels,
        };
        let cancelled = encode_gif(&[frame.clone()], 10, || true, |_, _| {}).unwrap_err();
        assert_eq!(cancelled, "已取消");
        let mut hits = 0u32;
        let mid = encode_gif(
            &[frame.clone()],
            10,
            || {
                hits += 1;
                hits >= 3
            },
            |_, _| {},
        )
        .expect_err("cancel inside a frame");
        assert_eq!(mid, "已取消");
        let bytes = encode_gif(&[frame.clone(), frame], 10, || false, |_, _| {}).expect("gif");
        assert!(bytes.starts_with(b"GIF89a"));
        let decoder = gif::DecodeOptions::new();
        let mut reader = decoder.read_info(bytes.as_slice()).expect("decode");
        assert!(reader.read_next_frame().expect("frame").is_some());
        assert!(reader.read_next_frame().expect("frame").is_some());
        assert!(reader.read_next_frame().expect("frame").is_none());
    }
}

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use tauri::{AppHandle, Manager};
    use windows_sys::Win32::Foundation::{
        COLORREF, GlobalFree, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
        CombineRgn, CreateFontW, CreateRectRgn, CreateSolidBrush, DEFAULT_CHARSET,
        DEFAULT_GUI_FONT, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
        DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_SEMIBOLD,
        FillRect, GetStockObject, HALFTONE, HDC, InvalidateRect, NULL_PEN, OUT_DEFAULT_PRECIS,
        PAINTSTRUCT, RGBQUAD, RGN_DIFF, RoundRect, SRCCOPY, SelectObject, SetBkMode,
        SetStretchBltMode, SetTextColor, SetWindowRgn, StretchDIBits, TRANSPARENT,
    };
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE,
    };
    use windows_sys::Win32::UI::Shell::DROPFILES;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
        GetClientRect, HWND_TOPMOST, IDC_ARROW, KBDLLHOOKSTRUCT, KillTimer, LoadCursorW,
        PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetTimer, SetWindowDisplayAffinity, SetWindowPos, SetWindowsHookExW,
        ShowWindow, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND,
        WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SYSKEYDOWN, WM_TIMER,
        WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_POPUP,
    };

    use super::super::capture::capture_bgra;
    use super::super::stitch::{BAR_H, BAR_W, LiveLayout, ScreenRect, live_layout};
    use super::{
        COUNTDOWN, MAX_BYTES, MAX_RECORD, MAX_WIDTH, SIZE_WARN, Scaled, encode_gif, format_clock,
        format_size, scale_bgra,
    };
    use super::{rgb, utf16_fixed};

    const CLASS_NAME: [u16; 32] = utf16_fixed("DaodianGifShot");
    const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
    const WM_APP_COUNT: u32 = 0x8000 + 41;
    const WM_APP_FRAME: u32 = 0x8000 + 42;
    const WM_APP_FINISH: u32 = 0x8000 + 43;
    const WM_APP_PROGRESS: u32 = 0x8000 + 44;
    const WM_APP_DONE: u32 = 0x8000 + 45;
    const WM_APP_CANCEL: u32 = 0x8000 + 46;
    const WM_APP_SAVED: u32 = 0x8000 + 47;
    const WM_APP_NOTE: u32 = 0x8000 + 48;
    const WM_MOUSELEAVE: u32 = 675;
    const FINISH_CANCEL: usize = 1;
    const FINISH_USER: usize = 2;
    const FINISH_TIME: usize = 3;
    const FINISH_MEMORY: usize = 4;
    const TIMER_PLAY: usize = 1;
    const TIMER_WATCH: usize = 2;
    const STOP_WAIT_MS: u64 = 800;
    const CAPTURE_STALL_MS: u64 = 2_000;
    const ENCODE_STALL_MS: u64 = 30_000;

    const BG: COLORREF = rgb(12, 31, 26);
    const TEXT: COLORREF = rgb(233, 241, 236);
    const HOVER: COLORREF = rgb(28, 54, 48);
    const PRESS: COLORREF = rgb(36, 68, 60);
    const FRAME: COLORREF = rgb(143, 208, 234);

    static SESSION: OnceLock<Mutex<Option<GifSession>>> = OnceLock::new();
    static HOOK_BAR: AtomicIsize = AtomicIsize::new(0);
    static RESULT: Mutex<Option<Result<Vec<u8>, String>>> = Mutex::new(None);
    static SAVE_ERROR: Mutex<String> = Mutex::new(String::new());
    static NOTE: Mutex<String> = Mutex::new(String::new());
    static CLIP_FILE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Stage {
        Countdown,
        Recording,
        Encoding,
        Preview,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Action {
        Fps,
        Cancel,
        Pause,
        Stop,
        Copy,
        Save,
        Again,
        Close,
    }

    struct Mail {
        stop: AtomicBool,
        cancel: AtomicBool,
        paused: AtomicBool,
        fps: AtomicU32,
        generation: AtomicU64,
        elapsed_ms: AtomicU64,
        frames: Mutex<Vec<Scaled>>,
        used: AtomicUsize,
        beat_ms: AtomicU64,
    }

    struct GifSession {
        app: AppHandle,
        layout: LiveLayout,
        monitor: ScreenRect,
        border: isize,
        bar: isize,
        preview: isize,
        hot: i32,
        down: i32,
        stage: Stage,
        frames: Option<Arc<Vec<Scaled>>>,
        play: usize,
        gif: Vec<u8>,
        hint: String,
        saving: bool,
        ending: bool,
        close_session: bool,
        mail: Arc<Mail>,
        hook_key: isize,
    }

    pub(super) fn is_open() -> bool {
        session().is_some()
    }

    pub(super) fn shutdown() {
        request_end(false);
    }

    pub(super) fn start(
        app: AppHandle,
        grab: super::super::longshot::LiveGrab,
    ) -> Result<(), String> {
        if session().is_some() {
            return Err("GIF 还开着".into());
        }
        if super::super::longshot::is_active() {
            return Err("长截图还开着".into());
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
        let layout = live_layout(selection, monitor).ok_or("选区太小，或贴着屏幕边")?;
        let border = create_window(
            layout.border.x,
            layout.border.y,
            layout.border.width,
            layout.border.height,
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
        )?;
        let bar = match create_window(
            layout.bar.x,
            layout.bar.y,
            layout.bar.width,
            layout.bar.height,
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
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
        let mail = Arc::new(Mail {
            stop: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            fps: AtomicU32::new(10),
            generation: AtomicU64::new(1),
            elapsed_ms: AtomicU64::new(0),
            frames: Mutex::new(Vec::new()),
            used: AtomicUsize::new(0),
            beat_ms: AtomicU64::new(now_ms()),
        });
        let hook = install_hook();
        HOOK_BAR.store(bar as isize, Ordering::SeqCst);
        {
            let mut guard = session();
            *guard = Some(GifSession {
                app,
                layout,
                monitor,
                border: border as isize,
                bar: bar as isize,
                preview: 0,
                hot: -1,
                down: -1,
                stage: Stage::Countdown,
                frames: None,
                play: 0,
                gif: Vec::new(),
                hint: COUNTDOWN.to_string(),
                saving: false,
                ending: false,
                close_session: false,
                mail: Arc::clone(&mail),
                hook_key: hook,
            });
        }
        show_top(border);
        show_top(bar);
        unsafe {
            SetTimer(bar, TIMER_WATCH, 500, None);
        }
        spawn_worker(mail, 1);
        // 同一条语句里两次 session() 会把互斥锁再锁一次，主线程直接卡死。
        let (width, height) = {
            let guard = session();
            guard
                .as_ref()
                .map(|live| (live.layout.capture.width, live.layout.capture.height))
                .unwrap_or((0, 0))
        };
        crate::logging::info(format_args!("gif started {width}x{height}"));
        Ok(())
    }

    fn session() -> std::sync::MutexGuard<'static, Option<GifSession>> {
        SESSION
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn spawn_worker(mail: Arc<Mail>, generation: u64) {
        let hole = session()
            .as_ref()
            .map(|live| live.layout.capture)
            .unwrap_or(ScreenRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
        let bar = session().as_ref().map(|live| live.bar).unwrap_or(0);
        thread::Builder::new()
            .name("gif-capture".into())
            .spawn(move || capture_loop(mail, hole, bar, generation))
            .ok();
    }

    fn capture_loop(mail: Arc<Mail>, hole: ScreenRect, bar: isize, generation: u64) {
        unsafe {
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
        for left in (1..=COUNTDOWN).rev() {
            if !current(&mail, generation) {
                post(bar, WM_APP_FINISH, FINISH_CANCEL, generation);
                return;
            }
            post(bar, WM_APP_COUNT, left as usize, generation);
            if !wait_slice(&mail, generation, Duration::from_secs(1)) {
                post(bar, WM_APP_FINISH, FINISH_CANCEL, generation);
                return;
            }
        }
        if !current(&mail, generation) {
            post(bar, WM_APP_FINISH, FINISH_CANCEL, generation);
            return;
        }
        post(bar, WM_APP_COUNT, 0, generation);
        let mut elapsed = Duration::ZERO;
        let mut stamp = Instant::now();
        let reason;
        loop {
            if !current(&mail, generation) {
                reason = if mail.cancel.load(Ordering::SeqCst) {
                    FINISH_CANCEL
                } else {
                    FINISH_USER
                };
                break;
            }
            if mail.paused.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(30));
                stamp = Instant::now();
                continue;
            }
            let now = Instant::now();
            elapsed += now.saturating_duration_since(stamp);
            stamp = now;
            mail.elapsed_ms
                .store(elapsed.as_millis() as u64, Ordering::SeqCst);
            if elapsed >= MAX_RECORD {
                reason = FINISH_TIME;
                break;
            }
            let started = Instant::now();
            touch(&mail);
            if let Ok(image) =
                capture_bgra(hole.x, hole.y, hole.width as i32, hole.height as i32, false)
            {
                if let Some(scaled) =
                    scale_bgra(image.width, image.height, &image.pixels, MAX_WIDTH)
                {
                    let mut slot = mail.frames.lock().unwrap_or_else(|err| err.into_inner());
                    if current(&mail, generation) && !mail.cancel.load(Ordering::SeqCst) {
                        let next = mail
                            .used
                            .load(Ordering::SeqCst)
                            .saturating_add(scaled.pixels.len());
                        if slot.is_empty() || next <= MAX_BYTES {
                            mail.used.fetch_add(scaled.pixels.len(), Ordering::SeqCst);
                            let full = mail.used.load(Ordering::SeqCst) >= MAX_BYTES;
                            slot.push(scaled);
                            drop(slot);
                            post(bar, WM_APP_FRAME, 0, generation);
                            if full {
                                reason = FINISH_MEMORY;
                                break;
                            }
                        } else {
                            reason = FINISH_MEMORY;
                            break;
                        }
                    }
                }
            }
            let fps = mail.fps.load(Ordering::SeqCst).clamp(10, 15) as u64;
            let interval = Duration::from_millis(1000 / fps);
            thread::sleep(interval.saturating_sub(started.elapsed()));
        }
        post(bar, WM_APP_FINISH, reason, generation);
    }

    fn current(mail: &Mail, generation: u64) -> bool {
        !mail.stop.load(Ordering::SeqCst) && mail.generation.load(Ordering::SeqCst) == generation
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    }

    fn touch(mail: &Mail) {
        mail.beat_ms.store(now_ms(), Ordering::SeqCst);
    }

    fn wait_slice(mail: &Mail, generation: u64, total: Duration) -> bool {
        let mut left = total;
        while !left.is_zero() {
            if !current(mail, generation) {
                return false;
            }
            let slice = left.min(Duration::from_millis(100));
            thread::sleep(slice);
            left = left.saturating_sub(slice);
            touch(mail);
        }
        current(mail, generation)
    }

    fn watch() {
        let action = {
            let guard = session();
            let Some(live) = guard.as_ref() else {
                return;
            };
            if live.ending {
                return;
            }
            let age = now_ms().saturating_sub(live.mail.beat_ms.load(Ordering::SeqCst));
            let generation = live.mail.generation.load(Ordering::SeqCst);
            if live.stage == Stage::Recording
                && ((live.mail.stop.load(Ordering::SeqCst) && age >= STOP_WAIT_MS)
                    || (!live.mail.paused.load(Ordering::SeqCst) && age >= CAPTURE_STALL_MS))
            {
                Some((true, generation))
            } else if live.stage == Stage::Encoding && age >= ENCODE_STALL_MS {
                Some((false, generation))
            } else {
                None
            }
        };
        let Some((capture, generation)) = action else {
            return;
        };
        if capture {
            crate::logging::warn(format_args!("gif capture stalled"));
            if let Some(live) = session().as_ref() {
                live.mail.stop.store(true, Ordering::SeqCst);
            }
            on_finish(FINISH_USER, generation);
            return;
        }
        let force = {
            let guard = session();
            let Some(live) = guard.as_ref() else {
                return;
            };
            live.mail.cancel.store(true, Ordering::SeqCst);
            let age = now_ms().saturating_sub(live.mail.beat_ms.load(Ordering::SeqCst));
            live.stage == Stage::Encoding && age >= ENCODE_STALL_MS + 1_000
        };
        if !force {
            return;
        }
        let mut slot = RESULT.lock().unwrap_or_else(|err| err.into_inner());
        if slot.is_none() {
            crate::logging::warn(format_args!("gif encode stalled"));
            *slot = Some(Err("生成超时了".into()));
            drop(slot);
            on_encoded(generation);
        }
    }

    fn post(hwnd: isize, msg: u32, wparam: usize, generation: u64) {
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd as HWND, msg, wparam, generation as LPARAM);
            }
        }
    }

    fn on_count(left: usize, generation: u64) {
        let bar = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.mail.generation.load(Ordering::SeqCst) != generation || live.ending {
                return;
            }
            if left == 0 {
                live.stage = Stage::Recording;
                live.hint = format!("00:00 · {}帧", live.mail.fps.load(Ordering::SeqCst));
            } else {
                live.stage = Stage::Countdown;
                live.hint = left.to_string();
            }
            live.bar
        };
        repaint(bar);
    }

    fn on_frame(generation: u64) {
        let bar = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.mail.generation.load(Ordering::SeqCst) != generation
                || live.stage != Stage::Recording
                || live.ending
            {
                return;
            }
            let ms = live.mail.elapsed_ms.load(Ordering::SeqCst);
            let fps = live.mail.fps.load(Ordering::SeqCst);
            let count = live
                .mail
                .frames
                .lock()
                .map(|frames| frames.len())
                .unwrap_or(0);
            live.hint = format!("{} · {fps}帧 · {count}", format_clock(ms));
            live.bar
        };
        repaint(bar);
    }

    fn on_finish(reason: usize, generation: u64) {
        let job = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.mail.generation.load(Ordering::SeqCst) != generation || live.ending {
                return;
            }
            if live.stage != Stage::Countdown && live.stage != Stage::Recording {
                return;
            }
            if reason == FINISH_CANCEL || live.mail.cancel.load(Ordering::SeqCst) {
                live.ending = true;
                live.close_session = true;
                None
            } else {
                let frames = std::mem::take(
                    &mut *live
                        .mail
                        .frames
                        .lock()
                        .unwrap_or_else(|err| err.into_inner()),
                );
                if frames.is_empty() {
                    live.ending = true;
                    live.close_session = true;
                    None
                } else {
                    live.hint = match reason {
                        FINISH_TIME => "已经 15 秒，正在生成".into(),
                        FINISH_MEMORY => "内存满了，正在生成".into(),
                        _ => "正在生成".into(),
                    };
                    live.stage = Stage::Encoding;
                    live.mail.cancel.store(false, Ordering::SeqCst);
                    let shared = Arc::new(frames);
                    live.frames = Some(Arc::clone(&shared));
                    Some((
                        shared,
                        live.mail.fps.load(Ordering::SeqCst),
                        Arc::clone(&live.mail),
                        live.bar,
                    ))
                }
            }
        };
        let Some((frames, fps, mail, bar)) = job else {
            close_windows();
            return;
        };
        repaint(bar);
        let spawned = spawn_encode(frames, fps, mail, bar, generation);
        if spawned.is_err() {
            encoding_failed(bar);
        }
    }

    fn spawn_encode(
        frames: Arc<Vec<Scaled>>,
        fps: u32,
        mail: Arc<Mail>,
        bar: isize,
        generation: u64,
    ) -> std::io::Result<()> {
        thread::Builder::new()
            .name("gif-encode".into())
            .spawn(move || {
                unsafe {
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
                }
                crate::logging::info(format_args!("gif encoding {} frames", frames.len()));
                let encoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    encode_gif(
                        &frames,
                        fps,
                        || {
                            touch(&mail);
                            mail.cancel.load(Ordering::SeqCst)
                        },
                        |done, _total| post(bar, WM_APP_PROGRESS, done as usize, generation),
                    )
                }));
                let encoded = match encoded {
                    Ok(result) => result,
                    Err(_) => {
                        crate::logging::warn(format_args!("gif encode panicked"));
                        Err("生成中断了".into())
                    }
                };
                if let Ok(bytes) = encoded.as_ref() {
                    crate::logging::info(format_args!("gif encoded {} bytes", bytes.len()));
                }
                *RESULT.lock().unwrap_or_else(|err| err.into_inner()) = Some(encoded);
                post(bar, WM_APP_DONE, 0, generation);
            })
            .map(|_| ())
    }

    fn encoding_failed(bar: isize) {
        if let Some(live) = session().as_mut() {
            live.stage = Stage::Recording;
            live.mail.paused.store(true, Ordering::SeqCst);
            live.mail.stop.store(true, Ordering::SeqCst);
            live.hint = "生成没有启动".into();
        }
        repaint(bar);
    }

    fn on_progress(done: usize, generation: u64) {
        let bar = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.stage != Stage::Encoding
                || live.mail.generation.load(Ordering::SeqCst) != generation
            {
                return;
            }
            let total = live.frames.as_ref().map(|frames| frames.len()).unwrap_or(0);
            live.hint = format!("正在生成 {done}/{total}");
            live.bar
        };
        repaint(bar);
    }

    fn on_encoded(generation: u64) {
        let open = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.mail.generation.load(Ordering::SeqCst) != generation
                || live.ending
                || live.stage != Stage::Encoding
            {
                return;
            }
            let outcome = RESULT.lock().unwrap_or_else(|err| err.into_inner()).take();
            match outcome {
                Some(Ok(bytes)) => {
                    let oversized = bytes.len() > SIZE_WARN;
                    let frames = live.frames.as_ref().map(|frames| frames.len()).unwrap_or(0);
                    let (width, height) = live
                        .frames
                        .as_ref()
                        .and_then(|frames| frames.first())
                        .map(|frame| (frame.width, frame.height))
                        .unwrap_or((0, 0));
                    live.gif = bytes;
                    live.stage = Stage::Preview;
                    live.play = 0;
                    live.hint = format!(
                        "{frames} 帧 · {width}×{height} · {}",
                        format_size(live.gif.len())
                    );
                    if oversized {
                        live.hint.push_str(" · 体积偏大，可以缩短或改成 10 帧");
                    }
                    Some((
                        live.monitor,
                        width,
                        height,
                        live.mail.fps.load(Ordering::SeqCst),
                    ))
                }
                Some(Err(err)) if err == "已取消" => {
                    live.stage = Stage::Recording;
                    live.mail.paused.store(true, Ordering::SeqCst);
                    live.hint = "生成已取消".into();
                    None
                }
                Some(Err(err)) => {
                    live.hint = err;
                    live.stage = Stage::Recording;
                    live.mail.paused.store(true, Ordering::SeqCst);
                    live.mail.stop.store(true, Ordering::SeqCst);
                    None
                }
                None => {
                    live.hint = "生成没有结果".into();
                    live.stage = Stage::Recording;
                    live.mail.paused.store(true, Ordering::SeqCst);
                    live.mail.stop.store(true, Ordering::SeqCst);
                    None
                }
            }
        };
        match open {
            Some((monitor, width, height, fps)) => open_preview(monitor, width, height, fps),
            None => {
                if session().as_ref().is_some_and(|live| live.ending) {
                    close_windows();
                } else if let Some(live) = session().as_ref() {
                    repaint(live.bar);
                }
            }
        }
    }

    fn open_preview(monitor: ScreenRect, width: u32, height: u32, fps: u32) {
        let (x, y, w, h) = preview_bounds(&monitor, width, height);
        let hwnd = match create_window(x, y, w as u32, h as u32, WS_EX_TOPMOST | WS_EX_TOOLWINDOW) {
            Ok(hwnd) => hwnd,
            Err(_) => {
                tell(0, "预览窗口打不开");
                return;
            }
        };
        let attached = {
            let mut guard = session();
            if let Some(live) = guard.as_mut() {
                live.preview = hwnd as isize;
                true
            } else {
                false
            }
        };
        if !attached {
            unsafe {
                DestroyWindow(hwnd);
            }
            return;
        }
        let (border, bar) = session()
            .as_ref()
            .map(|live| (live.border, live.bar))
            .unwrap_or((0, 0));
        unsafe {
            ShowWindow(border as HWND, SW_HIDE);
            ShowWindow(bar as HWND, SW_HIDE);
            let delay = (1000 / fps.clamp(10, 15)).max(1);
            SetTimer(hwnd, TIMER_PLAY, delay, None);
        }
        show_top(hwnd);
    }

    fn preview_bounds(monitor: &ScreenRect, width: u32, height: u32) -> (i32, i32, i32, i32) {
        let max_w = (monitor.width as i32 - 48).max(360);
        let max_h = (monitor.height as i32 - 80).max(280);
        let avail_h = (max_h - BAR_H).max(1);
        let scale = 1.0f64
            .min(max_w as f64 / f64::from(width.max(1)))
            .min(avail_h as f64 / f64::from(height.max(1)));
        let img_w = (f64::from(width) * scale).round().max(1.0) as i32;
        let img_h = (f64::from(height) * scale).round().max(1.0) as i32;
        let win_w = img_w.max(360).min(max_w);
        let win_h = (img_h + BAR_H).min(max_h);
        let x = monitor.x + (monitor.width as i32 - win_w) / 2;
        let y = monitor.y + (monitor.height as i32 - win_h) / 2;
        (x, y, win_w, win_h)
    }

    fn on_saved(code: usize) {
        let (preview, message) = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            live.saving = false;
            if live.ending {
                return;
            }
            let message = (code == 1).then(|| {
                std::mem::take(&mut *SAVE_ERROR.lock().unwrap_or_else(|err| err.into_inner()))
            });
            (live.preview, message)
        };
        if preview != 0 {
            unsafe {
                ShowWindow(preview as HWND, SW_SHOWNOACTIVATE);
            }
        }
        if let Some(message) = message.filter(|message| !message.is_empty()) {
            tell(preview, &message);
        } else if let Some(live) = session().as_mut() {
            if code == 0 {
                let base = live.hint.split(" · 已").next().unwrap_or("").to_string();
                live.hint = format!("{base} · 已保存");
            }
            repaint(live.preview);
        }
    }

    fn copy_gif() {
        let (bytes, name, preview, app) = {
            let guard = session();
            let Some(live) = guard.as_ref() else {
                return;
            };
            if live.gif.is_empty() || live.saving {
                return;
            }
            (
                live.gif.clone(),
                super::super::save::gif_name(),
                live.preview,
                live.app.clone(),
            )
        };
        if let Some(live) = session().as_mut() {
            live.hint = format!("{} · 正在复制", hint_base(&live.hint));
            repaint(live.preview);
        }
        let spawned = thread::Builder::new()
            .name("gif-copy".into())
            .spawn(move || {
                crate::logging::info(format_args!("gif copy started"));
                let result = (|| -> Result<std::path::PathBuf, String> {
                    let dir = clipboard_dir(&app)?;
                    let path = dir.join(&name);
                    std::fs::write(&path, &bytes).map_err(|err| format!("GIF 写不出来: {err}"))?;
                    copy_file(&path)?;
                    Ok(path)
                })();
                match result {
                    Ok(path) => {
                        let previous = CLIP_FILE
                            .lock()
                            .unwrap_or_else(|err| err.into_inner())
                            .replace(path);
                        if let Some(previous) = previous {
                            let _ = std::fs::remove_file(previous);
                        }
                        crate::logging::info(format_args!("gif copy finished"));
                        post(preview, WM_APP_NOTE, 0, 0);
                    }
                    Err(err) => {
                        crate::logging::warn(format_args!("gif copy failed: {err}"));
                        *NOTE.lock().unwrap_or_else(|err| err.into_inner()) = err;
                        post(preview, WM_APP_NOTE, 1, 0);
                    }
                }
            });
        if spawned.is_err() {
            tell(preview, "复制没有启动");
        }
    }

    fn on_note(code: usize) {
        let preview = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.ending {
                return;
            }
            if code == 0 {
                live.hint = format!("{} · 已复制文件", hint_base(&live.hint));
            } else {
                live.hint =
                    std::mem::take(&mut *NOTE.lock().unwrap_or_else(|err| err.into_inner()));
                if live.hint.is_empty() {
                    live.hint = "复制失败".into();
                }
            }
            live.preview
        };
        repaint(preview);
    }

    fn clipboard_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
        let store = app.state::<crate::store::SharedStore>();
        let base = store
            .lock()
            .map_err(|err| err.to_string())?
            .data_dir()?
            .to_path_buf();
        let dir = base.join("gif-cache");
        std::fs::create_dir_all(&dir).map_err(|err| format!("复制目录打不开: {err}"))?;
        Ok(dir)
    }

    fn hint_base(hint: &str) -> &str {
        hint.split(" · 正在")
            .next()
            .unwrap_or(hint)
            .split(" · 已")
            .next()
            .unwrap_or(hint)
    }

    fn copy_file(path: &std::path::Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .chain(std::iter::once(0))
            .collect();
        const CF_HDROP: u32 = 15;
        let header = std::mem::size_of::<DROPFILES>();
        let bytes = header + wide.len() * 2;
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if memory.is_null() {
                return Err("剪贴板没有空间".into());
            }
            let locked = GlobalLock(memory) as *mut u8;
            if locked.is_null() {
                GlobalFree(memory);
                return Err("剪贴板没有空间".into());
            }
            let drop = DROPFILES {
                pFiles: header as u32,
                pt: POINT { x: 0, y: 0 },
                fNC: 0,
                fWide: 1,
            };
            std::ptr::write(locked as *mut DROPFILES, drop);
            std::ptr::copy_nonoverlapping(
                wide.as_ptr() as *const u8,
                locked.add(header),
                wide.len() * 2,
            );
            GlobalUnlock(memory);
            let started = Instant::now();
            let mut opened = false;
            while started.elapsed() < Duration::from_secs(1) {
                if OpenClipboard(std::ptr::null_mut()) != 0 {
                    opened = true;
                    break;
                }
                thread::sleep(Duration::from_millis(40));
            }
            if !opened {
                GlobalFree(memory);
                return Err("剪贴板被占用".into());
            }
            EmptyClipboard();
            if SetClipboardData(CF_HDROP, memory).is_null() {
                CloseClipboard();
                GlobalFree(memory);
                return Err("复制文件失败".into());
            }
            CloseClipboard();
        }
        Ok(())
    }

    fn save_gif() {
        let (app, preview, windows, bytes, name) = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.saving || live.gif.is_empty() {
                return;
            }
            live.saving = true;
            let windows = [live.border, live.bar, live.preview];
            (
                live.app.clone(),
                live.preview,
                windows,
                live.gif.clone(),
                super::super::save::gif_name(),
            )
        };
        for hwnd in windows {
            if hwnd != 0 {
                unsafe {
                    ShowWindow(hwnd as HWND, SW_HIDE);
                }
            }
        }
        crate::logging::info(format_args!("gif save started"));
        let spawned = thread::Builder::new()
            .name("gif-save".into())
            .spawn(move || {
                let code = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    super::super::save::save_gif_dialog(&app, &bytes, &name)
                })) {
                    Ok(Ok(())) => 0,
                    Ok(Err(err)) => {
                        *SAVE_ERROR.lock().unwrap_or_else(|err| err.into_inner()) = err;
                        1
                    }
                    Err(_) => {
                        *SAVE_ERROR.lock().unwrap_or_else(|err| err.into_inner()) =
                            "保存中断了".into();
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
                ShowWindow(preview as HWND, SW_SHOWNOACTIVATE);
            }
            tell(preview, "保存没有启动");
        }
    }

    fn reroll() {
        let (mail, border, bar, preview) = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.saving {
                return;
            }
            let generation = live.mail.generation.fetch_add(1, Ordering::SeqCst) + 1;
            live.mail.stop.store(true, Ordering::SeqCst);
            live.mail.cancel.store(true, Ordering::SeqCst);
            live.mail.paused.store(false, Ordering::SeqCst);
            live.mail.elapsed_ms.store(0, Ordering::SeqCst);
            live.mail.used.store(0, Ordering::SeqCst);
            live.mail
                .frames
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clear();
            live.mail.stop.store(false, Ordering::SeqCst);
            live.mail.cancel.store(false, Ordering::SeqCst);
            live.mail.generation.store(generation, Ordering::SeqCst);
            touch(&live.mail);
            live.stage = Stage::Countdown;
            live.frames = None;
            live.play = 0;
            live.gif.clear();
            live.hint = COUNTDOWN.to_string();
            live.hot = -1;
            live.down = -1;
            (
                Arc::clone(&live.mail),
                live.border,
                live.bar,
                std::mem::take(&mut live.preview),
            )
        };
        if preview != 0 {
            unsafe {
                KillTimer(preview as HWND, TIMER_PLAY);
                DestroyWindow(preview as HWND);
            }
        }
        unsafe {
            ShowWindow(border as HWND, SW_SHOWNOACTIVATE);
            ShowWindow(bar as HWND, SW_SHOWNOACTIVATE);
        }
        let generation = mail.generation.load(Ordering::SeqCst);
        spawn_worker(mail, generation);
        repaint(bar);
    }

    fn press(action: Action) {
        match action {
            Action::Fps => {
                let mut guard = session();
                let Some(live) = guard.as_mut() else {
                    return;
                };
                if live.stage != Stage::Countdown {
                    return;
                }
                let next = if live.mail.fps.load(Ordering::SeqCst) == 15 {
                    10
                } else {
                    15
                };
                live.mail.fps.store(next, Ordering::SeqCst);
            }
            Action::Pause => {
                let mut guard = session();
                let Some(live) = guard.as_mut() else {
                    return;
                };
                if live.stage != Stage::Recording {
                    return;
                }
                let paused = !live.mail.paused.load(Ordering::SeqCst);
                live.mail.paused.store(paused, Ordering::SeqCst);
            }
            Action::Stop => {
                let again = session().as_ref().and_then(|live| {
                    (live.stage == Stage::Recording
                        && live.frames.is_some()
                        && live.mail.stop.load(Ordering::SeqCst))
                    .then(|| {
                        (
                            Arc::clone(live.frames.as_ref().expect("frames")),
                            live.mail.fps.load(Ordering::SeqCst),
                            Arc::clone(&live.mail),
                            live.bar,
                            live.mail.generation.load(Ordering::SeqCst),
                        )
                    })
                });
                if let Some((frames, fps, mail, bar, generation)) = again {
                    mail.cancel.store(false, Ordering::SeqCst);
                    if let Some(live) = session().as_mut() {
                        live.stage = Stage::Encoding;
                        live.hint = "正在生成".into();
                    }
                    if spawn_encode(frames, fps, mail, bar, generation).is_err() {
                        encoding_failed(bar);
                    }
                } else if let Some(live) = session().as_ref() {
                    if live.stage == Stage::Recording {
                        live.mail.stop.store(true, Ordering::SeqCst);
                    }
                }
            }
            Action::Cancel => {
                let close = {
                    let mut guard = session();
                    let Some(live) = guard.as_mut() else {
                        return;
                    };
                    live.mail.cancel.store(true, Ordering::SeqCst);
                    live.mail.stop.store(true, Ordering::SeqCst);
                    if live.stage == Stage::Encoding {
                        live.hint = "正在取消".into();
                        false
                    } else {
                        true
                    }
                };
                if close {
                    request_end(true);
                }
            }
            Action::Copy => copy_gif(),
            Action::Save => save_gif(),
            Action::Again => reroll(),
            Action::Close => request_end(true),
        }
        if let Some(live) = session().as_ref() {
            repaint(if live.stage == Stage::Preview {
                live.preview
            } else {
                live.bar
            });
        }
    }

    fn request_end(close_session: bool) {
        let hwnds = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.ending {
                live.close_session |= close_session;
                return;
            }
            live.ending = true;
            live.close_session = close_session;
            live.mail.stop.store(true, Ordering::SeqCst);
            live.mail.cancel.store(true, Ordering::SeqCst);
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

    fn close_windows() {
        let hwnds = session()
            .as_ref()
            .map(|live| [live.border, live.bar, live.preview])
            .unwrap_or([0, 0, 0]);
        for hwnd in hwnds {
            if hwnd != 0 {
                unsafe {
                    PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
                }
            }
        }
    }

    fn create_window(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        ex_style: u32,
    ) -> Result<HWND, String> {
        ensure_class()?;
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                CLASS_NAME.as_ptr(),
                CLASS_NAME.as_ptr(),
                WS_POPUP,
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
            Err("GIF 窗口打不开".into())
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
                return Err("GIF 边框打不开".into());
            }
            Ok(())
        }
    }

    fn exclude_from_capture(hwnd: HWND) {
        unsafe {
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
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
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }

    fn install_hook() -> isize {
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), instance(), 0) as isize }
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
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
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
                if unsafe { RegisterClassExW(&class) } == 0 {
                    Err("注册 GIF 窗口失败".into())
                } else {
                    Ok(())
                }
            })
            .clone()
    }

    fn instance() -> windows_sys::Win32::Foundation::HINSTANCE {
        unsafe { GetModuleHandleW(std::ptr::null()) }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_MOUSEMOVE => {
                pointer_move(hwnd, lparam);
                0
            }
            WM_LBUTTONDOWN => {
                pointer_down(hwnd, lparam);
                0
            }
            WM_LBUTTONUP => {
                if let Some(action) = pointer_up(hwnd, lparam) {
                    press(action);
                }
                0
            }
            WM_MOUSELEAVE => {
                pointer_leave(hwnd);
                0
            }
            WM_TIMER => {
                if wparam == TIMER_WATCH {
                    watch();
                } else {
                    advance(hwnd);
                }
                0
            }
            WM_APP_COUNT => {
                on_count(wparam, lparam as u64);
                0
            }
            WM_APP_FRAME => {
                on_frame(lparam as u64);
                0
            }
            WM_APP_FINISH => {
                on_finish(wparam, lparam as u64);
                0
            }
            WM_APP_PROGRESS => {
                on_progress(wparam, lparam as u64);
                0
            }
            WM_APP_DONE => {
                on_encoded(lparam as u64);
                0
            }
            WM_APP_CANCEL => {
                press(Action::Cancel);
                0
            }
            WM_APP_SAVED => {
                on_saved(wparam);
                0
            }
            WM_APP_NOTE => {
                on_note(wparam);
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
                unsafe {
                    KillTimer(hwnd, TIMER_WATCH);
                }
                live.bar = 0;
            }
            if live.preview == hwnd as isize {
                unsafe {
                    KillTimer(hwnd, TIMER_PLAY);
                }
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
        if live.hook_key != 0 {
            unsafe {
                UnhookWindowsHookEx(live.hook_key as _);
            }
        }
        HOOK_BAR.store(0, Ordering::SeqCst);
        if live.close_session {
            super::super::close_from_native(&live.app);
        }
    }

    fn paint(hwnd: HWND) {
        unsafe {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut paint);
            if hdc.is_null() {
                return;
            }
            let role = session().as_ref().and_then(|live| {
                if live.border == hwnd as isize {
                    Some(0)
                } else if live.bar == hwnd as isize {
                    Some(1)
                } else if live.preview == hwnd as isize {
                    Some(2)
                } else {
                    None
                }
            });
            match role {
                Some(0) => fill_client(hwnd, hdc, FRAME),
                Some(1) => paint_bar(hwnd, hdc),
                Some(2) => paint_preview(hwnd, hdc),
                _ => fill_client(hwnd, hdc, BG),
            }
            EndPaint(hwnd, &paint);
        }
    }

    fn paint_bar(hwnd: HWND, hdc: HDC) {
        fill_client(hwnd, hdc, BG);
        let (hint, slots) = {
            let guard = session();
            let Some(live) = guard.as_ref() else {
                return;
            };
            (live.hint.clone(), live_slots(live))
        };
        draw_chrome(hdc, &hint, &slots, false);
    }

    fn paint_preview(hwnd: HWND, hdc: HDC) {
        fill_client(hwnd, hdc, BG);
        let (width, height) = client_size(hwnd);
        let drawn = {
            let guard = session();
            let Some(live) = guard.as_ref() else {
                return;
            };
            (
                live.frames.clone(),
                live.play,
                live.hint.clone(),
                preview_slots(width, height, live.hot, live.down),
            )
        };
        if let Some(frame) = drawn.0.as_ref().and_then(|frames| frames.get(drawn.1)) {
            blit_frame(hdc, frame, width, height - BAR_H);
        }
        draw_chrome(hdc, &drawn.2, &drawn.3, true);
    }

    fn blit_frame(hdc: HDC, frame: &Scaled, area_w: i32, area_h: i32) {
        if area_w <= 0 || area_h <= 0 || frame.width == 0 || frame.height == 0 {
            return;
        }
        let scale =
            (area_w as f64 / f64::from(frame.width)).min(area_h as f64 / f64::from(frame.height));
        let dest_w = (f64::from(frame.width) * scale).round().max(1.0) as i32;
        let dest_h = (f64::from(frame.height) * scale).round().max(1.0) as i32;
        let left = (area_w - dest_w) / 2;
        let top = (area_h - dest_h) / 2;
        let info = dib_top_down(frame.width as i32, frame.height as i32);
        unsafe {
            SetStretchBltMode(hdc, HALFTONE);
            StretchDIBits(
                hdc,
                left,
                top,
                dest_w,
                dest_h,
                0,
                0,
                frame.width as i32,
                frame.height as i32,
                frame.pixels.as_ptr().cast(),
                &info,
                0,
                SRCCOPY,
            );
        }
    }

    fn draw_chrome(hdc: HDC, hint: &str, slots: &[Slot], dock: bool) {
        let _ = dock;
        unsafe {
            let font = ui_font();
            let old = SelectObject(hdc, font as _);
            SetBkMode(hdc, TRANSPARENT as i32);
            if let Some(first) = slots.first() {
                let mut text = RECT {
                    left: 12,
                    top: first.rect.top,
                    right: first.rect.left - 8,
                    bottom: first.rect.bottom,
                };
                if text.right > text.left {
                    SetTextColor(hdc, TEXT);
                    let wide = utf16(hint);
                    DrawTextW(
                        hdc,
                        wide.as_ptr(),
                        -1,
                        &mut text,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                }
            }
            for slot in slots {
                let fill = if slot.primary {
                    TEXT
                } else if slot.pressed {
                    PRESS
                } else if slot.hot {
                    HOVER
                } else {
                    BG
                };
                fill_round(hdc, slot.rect, 8, fill);
                SetTextColor(hdc, if slot.primary { BG } else { TEXT });
                let mut bounds = slot.rect;
                let wide = utf16(&slot.label);
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

    struct Slot {
        action: Action,
        label: String,
        primary: bool,
        hot: bool,
        pressed: bool,
        rect: RECT,
    }

    fn live_slots(live: &GifSession) -> Vec<Slot> {
        let fps = live.mail.fps.load(Ordering::SeqCst);
        let paused = live.mail.paused.load(Ordering::SeqCst);
        let actions: &[(Action, &str, bool)] = match live.stage {
            Stage::Countdown => &[
                (Action::Fps, if fps == 15 { "15帧" } else { "10帧" }, false),
                (Action::Cancel, "取消", false),
            ],
            Stage::Recording => &[
                (Action::Pause, if paused { "继续" } else { "暂停" }, false),
                (Action::Stop, "停止", true),
            ],
            Stage::Encoding => &[(Action::Cancel, "取消", false)],
            Stage::Preview => &[],
        };
        layout_right(BAR_W, BAR_H, actions, live.hot, live.down)
    }

    fn preview_slots(width: i32, height: i32, hot: i32, down: i32) -> Vec<Slot> {
        const ACTIONS: &[(Action, &str, bool)] = &[
            (Action::Copy, "复制", true),
            (Action::Save, "保存", false),
            (Action::Again, "重录", false),
            (Action::Close, "关闭", false),
        ];
        let mut slots = layout_right(width, height, ACTIONS, hot, down);
        let top = height - BAR_H + 8;
        for slot in &mut slots {
            let shift = top - slot.rect.top;
            slot.rect.top += shift;
            slot.rect.bottom += shift;
        }
        slots
    }

    fn layout_right(
        width: i32,
        height: i32,
        actions: &[(Action, &str, bool)],
        hot: i32,
        down: i32,
    ) -> Vec<Slot> {
        let button_w = 64i32;
        let button_h = 28i32;
        let gap = 6i32;
        let count = actions.len() as i32;
        let total = count * button_w + (count - 1).max(0) * gap;
        let mut x = width - 8 - total;
        let y = (height - button_h) / 2;
        actions
            .iter()
            .enumerate()
            .map(|(index, (action, label, primary))| {
                let rect = RECT {
                    left: x,
                    top: y,
                    right: x + button_w,
                    bottom: y + button_h,
                };
                x += button_w + gap;
                Slot {
                    action: *action,
                    label: label.to_string(),
                    primary: *primary,
                    hot: hot == index as i32,
                    pressed: down == index as i32,
                    rect,
                }
            })
            .collect()
    }

    fn hit(hwnd: HWND, x: i32, y: i32) -> i32 {
        let guard = session();
        let Some(live) = guard.as_ref() else {
            return -1;
        };
        let slots = if live.preview == hwnd as isize {
            let (width, height) = client_size(hwnd);
            preview_slots(width, height, live.hot, live.down)
        } else if live.bar == hwnd as isize {
            live_slots(live)
        } else {
            return -1;
        };
        slots
            .iter()
            .position(|slot| contains(slot.rect, x, y))
            .map(|index| index as i32)
            .unwrap_or(-1)
    }

    fn action_at(hwnd: HWND, index: i32) -> Option<Action> {
        if index < 0 {
            return None;
        }
        let guard = session();
        let live = guard.as_ref()?;
        let slots = if live.preview == hwnd as isize {
            let (width, height) = client_size(hwnd);
            preview_slots(width, height, live.hot, live.down)
        } else {
            live_slots(live)
        };
        slots.get(index as usize).map(|slot| slot.action)
    }

    fn pointer_move(hwnd: HWND, lparam: LPARAM) {
        let (x, y) = point_lparam(lparam);
        let next = hit(hwnd, x, y);
        let changed = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.hot == next {
                false
            } else {
                live.hot = next;
                true
            }
        };
        track_leave(hwnd);
        if changed {
            repaint(hwnd as isize);
        }
    }

    fn pointer_down(hwnd: HWND, lparam: LPARAM) {
        let (x, y) = point_lparam(lparam);
        let next = hit(hwnd, x, y);
        if next < 0 {
            return;
        }
        if let Some(live) = session().as_mut() {
            live.hot = next;
            live.down = next;
        }
        unsafe {
            SetCapture(hwnd);
        }
        repaint(hwnd as isize);
    }

    fn pointer_up(hwnd: HWND, lparam: LPARAM) -> Option<Action> {
        let down = session().as_mut().map(|live| {
            let down = live.down;
            live.down = -1;
            down
        });
        unsafe {
            ReleaseCapture();
        }
        let (x, y) = point_lparam(lparam);
        let next = hit(hwnd, x, y);
        if let Some(live) = session().as_mut() {
            live.hot = next;
        }
        repaint(hwnd as isize);
        let down = down?;
        if down >= 0 && down == next {
            action_at(hwnd, down)
        } else {
            None
        }
    }

    fn pointer_leave(hwnd: HWND) {
        let pressed = session().as_mut().is_some_and(|live| {
            if live.down >= 0 {
                return true;
            }
            live.hot = -1;
            false
        });
        if !pressed {
            repaint(hwnd as isize);
        }
    }

    fn advance(hwnd: HWND) {
        let preview = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            if live.preview != hwnd as isize || live.stage != Stage::Preview {
                return;
            }
            let len = live.frames.as_ref().map(|frames| frames.len()).unwrap_or(0);
            if len == 0 {
                return;
            }
            live.play = (live.play + 1) % len;
            live.preview
        };
        repaint(preview);
    }

    fn contains(rect: RECT, x: i32, y: i32) -> bool {
        x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
    }

    fn point_lparam(lparam: LPARAM) -> (i32, i32) {
        let value = lparam as u32;
        ((value & 0xffff) as i16 as i32, (value >> 16) as i16 as i32)
    }

    fn client_size(hwnd: HWND) -> (i32, i32) {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    fn fill_client(hwnd: HWND, hdc: HDC, color: COLORREF) {
        unsafe {
            let mut client = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(hwnd, &mut client);
            let brush = CreateSolidBrush(color);
            FillRect(hdc, &client, brush);
            DeleteObject(brush as _);
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

    fn repaint(hwnd: isize) {
        if hwnd != 0 {
            unsafe {
                InvalidateRect(hwnd as HWND, std::ptr::null(), 0);
            }
        }
    }

    fn tell(_hwnd: isize, message: &str) {
        let target = {
            let mut guard = session();
            let Some(live) = guard.as_mut() else {
                return;
            };
            live.hint = message.to_string();
            if live.stage == Stage::Preview && live.preview != 0 {
                live.preview
            } else {
                live.bar
            }
        };
        repaint(target);
    }

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
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

const fn rgb(red: u32, green: u32, blue: u32) -> u32 {
    red | (green << 8) | (blue << 16)
}
