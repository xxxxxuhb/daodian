use std::mem::size_of;
use std::ptr::null_mut;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIIF_INFO, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{IDI_INFORMATION, LoadIconW};

const BALLOON_ID: u32 = 0xDA01;

pub fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let hwnd = window.hwnd().map_err(|err| err.to_string())?.0 as usize;
    let mut data = notification_data(hwnd, title, body);
    data.hIcon = unsafe { LoadIconW(null_mut(), IDI_INFORMATION) };

    unsafe { set_last_error(0) };
    let shown = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    if shown == 0 {
        return Err(format!(
            "Shell_NotifyIconW(NIM_ADD) 失败: {}",
            std::io::Error::last_os_error()
        ));
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(12));
        let data = notification_data(hwnd, "", "");
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
    });
    Ok(())
}

fn notification_data(hwnd: usize, title: &str, body: &str) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd as _,
        uID: BALLOON_ID,
        uFlags: NIF_ICON | NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };
    copy_wide(title, &mut data.szInfoTitle);
    copy_wide(body, &mut data.szInfo);
    data.Anonymous.uTimeout = 10_000;
    data
}

unsafe extern "system" {
    fn SetLastError(code: u32);
}

unsafe fn set_last_error(code: u32) {
    unsafe { SetLastError(code) };
}

fn copy_wide<const N: usize>(value: &str, target: &mut [u16; N]) {
    for (slot, code) in target
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(value.encode_utf16())
    {
        *slot = code;
    }
}
