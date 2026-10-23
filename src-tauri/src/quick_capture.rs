use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::settings::SharedSettings;
use crate::window_state::{self, QuickCaptureWindow};

#[derive(Default)]
pub(crate) struct QuickCaptureRuntime {
    inner: Mutex<RuntimeState>,
}

#[derive(Default)]
struct RuntimeState {
    registered: Option<String>,
    capture: CaptureState,
}

#[derive(Default)]
enum CaptureState {
    #[default]
    Idle,
    Entering,
    Active(QuickCaptureWindow),
}

impl CaptureState {
    fn begin(&mut self) -> bool {
        if !matches!(self, Self::Idle) {
            return false;
        }
        *self = Self::Entering;
        true
    }

    fn activate(&mut self, window: QuickCaptureWindow) {
        *self = Self::Active(window);
    }

    fn cancel(&mut self) {
        *self = Self::Idle;
    }

    fn take(&mut self) -> Option<QuickCaptureWindow> {
        match std::mem::take(self) {
            Self::Active(window) => Some(window),
            other => {
                *self = other;
                None
            }
        }
    }

    fn is_active(&self) -> bool {
        matches!(self, Self::Active(_))
    }
}

pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::<tauri::Wry>::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                trigger(app);
            }
        })
        .build()
}

pub(crate) fn sync_registration(app: &AppHandle) -> String {
    let configured = match app.state::<SharedSettings>().lock() {
        Ok(store) => store.quick_capture_settings(),
        Err(err) => return format!("快速记录状态读取失败：{err}"),
    };
    let shortcut = configured.shortcut.trim().to_string();
    let runtime = app.state::<QuickCaptureRuntime>();
    let mut state = match runtime.inner.lock() {
        Ok(state) => state,
        Err(err) => return format!("快速记录状态锁定失败：{err}"),
    };

    let desired = configured.enabled.then_some(shortcut.clone());
    if state.registered == desired {
        return if let Some(value) = desired {
            format!("已启用：{value}")
        } else {
            "已关闭".into()
        };
    }

    if let Some(previous) = state.registered.clone() {
        if let Err(err) = app.global_shortcut().unregister(previous.as_str()) {
            return format!("注销快捷键失败：{err}");
        }
        state.registered = None;
    }

    if !configured.enabled {
        return "已关闭".into();
    }
    if shortcut.is_empty() {
        return "注册失败：快捷键不能为空".into();
    }
    match app.global_shortcut().register(shortcut.as_str()) {
        Ok(()) => {
            state.registered = Some(shortcut.clone());
            format!("已启用：{shortcut}")
        }
        Err(err) => format!("注册失败：{err}"),
    }
}

pub(crate) fn persist_registration_status(app: &AppHandle) -> Result<String, String> {
    let status = sync_registration(app);
    app.state::<SharedSettings>()
        .lock()
        .map_err(|err| err.to_string())?
        .set_quick_capture_status(&status)?;
    Ok(status)
}

fn trigger(app: &AppHandle) {
    let runtime = app.state::<QuickCaptureRuntime>();
    {
        let Ok(mut state) = runtime.inner.lock() else {
            return;
        };
        if !state.capture.begin() {
            return;
        }
    }

    match window_state::enter_quick_capture(app) {
        Ok(window) => {
            if let Ok(mut state) = runtime.inner.lock() {
                state.capture.activate(window);
            }
            if let Err(err) = app.emit("quick-capture", ()) {
                crate::logging::warn(format_args!("发送快速记录事件失败: {err}"));
            }
        }
        Err(err) => {
            if let Ok(mut state) = runtime.inner.lock() {
                state.capture.cancel();
            }
            crate::logging::warn(format_args!("进入快速记录失败: {err}"));
        }
    }
}

#[tauri::command]
pub fn dismiss_quick_capture(
    app: AppHandle,
    runtime: State<QuickCaptureRuntime>,
) -> Result<bool, String> {
    let previous = runtime
        .inner
        .lock()
        .map_err(|err| err.to_string())?
        .capture
        .take();
    let Some(previous) = previous else {
        return Ok(false);
    };
    if let Err(err) = window_state::leave_quick_capture(&app, previous.clone()) {
        runtime
            .inner
            .lock()
            .map_err(|lock_err| lock_err.to_string())?
            .capture
            .activate(previous);
        return Err(err);
    }
    Ok(true)
}

#[tauri::command]
pub fn is_quick_capture_active(runtime: State<QuickCaptureRuntime>) -> Result<bool, String> {
    Ok(runtime
        .inner
        .lock()
        .map_err(|err| err.to_string())?
        .capture
        .is_active())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> QuickCaptureWindow {
        QuickCaptureWindow {
            tier: crate::window_state::Tier::Standard,
            was_visible: true,
        }
    }

    #[test]
    fn capture_state_rejects_duplicate_activation() {
        let mut state = CaptureState::default();
        assert!(state.begin());
        assert!(!state.begin());
        state.activate(snapshot());
        assert!(state.is_active());
        assert!(!state.begin());
    }

    #[test]
    fn capture_state_restores_only_once() {
        let mut state = CaptureState::default();
        assert!(state.begin());
        state.activate(snapshot());
        assert!(state.take().is_some());
        assert!(state.take().is_none());
        assert!(!state.is_active());
        assert!(state.begin());
    }
}
