use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::SharedSettings;

const POLL: Duration = Duration::from_millis(30);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PetKeyEvent {
    id: u64,
    vk: u16,
    edge: &'static str,
    mods: u8,
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
    let mut hotspots = Vec::new();

    loop {
        std::thread::sleep(POLL);
        let Some(gate) = gate(&app) else {
            if running {
                running = false;
                crate::logging::info(format_args!("输入跟随暂停"));
            }
            if !hotspots.is_empty() {
                hotspots.clear();
                crate::pet::clear_pointer_hotspots(&app);
            }
            continue;
        };
        if !running {
            snapshot_keys(&mut down);
            running = true;
            crate::logging::info(format_args!("输入跟随启动"));
        }

        let mut edges = Vec::new();
        for vk in 1u16..256 {
            let pressed = unsafe { GetAsyncKeyState(i32::from(vk)) } < 0;
            let was = down[vk as usize];
            if pressed != was {
                edges.push((vk, pressed));
            }
            down[vk as usize] = pressed;
        }
        let mods = modifier_bits(&down);
        let mut events = Vec::new();
        for (vk, pressed) in edges {
            let label = key_label(vk);
            if label.is_none() && !is_modifier(vk) {
                continue;
            }
            let mut id = 0;
            if pressed && !is_modifier(vk) {
                next_id = next_id.saturating_add(1);
                id = next_id;
            }
            events.push(PetKeyEvent {
                id,
                vk,
                edge: if pressed { "down" } else { "up" },
                mods,
                label: if is_modifier(vk) {
                    None
                } else {
                    gate.show_names.then_some(label).flatten()
                },
            });
        }
        for event in events {
            let _ = app.emit_to("pet", "pet-key", event);
        }
        crate::pet::sync_pointer_hotspots(&app, &mut hotspots);
    }
}

fn is_modifier(vk: u16) -> bool {
    // 0x10 / 0x11 是左右键的汇总，和 0xA0..=0xA3 会同时按下。只听左右键，避免一次按出两块。
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA4 | 0xA5)
}

fn modifier_bits(down: &[bool; 256]) -> u8 {
    let shift = down[0x10] || down[0xA0] || down[0xA1];
    let ctrl = down[0x11] || down[0xA2] || down[0xA3];
    let alt = down[0x12] || down[0xA4] || down[0xA5];
    u8::from(shift) | (u8::from(ctrl) << 1) | (u8::from(alt) << 2)
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
        0x10 | 0xA0 | 0xA1 => Some("Shift"),
        0x11 | 0xA2 | 0xA3 => Some("Ctrl"),
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
        assert_eq!(key_label(0x10), Some("Shift"));
        assert_eq!(key_label(0xA0), Some("Shift"));
        assert_eq!(key_label(0xA1), Some("Shift"));
        assert_eq!(key_label(0x11), Some("Ctrl"));
        assert_eq!(key_label(0xA2), Some("Ctrl"));
        assert_eq!(key_label(0xA3), Some("Ctrl"));
        assert_eq!(key_label(0x01), None);
        assert_eq!(key_label(0x02), None);
    }

    #[test]
    fn modifiers_do_not_become_glyphs() {
        assert!(super::is_modifier(0x10));
        assert!(super::is_modifier(0x11));
        assert!(!super::is_modifier(0xA0));
        assert!(!super::is_modifier(0xA2));
        assert!(super::is_modifier(0x12));
        assert!(super::is_modifier(0xA4));
        assert!(!super::is_modifier(0x41));
        assert!(!super::is_modifier(0x20));
    }
}
