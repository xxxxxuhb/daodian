mod domain;
mod logging;
mod remind;
mod store;
mod tray;
#[cfg(windows)]
mod win_balloon;
#[cfg(windows)]
mod win_toast;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_path = logging::init();
    logging::info(format_args!(
        "application starting; exe={}; cwd={}; log={}",
        std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|err| format!("<error: {err}>")),
        std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|err| format!("<error: {err}>")),
        log_path
            .as_deref()
            .map(Path::display)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<unavailable>".to_string())
    ));

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            logging::info(format_args!(
                "second instance requested; showing main window"
            ));
            tray::show_main(&app);
        }))
        .plugin(tauri_plugin_notification::init())
        // 企业安全策略可能禁止注册托盘图标；关闭时保留任务栏入口，避免程序彻底找不到。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(focused) = event {
                logging::info(format_args!("main window focus changed: {focused}"));
                if *focused {
                    tray::mark_read(window.app_handle());
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if tray::QUITTING.load(std::sync::atomic::Ordering::SeqCst) {
                    logging::info(format_args!(
                        "main window close allowed because app is quitting"
                    ));
                    return;
                }
                api.prevent_close();
                if tray::READY.load(std::sync::atomic::Ordering::SeqCst) {
                    logging::info(format_args!("main window close requested; hiding to tray"));
                    let _ = window.hide();
                } else {
                    logging::info(format_args!(
                        "main window close requested; tray unavailable, minimizing to taskbar"
                    ));
                    let _ = window.minimize();
                }
            }
        })
        .setup(|app| {
            logging::info(format_args!("setup started"));
            let dir = data_dir(app).map_err(|err| logged_step("取数据目录", err))?;
            logging::info(format_args!("data directory selected: {}", dir.display()));
            let library = store::Library::open(&dir)
                .map_err(|err| logged_step(format!("打开 {}", dir.display()), err.into()))?;
            app.manage(Mutex::new(library));
            logging::info(format_args!("library opened"));
            open_main(app).map_err(|err| logged_step("打开主窗口", err))?;
            remind::spawn(app.handle().clone());
            logging::info(format_args!("reminder worker spawned; setup complete"));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            store::load_library,
            store::todos::save_todo,
            store::todos::set_todo_done,
            store::todos::skip_todo_occurrence,
            store::todos::delete_todo,
            store::plans::save_plan,
            store::plans::set_plan_done,
            store::plans::set_stage_done,
            store::plans::delete_plan,
            store::templates::load_templates,
            store::templates::save_template,
            store::templates::rename_template,
            store::templates::delete_template,
            remind::sync_clock,
            remind::sync_reminders,
            frontend_log,
        ]);

    match builder.build(tauri::generate_context!()) {
        Ok(app) => {
            app.run(|app, event| {
                if matches!(event, tauri::RunEvent::Ready) {
                    match tray::setup(app) {
                        Ok(()) => {
                            logging::info(format_args!("tray initialized after event loop ready"))
                        }
                        Err(err) => logging::error(format_args!("初始化托盘: {err:#}")),
                    }
                }
            });
            logging::info(format_args!("application exited normally"));
        }
        Err(err) => {
            logging::error(format_args!("application failed: {err:#}"));
            panic!("到点启动失败: {err}");
        }
    }
}

/// 正式版优先写在程序旁边。这个进程要是写不进去，就换一个真能落盘的目录。
fn data_dir(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let roaming = app.path().app_data_dir()?;
    let preferred = if cfg!(debug_assertions) {
        roaming.clone()
    } else {
        std::env::current_exe()?
            .parent()
            .ok_or_else(|| std::io::Error::other("找不到安装目录"))?
            .to_path_buf()
    };
    let dir = if usable(&preferred) {
        preferred
    } else {
        logging::warn(format_args!(
            "preferred data directory is not writable: {}",
            preferred.display()
        ));
        writable_elsewhere(&preferred)
    };
    if dir != roaming && !dir.join("store.json").exists() {
        if let Err(err) = copy_if_present(&roaming, &dir, "store.json") {
            logging::warn(format_args!("existing store migration failed: {err:#}"));
        }
        if !dir.join("store.json").exists() {
            if let Err(err) = copy_if_present(&roaming, &dir, "todos.json") {
                logging::warn(format_args!("legacy store migration failed: {err:#}"));
            }
        }
    }
    if dir != roaming && !dir.join("templates.json").exists() {
        if let Err(err) = copy_if_present(&roaming, &dir, "templates.json") {
            logging::warn(format_args!("template migration failed: {err:#}"));
        }
    }
    Ok(dir)
}

fn open_main(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    if app.get_webview_window("main").is_some() {
        logging::info(format_args!("main window already exists"));
        return Ok(());
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or_else(|| std::io::Error::other("缺少主窗口配置"))?;
    let webview = webview_data_dir()?;
    logging::info(format_args!(
        "building main window; webview data directory={}",
        webview.display()
    ));
    let mut builder =
        tauri::WebviewWindowBuilder::from_config(app.handle(), config)?.data_directory(webview);
    builder = builder.on_page_load(|_window, payload| {
        logging::info(format_args!(
            "page load {:?}: {}",
            payload.event(),
            payload.url()
        ));
    });
    builder.build().map_err(|err| {
        logging::error(format_args!("main window build failed: {err:#}"));
        err
    })?;
    logging::info(format_args!("main window built"));
    Ok(())
}

fn webview_data_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut candidates = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("daodian").join("webview"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("webview-data"));
        }
    }
    candidates.push(std::env::temp_dir().join("daodian-webview"));

    for candidate in candidates {
        if usable(&candidate) {
            return Ok(candidate);
        }
        logging::warn(format_args!(
            "WebView2 data directory is not writable: {}",
            candidate.display()
        ));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "找不到可写的 WebView2 数据目录",
    )
    .into())
}

#[tauri::command]
fn frontend_log(level: String, message: String) {
    match level.as_str() {
        "error" => logging::error(format_args!("frontend: {message}")),
        "warn" => logging::warn(format_args!("frontend: {message}")),
        _ => logging::info(format_args!("frontend: {message}")),
    }
}

/// 要能写下文件才算能用。有的安全策略只拦「创建目录」，目录在的时候写文件仍然可以。
fn usable(dir: &Path) -> bool {
    if !dir.exists() && std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-probe");
    if std::fs::write(&probe, b"ok").is_err() {
        return false;
    }
    let _ = std::fs::remove_file(&probe);
    true
}

/// 主目录用不了时，先用 exe 所在的现成目录，避免安全软件拦截创建目录。
fn writable_elsewhere(preferred: &Path) -> PathBuf {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.to_path_buf());
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("daodian-data"));
    }
    candidates.push(std::env::temp_dir().join("daodian-data"));
    for dir in &candidates {
        if usable(dir) {
            logging::warn(format_args!(
                "data directory fallback: {} -> {}",
                preferred.display(),
                dir.display()
            ));
            return dir.clone();
        }
    }
    preferred.to_path_buf()
}

fn copy_if_present(from: &Path, to: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let source = from.join(name);
    let target = to.join(name);
    if target.exists() || !source.exists() {
        return Ok(());
    }
    if let Err(err) = std::fs::copy(&source, &target) {
        return Err(std::io::Error::other(format!(
            "把 {name} 拷到 {} 失败: {err}",
            target.display()
        ))
        .into());
    }
    Ok(())
}

/// 出错时说清楚是哪一步，别把 io 错误直接甩给用户。
fn step(
    context: impl std::fmt::Display,
    err: Box<dyn std::error::Error>,
) -> Box<dyn std::error::Error> {
    std::io::Error::other(format!("{context}: {err}")).into()
}

fn logged_step(
    context: impl std::fmt::Display,
    err: Box<dyn std::error::Error>,
) -> Box<dyn std::error::Error> {
    logging::error(format_args!("{context}: {err:#}"));
    step(context, err)
}
