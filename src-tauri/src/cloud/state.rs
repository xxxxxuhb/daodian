use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::now_ms;

const TOMB_KEEP_MS: i64 = 30 * 86_400_000;
static DIRTY_GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    #[serde(default)]
    pub remote_sha: String,
    #[serde(default)]
    pub last_pull_at: i64,
    #[serde(default)]
    pub last_push_at: i64,
    #[serde(default)]
    pub dirty: bool,
    /// 今天已经自动同步过的班次，例如 `2026-11-02:in`。
    #[serde(default)]
    pub last_auto_slot: String,
    /// 备忘录纳入同步之后的第一次成功。旧状态没有这一项时，会再推一次备忘录。
    #[serde(default)]
    pub notes_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Tomb {
    pub id: String,
    pub deleted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tombstones {
    #[serde(default)]
    pub todos: Vec<Tomb>,
    #[serde(default)]
    pub plans: Vec<Tomb>,
    #[serde(default)]
    pub templates: Vec<Tomb>,
    #[serde(default)]
    pub notes: Vec<Tomb>,
}

pub fn new_device_id() -> String {
    let mut hasher = Sha256::new();
    hasher.update(now_ms().to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let digest = hasher.finalize();
    format!("desktop-{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2])
}

pub fn dirty_generation() -> u64 {
    DIRTY_GEN.load(Ordering::SeqCst)
}

pub fn mark_dirty(dir: &Path) {
    DIRTY_GEN.fetch_add(1, Ordering::SeqCst);
    let mut state = load_state(dir);
    state.dirty = true;
    let _ = save_state(dir, &state);
}

pub fn load_state(dir: &Path) -> SyncState {
    read_json(&sync_dir(dir).join("state.json")).unwrap_or_default()
}

pub fn save_state(dir: &Path, state: &SyncState) -> Result<(), String> {
    write_json(&sync_dir(dir).join("state.json"), state)
}

pub fn load_tombs(dir: &Path) -> Tombstones {
    read_json(&sync_dir(dir).join("tombstones.json")).unwrap_or_default()
}

pub fn save_tombs(dir: &Path, tombs: &Tombstones) -> Result<(), String> {
    write_json(&sync_dir(dir).join("tombstones.json"), tombs)
}

pub fn add_tombstone(dir: &Path, kind: &str, id: &str, at: i64) {
    let mut tombs = load_tombs(dir);
    let list = match kind {
        "plans" => &mut tombs.plans,
        "templates" => &mut tombs.templates,
        "notes" => &mut tombs.notes,
        _ => &mut tombs.todos,
    };
    list.retain(|item| item.id != id);
    list.push(Tomb {
        id: id.to_string(),
        deleted_at: at,
    });
    prune_tombs(&mut tombs, at);
    let _ = save_tombs(dir, &tombs);
    mark_dirty(dir);
}

pub fn prune_tombs(tombs: &mut Tombstones, now: i64) {
    let keep = now.saturating_sub(TOMB_KEEP_MS);
    tombs.todos.retain(|item| item.deleted_at >= keep);
    tombs.plans.retain(|item| item.deleted_at >= keep);
    tombs.templates.retain(|item| item.deleted_at >= keep);
    tombs.notes.retain(|item| item.deleted_at >= keep);
}

pub fn clear_sync_dir(dir: &Path) {
    let path = sync_dir(dir);
    let _ = fs::remove_dir_all(path);
}

pub fn sync_dir(dir: &Path) -> PathBuf {
    dir.join(".sync")
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let raw = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("sync");
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    fs::write(&tmp, raw).map_err(|err| err.to_string())?;
    if fs::rename(&tmp, path).is_err() {
        let backup = path.with_extension("replacing");
        let _ = fs::remove_file(&backup);
        if path.exists() {
            fs::rename(path, &backup).map_err(|err| err.to_string())?;
        }
        if let Err(err) = fs::rename(&tmp, path) {
            let _ = fs::rename(&backup, path);
            return Err(err.to_string());
        }
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tombs_older_than_thirty_days_are_dropped() {
        let now = 40 * 86_400_000;
        let mut tombs = Tombstones {
            todos: vec![
                Tomb {
                    id: "old".into(),
                    deleted_at: 1,
                },
                Tomb {
                    id: "new".into(),
                    deleted_at: now - 86_400_000,
                },
            ],
            plans: Vec::new(),
            templates: Vec::new(),
            notes: Vec::new(),
        };
        prune_tombs(&mut tombs, now);
        assert_eq!(tombs.todos.len(), 1);
        assert_eq!(tombs.todos[0].id, "new");
    }
}
