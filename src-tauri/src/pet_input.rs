use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::SharedSettings;

const POLL: Duration = Duration::from_millis(30);

#[derive(Clone, Serialize)]
struct PetKeyEvent {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'static str>,
}

struct FollowGate {
    show_names: bool,
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        run(app);
        #[cfg(not(windows))]
        let _ = app;
    });
}

#[cfg(windows)]
fn run(app: AppHandle) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let mut down = [false; 256];
    let mut running = false;
    let mut next_id = 0u64;

    loop {
        std::thread::sleep(POLL);
        let Some(gate) = gate(&app) else {
            if running {
                running = false;
                crate::logging::info(format_args!("输入跟随暂停"));
            }
            continue;
        };
        if !running {
            snapshot_keys(&mut down);
            running = true;
            crate::logging::info(format_args!("输入跟随启动"));
        }

        let mut events = Vec::new();
        for vk in 1u16..256 {
            let pressed = unsafe { GetAsyncKeyState(i32::from(vk)) } < 0;
            let was = down[vk as usize];
            down[vk as usize] = pressed;
            if !pressed || was {
                continue;
            }
            let Some(label) = key_label(vk) else {
                continue;
            };
            next_id = next_id.saturating_add(1);
            events.push(PetKeyEvent {
                id: next_id,
                label: gate.show_names.then_some(label),
            });
        }
        for event in events {
            let _ = app.emit_to("pet", "pet-key", event);
        }
    }
}

#[cfg(windows)]
fn snapshot_keys(down: &mut [bool; 256]) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    for vk in 1u16..256 {
        down[vk as usize] = unsafe { GetAsyncKeyState(i32::from(vk)) } < 0;
    }
}

fn gate(app: &AppHandle) -> Option<FollowGate> {
    if !crate::pet::system_allows_motion() {
        return None;
    }
    let store = app.state::<SharedSettings>();
    let pet = store.lock().ok()?.pet_settings();
    if !pet.enabled || !pet.follow_input {
        return None;
    }
    let window = app.get_webview_window("pet")?;
    if !window.is_visible().unwrap_or(false) {
        return None;
    }
    Some(FollowGate {
        show_names: pet.show_key_names,
    })
}

pub(crate) fn key_label(vk: u16) -> Option<&'static str> {
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const FUNCTIONS: [&str; 12] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    match vk {
        0x08 => Some("Bksp"),
        0x09 => Some("Tab"),
        0x0D => Some("Enter"),
        0x11 => Some("Ctrl"),
        0x12 => Some("Alt"),
        0x13 => Some("Pause"),
        0x14 => Some("Caps"),
        0x1B => Some("Esc"),
        0x20 => Some("Space"),
        0x21 => Some("PgUp"),
        0x22 => Some("PgDn"),
        0x23 => Some("End"),
        0x24 => Some("Home"),
        0x25 => Some("Left"),
        0x26 => Some("Up"),
        0x27 => Some("Right"),
        0x28 => Some("Down"),
        0x2C => Some("Print"),
        0x2D => Some("Ins"),
        0x2E => Some("Del"),
        0x30..=0x39 => Some(DIGITS[(vk - 0x30) as usize]),
        0x41..=0x5A => Some(LETTERS[(vk - 0x41) as usize]),
        0x5B | 0x5C => Some("Win"),
        0x60..=0x69 => Some(DIGITS[(vk - 0x60) as usize]),
        0x6A => Some("*"),
        0x6B => Some("+"),
        0x6D => Some("-"),
        0x6E => Some("."),
        0x6F => Some("/"),
        0x70..=0x7B => Some(FUNCTIONS[(vk - 0x70) as usize]),
        0xBA => Some(";"),
        0xBB => Some("="),
        0xBC => Some(","),
        0xBD => Some("-"),
        0xBE => Some("."),
        0xBF => Some("/"),
        0xC0 => Some("`"),
        0xDB => Some("["),
        0xDC => Some("\\"),
        0xDD => Some("]"),
        0xDE => Some("'"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::key_label;

    #[test]
    fn key_label_names_physical_keys_and_skips_mouse() {
        assert_eq!(key_label(0x41), Some("A"));
        assert_eq!(key_label(0x20), Some("Space"));
        assert_eq!(key_label(0x0D), Some("Enter"));
        assert_eq!(key_label(0x70), Some("F1"));
        assert_eq!(key_label(0x10), None);
        assert_eq!(key_label(0x01), None);
        assert_eq!(key_label(0x02), None);
        assert_eq!(key_label(0xA0), None);
    }
}
