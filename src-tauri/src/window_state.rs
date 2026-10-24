use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, window::Monitor};

use crate::settings::{SharedSettings, WindowFrame, WindowSettings};

const MINI_WIDTH: f64 = 720.0;
const WIDE_WIDTH: f64 = 1180.0;
static SAVE_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tier {
    Mini,
    Standard,
    Wide,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuickCaptureWindow {
    pub(crate) tier: Tier,
    pub(crate) was_visible: bool,
}

impl Tier {
    fn from_width(width: f64) -> Self {
        if width < MINI_WIDTH {
            Self::Mini
        } else if width < WIDE_WIDTH {
            Self::Standard
        } else {
            Self::Wide
        }
    }

    fn from_name(value: &str) -> Option<Self> {
        match value {
            "mini" => Some(Self::Mini),
            "standard" => Some(Self::Standard),
            "wide" => Some(Self::Wide),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Mini => "mini",
            Self::Standard => "standard",
            Self::Wide => "wide",
        }
    }

    fn default_frame(self) -> WindowFrame {
        match self {
            Self::Mini => WindowFrame::sized(380.0, 552.0),
            Self::Standard => WindowFrame::sized(900.0, 600.0),
            Self::Wide => WindowFrame::sized(1280.0, 760.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn overlap_area(a: Rect, b: Rect) -> u64 {
    let left = i64::from(a.x).max(i64::from(b.x));
    let top = i64::from(a.y).max(i64::from(b.y));
    let right = (i64::from(a.x) + i64::from(a.width)).min(i64::from(b.x) + i64::from(b.width));
    let bottom = (i64::from(a.y) + i64::from(a.height)).min(i64::from(b.y) + i64::from(b.height));
    u64::try_from((right - left).max(0) * (bottom - top).max(0)).unwrap_or(0)
}

/// Keeps the whole window inside the best matching display work area.
/// `fallback` is used when the remembered display has gone away.
fn clamp_rect(frame: Rect, work_areas: &[Rect], fallback: usize) -> Rect {
    let Some(default_area) = work_areas
        .get(fallback)
        .or_else(|| work_areas.first())
        .copied()
    else {
        return frame;
    };
    let area = work_areas
        .iter()
        .copied()
        .max_by_key(|candidate| overlap_area(frame, *candidate))
        .filter(|candidate| overlap_area(frame, *candidate) > 0)
        .unwrap_or(default_area);
    let width = frame.width.min(area.width);
    let height = frame.height.min(area.height);
    let max_x = i64::from(area.x) + i64::from(area.width - width);
    let max_y = i64::from(area.y) + i64::from(area.height - height);
    Rect {
        x: i64::from(frame.x)
            .clamp(i64::from(area.x), max_x)
            .try_into()
            .unwrap_or(area.x),
        y: i64::from(frame.y)
            .clamp(i64::from(area.y), max_y)
            .try_into()
            .unwrap_or(area.y),
        width,
        height,
    }
}

fn work_area(monitor: &Monitor) -> Rect {
    let area = monitor.work_area();
    Rect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    }
}

fn current_settings(app: &AppHandle) -> Result<WindowSettings, String> {
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())
        .map(|store| store.window_settings())
}

fn physical_frame(frame: &WindowFrame, monitor: &Monitor) -> Rect {
    let scale = monitor.scale_factor();
    let area = work_area(monitor);
    let width = (frame.w * scale).round().max(1.0) as u32;
    let height = (frame.h * scale).round().max(1.0) as u32;
    let (x, y) = match (frame.x, frame.y) {
        (Some(x), Some(y)) => ((x * scale).round() as i32, (y * scale).round() as i32),
        _ => (
            area.x + (area.width.saturating_sub(width) / 2) as i32,
            area.y + (area.height.saturating_sub(height) / 2) as i32,
        ),
    };
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn apply_tier(window: &WebviewWindow, tier: Tier) -> Result<(), String> {
    let app = window.app_handle();
    let settings = current_settings(app)?;
    let frame = settings
        .frames
        .get(tier.name())
        .cloned()
        .unwrap_or_else(|| tier.default_frame());
    let monitors = window.available_monitors().map_err(|err| err.to_string())?;
    let fallback_monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| monitors.first().cloned())
        .ok_or_else(|| "没有可用显示器".to_string())?;

    // Stored values are logical pixels. Convert with the candidate monitor's
    // scale, then clamp and send only physical pixels to Tauri.
    let areas: Vec<_> = monitors.iter().map(work_area).collect();
    let fallback = monitors
        .iter()
        .position(|monitor| work_area(monitor) == work_area(&fallback_monitor))
        .unwrap_or(0);
    let target = if frame.x.is_some() && frame.y.is_some() {
        monitors
            .iter()
            .enumerate()
            .map(|(index, monitor)| {
                let candidate = physical_frame(&frame, monitor);
                (index, overlap_area(candidate, work_area(monitor)))
            })
            .max_by_key(|(_, overlap)| *overlap)
            .filter(|(_, overlap)| *overlap > 0)
            .map(|(index, _)| index)
            .unwrap_or(fallback)
    } else {
        fallback
    };
    let candidate = physical_frame(&frame, &monitors[target]);
    let clamped = clamp_rect(candidate, &areas, fallback);
    window
        .set_size(PhysicalSize::new(clamped.width, clamped.height))
        .map_err(|err| err.to_string())?;
    window
        .set_position(PhysicalPosition::new(clamped.x, clamped.y))
        .map_err(|err| err.to_string())?;
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .set_active_window_tier(tier.name())?;
    Ok(())
}

fn remember_window(window: &WebviewWindow) -> Result<(), String> {
    if window.is_minimized().unwrap_or(false) {
        return Ok(());
    }
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    let size = window.inner_size().map_err(|err| err.to_string())?;
    let position = window.outer_position().map_err(|err| err.to_string())?;
    let width = f64::from(size.width) / scale;
    let tier = Tier::from_width(width);
    let frame = WindowFrame {
        w: width.round(),
        h: (f64::from(size.height) / scale).round(),
        x: Some((f64::from(position.x) / scale).round()),
        y: Some((f64::from(position.y) / scale).round()),
    };
    window
        .app_handle()
        .state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .save_window_frame(tier.name(), frame)
}

pub fn restore(window: &WebviewWindow) -> Result<(), String> {
    let settings = current_settings(window.app_handle())?;
    let Some(tier) =
        Tier::from_name(&settings.default_tier).or_else(|| Tier::from_name(&settings.last_tier))
    else {
        return Ok(());
    };
    apply_tier(window, tier)
}

pub fn schedule_remember(app: &AppHandle) {
    let generation = SAVE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        if SAVE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let app_for_task = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = app_for_task.get_webview_window("main") {
                if let Err(err) = remember_window(&window) {
                    crate::logging::warn(format_args!("保存窗口位置失败: {err}"));
                }
            }
        });
    });
}

pub fn toggle(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    remember_window(&window)?;
    let settings = current_settings(app)?;
    let current = Tier::from_width(
        f64::from(window.inner_size().map_err(|err| err.to_string())?.width)
            / window.scale_factor().map_err(|err| err.to_string())?,
    );
    let target = if current == Tier::Mini {
        Tier::from_name(&settings.last_non_mini_tier)
            .filter(|tier| *tier != Tier::Mini)
            .unwrap_or(Tier::Standard)
    } else {
        Tier::Mini
    };
    apply_tier(&window, target)?;
    crate::tray::show_main(app);
    Ok(())
}

pub(crate) fn enter_quick_capture(app: &AppHandle) -> Result<QuickCaptureWindow, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let was_visible = window.is_visible().map_err(|err| err.to_string())?;
    let tier = Tier::from_width(
        f64::from(window.inner_size().map_err(|err| err.to_string())?.width)
            / window.scale_factor().map_err(|err| err.to_string())?,
    );
    if tier != Tier::Mini {
        remember_window(&window)?;
        apply_tier(&window, Tier::Mini)?;
    }
    window.unminimize().map_err(|err| err.to_string())?;
    window.show().map_err(|err| err.to_string())?;
    window.set_focus().map_err(|err| err.to_string())?;
    Ok(QuickCaptureWindow { tier, was_visible })
}

pub(crate) fn leave_quick_capture(
    app: &AppHandle,
    previous: QuickCaptureWindow,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    if previous.tier != Tier::Mini {
        apply_tier(&window, previous.tier)?;
    }
    if previous.was_visible {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
    } else {
        window.hide().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn toggle_window_tier(app: AppHandle) -> Result<(), String> {
    toggle(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_size_and_position_to_work_area() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 1200,
            height: 800,
        };
        assert_eq!(
            clamp_rect(
                Rect {
                    x: -200,
                    y: 700,
                    width: 1400,
                    height: 900,
                },
                &[area],
                0,
            ),
            area
        );
    }

    #[test]
    fn uses_fallback_when_remembered_display_is_gone() {
        let primary = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        assert_eq!(
            clamp_rect(
                Rect {
                    x: 3000,
                    y: 200,
                    width: 900,
                    height: 600,
                },
                &[primary],
                0,
            ),
            Rect {
                x: 1020,
                y: 200,
                width: 900,
                height: 600,
            }
        );
    }

    #[test]
    fn keeps_frame_on_intersecting_secondary_display() {
        let areas = [
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
            Rect {
                x: 1920,
                y: 0,
                width: 1280,
                height: 984,
            },
        ];
        let frame = Rect {
            x: 2500,
            y: 100,
            width: 900,
            height: 600,
        };
        assert_eq!(clamp_rect(frame, &areas, 0).x, 2300);
    }
}
