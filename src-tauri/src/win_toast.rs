use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri_winrt_notification::{IconCrop, Toast};

const APP_ID: &str = "com.daodian.app";

// 不要在进程启动时调用 SetCurrentProcessExplicitAppUserModelID。
// WebView2 创建窗口时会返回 0x800700AA（资源正在使用）。通知身份只在真正弹出时登记。

static READY: AtomicBool = AtomicBool::new(false);

pub fn show(title: &str, body: &str) -> Result<(), String> {
    if let Err(err) = register() {
        // 注册表写不进去时仍尝试弹出。身份可能已经登记过。
        crate::logging::warn(format_args!(
            "toast identity registration failed; trying to show anyway: {err}"
        ));
    }
    let mut toast = Toast::new(APP_ID).title(title).text1(body);
    if let Some(icon) = notification_icon() {
        toast = toast.icon(&icon, IconCrop::Circular, "到点");
    }
    toast
        .show()
        .map_err(|err| format!("显示 Windows 通知失败（identity={APP_ID}）: {err}"))
}

fn register() -> Result<(), String> {
    if READY.load(Ordering::Relaxed) {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let path = format!(r"SOFTWARE\Classes\AppUserModelId\{APP_ID}");
    let key = match windows_registry::CURRENT_USER.create(&path) {
        Ok(key) => key,
        Err(err) => windows_registry::CURRENT_USER
            .open(&path)
            .map_err(|open_err| {
                format!("创建通知注册表项失败: {err}; 打开已有项也失败: {open_err}")
            })?,
    };
    key.set_string("DisplayName", "到点")
        .map_err(|err| format!("写入通知名称失败: {err}"))?;
    key.set_string("IconBackgroundColor", "0")
        .map_err(|err| format!("写入通知背景色失败: {err}"))?;
    let icon = notification_icon().unwrap_or(exe);
    key.set_string("IconUri", icon.display().to_string())
        .map_err(|err| format!("写入通知图标失败: {err}"))?;
    READY.store(true, Ordering::Relaxed);
    Ok(())
}

fn notification_icon() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    [
        parent.join("notification-icon.png"),
        parent.join("resources").join("notification-icon.png"),
        parent
            .parent()
            .and_then(|dir| dir.parent())
            .map(|dir| dir.join("icons").join("128x128.png"))
            .unwrap_or_default(),
        parent
            .parent()
            .map(|dir| dir.join("icons").join("128x128.png"))
            .unwrap_or_default(),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
