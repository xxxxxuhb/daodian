/// 和设置里保存的快捷键同一套写法：`Ctrl+1`、`Alt+Space`、`Ctrl+Shift+A`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub alt: bool,
    pub control: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u16,
}

pub fn parse_shortcut(input: &str) -> Result<Hotkey, String> {
    let mut alt = false;
    let mut control = false;
    let mut shift = false;
    let mut win = false;
    let mut key: Option<u16> = None;
    let parts = input
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Err("快捷键不能为空".into());
    }
    for part in parts {
        let token = part.to_ascii_lowercase();
        match token.as_str() {
            "ctrl" | "control" => control = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "win" | "super" | "meta" | "cmd" | "command" => win = true,
            other => {
                if key.is_some() {
                    return Err(format!("快捷键里有两个按键: {input}"));
                }
                key = Some(key_vk(other).ok_or_else(|| format!("不认识的按键: {part}"))?);
            }
        }
    }
    let vk = key.ok_or_else(|| format!("快捷键缺少按键: {input}"))?;
    if !(alt || control || shift || win) {
        return Err("快捷键至少要有一个修饰键".into());
    }
    Ok(Hotkey {
        alt,
        control,
        shift,
        win,
        vk,
    })
}

fn key_vk(token: &str) -> Option<u16> {
    if let Some(rest) = token.strip_prefix("digit").or_else(|| token.strip_prefix("key")) {
        return key_vk(rest);
    }
    if let Some(number) = token.strip_prefix('f') {
        let n: u16 = number.parse().ok()?;
        return (1..=24).contains(&n).then_some(0x6F + n);
    }
    match token {
        "space" => Some(0x20),
        "tab" => Some(0x09),
        "enter" | "return" => Some(0x0D),
        "esc" | "escape" => Some(0x1B),
        other if other.len() == 1 => {
            let ch = other.chars().next()?;
            match ch {
                'a'..='z' => Some(ch as u16 - b'a' as u16 + 0x41),
                '0'..='9' => Some(ch as u16),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shortcuts_settings_already_store() {
        let ctrl_1 = parse_shortcut("Ctrl+1").unwrap();
        assert!(ctrl_1.control);
        assert!(!ctrl_1.alt);
        assert_eq!(ctrl_1.vk, 0x31);

        let alt_space = parse_shortcut("alt+space").unwrap();
        assert!(alt_space.alt);
        assert_eq!(alt_space.vk, 0x20);

        let chord = parse_shortcut("Ctrl+Shift+A").unwrap();
        assert!(chord.control && chord.shift);
        assert_eq!(chord.vk, 0x41);

        let f24 = parse_shortcut("Ctrl+Alt+Shift+F24").unwrap();
        assert!(f24.control && f24.alt && f24.shift);
        assert_eq!(f24.vk, 0x87);
    }

    #[test]
    fn rejects_a_bare_key_or_two_keys() {
        assert!(parse_shortcut("").is_err());
        assert!(parse_shortcut("A").is_err());
        assert!(parse_shortcut("Ctrl+A+B").is_err());
        assert!(parse_shortcut("Ctrl").is_err());
    }
}
