//! 把截图窗口按物理像素铺到对应显示器上。
use std::sync::atomic::Ordering;

use tauri::WebviewWindow;

#[cfg(not(windows))]
use tauri::{PhysicalPosition, PhysicalSize};

#[cfg(windows)]
use super::{
    MAX_SCREENS, SHOT_LOCK_H, SHOT_LOCK_HWND, SHOT_LOCK_W, SHOT_LOCK_X, SHOT_LOCK_Y, shot_index,
};

#[cfg(windows)]
pub(super) fn place_shot_physical(window: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    remember_shot_lock(window, x, y, width, height);
    if let Ok(handle) = window.hwnd() {
        apply_shot_rect(handle.0 as _, x, y, width as i32, height as i32);
    }
    let webview: &tauri::Webview<_> = window.as_ref();
    let _ = webview.set_zoom(1.0);
    let placed = window.clone();
    let _ = window.run_on_main_thread(move || {
        finish_shot_place(&placed, x, y, width, height);
    });
}

#[cfg(windows)]
fn remember_shot_lock(window: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    let Some(index) = shot_index(&window.label()) else {
        return;
    };
    SHOT_LOCK_X[index].store(x, Ordering::SeqCst);
    SHOT_LOCK_Y[index].store(y, Ordering::SeqCst);
    SHOT_LOCK_W[index].store(width as i32, Ordering::SeqCst);
    SHOT_LOCK_H[index].store(height as i32, Ordering::SeqCst);
    if let Ok(handle) = window.hwnd() {
        SHOT_LOCK_HWND[index].store(handle.0 as isize, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn shot_lock_for(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    for index in 0..MAX_SCREENS {
        if SHOT_LOCK_HWND[index].load(Ordering::SeqCst) != hwnd {
            continue;
        }
        let width = SHOT_LOCK_W[index].load(Ordering::SeqCst);
        let height = SHOT_LOCK_H[index].load(Ordering::SeqCst);
        if width <= 0 || height <= 0 {
            return None;
        }
        return Some((
            SHOT_LOCK_X[index].load(Ordering::SeqCst),
            SHOT_LOCK_Y[index].load(Ordering::SeqCst),
            width,
            height,
        ));
    }
    None
}

#[cfg(windows)]
unsafe extern "system" fn lock_shot_pos(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_HIDEWINDOW, SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_WINDOWPOSCHANGING,
    };

    unsafe {
        if msg == WM_WINDOWPOSCHANGING {
            if let Some((x, y, width, height)) = shot_lock_for(hwnd as isize) {
                let pos = lparam as *mut WINDOWPOS;
                if !pos.is_null() && (*pos).flags & SWP_HIDEWINDOW == 0 {
                    (*pos).x = x;
                    (*pos).y = y;
                    (*pos).cx = width;
                    (*pos).cy = height;
                    (*pos).flags &= !(SWP_NOMOVE | SWP_NOSIZE);
                }
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

#[cfg(windows)]
fn finish_shot_place(window: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;

    let Ok(handle) = window.hwnd() else {
        return;
    };
    let hwnd = handle.0 as windows_sys::Win32::Foundation::HWND;
    remember_shot_lock(window, x, y, width, height);
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(lock_shot_pos), 0xD40D_5A07, 0);
    }
    apply_shot_rect(hwnd, x, y, width as i32, height as i32);
    let (view_w, view_h) = client_size(hwnd).unwrap_or((width as i32, height as i32));
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let scale = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
    let view: &tauri::Webview<_> = window.as_ref();
    let _ = view.set_zoom(scale);
    let _ = view.with_webview(move |platform| sync_shot_surface(&platform, scale, view_w, view_h));
    fill_descendants(hwnd, view_w, view_h);
}

#[cfg(windows)]
fn fill_descendants(hwnd: windows_sys::Win32::Foundation::HWND, width: i32, height: i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_CHILD, GW_HWNDNEXT, GetWindow, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
    };
    unsafe {
        let mut child = GetWindow(hwnd, GW_CHILD);
        while !child.is_null() {
            let before = client_size(child).unwrap_or((0, 0));
            let _ = SetWindowPos(
                child,
                std::ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let after = client_size(child).unwrap_or((0, 0));
            if after != (width, height) {
                crate::logging::info(format_args!(
                    "screenshot child: {}x{} -> {}x{} want {width}x{height}",
                    before.0, before.1, after.0, after.1
                ));
            }
            fill_descendants(child, width, height);
            child = GetWindow(child, GW_HWNDNEXT);
        }
    }
}

fn client_size(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
        return None;
    }
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    (width > 0 && height > 0).then_some((width, height))
}

/// 网页视图的合成尺寸和光栅化缩放必须跟物理客户区一致，否则冻结图缩在左上角。
#[cfg(windows)]
fn sync_shot_surface(
    platform: &tauri::webview::PlatformWebview,
    scale: f64,
    width: i32,
    height: i32,
) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS, COREWEBVIEW2_COLOR, ICoreWebView2Controller2,
        ICoreWebView2Controller3,
    };
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};
    use windows_core::Interface;

    unsafe {
        let controller = platform.controller();
        if let Ok(color) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = color.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 0,
                R: 0,
                G: 0,
                B: 0,
            });
        }
        if let Ok(extended) = controller.cast::<ICoreWebView2Controller3>() {
            let _ = extended.SetShouldDetectMonitorScaleChanges(false);
            // 光栅保持 1，一个 CSS 像素对应一个物理像素，页面才能铺满客户区。
            // 界面大小交给缩放，否则 200% 屏上按钮会小一倍。
            let _ = extended.SetRasterizationScale(1.0);
            let _ = extended.SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS);
        }
        let mut host = windows::Win32::Foundation::HWND::default();
        if controller.ParentWindow(&mut host).is_ok() {
            // 宿主子窗口才是网页实际占的那一块。只改外层窗口，画面仍会缩在左上角。
            let _ = SetWindowPos(
                host,
                None,
                0,
                0,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let _ = controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        });
        let _ = controller.SetZoomFactor(if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        });
        let _ = controller.NotifyParentWindowPositionChanged();
        let mut fitted = RECT::default();
        if controller.Bounds(&mut fitted).is_ok() {
            let scale_text = format!("{scale:.2}");
            crate::logging::info(format_args!(
                "screenshot surface: {}x{} scale {scale_text}",
                fitted.right - fitted.left,
                fitted.bottom - fitted.top
            ));
        }
    }
}

#[cfg(windows)]
fn apply_shot_rect(
    hwnd: windows_sys::Win32::Foundation::HWND,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_CHILD, GW_HWNDNEXT, GetClientRect, GetWindow, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowPos,
    };

    unsafe {
        // 先挪到目标屏，让 DPI 切换在这一步发生。系统那个切换会按比例改尺寸。
        let _ = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOZORDER | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetClientRect(hwnd, &mut client) != 0 {
            let cw = client.right - client.left;
            let ch = client.bottom - client.top;
            if cw > 0 && ch > 0 && (cw != width || ch != height) {
                let _ = SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    x,
                    y,
                    width + (width - cw),
                    height + (height - ch),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let mut fitted = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetClientRect(hwnd, &mut fitted) != 0 {
                let fw = fitted.right - fitted.left;
                let fh = fitted.bottom - fitted.top;
                let mut child = GetWindow(hwnd, GW_CHILD);
                while !child.is_null() {
                    let _ = SetWindowPos(
                        child,
                        std::ptr::null_mut(),
                        0,
                        0,
                        fw,
                        fh,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    child = GetWindow(child, GW_HWNDNEXT);
                }
                crate::logging::info(format_args!(
                    "screenshot placed: {width}x{height} at {x},{y} client {fw}x{fh}"
                ));
            }
        }
    }
}

#[cfg(not(windows))]
pub(super) fn place_shot_physical(window: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    let _ = window
        .set_position(PhysicalPosition::new(x, y))
        .and_then(|_| window.set_size(PhysicalSize::new(width, height)));
    let webview: &tauri::Webview<_> = window.as_ref();
    let _ = webview.set_zoom(1.0);
}
