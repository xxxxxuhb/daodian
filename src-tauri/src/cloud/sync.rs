use std::collections::HashSet;
use std::fs;
use std::sync::{LazyLock, Mutex, OnceLock, mpsc};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::cloud::api::{ApiError, Gitee};
use crate::cloud::merge::{conflict_document, merge_library, merge_notes};
use crate::cloud::state;
use crate::domain::{Document, now_ms};
use crate::notes::{Note, SharedNotes};
use crate::settings::{CloudSettings, SharedSettings};
use crate::store::templates::from_json;
use crate::store::{SharedStore, lock};

static BUSY: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static WAKE: OnceLock<mpsc::Sender<()>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudReport {
    pub pulled: bool,
    pub pushed: bool,
    pub conflict: bool,
    pub message: String,
}

pub fn spawn(app: AppHandle) {
    let (wake, sleeper) = mpsc::channel();
    if WAKE.set(wake).is_err() {
        return;
    }
    std::thread::spawn(move || {
        let _ = sleeper.recv_timeout(Duration::from_secs(5));
        loop {
            if let Some(slot) = due_slot(&app) {
                match run(&app) {
                    Ok(_) => remember_slot(&app, &slot),
                    Err(err) => crate::logging::warn(format_args!("自动同步失败: {err}")),
                }
            }
            match sleeper.recv_timeout(Duration::from_secs(30)) {
                Ok(()) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

pub fn run(app: &AppHandle) -> Result<CloudReport, String> {
    let _busy = BUSY.lock().map_err(|err| err.to_string())?;
    sync(app)
}

fn sync(app: &AppHandle) -> Result<CloudReport, String> {
    let cloud = cloud_config(app)?;
    if !cloud.enabled {
        return Err("云同步还没打开".into());
    }
    if cloud.owner.trim().is_empty() || cloud.repo.trim().is_empty() {
        publish(app, "error:repo", None, "还没有连接仓库");
        return Err("还没有连接仓库".into());
    }
    let token = token(app)?;
    let gitee = Gitee::new(&token).map_err(|err| fail(app, err))?;
    publish(app, "syncing", None, "同步中");
    let repo = gitee.repo(&cloud.owner, &cloud.repo).map_err(|err| fail(app, err))?;
    if !repo.private {
        publish(app, "error:repo", None, "仓库是公开的，已停止同步");
        return Err("仓库是公开的，已停止同步".into());
    }
    let branch = if cloud.branch.trim().is_empty() {
        remember_branch(app, &repo.branch)?;
        repo.branch
    } else {
        cloud.branch.clone()
    };
    if branch.is_empty() {
        publish(app, "error:repo", None, "仓库还没有分支");
        return Err("仓库还没有分支".into());
    }

    let mut pulled = false;
    let mut conflict = false;
    for attempt in 0..2 {
        let dir = data_dir(app)?;
        let device = device_id(app);
        let mut state = state::load_state(&dir);
        let head = gitee
            .head(&cloud.owner, &cloud.repo, &branch)
            .map_err(|err| fail(app, err))?;
        if head == state.remote_sha && !state.dirty && state.notes_ready {
            publish(app, "ok", Some(now_ms()), "已经是最新");
            return Ok(CloudReport {
                pulled: false,
                pushed: false,
                conflict: false,
                message: "已经是最新".into(),
            });
        }
        let remote_changed = head != state.remote_sha || state.remote_sha.is_empty();
        let store_path = format!("{}/store.json", cloud.dir);
        let template_path = format!("{}/templates.json", cloud.dir);
        let remote_store = if remote_changed {
            gitee
                .read(&cloud.owner, &cloud.repo, &store_path, &branch)
                .map_err(|err| fail(app, err))?
        } else {
            None
        };
        let remote_templates = if remote_changed {
            gitee
                .read(&cloud.owner, &cloud.repo, &template_path, &branch)
                .map_err(|err| fail(app, err))?
        } else {
            None
        };
        if remote_changed {
            pulled = true;
        }
        let generation = {
            let store = app.state::<SharedStore>();
            let mut guard = lock(&store)?;
            if remote_changed {
                let remote_doc = match &remote_store {
                    Some(file) if !file.bytes.is_empty() => match parse_doc(&file.bytes) {
                        Ok(doc) => doc,
                        Err(message) => {
                            publish(app, "error:gitee", None, &message);
                            return Err(format!("error:gitee|{message}"));
                        }
                    },
                    _ => Document::default(),
                };
                let (remote_templates, remote_quick) = match &remote_templates {
                    Some(file) if !file.bytes.is_empty() => {
                        from_json(&String::from_utf8_lossy(&file.bytes))
                    }
                    _ => (Vec::new(), Vec::new()),
                };
                let mut tombs = state::load_tombs(&dir);
                let merged = merge_library(
                    guard.doc.clone(),
                    remote_doc,
                    guard.custom_templates.clone(),
                    remote_templates,
                    guard.quick_template_ids.clone(),
                    remote_quick,
                    tombs.clone(),
                    state.last_pull_at,
                    !state.remote_sha.is_empty(),
                    now_ms(),
                );
                tombs = merged.tombs;
                conflict = !merged.conflicts.is_empty();
                if conflict {
                    let path = dir.join(format!("store.conflict.{device}.json"));
                    let bytes = serde_json::to_vec_pretty(&conflict_document(&merged.conflicts))
                        .map_err(|err| err.to_string())?;
                    fs::write(&path, bytes).map_err(|err| err.to_string())?;
                }
                guard.doc = merged.document;
                guard.custom_templates = merged.templates;
                guard.quick_template_ids = merged.quick_ids;
                guard.persist_quiet()?;
                guard.persist_templates_quiet()?;
                state::save_tombs(&dir, &tombs)?;
                state.last_pull_at = now_ms();
                state::save_state(&dir, &state)?;
            }
            state::dirty_generation()
        };
        let store_bytes = fs::read(dir.join("store.json")).map_err(|err| err.to_string())?;
        let template_bytes = fs::read(dir.join("templates.json")).unwrap_or_else(|_| b"{}\n".to_vec());
        let push_store = push_file(&gitee, &cloud.owner, &cloud.repo, &store_path, &branch, &store_bytes);
        if matches!(&push_store, Err(err) if err.kind == "sha") && attempt == 0 {
            continue;
        }
        let store_commit = push_store.map_err(|err| fail(app, err))?;
        let push_templates = push_file(
            &gitee,
            &cloud.owner,
            &cloud.repo,
            &template_path,
            &branch,
            &template_bytes,
        );
        if matches!(&push_templates, Err(err) if err.kind == "sha") && attempt == 0 {
            continue;
        }
        let template_commit = push_templates.map_err(|err| fail(app, err))?;
        let notes = match sync_notes(app, &gitee, &cloud, &branch, &dir, &device, remote_changed, &mut state, &mut conflict) {
            Ok(notes) => notes,
            Err(err) if err.kind == "sha" && attempt == 0 => continue,
            Err(err) => return Err(fail(app, err)),
        };
        let pushed = store_commit.is_some() || template_commit.is_some() || notes.pushed;
        state.remote_sha = notes
            .commit
            .or(template_commit)
            .or(store_commit)
            .filter(|sha| !sha.is_empty())
            .unwrap_or(head);
        state.last_push_at = if pushed { now_ms() } else { state.last_push_at };
        state.dirty = state::dirty_generation() != generation;
        state.notes_ready = true;
        state::save_state(&dir, &state)?;
        let status = if conflict { "conflict" } else { "ok" };
        let message = if conflict {
            "有内容在两边同时改过，已保留这台电脑上的，另一份写在冲突文件里"
        } else if notes.skipped > 0 {
            "同步完成，有超过 1MB 的图片没能上传"
        } else if pushed {
            "同步完成"
        } else {
            "已经是最新"
        };
        publish(app, status, Some(now_ms()), message);
        let _ = app.emit("data-changed", ());
        let _ = app.emit("notes-changed", ());
        crate::remind::check(app);
        return Ok(CloudReport {
            pulled,
            pushed,
            conflict,
            message: message.into(),
        });
    }
    publish(app, "error:gitee", None, "远端在同步过程中又变了，请再试一次");
    Err("远端在同步过程中又变了，请再试一次".into())
}

struct NoteSync {
    commit: Option<String>,
    pushed: bool,
    skipped: usize,
}

fn sync_notes(
    app: &AppHandle,
    gitee: &Gitee,
    cloud: &CloudSettings,
    branch: &str,
    dir: &std::path::Path,
    device: &str,
    remote_changed: bool,
    state: &mut state::SyncState,
    conflict: &mut bool,
) -> Result<NoteSync, ApiError> {
    let notes_prefix = format!("{}/notes", cloud.dir);
    let mut remote_names = Vec::new();
    let mut merged = None;
    if remote_changed {
        let listed = gitee.list(&cloud.owner, &cloud.repo, &notes_prefix, branch)?;
        let mut remote = Vec::new();
        for entry in &listed {
            if entry.name == "index.json" || !entry.name.ends_with(".json") {
                continue;
            }
            let id = entry.name.trim_end_matches(".json");
            if !note_id_ok(id) {
                continue;
            }
            let Some(file) = gitee.read(&cloud.owner, &cloud.repo, &format!("{notes_prefix}/{}", entry.name), branch)? else {
                continue;
            };
            let note: Note = serde_json::from_slice(&file.bytes).map_err(|_| ApiError {
                code: 200,
                kind: "gitee",
                message: "远端备忘录读不出来，已保留本地".into(),
            })?;
            remote.push(note);
            remote_names.push((id.to_string(), entry.sha.clone(), entry.name.clone()));
        }
        let notes_state = app.state::<SharedNotes>();
        let mut guard = notes_state.lock().map_err(|err| local_error(err.to_string()))?;
        let local = guard.read_notes().map_err(local_error)?;
        let tombs = state::load_tombs(dir);
        let (notes, tombs, conflicts) = merge_notes(
            local,
            remote,
            tombs,
            state.last_pull_at,
            !state.remote_sha.is_empty(),
            now_ms(),
        );
        if !conflicts.is_empty() {
            *conflict = true;
            let path = dir.join(format!("notes.conflict.{device}.json"));
            let bytes = serde_json::to_vec_pretty(&conflict_document(&conflicts)).unwrap_or_default();
            let _ = fs::write(path, bytes);
        }
        guard.write_notes(notes.clone()).map_err(local_error)?;
        state::save_tombs(dir, &tombs).map_err(local_error)?;
        merged = Some(notes);
    }
    let notes_state = app.state::<SharedNotes>();
    let guard = notes_state.lock().map_err(|err| local_error(err.to_string()))?;
    let notes = merged.unwrap_or(guard.read_notes().map_err(local_error)?);
    let attachment_dir = guard.attachment_dir().to_path_buf();
    drop(guard);
    let mut commit = None;
    let mut pushed = false;
    for note in &notes {
        let bytes = serde_json::to_vec_pretty(note).map_err(|err| local_error(err.to_string()))?;
        let path = format!("{notes_prefix}/{}.json", note.id);
        match push_file(gitee, &cloud.owner, &cloud.repo, &path, branch, &bytes)? {
            Some(sha) => {
                pushed = true;
                if !sha.is_empty() {
                    commit = Some(sha);
                }
            }
            None => {}
        }
    }
    if remote_changed {
        let keep: HashSet<&str> = notes.iter().map(|note| note.id.as_str()).collect();
        for (id, sha, name) in &remote_names {
            if keep.contains(id.as_str()) || sha.is_empty() {
                continue;
            }
            gitee.remove(&cloud.owner, &cloud.repo, &format!("{notes_prefix}/{name}"), branch, sha)?;
            pushed = true;
        }
    }
    let (skipped, attachment_commit) = sync_attachments(gitee, cloud, branch, &attachment_dir, &notes)?;
    if let Some(sha) = attachment_commit {
        pushed = true;
        if !sha.is_empty() {
            commit = Some(sha);
        }
    }
    if skipped > 0 {
        crate::logging::warn(format_args!("有 {skipped} 张备忘录图片超过 1MB，Gitee 接口放不上去"));
    }
    Ok(NoteSync {
        commit,
        pushed,
        skipped,
    })
}

fn sync_attachments(
    gitee: &Gitee,
    cloud: &CloudSettings,
    branch: &str,
    dir: &std::path::Path,
    notes: &[Note],
) -> Result<(usize, Option<String>), ApiError> {
    let hashes: HashSet<&str> = notes
        .iter()
        .flat_map(|note| note.attachments.iter().chain(note.thumb.iter()))
        .map(String::as_str)
        .collect();
    if hashes.is_empty() {
        return Ok((0, None));
    }
    let prefix = format!("{}/attachments", cloud.dir);
    let remote = gitee.list(&cloud.owner, &cloud.repo, &prefix, branch)?;
    let mut names = HashSet::new();
    if dir.is_dir() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if attachment_needed(&name, &hashes) {
                    names.insert(name);
                }
            }
        }
    }
    for entry in &remote {
        if attachment_needed(&entry.name, &hashes) {
            names.insert(entry.name.clone());
        }
    }
    let mut skipped = 0;
    let mut commit = None;
    for name in names {
        let path = dir.join(&name);
        let remote_path = format!("{prefix}/{name}");
        if path.is_file() {
            let bytes = fs::read(&path).map_err(|err| local_error(err.to_string()))?;
            if bytes.len() > 1024 * 1024 {
                skipped += 1;
                continue;
            }
            if let Some(sha) = push_file(gitee, &cloud.owner, &cloud.repo, &remote_path, branch, &bytes)? {
                if !sha.is_empty() {
                    commit = Some(sha);
                }
            }
        } else if let Some(file) = gitee.read(&cloud.owner, &cloud.repo, &remote_path, branch)? {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|err| local_error(err.to_string()))?;
            }
            fs::write(&path, file.bytes).map_err(|err| local_error(err.to_string()))?;
        }
    }
    Ok((skipped, commit))
}

fn attachment_needed(name: &str, hashes: &HashSet<&str>) -> bool {
    let stem = name.split(['.', '@']).next().unwrap_or("");
    stem.len() == 64 && hashes.contains(stem)
}

fn note_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn local_error(message: String) -> ApiError {
    ApiError {
        code: 0,
        kind: "gitee",
        message,
    }
}

fn push_file(
    gitee: &Gitee,
    owner: &str,
    repo: &str,
    path: &str,
    branch: &str,
    bytes: &[u8],
) -> Result<Option<String>, ApiError> {
    let current = gitee.read(owner, repo, path, branch)?;
    if let Some(file) = current {
        if file.bytes == bytes {
            return Ok(None);
        }
        return gitee
            .write(owner, repo, path, branch, Some(&file.sha), bytes)
            .map(Some);
    }
    gitee.write(owner, repo, path, branch, None, bytes).map(Some)
}

fn parse_doc(bytes: &[u8]) -> Result<Document, String> {
    serde_json::from_slice(bytes).map_err(|_| "远端数据读不出来，已保留本地".to_string())
}

fn cloud_config(app: &AppHandle) -> Result<CloudSettings, String> {
    let settings = app.state::<SharedSettings>();
    Ok(settings
        .lock()
        .map_err(|err| err.to_string())?
        .public_settings()
        .cloud)
}

fn token(app: &AppHandle) -> Result<String, String> {
    let settings = app.state::<SharedSettings>();
    let guard = settings.lock().map_err(|err| err.to_string())?;
    let bytes = guard.cloud_token()?;
    String::from_utf8(bytes).map_err(|_| "令牌读不出来，请重新填写".to_string())
}

fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let store = app.state::<SharedStore>();
    let guard = lock(&store)?;
    Ok(guard.data_dir()?.to_path_buf())
}

fn device_id(app: &AppHandle) -> String {
    cloud_config(app)
        .map(|cloud| cloud.device_id)
        .unwrap_or_else(|_| "desktop".into())
}

fn remember_branch(app: &AppHandle, branch: &str) -> Result<(), String> {
    if branch.is_empty() {
        return Ok(());
    }
    let settings = app.state::<SharedSettings>();
    let mut guard = settings.lock().map_err(|err| err.to_string())?;
    guard.set_cloud_branch(branch)
}

fn publish(app: &AppHandle, status: &str, synced_at: Option<i64>, message: &str) {
    let settings = app.state::<SharedSettings>();
    let Ok(mut guard) = settings.lock() else {
        return;
    };
    if guard.set_cloud_status(status, synced_at).is_err() {
        return;
    }
    let public = guard.public_settings();
    drop(guard);
    let _ = app.emit("settings-changed", &public);
    let _ = app.emit(
        "cloud-status",
        serde_json::json!({
            "enabled": public.cloud.enabled,
            "hasToken": public.cloud.has_token,
            "owner": public.cloud.owner,
            "repo": public.cloud.repo,
            "branch": public.cloud.branch,
            "dir": public.cloud.dir,
            "autoSync": public.cloud.auto_sync,
            "intervalMinutes": public.cloud.interval_minutes,
            "lastSyncAt": public.cloud.last_sync_at,
            "lastStatus": public.cloud.last_status,
            "message": message,
        }),
    );
}

fn fail(app: &AppHandle, err: ApiError) -> String {
    let status = match err.kind {
        "offline" => "error:offline",
        "auth" => "error:auth",
        "missing" | "name" | "repo" => "error:repo",
        _ => "error:gitee",
    };
    crate::logging::warn(format_args!("云同步: {}", err.message));
    publish(app, status, None, &err.message);
    format!("{status}|{}", err.message)
}

fn auto_enabled(app: &AppHandle) -> bool {
    cloud_config(app)
        .map(|cloud| cloud.enabled && cloud.auto_sync && !cloud.owner.is_empty() && !cloud.repo.is_empty())
        .unwrap_or(false)
}

fn due_slot(app: &AppHandle) -> Option<String> {
    if !auto_enabled(app) {
        return None;
    }
    let settings = app.state::<SharedSettings>();
    let guard = settings.lock().ok()?;
    let hours = guard.public_settings().work_hours;
    drop(guard);
    let now = chrono::Local::now();
    let clock = now.format("%H:%M").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    let already = data_dir(app)
        .ok()
        .map(|dir| state::load_state(&dir).last_auto_slot)
        .unwrap_or_default();
    commute_slot(&clock, &date, &hours.morning_start, &hours.afternoon_end, &already)
}

fn remember_slot(app: &AppHandle, slot: &str) {
    let Ok(dir) = data_dir(app) else {
        return;
    };
    let mut saved = state::load_state(&dir);
    saved.last_auto_slot = slot.to_string();
    let _ = state::save_state(&dir, &saved);
}

/// 上班、下班那一分钟各同步一次。同一分钟里已经成功过就不再自动跑。
fn commute_slot(clock: &str, date: &str, morning_start: &str, afternoon_end: &str, already: &str) -> Option<String> {
    let clock = clock.trim();
    let kind = if !morning_start.trim().is_empty() && clock == morning_start.trim() {
        "in"
    } else if !afternoon_end.trim().is_empty() && clock == afternoon_end.trim() {
        "out"
    } else {
        return None;
    };
    let slot = format!("{date}:{kind}");
    if already == slot { None } else { Some(slot) }
}

pub fn failure_status(err: &str) -> (&str, &str) {
    err.split_once('|').unwrap_or(("error:gitee", err))
}

#[cfg(test)]
mod tests {
    use super::commute_slot;

    #[test]
    fn auto_sync_only_at_commute_times() {
        assert_eq!(
            commute_slot("09:00", "2026-11-02", "09:00", "18:00", ""),
            Some("2026-11-02:in".into())
        );
        assert_eq!(
            commute_slot("18:00", "2026-11-02", "09:00", "18:00", ""),
            Some("2026-11-02:out".into())
        );
        assert_eq!(commute_slot("12:00", "2026-11-02", "09:00", "18:00", ""), None);
        assert_eq!(
            commute_slot("09:00", "2026-11-02", "09:00", "18:00", "2026-11-02:in"),
            None
        );
        assert_eq!(
            commute_slot("09:00", "2026-11-03", "09:00", "18:00", "2026-11-02:in"),
            Some("2026-11-03:in".into())
        );
    }
}
