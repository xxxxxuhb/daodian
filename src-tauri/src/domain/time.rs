use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(1);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    format!("{}-{}", now_ms(), SEQ.fetch_add(1, Ordering::Relaxed))
}

pub fn normalize_priority(value: &str) -> String {
    match value {
        "high" | "low" => value.to_string(),
        _ => "normal".to_string(),
    }
}

pub fn clean_note(note: &str) -> Result<String, String> {
    let note = note.trim().to_string();
    if note.chars().count() > 500 {
        return Err("备注最多 500 个字".into());
    }
    Ok(note)
}

pub fn require_title(title: &str, max: usize) -> Result<String, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("先写个标题".into());
    }
    if title.chars().count() > max {
        return Err(format!("标题最多 {max} 个字"));
    }
    Ok(title)
}
