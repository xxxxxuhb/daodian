use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder,
};

use crate::settings::SharedSettings;

const COMPACT: f64 = 96.0;
const WINDOW_WIDTH: f64 = 390.0;
const WINDOW_HEIGHT: f64 = 292.0;
const EDGE_GAP: i32 = 14;

#[derive(Default)]
pub struct PetRuntime {
    compact_position: Mutex<Option<PhysicalPosition<i32>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PetSnapshot {
    pinches: u32,
    hidden_until: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PetOpenTarget {
    kind: String,
    title: String,
    todo_id: Option<String>,
    plan_id: Option<String>,
    stage_id: Option<String>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn pet_settings(app: &AppHandle) -> Result<crate::settings::PetSettings, String> {
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())
        .map(|store| store.pet_settings())
}

pub fn sync_visibility(app: &AppHandle) -> Result<(), String> {
    let settings = pet_settings(app)?;
    if !settings.enabled {
        if let Some(window) = app.get_webview_window("pet") {
            window.hide().map_err(|err| err.to_string())?;
        }
        return Ok(());
    }
    if settings.hidden_until > now_ms() {
        if let Some(window) = app.get_webview_window("pet") {
            window.hide().map_err(|err| err.to_string())?;
        }
        schedule_wake(app.clone(), settings.hidden_until);
        return Ok(());
    }
    let window = match app.get_webview_window("pet") {
        Some(window) => window,
        None => create(app, settings.x.zip(settings.y))?,
    };
    window.show().map_err(|err| err.to_string())
}

fn create(app: &AppHandle, remembered: Option<(i32, i32)>) -> Result<tauri::WebviewWindow, String> {
    let webview_data = crate::webview_data_dir().map_err(|err| err.to_string())?;
    let window =
        WebviewWindowBuilder::new(app, "pet", WebviewUrl::App("index.html?window=pet".into()))
            .data_directory(webview_data)
            .title("到点桌面珠子")
            .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
            .map_err(|err| err.to_string())?;

    let position = if let Some((x, y)) = remembered {
        PhysicalPosition::new(x, y)
    } else {
        default_position(&window)?
    };
    let window_position = expanded_position(&window, position)?;
    window
        .set_position(window_position)
        .map_err(|err| err.to_string())?;
    apply_hit_region(&window, "compact")?;
    *app.state::<PetRuntime>()
        .compact_position
        .lock()
        .map_err(|err| err.to_string())? = Some(position);
    crate::logging::info(format_args!(
        "desktop pet created at {},{}",
        position.x, position.y
    ));
    Ok(window)
}

fn expanded_position(
    window: &tauri::WebviewWindow,
    compact: PhysicalPosition<i32>,
) -> Result<PhysicalPosition<i32>, String> {
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    Ok(PhysicalPosition::new(
        compact.x - ((WINDOW_WIDTH - COMPACT) * scale).round() as i32,
        compact.y - ((WINDOW_HEIGHT - COMPACT) * scale).round() as i32,
    ))
}

fn default_position(window: &tauri::WebviewWindow) -> Result<PhysicalPosition<i32>, String> {
    let monitor = window
        .primary_monitor()
        .map_err(|err| err.to_string())?
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or_else(|| "没有可用显示器".to_string())?;
    let area = monitor.work_area();
    let size = (COMPACT * monitor.scale_factor()).round() as i32;
    Ok(PhysicalPosition::new(
        area.position.x + area.size.width as i32 - size - 28,
        area.position.y + area.size.height as i32 - size - 28,
    ))
}

#[tauri::command]
pub fn pet_set_mode(
    app: AppHandle,
    _runtime: State<PetRuntime>,
    mode: String,
) -> Result<(), String> {
    let window = app
        .get_webview_window("pet")
        .ok_or_else(|| "桌面珠子窗口不存在".to_string())?;
    apply_hit_region(&window, &mode)
}

#[cfg(windows)]
fn apply_hit_region(window: &tauri::WebviewWindow, mode: &str) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateEllipticRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
    };

    let hwnd = window.hwnd().map_err(|err| err.to_string())?.0;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    let inset = (5.0 * scale).round() as i32;
    let width = (WINDOW_WIDTH * scale).round() as i32;
    let height = (WINDOW_HEIGHT * scale).round() as i32;
    let pet_left = ((WINDOW_WIDTH - COMPACT) * scale).round() as i32;
    let pet_top = ((WINDOW_HEIGHT - COMPACT) * scale).round() as i32;
    let region = unsafe {
        CreateEllipticRgn(
            pet_left + inset,
            pet_top + inset,
            width - inset,
            height - inset,
        )
    };
    if region.is_null() {
        return Err("创建桌面珠子点击区域失败".into());
    }
    let bounds = match mode {
        "hover" => Some((32.0, 116.0, WINDOW_WIDTH, WINDOW_HEIGHT)),
        "card" => Some((0.0, 48.0, 310.0, WINDOW_HEIGHT)),
        "menu" => Some((76.0, 24.0, 326.0, 228.0)),
        _ => None,
    };
    if let Some((left, top, right, bottom)) = bounds {
        let panel = unsafe {
            CreateRectRgn(
                (left * scale).round() as i32,
                (top * scale).round() as i32,
                (right * scale).round() as i32,
                (bottom * scale).round() as i32,
            )
        };
        if !panel.is_null() {
            unsafe {
                CombineRgn(region, region, panel, RGN_OR);
                DeleteObject(panel);
            }
        }
    }
    if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
        unsafe {
            DeleteObject(region);
        }
        return Err("设置桌面珠子点击区域失败".into());
    }
    // SetWindowRgn 成功后区域所有权归系统，不再手动释放。
    Ok(())
}

#[cfg(not(windows))]
fn apply_hit_region(_window: &tauri::WebviewWindow, _mode: &str) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn pet_move_to(
    app: AppHandle,
    runtime: State<PetRuntime>,
    x: i32,
    y: i32,
) -> Result<(), String> {
    let window = app
        .get_webview_window("pet")
        .ok_or_else(|| "桌面珠子窗口不存在".to_string())?;
    let position = PhysicalPosition::new(x, y);
    let window_position = expanded_position(&window, position)?;
    window
        .set_position(window_position)
        .map_err(|err| err.to_string())?;
    *runtime
        .compact_position
        .lock()
        .map_err(|err| err.to_string())? = Some(position);
    Ok(())
}

#[tauri::command]
pub fn pet_snap(app: AppHandle, runtime: State<PetRuntime>) -> Result<(), String> {
    let window = app
        .get_webview_window("pet")
        .ok_or_else(|| "桌面珠子窗口不存在".to_string())?;
    let position = runtime
        .compact_position
        .lock()
        .map_err(|err| err.to_string())?
        .unwrap_or(default_position(&window)?);
    let monitor = window
        .current_monitor()
        .map_err(|err| err.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "没有可用显示器".to_string())?;
    let area = monitor.work_area();
    let size = (COMPACT * monitor.scale_factor()).round() as i32;
    let left = area.position.x + EDGE_GAP;
    let top = area.position.y + EDGE_GAP;
    let right = area.position.x + area.size.width as i32 - size - EDGE_GAP;
    let bottom = area.position.y + area.size.height as i32 - size - EDGE_GAP;
    let distances = [
        ((position.x - left).abs(), 0),
        ((right - position.x).abs(), 1),
        ((position.y - top).abs(), 2),
        ((bottom - position.y).abs(), 3),
    ];
    let edge = distances
        .into_iter()
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, edge)| edge)
        .unwrap_or(1);
    let target = match edge {
        0 => PhysicalPosition::new(left, position.y.clamp(top, bottom)),
        1 => PhysicalPosition::new(right, position.y.clamp(top, bottom)),
        2 => PhysicalPosition::new(position.x.clamp(left, right), top),
        _ => PhysicalPosition::new(position.x.clamp(left, right), bottom),
    };
    window
        .set_position(expanded_position(&window, target)?)
        .map_err(|err| err.to_string())?;
    *runtime
        .compact_position
        .lock()
        .map_err(|err| err.to_string())? = Some(target);
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .save_pet_position(target.x, target.y)
}

#[tauri::command]
pub fn pet_record_pinch(app: AppHandle) -> Result<u32, String> {
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .record_pet_pinch()
}

#[tauri::command]
pub fn pet_snapshot(app: AppHandle) -> Result<PetSnapshot, String> {
    let settings = pet_settings(&app)?;
    Ok(PetSnapshot {
        pinches: settings.pinches,
        hidden_until: settings.hidden_until,
    })
}

#[tauri::command]
pub fn pet_can_idle() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{
            QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
            SHQueryUserNotificationState,
        };
        let mut state = 0;
        let result = unsafe { SHQueryUserNotificationState(&mut state) };
        // 免打扰和普通前台应用不阻止珠子自己动；锁屏、全屏和演示模式才停。
        result < 0
            || !matches!(
                state,
                QUNS_NOT_PRESENT | QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
            )
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[tauri::command]
pub fn pet_hide_until(app: AppHandle, until: i64) -> Result<(), String> {
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .hide_pet_until(until)?;
    if let Some(window) = app.get_webview_window("pet") {
        window.hide().map_err(|err| err.to_string())?;
    }
    schedule_wake(app, until);
    Ok(())
}

fn schedule_wake(app: AppHandle, until: i64) {
    std::thread::spawn(move || {
        let wait = (until - now_ms()).max(0) as u64;
        std::thread::sleep(Duration::from_millis(wait));
        let app_for_task = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Err(err) = sync_visibility(&app_for_task) {
                crate::logging::warn(format_args!("唤醒桌面珠子失败: {err}"));
            }
        });
    });
}

#[tauri::command]
pub fn pet_open_main(
    app: AppHandle,
    kind: String,
    title: String,
    todo_id: Option<String>,
    plan_id: Option<String>,
    stage_id: Option<String>,
) -> Result<(), String> {
    crate::tray::show_main(&app);
    app.emit_to(
        "main",
        "pet-open-target",
        PetOpenTarget {
            kind,
            title,
            todo_id,
            plan_id,
            stage_id,
        },
    )
    .map_err(|err| err.to_string())
}
