use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{
    AppHandle, LogicalSize, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};

/// 点托盘「退出」时放行关闭。平时点窗口关闭只是藏进托盘，提醒继续跑。
pub static QUITTING: AtomicBool = AtomicBool::new(false);
pub static READY: AtomicBool = AtomicBool::new(false);
static UNREAD: AtomicBool = AtomicBool::new(false);
static BLINKING: AtomicBool = AtomicBool::new(false);
const TRAY_NORMAL: &[u8] = include_bytes!("../icons/32x32.png");
const TRAY_DUE: &[u8] = include_bytes!("../icons/tray-due.png");

fn toggle_mini(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let width = size.width as f64 / scale;
    let next = if width < 720.0 {
        LogicalSize::new(900.0, 600.0)
    } else {
        LogicalSize::new(380.0, 552.0)
    };
    let _ = window.set_size(next);
    show_main(app);
}

pub fn show_main(app: &AppHandle) {
    mark_read(app);
    let Some(window) = app.get_webview_window("main") else {
        crate::logging::error(format_args!("show main requested but main window is missing"));
        return;
    };
    crate::logging::info(format_args!("show main requested"));
    if let Err(err) = window.unminimize() {
        crate::logging::warn(format_args!("unminimize main failed: {err}"));
    }
    if let Err(err) = window.show() {
        crate::logging::warn(format_args!("show main failed: {err}"));
    }
    if let Err(err) = window.set_focus() {
        crate::logging::warn(format_args!("focus main failed: {err}"));
    }
}

pub fn alert(app: &AppHandle) {
    if app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false)
    {
        crate::logging::info(format_args!("tray alert skipped because main window is focused"));
        return;
    }
    UNREAD.store(true, Ordering::SeqCst);
    if BLINKING.swap(true, Ordering::SeqCst) {
        crate::logging::info(format_args!("tray alert already blinking; unread kept"));
        return;
    }
    crate::logging::info(format_args!("tray alert started"));
    let app = app.clone();
    std::thread::spawn(move || {
        let mut due = true;
        loop {
            while UNREAD.load(Ordering::SeqCst) {
                set_icon(&app, if due { TRAY_DUE } else { TRAY_NORMAL });
                due = !due;
                std::thread::sleep(Duration::from_millis(550));
            }
            set_icon(&app, TRAY_NORMAL);
            BLINKING.store(false, Ordering::SeqCst);
            if !UNREAD.load(Ordering::SeqCst) || BLINKING.swap(true, Ordering::SeqCst) {
                crate::logging::info(format_args!("tray alert stopped"));
                break;
            }
        }
    });
}

pub fn mark_read(app: &AppHandle) {
    if UNREAD.swap(false, Ordering::SeqCst) {
        set_icon(app, TRAY_NORMAL);
        crate::logging::info(format_args!("tray reminder marked as read"));
    }
}

fn set_icon(app: &AppHandle, bytes: &'static [u8]) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Ok(icon) = tauri::image::Image::from_bytes(bytes) {
        let _ = tray.set_icon(Some(icon));
    }
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "打开到点", true, None::<&str>)?;
    let mini = MenuItem::with_id(app, "mini", "迷你窗", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &mini, &quit])?;
    let icon = match tauri::image::Image::from_bytes(TRAY_NORMAL) {
        Ok(icon) => icon,
        Err(err) => {
            crate::logging::warn(format_args!(
                "embedded tray icon failed, using window icon: {err}"
            ));
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| std::io::Error::other("缺少默认窗口图标"))?
        }
    };

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .menu(&menu)
        .tooltip("到点")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id: &str = event.id().as_ref();
            crate::logging::info(format_args!("tray menu event: {id}"));
            match id {
                "show" => show_main(app),
                "mini" => toggle_mini(app),
                "quit" => {
                    QUITTING.store(true, Ordering::SeqCst);
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(event, TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. }) {
                crate::logging::info(format_args!("tray pointer event: {event:?}"));
            }
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    // 创建成功就可以隐藏主窗口。图标位于 Windows 溢出区时 rect() 会返回 None，
    // 但并不代表托盘不可用。
    READY.store(true, Ordering::SeqCst);
    // Windows 11 上 Shell_NotifyIconGetRect 经常返回失败，即使图标已经加进通知区。
    // 不能据此判断托盘不存在。图标默认进溢出区，后面再把它设成直接显示。
    match tray.rect() {
        Ok(Some(rect)) => {
            crate::logging::info(format_args!(
                "tray icon registered; position={:?}; size={:?}",
                rect.position, rect.size
            ));
        }
        Ok(None) => crate::logging::info(format_args!(
            "tray icon created; Windows did not report a rectangle yet"
        )),
        Err(err) => crate::logging::warn(format_args!(
            "tray icon rectangle unavailable: {err}"
        )),
    }
    Ok(())
}
