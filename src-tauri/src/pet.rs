use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl,
    WebviewWindowBuilder,
};

use crate::settings::SharedSettings;

const COMPACT: f64 = 96.0;
const WINDOW_WIDTH: f64 = 300.0;
const WINDOW_HEIGHT: f64 = 300.0;
const EDGE_GAP: i32 = 14;

pub struct PetRuntime {
    compact_position: Mutex<Option<PhysicalPosition<i32>>>,
    opens_right: Mutex<bool>,
    mode: Mutex<String>,
}

impl Default for PetRuntime {
    fn default() -> Self {
        Self {
            compact_position: Mutex::new(None),
            opens_right: Mutex::new(false),
            mode: Mutex::new("compact".into()),
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PetLayout {
    pub opens_right: bool,
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
    let remembered_pos = remembered.map(|(x, y)| PhysicalPosition::new(x, y));
    let hinted_right = remembered_pos
        .and_then(|pos| {
            app.get_webview_window("main")
                .and_then(|probe| opens_right_at(&probe, pos).ok())
        })
        .unwrap_or(false);
    let url = if hinted_right {
        "index.html?window=pet&stage=right"
    } else {
        "index.html?window=pet"
    };
    let window =
        WebviewWindowBuilder::new(app, "pet", WebviewUrl::App(url.into()))
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

    let position = remembered_pos.unwrap_or(default_position(&window)?);
    let opens_right = opens_right_at(&window, position).unwrap_or(hinted_right);
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    window
        .set_position(window_origin(position, opens_right, scale))
        .map_err(|err| err.to_string())?;
    let runtime = app.state::<PetRuntime>();
    *runtime
        .opens_right
        .lock()
        .map_err(|err| err.to_string())? = opens_right;
    apply_hit_region(&window, "compact", opens_right)?;
    *runtime
        .compact_position
        .lock()
        .map_err(|err| err.to_string())? = Some(position);
    crate::logging::info(format_args!(
        "desktop pet created at {},{}",
        position.x, position.y
    ));
    Ok(window)
}

fn stage_opens_right(space_left: i32, space_right: i32, extend: i32) -> bool {
    space_left < extend && space_right >= space_left
}

fn monitor_containing(
    probe: &tauri::WebviewWindow,
    x: i32,
    y: i32,
) -> Result<tauri::Monitor, String> {
    let monitors = probe.available_monitors().map_err(|err| err.to_string())?;
    if let Some(monitor) = monitors.into_iter().find(|monitor| {
        let area = monitor.work_area();
        let right = area.position.x + area.size.width as i32;
        let bottom = area.position.y + area.size.height as i32;
        x >= area.position.x && x < right && y >= area.position.y && y < bottom
    }) {
        return Ok(monitor);
    }
    probe
        .current_monitor()
        .map_err(|err| err.to_string())?
        .or_else(|| probe.primary_monitor().ok().flatten())
        .ok_or_else(|| "没有可用显示器".to_string())
}

fn opens_right_at(
    probe: &tauri::WebviewWindow,
    compact: PhysicalPosition<i32>,
) -> Result<bool, String> {
    let monitor = monitor_containing(probe, compact.x, compact.y)?;
    let scale = monitor.scale_factor();
    let extend = ((WINDOW_WIDTH - COMPACT) * scale).round() as i32;
    let pet = (COMPACT * scale).round() as i32;
    let area = monitor.work_area();
    let space_left = compact.x - area.position.x;
    let space_right = area.position.x + area.size.width as i32 - (compact.x + pet);
    Ok(stage_opens_right(space_left, space_right, extend))
}

fn window_origin(
    compact: PhysicalPosition<i32>,
    opens_right: bool,
    scale: f64,
) -> PhysicalPosition<i32> {
    let extend_x = ((WINDOW_WIDTH - COMPACT) * scale).round() as i32;
    let extend_y = ((WINDOW_HEIGHT - COMPACT) * scale).round() as i32;
    PhysicalPosition::new(
        if opens_right {
            compact.x
        } else {
            compact.x - extend_x
        },
        compact.y - extend_y,
    )
}

fn emit_layout(app: &AppHandle, opens_right: bool) {
    let _ = app.emit_to("pet", "pet-layout", PetLayout { opens_right });
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
    runtime: State<PetRuntime>,
    mode: String,
) -> Result<(), String> {
    let window = app
        .get_webview_window("pet")
        .ok_or_else(|| "桌面珠子窗口不存在".to_string())?;
    *runtime.mode.lock().map_err(|err| err.to_string())? = mode.clone();
    let opens_right = *runtime.opens_right.lock().map_err(|err| err.to_string())?;
    apply_hit_region(&window, &mode, opens_right)
}

#[cfg(windows)]
fn apply_hit_region(
    window: &tauri::WebviewWindow,
    mode: &str,
    opens_right: bool,
) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateEllipticRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
    };

    let hwnd = window.hwnd().map_err(|err| err.to_string())?.0;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    let inset = (5.0 * scale).round() as i32;
    let width = (WINDOW_WIDTH * scale).round() as i32;
    let height = (WINDOW_HEIGHT * scale).round() as i32;
    let pet_size = (COMPACT * scale).round() as i32;
    let pet_left = if opens_right { 0 } else { width - pet_size };
    let pet_top = height - pet_size;
    let region = unsafe {
        CreateEllipticRgn(
            pet_left + inset,
            pet_top + inset,
            pet_left + pet_size - inset,
            pet_top + pet_size - inset,
        )
    };
    if region.is_null() {
        return Err("创建桌面珠子点击区域失败".into());
    }
    let bounds = match mode {
        "hover" => Some((0.0, 160.0, WINDOW_WIDTH, WINDOW_HEIGHT)),
        "card" => Some((0.0, 16.0, WINDOW_WIDTH, 208.0)),
        "menu" => Some((0.0, 4.0, WINDOW_WIDTH, 208.0)),
        // 有字块在飞时临时盖住 300×260 场景，打完回到珠子。
        "effect" => Some((0.0, 40.0, WINDOW_WIDTH, WINDOW_HEIGHT)),
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
fn apply_hit_region(
    _window: &tauri::WebviewWindow,
    _mode: &str,
    _opens_right: bool,
) -> Result<(), String> {
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
    let opens_right = *runtime.opens_right.lock().map_err(|err| err.to_string())?;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    window
        .set_position(window_origin(position, opens_right, scale))
        .map_err(|err| err.to_string())?;
    *runtime
        .compact_position
        .lock()
        .map_err(|err| err.to_string())? = Some(position);
    Ok(())
}

#[tauri::command]
pub fn pet_snap(app: AppHandle, runtime: State<PetRuntime>) -> Result<PetLayout, String> {
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
    let opens_right = opens_right_at(&window, target)?;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    window
        .set_position(window_origin(target, opens_right, scale))
        .map_err(|err| err.to_string())?;
    *runtime
        .compact_position
        .lock()
        .map_err(|err| err.to_string())? = Some(target);
    *runtime
        .opens_right
        .lock()
        .map_err(|err| err.to_string())? = opens_right;
    let mode = runtime.mode.lock().map_err(|err| err.to_string())?.clone();
    apply_hit_region(&window, &mode, opens_right)?;
    emit_layout(&app, opens_right);
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .save_pet_position(target.x, target.y)?;
    Ok(PetLayout { opens_right })
}

#[tauri::command]
pub fn pet_layout(runtime: State<PetRuntime>) -> Result<PetLayout, String> {
    Ok(PetLayout {
        opens_right: *runtime.opens_right.lock().map_err(|err| err.to_string())?,
    })
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

/// 锁屏、全屏和演示模式时珠子不自己动，也不读取按键。
pub(crate) fn system_allows_motion() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{
            QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
            SHQueryUserNotificationState,
        };
        let mut state = 0;
        let result = unsafe { SHQueryUserNotificationState(&mut state) };
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
pub fn pet_can_idle() -> bool {
    system_allows_motion()
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

#[cfg(test)]
mod tests {
    use super::stage_opens_right;

    #[test]
    fn stage_opens_toward_the_free_side() {
        assert!(stage_opens_right(14, 1600, 204));
        assert!(!stage_opens_right(1600, 14, 204));
        assert!(!stage_opens_right(800, 800, 204));
    }
}
