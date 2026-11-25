use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder,
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
    /// 字块在窗口里的 CSS 像素坐标，由前端约每 100ms 上报。
    spots: Mutex<Vec<(f64, f64)>>,
    /// 必须画出来的圈外形状。SetWindowRgn 会裁掉区域外的像素，所以字块和激光得并进来。
    patches: Mutex<Vec<ScenePatch>>,
    /// 窗口外框的物理屏幕坐标，和 scale 一起把光标换算进窗口。
    origin: Mutex<(i32, i32)>,
    scale: Mutex<f64>,
    /// 已经并进点击区域的热区，物理像素，相对窗口。
    hotspots: Mutex<Vec<(i32, i32, i32, i32)>>,
}

impl Default for PetRuntime {
    fn default() -> Self {
        Self {
            compact_position: Mutex::new(None),
            opens_right: Mutex::new(false),
            mode: Mutex::new("compact".into()),
            spots: Mutex::new(Vec::new()),
            patches: Mutex::new(Vec::new()),
            origin: Mutex::new((0, 0)),
            scale: Mutex::new(0.0),
            hotspots: Mutex::new(Vec::new()),
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
    let window = WebviewWindowBuilder::new(app, "pet", WebviewUrl::App(url.into()))
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
    *runtime.opens_right.lock().map_err(|err| err.to_string())? = opens_right;
    apply_hit_region(&window, "compact", opens_right, &[], &[])?;
    remember_frame(&window, &runtime);
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
    let hotspots = runtime.hotspots.lock().map_err(|err| err.to_string())?.clone();
    let patches = runtime.patches.lock().map_err(|err| err.to_string())?.clone();
    remember_frame(&window, &runtime);
    apply_hit_region(&window, &mode, opens_right, &hotspots, &patches)
}

#[derive(Deserialize)]
pub struct GlyphSpot {
    x: f64,
    y: f64,
}

/// 圈外要留下来的像素。坐标是窗口 CSS 像素。
/// `rect` 用对角 (x,y)-(x2,y2)；`beam` 用两端；`disk` 用圆心和半径 x2。
#[derive(Deserialize, Clone)]
pub struct ScenePatch {
    kind: String,
    x: f64,
    y: f64,
    x2: f64,
    y2: f64,
}

#[tauri::command]
pub fn pet_set_hotspots(
    app: AppHandle,
    runtime: State<PetRuntime>,
    spots: Vec<GlyphSpot>,
    patches: Vec<ScenePatch>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("pet")
        .ok_or_else(|| "桌面珠子窗口不存在".to_string())?;
    remember_frame(&window, &runtime);
    {
        let mut guard = runtime.spots.lock().map_err(|err| err.to_string())?;
        guard.clear();
        for spot in spots.into_iter().take(16) {
            guard.push((spot.x, spot.y));
        }
    }
    let patches: Vec<ScenePatch> = patches.into_iter().take(64).collect();
    *runtime.patches.lock().map_err(|err| err.to_string())? = patches.clone();
    let hotspots = runtime.hotspots.lock().map_err(|err| err.to_string())?.clone();
    let mode = runtime.mode.lock().map_err(|err| err.to_string())?.clone();
    let opens_right = *runtime.opens_right.lock().map_err(|err| err.to_string())?;
    apply_hit_region(&window, &mode, opens_right, &hotspots, &patches)
}

fn remember_frame(window: &tauri::WebviewWindow, runtime: &PetRuntime) {
    if let Ok(pos) = window.outer_position() {
        if let Ok(mut origin) = runtime.origin.lock() {
            *origin = (pos.x, pos.y);
        }
    }
    if let Ok(scale) = window.scale_factor() {
        if let Ok(mut slot) = runtime.scale.lock() {
            *slot = scale;
        }
    }
}

#[cfg(windows)]
fn union_scene_patches(
    region: windows_sys::Win32::Graphics::Gdi::HRGN,
    scale: f64,
    patches: &[ScenePatch],
) {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateEllipticRgn, CreatePolygonRgn, CreateRectRgn, DeleteObject, ALTERNATE, RGN_OR,
    };

    let px = |value: f64| (value * scale).round() as i32;
    for patch in patches.iter().take(64) {
        let extra = match patch.kind.as_str() {
            "rect" => {
                let left = px(patch.x.min(patch.x2));
                let top = px(patch.y.min(patch.y2));
                let right = px(patch.x.max(patch.x2));
                let bottom = px(patch.y.max(patch.y2));
                if right <= left || bottom <= top {
                    continue;
                }
                unsafe { CreateRectRgn(left, top, right, bottom) }
            }
            "disk" => {
                let radius = px(patch.x2);
                if radius <= 0 {
                    continue;
                }
                let cx = px(patch.x);
                let cy = px(patch.y);
                unsafe { CreateEllipticRgn(cx - radius, cy - radius, cx + radius, cy + radius) }
            }
            "beam" => {
                let half = ((7.0 * scale).round() as i32).max(4);
                let points = beam_quad(px(patch.x), px(patch.y), px(patch.x2), px(patch.y2), half);
                unsafe { CreatePolygonRgn(points.as_ptr(), 4, ALTERNATE) }
            }
            _ => continue,
        };
        if extra.is_null() {
            continue;
        }
        unsafe {
            CombineRgn(region, region, extra, RGN_OR);
            DeleteObject(extra);
        }
    }
}

#[cfg(windows)]
fn beam_quad(x1: i32, y1: i32, x2: i32, y2: i32, half: i32) -> [windows_sys::Win32::Foundation::POINT; 4] {
    use windows_sys::Win32::Foundation::POINT;

    let dx = (x2 - x1) as f64;
    let dy = (y2 - y1) as f64;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return [
            POINT { x: x1 - half, y: y1 - half },
            POINT { x: x1 + half, y: y1 - half },
            POINT { x: x1 + half, y: y1 + half },
            POINT { x: x1 - half, y: y1 + half },
        ];
    }
    let ux = dx / len;
    let uy = dy / len;
    let px = -uy * half as f64;
    let py = ux * half as f64;
    let ox = ux * half as f64;
    let oy = uy * half as f64;
    let ax = x1 as f64 - ox;
    let ay = y1 as f64 - oy;
    let bx = x2 as f64 + ox;
    let by = y2 as f64 + oy;
    [
        POINT { x: (ax + px).round() as i32, y: (ay + py).round() as i32 },
        POINT { x: (bx + px).round() as i32, y: (by + py).round() as i32 },
        POINT { x: (bx - px).round() as i32, y: (by - py).round() as i32 },
        POINT { x: (ax - px).round() as i32, y: (ay - py).round() as i32 },
    ]
}

#[cfg(windows)]
fn apply_hit_region(
    window: &tauri::WebviewWindow,
    mode: &str,
    opens_right: bool,
    hotspots: &[(i32, i32, i32, i32)],
    patches: &[ScenePatch],
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
        // 只有拖着字块时才盖住场景，松手立刻收回，避免平时吃掉桌面点击。
        "drag" => Some((0.0, 40.0, WINDOW_WIDTH, WINDOW_HEIGHT)),
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
    for (left, top, right, bottom) in hotspots {
        if *right <= *left || *bottom <= *top {
            continue;
        }
        let patch = unsafe { CreateRectRgn(*left, *top, *right, *bottom) };
        if patch.is_null() {
            continue;
        }
        unsafe {
            CombineRgn(region, region, patch, RGN_OR);
            DeleteObject(patch);
        }
    }
    // 字块、激光、墨点在珠子圆外。区域外的像素不会画出来，所以按它们的形状并进去，
    // 而不是把整块场景打开——空白处仍然点得穿桌面。
    union_scene_patches(region, scale, patches);
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
    _hotspots: &[(i32, i32, i32, i32)],
    _patches: &[ScenePatch],
) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn sync_pointer_hotspots(app: &AppHandle, sent: &mut Vec<(i32, i32, i32, i32)>) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let runtime = app.state::<PetRuntime>();
    let scale = runtime.scale.lock().map(|slot| *slot).unwrap_or(0.0);
    if scale < 0.5 {
        return;
    }
    let origin = runtime.origin.lock().map(|slot| *slot).unwrap_or((0, 0));
    let spots = runtime.spots.lock().map(|slot| slot.clone()).unwrap_or_default();
    let mut cursor = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut cursor) } == 0 {
        return;
    }
    let cx = (cursor.x - origin.0) as f64 / scale;
    let cy = (cursor.y - origin.1) as f64 / scale;
    let mut ranked: Vec<(i64, f64, f64)> = spots
        .iter()
        .filter_map(|(x, y)| {
            let dx = cx - *x;
            let dy = cy - *y;
            let dist = (dx * dx + dy * dy).sqrt();
            (dist <= 20.0).then_some(((dist * 100.0) as i64, *x, *y))
        })
        .collect();
    ranked.sort_by_key(|(dist, _, _)| *dist);
    ranked.truncate(2);
    let half = (20.0 * scale).round() as i32;
    let next: Vec<(i32, i32, i32, i32)> = ranked
        .iter()
        .map(|(_, x, y)| {
            let left = (*x * scale).round() as i32 - half;
            let top = (*y * scale).round() as i32 - half;
            (left, top, left + half * 2, top + half * 2)
        })
        .collect();
    if next == *sent {
        return;
    }
    *sent = next.clone();
    let app_for_task = app.clone();
    let _ = app.run_on_main_thread(move || {
        let runtime = app_for_task.state::<PetRuntime>();
        if let Ok(mut slot) = runtime.hotspots.lock() {
            *slot = next.clone();
        }
        let Some(window) = app_for_task.get_webview_window("pet") else {
            return;
        };
        let mode = runtime
            .mode
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_else(|_| "compact".into());
        let opens_right = runtime.opens_right.lock().map(|slot| *slot).unwrap_or(false);
        let patches = runtime.patches.lock().map(|slot| slot.clone()).unwrap_or_default();
        let _ = apply_hit_region(&window, &mode, opens_right, &next, &patches);
    });
}

#[cfg(windows)]
pub(crate) fn clear_pointer_hotspots(app: &AppHandle) {
    let runtime = app.state::<PetRuntime>();
    if let Ok(mut spots) = runtime.spots.lock() {
        spots.clear();
    }
    let empty = runtime.hotspots.lock().map(|slot| slot.is_empty()).unwrap_or(true);
    if empty {
        return;
    }
    let app_for_task = app.clone();
    let _ = app.run_on_main_thread(move || {
        let runtime = app_for_task.state::<PetRuntime>();
        if let Ok(mut slot) = runtime.hotspots.lock() {
            slot.clear();
        }
        let Some(window) = app_for_task.get_webview_window("pet") else {
            return;
        };
        let mode = runtime
            .mode
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_else(|_| "compact".into());
        let opens_right = runtime.opens_right.lock().map(|slot| *slot).unwrap_or(false);
        let patches = runtime.patches.lock().map(|slot| slot.clone()).unwrap_or_default();
        let _ = apply_hit_region(&window, &mode, opens_right, &[], &patches);
    });
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
    remember_frame(&window, &runtime);
    Ok(())
}

/// 松手吸附：从当前窗口位置缓动到目标位置，最后一帧由调用方补精确值。
/// 150ms / 九帧，够让 90–200px 的位移看起来是"滑过去"而不是"瞬移"。
fn ease_position(
    window: &tauri::WebviewWindow,
    from: PhysicalPosition<i32>,
    to: PhysicalPosition<i32>,
    ms: u64,
) {
    let distance = ((to.x - from.x).abs() + (to.y - from.y).abs()) as f64;
    if distance < 2.0 {
        return;
    }
    let steps = (ms / 16).max(2) as i32;
    for index in 1..=steps {
        let t = index as f64 / steps as f64;
        let eased = 1.0 - (1.0 - t).powi(3);
        let x = from.x + (((to.x - from.x) as f64) * eased).round() as i32;
        let y = from.y + (((to.y - from.y) as f64) * eased).round() as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
        std::thread::sleep(Duration::from_millis(16));
    }
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
    let to = window_origin(target, opens_right, scale);
    if let Ok(from) = window.outer_position() {
        ease_position(&window, from, to, 150);
    }
    window.set_position(to).map_err(|err| err.to_string())?;
    *runtime
        .compact_position
        .lock()
        .map_err(|err| err.to_string())? = Some(target);
    *runtime.opens_right.lock().map_err(|err| err.to_string())? = opens_right;
    let mode = runtime.mode.lock().map_err(|err| err.to_string())?.clone();
    let hotspots = runtime.hotspots.lock().map_err(|err| err.to_string())?.clone();
    let patches = runtime.patches.lock().map_err(|err| err.to_string())?.clone();
    remember_frame(&window, &runtime);
    apply_hit_region(&window, &mode, opens_right, &hotspots, &patches)?;
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
                QUNS_NOT_PRESENT
                    | QUNS_BUSY
                    | QUNS_RUNNING_D3D_FULL_SCREEN
                    | QUNS_PRESENTATION_MODE
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
