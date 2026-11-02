use std::collections::HashMap;
use std::fs;
#[cfg(test)]
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::domain::{RemindAction, RemindTarget, new_id, now_ms};
use crate::logging;

const DOC_VERSION: u32 = 1;
const INDEX_VERSION: u32 = 1;
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_DOC_BYTES: usize = 2 * 1024 * 1024;
const MAX_ATTACHMENTS: usize = 200;
const TEXT_BYTES: usize = 4096;
const SUMMARY_CHARS: usize = 140;
const GRACE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub type SharedNotes = Mutex<NotesStore>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteIndex {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub due_at: Option<i64>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub reminded: bool,
    #[serde(default)]
    pub thumb: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub doc: Value,
    #[serde(default = "doc_version")]
    pub doc_version: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub due_at: Option<i64>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub reminded: bool,
    #[serde(default)]
    pub thumb: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

fn doc_version() -> u32 {
    DOC_VERSION
}

impl Note {
    fn to_index(&self) -> NoteIndex {
        NoteIndex {
            id: self.id.clone(),
            title: self.title.clone(),
            summary: self.summary.clone(),
            text: self.text.clone(),
            tags: self.tags.clone(),
            due_at: self.due_at,
            pinned: self.pinned,
            archived: self.archived,
            reminded: self.reminded,
            thumb: self.thumb.clone(),
            attachments: self.attachments.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NoteIndexFile {
    version: u32,
    notes: Vec<NoteIndex>,
}

impl Default for NoteIndexFile {
    fn default() -> Self {
        Self {
            version: INDEX_VERSION,
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesIndexResponse {
    pub version: u32,
    pub attachments_dir: String,
    pub notes: Vec<NoteIndex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSave {
    pub id: String,
    pub title: String,
    pub doc: Value,
    #[serde(default = "doc_version")]
    pub doc_version: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub due_at: Option<i64>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub thumb: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub summary: String,
    pub base_updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMetaPatch {
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub due_at: Option<Option<i64>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub hash: String,
    pub ext: String,
    pub thumb: Option<String>,
}

pub struct NotesStore {
    data_dir: PathBuf,
    notes_dir: PathBuf,
    attachments_dir: PathBuf,
    index_path: PathBuf,
    index: NoteIndexFile,
    uploads: HashMap<String, (PathBuf, usize)>,
}

impl NotesStore {
    pub fn open(dir: &Path) -> Result<Self, std::io::Error> {
        if !dir.exists() {
            fs::create_dir_all(dir)?;
        }
        let notes_dir = dir.join("notes");
        let attachments_dir = dir.join("attachments");
        fs::create_dir_all(&notes_dir)?;
        fs::create_dir_all(attachments_dir.join(".incoming"))?;
        let index_path = notes_dir.join("index.json");
        let mut store = Self {
            data_dir: dir.to_path_buf(),
            notes_dir,
            attachments_dir,
            index_path,
            index: NoteIndexFile::default(),
            uploads: HashMap::new(),
        };
        store.load_index().map_err(|err| std::io::Error::other(err))?;
        logging::info(format_args!(
            "notes opened; count={}",
            store.index.notes.len()
        ));
        Ok(store)
    }

    #[cfg(test)]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn reload_from_disk(&mut self) -> Result<(), String> {
        let dir = self.data_dir.clone();
        *self = Self::open(&dir).map_err(|err| err.to_string())?;
        Ok(())
    }

    fn load_index(&mut self) -> Result<(), String> {
        if self.index_path.is_file() {
            let raw = fs::read_to_string(&self.index_path).map_err(|err| err.to_string())?;
            match serde_json::from_str::<NoteIndexFile>(&raw) {
                Ok(index) => {
                    self.index = index;
                    self.reconcile()?;
                    return Ok(());
                }
                Err(err) => {
                    logging::warn(format_args!("笔记索引损坏，改为从正文重建: {err}"));
                }
            }
        }
        self.rebuild_index()
    }

    fn reconcile(&mut self) -> Result<(), String> {
        let mut changed = false;
        let ids: Vec<String> = self.index.notes.iter().map(|entry| entry.id.clone()).collect();
        for id in ids {
            let path = self.note_path(&id);
            if !path.is_file() {
                continue;
            }
            let Ok(note) = read_note(&path) else {
                continue;
            };
            let Some(entry) = self.index.notes.iter_mut().find(|item| item.id == id) else {
                continue;
            };
            if note.updated_at != entry.updated_at {
                *entry = note.to_index();
                changed = true;
            }
        }
        if changed {
            self.persist_index()?;
        }
        Ok(())
    }

    fn rebuild_index(&mut self) -> Result<(), String> {
        let mut notes = Vec::new();
        for entry in fs::read_dir(&self.notes_dir).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || name == "index.json" || name.starts_with('.') {
                continue;
            }
            let Ok(mut note) = read_note(&entry.path()) else {
                logging::warn(format_args!("跳过读不出来的笔记: {name}"));
                continue;
            };
            if note.text.trim().is_empty() {
                note.text = clip_bytes(&plain_text(&note.doc), TEXT_BYTES);
            }
            if note.summary.trim().is_empty() {
                note.summary = summarize(&note.text);
            }
            notes.push(note.to_index());
        }
        notes.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        self.index = NoteIndexFile {
            version: INDEX_VERSION,
            notes,
        };
        self.persist_index()
    }

    fn note_path(&self, id: &str) -> PathBuf {
        self.notes_dir.join(format!("{id}.json"))
    }

    pub(crate) fn read_notes(&self) -> Result<Vec<Note>, String> {
        let mut notes = Vec::new();
        if !self.notes_dir.is_dir() {
            return Ok(notes);
        }
        for entry in fs::read_dir(&self.notes_dir).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || name == "index.json" || name.starts_with('.') {
                continue;
            }
            notes.push(read_note(&entry.path())?);
        }
        Ok(notes)
    }

    pub(crate) fn write_notes(&mut self, notes: Vec<Note>) -> Result<(), String> {
        let mut ids = Vec::new();
        for note in &notes {
            safe_id(&note.id)?;
            self.write_note(note)?;
            ids.push(note.id.clone());
        }
        if self.notes_dir.is_dir() {
            for entry in fs::read_dir(&self.notes_dir).map_err(|err| err.to_string())? {
                let entry = entry.map_err(|err| err.to_string())?;
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.ends_with(".json") || name == "index.json" || name.starts_with('.') {
                    continue;
                }
                let id = name.trim_end_matches(".json");
                if !ids.iter().any(|item| item == id) {
                    fs::remove_file(entry.path()).map_err(|err| err.to_string())?;
                }
            }
        }
        let mut index_notes: Vec<NoteIndex> = notes.iter().map(Note::to_index).collect();
        index_notes.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        self.index = NoteIndexFile {
            version: INDEX_VERSION,
            notes: index_notes,
        };
        self.persist_index()
    }

    pub(crate) fn attachment_dir(&self) -> &Path {
        &self.attachments_dir
    }

    fn persist_index(&self) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(&self.index).map_err(|err| err.to_string())?;
        atomic_write(&self.index_path, raw.as_bytes())
    }

    fn write_note(&self, note: &Note) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(note).map_err(|err| err.to_string())?;
        atomic_write(&self.note_path(&note.id), raw.as_bytes())
    }

    fn upsert(&mut self, note: Note) -> Result<NoteIndex, String> {
        self.write_note(&note)?;
        let index = note.to_index();
        if let Some(slot) = self.index.notes.iter_mut().find(|item| item.id == note.id) {
            *slot = index.clone();
        } else {
            self.index.notes.push(index.clone());
        }
        self.persist_index()?;
        Ok(index)
    }

    fn require(&self, id: &str) -> Result<Note, String> {
        safe_id(id)?;
        read_note(&self.note_path(id))
    }
}

fn read_note(path: &Path) -> Result<Note, String> {
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&raw).map_err(|err| format!("笔记读不出来: {err}"))
}

fn lock(store: &SharedNotes) -> Result<MutexGuard<'_, NotesStore>, String> {
    store.lock().map_err(|err| err.to_string())
}

fn wake(app: &AppHandle) {
    let _ = app.emit("notes-changed", ());
    crate::remind::sync_reminders(app.clone());
}

pub fn due_actions(store: &SharedNotes, now: i64) -> Result<Vec<RemindAction>, String> {
    let guard = lock(store)?;
    Ok(actions_for(&guard.index.notes, now))
}

pub fn next_note_due(store: &SharedNotes, now: i64) -> Option<i64> {
    let guard = lock(store).ok()?;
    guard
        .index
        .notes
        .iter()
        .filter(|note| !note.archived && !note.reminded)
        .filter_map(|note| note.due_at.filter(|at| *at > now))
        .min()
}

pub fn mark_notes_reminded(
    store: &SharedNotes,
    sent: &[(RemindTarget, i64, bool)],
) -> Result<(), String> {
    let notes: Vec<(String, i64)> = sent
        .iter()
        .filter_map(|(target, at, _lead)| match target {
            RemindTarget::Note { id } => Some((id.clone(), *at)),
            _ => None,
        })
        .collect();
    if notes.is_empty() {
        return Ok(());
    }
    let mut guard = lock(store)?;
    let mut changed = false;
    for (id, at) in notes {
        let Some(entry) = guard.index.notes.iter_mut().find(|item| item.id == id) else {
            continue;
        };
        if entry.due_at != Some(at) || entry.reminded {
            continue;
        }
        entry.reminded = true;
        changed = true;
        if let Ok(mut note) = read_note(&guard.note_path(&id)) {
            note.reminded = true;
            let _ = guard.write_note(&note);
        }
    }
    if changed {
        guard.persist_index()?;
        crate::cloud::state::mark_dirty(&guard.data_dir);
    }
    Ok(())
}

fn actions_for(notes: &[NoteIndex], now: i64) -> Vec<RemindAction> {
    notes
        .iter()
        .filter(|note| !note.archived && !note.reminded)
        .filter_map(|note| {
            let at = note.due_at?;
            if at > now {
                return None;
            }
            let title = if note.title.trim().is_empty() {
                "未命名".to_string()
            } else {
                note.title.clone()
            };
            let detail = if note.summary.trim().is_empty() {
                "到点了，打开看看".to_string()
            } else {
                note.summary.chars().take(48).collect()
            };
            Some(RemindAction {
                target: RemindTarget::Note { id: note.id.clone() },
                at,
                title: format!("备忘录 · {title}"),
                detail,
                notify: true,
                lead: false,
            })
        })
        .collect()
}

#[tauri::command]
pub fn notes_index(store: State<SharedNotes>) -> Result<NotesIndexResponse, String> {
    let guard = lock(&store)?;
    Ok(NotesIndexResponse {
        version: guard.index.version,
        attachments_dir: guard.attachments_dir.display().to_string(),
        notes: guard.index.notes.clone(),
    })
}

#[tauri::command]
pub fn note_load(store: State<SharedNotes>, id: String) -> Result<Note, String> {
    let guard = lock(&store)?;
    guard.require(&id)
}

#[tauri::command]
pub fn note_create(
    app: AppHandle,
    store: State<SharedNotes>,
    seed: Option<String>,
) -> Result<Note, String> {
    let now = now_ms();
    let title = clean_title(seed.as_deref().unwrap_or(""));
    let note = Note {
        id: format!("n_{}", new_id()),
        title,
        doc: empty_doc(),
        doc_version: DOC_VERSION,
        tags: Vec::new(),
        due_at: None,
        pinned: false,
        archived: false,
        reminded: false,
        thumb: None,
        attachments: Vec::new(),
        summary: String::new(),
        text: String::new(),
        created_at: now,
        updated_at: now,
    };
    {
        let mut guard = lock(&store)?;
        guard.upsert(note.clone())?;
        crate::cloud::state::mark_dirty(&guard.data_dir);
    }
    wake(&app);
    Ok(note)
}

#[tauri::command]
pub fn note_save(
    app: AppHandle,
    store: State<SharedNotes>,
    input: NoteSave,
) -> Result<NoteIndex, String> {
    let index = {
        let mut guard = lock(&store)?;
        let index = guard.save(input)?;
        crate::cloud::state::mark_dirty(&guard.data_dir);
        index
    };
    wake(&app);
    Ok(index)
}

impl NotesStore {
    fn save(&mut self, input: NoteSave) -> Result<NoteIndex, String> {
        safe_id(&input.id)?;
        let existing = self
            .index
            .notes
            .iter()
            .find(|item| item.id == input.id)
            .cloned()
            .ok_or_else(|| "这篇笔记已经不在了".to_string())?;
        if existing.updated_at != input.base_updated_at {
            return Err("这篇在别处被改过".into());
        }
        let doc_bytes = serde_json::to_vec(&input.doc).map_err(|err| err.to_string())?;
        if doc_bytes.len() > MAX_DOC_BYTES {
            return Err("正文超过 2MB，先拆成几篇".into());
        }
        let (doc, doc_version) = migrate_doc(input.doc, input.doc_version)?;
        let mut text = clip_bytes(input.text.trim(), TEXT_BYTES);
        if text.is_empty() {
            text = clip_bytes(&plain_text(&doc), TEXT_BYTES);
        }
        let summary = {
            let given = clip_chars(input.summary.trim(), SUMMARY_CHARS);
            if given.is_empty() {
                summarize(&text)
            } else {
                given
            }
        };
        let attachments = clean_hashes(&input.attachments);
        if attachments.len() > MAX_ATTACHMENTS {
            return Err("一篇笔记最多 200 张图".into());
        }
        let due_at = input.due_at;
        let reminded = if existing.due_at != due_at {
            false
        } else {
            existing.reminded
        };
        let now = now_ms();
        let note = Note {
            id: input.id,
            title: clean_title(&input.title),
            doc,
            doc_version,
            tags: clean_tags(&input.tags),
            due_at,
            pinned: input.pinned,
            archived: input.archived,
            reminded,
            thumb: clean_hash(input.thumb.as_deref()),
            attachments,
            summary,
            text,
            created_at: existing.created_at,
            updated_at: now,
        };
        self.upsert(note)
    }

    fn patch(&mut self, id: &str, patch: NoteMetaPatch) -> Result<NoteIndex, String> {
        safe_id(id)?;
        let mut note = self.require(id)?;
        if let Some(pinned) = patch.pinned {
            note.pinned = pinned;
        }
        if let Some(archived) = patch.archived {
            note.archived = archived;
        }
        if let Some(tags) = patch.tags {
            note.tags = clean_tags(&tags);
        }
        if let Some(due_at) = patch.due_at {
            if note.due_at != due_at {
                note.reminded = false;
            }
            note.due_at = due_at;
        }
        note.updated_at = now_ms();
        self.upsert(note)
    }
}

#[tauri::command]
pub fn note_set_meta(
    app: AppHandle,
    store: State<SharedNotes>,
    id: String,
    patch: NoteMetaPatch,
) -> Result<NoteIndex, String> {
    let index = {
        let mut guard = lock(&store)?;
        let index = guard.patch(&id, patch)?;
        crate::cloud::state::mark_dirty(&guard.data_dir);
        index
    };
    wake(&app);
    Ok(index)
}

#[tauri::command]
pub fn note_delete(app: AppHandle, store: State<SharedNotes>, id: String) -> Result<(), String> {
    {
        let mut guard = lock(&store)?;
        safe_id(&id)?;
        let path = guard.note_path(&id);
        if path.exists() {
            fs::remove_file(&path).map_err(|err| err.to_string())?;
        }
        guard.index.notes.retain(|item| item.id != id);
        guard.persist_index()?;
        crate::cloud::state::add_tombstone(&guard.data_dir, "notes", &id, now_ms());
    }
    wake(&app);
    Ok(())
}

#[tauri::command]
pub fn note_attachment_prepare(store: State<SharedNotes>) -> Result<String, String> {
    let guard = lock(&store)?;
    let dir = guard.attachments_dir.join(".incoming");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(format!("{}.bin", new_id()));
    fs::write(&path, []).map_err(|err| err.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn note_attachment_commit(
    store: State<SharedNotes>,
    tmp: String,
    ext: String,
) -> Result<Attachment, String> {
    let guard = lock(&store)?;
    let path = PathBuf::from(&tmp);
    guard.ensure_incoming(&path)?;
    let attachment = guard.commit_file(&path, &ext, true)?;
    Ok(attachment)
}

#[tauri::command]
pub fn note_attachment_import(
    store: State<SharedNotes>,
    path: String,
) -> Result<Attachment, String> {
    let guard = lock(&store)?;
    let source = PathBuf::from(&path);
    if !source.is_file() {
        return Err("找不到这张图".into());
    }
    let ext = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("png");
    guard.commit_file(&source, ext, false)
}

#[tauri::command]
pub fn note_attachment_begin(store: State<SharedNotes>) -> Result<String, String> {
    let mut guard = lock(&store)?;
    let dir = guard.attachments_dir.join(".incoming");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let id = new_id();
    let path = dir.join(format!("{id}.bin"));
    fs::write(&path, []).map_err(|err| err.to_string())?;
    guard.uploads.insert(id.clone(), (path, 0));
    Ok(id)
}

#[tauri::command]
pub fn note_attachment_chunk(
    store: State<SharedNotes>,
    id: String,
    chunk: String,
) -> Result<(), String> {
    let mut guard = lock(&store)?;
    let Some((path, size)) = guard.uploads.get(&id).cloned() else {
        return Err("上传已经中断".into());
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(chunk.trim())
        .map_err(|err| format!("图片数据无效: {err}"))?;
    let next = size.saturating_add(bytes.len());
    if next > MAX_IMAGE_BYTES {
        guard.uploads.remove(&id);
        let _ = fs::remove_file(path);
        return Err("图片超过 10MB，压缩后再传".into());
    }
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    file.write_all(&bytes).map_err(|err| err.to_string())?;
    guard.uploads.insert(id, (path, next));
    Ok(())
}

#[tauri::command]
pub fn note_attachment_finish(
    store: State<SharedNotes>,
    id: String,
    ext: String,
) -> Result<Attachment, String> {
    let mut guard = lock(&store)?;
    let Some((path, _)) = guard.uploads.remove(&id) else {
        return Err("上传已经中断".into());
    };
    guard.commit_file(&path, &ext, true)
}

#[tauri::command]
pub fn note_export(
    store: State<SharedNotes>,
    id: String,
    format: String,
    path: Option<String>,
) -> Result<String, String> {
    let note = {
        let guard = lock(&store)?;
        guard.require(&id)?
    };
    let body = match format.as_str() {
        "html" => render_html(&note),
        _ => render_markdown(&note),
    };
    if let Some(path) = path {
        if path.trim().is_empty() {
            return Err("先选一个保存位置".into());
        }
        fs::write(&path, &body).map_err(|err| format!("导出失败: {err}"))?;
    }
    Ok(body)
}

impl NotesStore {
    fn ensure_incoming(&self, tmp: &Path) -> Result<(), String> {
        let incoming = self
            .attachments_dir
            .join(".incoming")
            .canonicalize()
            .map_err(|_| "附件目录不可用".to_string())?;
        let tmp = tmp
            .canonicalize()
            .map_err(|_| "找不到刚写入的图片".to_string())?;
        if !tmp.starts_with(&incoming) {
            return Err("附件路径无效".into());
        }
        Ok(())
    }

    fn commit_file(&self, source: &Path, ext: &str, remove_source: bool) -> Result<Attachment, String> {
        let ext = normalize_ext(ext)?;
        let bytes = fs::read(source).map_err(|err| err.to_string())?;
        if remove_source {
            let _ = fs::remove_file(source);
        }
        if bytes.is_empty() {
            return Err("图片是空的".into());
        }
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("图片超过 10MB，压缩后再传".into());
        }
        let hash = sha256_hex(&bytes);
        let dest = self.attachments_dir.join(format!("{hash}.{ext}"));
        if !dest.exists() {
            atomic_write(&dest, &bytes)?;
        }
        let thumb_name = format!("{hash}@480.webp");
        let thumb_path = self.attachments_dir.join(&thumb_name);
        if !thumb_path.exists() {
            if let Err(err) = write_thumb(&bytes, &thumb_path) {
                logging::warn(format_args!("缩略图跳过: {err}"));
            }
        }
        Ok(Attachment {
            hash,
            ext,
            thumb: thumb_path.is_file().then_some(thumb_name),
        })
    }

    pub fn gc(&mut self) -> Result<usize, String> {
        self.gc_with_grace(GRACE)
    }

    fn gc_with_grace(&mut self, grace: Duration) -> Result<usize, String> {
        let referenced: std::collections::HashSet<String> = self
            .index
            .notes
            .iter()
            .flat_map(|note| note.attachments.iter().cloned())
            .collect();
        let mut removed = 0;
        if !self.attachments_dir.is_dir() {
            return Ok(0);
        }
        for entry in fs::read_dir(&self.attachments_dir).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                if name == ".incoming" {
                    removed += sweep_incoming(&entry.path(), grace)?;
                }
                continue;
            }
            let Some(hash) = file_hash(&name) else {
                continue;
            };
            if referenced.contains(&hash) {
                continue;
            }
            if younger_than(&entry.path(), grace) {
                continue;
            }
            if fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

pub fn spawn_gc(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        loop {
            let removed = {
                let state = app.state::<SharedNotes>();
                state.lock().ok().and_then(|mut guard| guard.gc().ok())
            };
            if let Some(count) = removed {
                if count > 0 {
                    logging::info(format_args!("附件清理完成: removed={count}"));
                }
            }
            std::thread::sleep(Duration::from_secs(24 * 60 * 60));
        }
    });
}

fn sweep_incoming(dir: &Path, grace: Duration) -> Result<usize, String> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        if younger_than(&entry.path(), grace) {
            continue;
        }
        if fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn younger_than(path: &Path, grace: Duration) -> bool {
    let Ok(modified) = path.metadata().and_then(|meta| meta.modified()) else {
        return true;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age < grace)
        .unwrap_or(true)
}

fn file_hash(name: &str) -> Option<String> {
    let stem = name.split(['.', '@']).next()?;
    if stem.len() == 64 && stem.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(stem.to_string())
    } else {
        None
    }
}

fn write_thumb(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let image = image::load_from_memory(bytes).map_err(|err| format!("这张图读不了: {err}"))?;
    let thumb = image.thumbnail(480, 480);
    let mut cursor = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut cursor, image::ImageFormat::WebP)
        .map_err(|err| format!("缩略图生成失败: {err}"))?;
    atomic_write(dest, cursor.get_ref())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn normalize_ext(ext: &str) -> Result<String, String> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => Ok(ext),
        _ => Err("只接收 png、jpg、gif、webp".into()),
    }
}

fn safe_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err("笔记标识无效".into());
    }
    Ok(())
}

fn clean_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "未命名".to_string()
    } else {
        clip_chars(title, 200)
    }
}

fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for tag in tags {
        let tag = tag.trim().trim_start_matches('#').trim();
        if tag.is_empty() || tag.chars().count() > 24 {
            continue;
        }
        if out.iter().any(|item: &String| item == tag) {
            continue;
        }
        out.push(tag.to_string());
        if out.len() == 12 {
            break;
        }
    }
    out
}

fn clean_hashes(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if let Some(hash) = clean_hash(Some(value)) {
            if !out.contains(&hash) {
                out.push(hash);
            }
        }
    }
    out
}

fn clean_hash(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(value.to_string())
    } else {
        None
    }
}

fn clip_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn clip_bytes(value: &str, max: usize) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if out.len() + ch.len_utf8() > max {
            break;
        }
        out.push(ch);
    }
    out
}

fn summarize(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    clip_chars(collapsed.trim(), SUMMARY_CHARS)
}

fn empty_doc() -> Value {
    serde_json::json!({
        "type": "doc",
        "content": [{ "type": "paragraph" }]
    })
}

fn migrate_doc(doc: Value, version: u32) -> Result<(Value, u32), String> {
    if version > DOC_VERSION {
        return Err("这篇笔记的格式太新，先更新到点".into());
    }
    Ok((doc, DOC_VERSION))
}

fn plain_text(node: &Value) -> String {
    let mut out = String::new();
    walk_text(node, &mut out);
    out
}

fn walk_text(node: &Value, out: &mut String) {
    let kind = node.get("type").and_then(|value| value.as_str()).unwrap_or("");
    if kind == "text" {
        if let Some(text) = node.get("text").and_then(|value| value.as_str()) {
            out.push_str(text);
        }
        return;
    }
    if kind == "hardBreak" {
        out.push('\n');
        return;
    }
    if kind == "image" {
        out.push_str("[图片]");
    }
    if kind == "noteRef" {
        out.push_str(&attr_str(node, "title"));
    }
    if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
        for child in content {
            walk_text(child, out);
        }
    }
    if matches!(
        kind,
        "paragraph" | "heading" | "blockquote" | "codeBlock" | "listItem" | "taskItem"
    ) {
        out.push('\n');
    }
}

fn attr_str(node: &Value, key: &str) -> String {
    node.get("attrs")
        .and_then(|attrs| attrs.get(key))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

fn render_markdown(note: &Note) -> String {
    let mut out = format!("# {}\n\n", note.title);
    if !note.tags.is_empty() {
        out.push_str(
            &note
                .tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        out.push_str("\n\n");
    }
    render_md(&note.doc, &mut out);
    out
}

fn render_md(node: &Value, out: &mut String) {
    let kind = node.get("type").and_then(|value| value.as_str()).unwrap_or("");
    match kind {
        "doc" => render_children_md(node, out),
        "paragraph" => {
            render_inline_md(node, out);
            out.push_str("\n\n");
        }
        "heading" => {
            let level = node
                .get("attrs")
                .and_then(|attrs| attrs.get("level"))
                .and_then(|value| value.as_u64())
                .unwrap_or(1)
                .clamp(1, 6);
            for _ in 0..level {
                out.push('#');
            }
            out.push(' ');
            render_inline_md(node, out);
            out.push_str("\n\n");
        }
        "blockquote" => {
            let mut inner = String::new();
            render_children_md(node, &mut inner);
            for line in inner.lines() {
                if line.is_empty() {
                    continue;
                }
                out.push_str("> ");
                out.push_str(line.trim());
                out.push('\n');
            }
            out.push('\n');
        }
        "codeBlock" => {
            out.push_str("```\n");
            render_inline_md(node, out);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        "bulletList" => render_list_md(node, out, false),
        "orderedList" => render_list_md(node, out, true),
        "taskList" => render_tasks_md(node, out),
        "horizontalRule" => out.push_str("---\n\n"),
        "image" => {
            let alt = attr_str(node, "alt");
            let hash = attr_str(node, "hash");
            let ext = attr_str(node, "ext");
            out.push_str(&format!("![{alt}](attachment:{hash}.{ext})\n\n"));
        }
        "noteRef" => {
            out.push_str(&format!("[[{}]]\n\n", attr_str(node, "title")));
        }
        "table" => render_table_md(node, out),
        _ => render_children_md(node, out),
    }
}

fn render_children_md(node: &Value, out: &mut String) {
    if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
        for child in content {
            render_md(child, out);
        }
    }
}

fn render_inline_md(node: &Value, out: &mut String) {
    let Some(content) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for child in content {
        let kind = child.get("type").and_then(|value| value.as_str()).unwrap_or("");
        if kind == "hardBreak" {
            out.push_str("  \n");
            continue;
        }
        if kind != "text" {
            render_md(child, out);
            continue;
        }
        let text = child.get("text").and_then(|value| value.as_str()).unwrap_or("");
        let mut wrapped = text.to_string();
        if let Some(marks) = child.get("marks").and_then(|value| value.as_array()) {
            for mark in marks {
                let mark_type = mark.get("type").and_then(|value| value.as_str()).unwrap_or("");
                wrapped = match mark_type {
                    "bold" => format!("**{wrapped}**"),
                    "italic" => format!("*{wrapped}*"),
                    "strike" => format!("~~{wrapped}~~"),
                    "code" => format!("`{wrapped}`"),
                    "highlight" => format!("=={wrapped}=="),
                    "link" => {
                        let href = mark
                            .get("attrs")
                            .and_then(|attrs| attrs.get("href"))
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        format!("[{wrapped}]({href})")
                    }
                    _ => wrapped,
                };
            }
        }
        out.push_str(&wrapped);
    }
}

fn render_list_md(node: &Value, out: &mut String, ordered: bool) {
    let Some(items) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let mut inner = String::new();
        render_children_md(item, &mut inner);
        let marker = if ordered {
            format!("{}. ", index + 1)
        } else {
            "- ".to_string()
        };
        out.push_str(&marker);
        out.push_str(inner.trim());
        out.push('\n');
    }
    out.push('\n');
}

fn render_tasks_md(node: &Value, out: &mut String) {
    let Some(items) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for item in items {
        let checked = item
            .get("attrs")
            .and_then(|attrs| attrs.get("checked"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let mut inner = String::new();
        render_children_md(item, &mut inner);
        out.push_str(if checked { "- [x] " } else { "- [ ] " });
        out.push_str(inner.trim());
        out.push('\n');
    }
    out.push('\n');
}

fn render_table_md(node: &Value, out: &mut String) {
    let Some(rows) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for (index, row) in rows.iter().enumerate() {
        let mut cells = Vec::new();
        if let Some(content) = row.get("content").and_then(|value| value.as_array()) {
            for cell in content {
                let mut text = String::new();
                render_inline_md(cell, &mut text);
                cells.push(text.replace('|', "\\|"));
            }
        }
        out.push('|');
        out.push_str(&cells.join("|"));
        out.push_str("|\n");
        if index == 0 {
            out.push('|');
            out.push_str(&vec!["---"; cells.len()].join("|"));
            out.push_str("|\n");
        }
    }
    out.push('\n');
}

fn render_html(note: &Note) -> String {
    let mut body = String::new();
    render_html_node(&note.doc, &mut body);
    format!(
        "<article><h1>{}</h1>{}</article>",
        escape(&note.title),
        body
    )
}

fn render_html_node(node: &Value, out: &mut String) {
    let kind = node.get("type").and_then(|value| value.as_str()).unwrap_or("");
    match kind {
        "doc" => {
            if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
                for child in content {
                    render_html_node(child, out);
                }
            }
        }
        "paragraph" => wrap(out, "p", node),
        "heading" => {
            let level = node
                .get("attrs")
                .and_then(|attrs| attrs.get("level"))
                .and_then(|value| value.as_u64())
                .unwrap_or(2)
                .clamp(1, 6);
            wrap(out, &format!("h{level}"), node);
        }
        "blockquote" => wrap_block(out, "blockquote", node),
        "codeBlock" => {
            out.push_str("<pre><code>");
            out.push_str(&escape(&plain_text(node)));
            out.push_str("</code></pre>");
        }
        "bulletList" => wrap_block(out, "ul", node),
        "orderedList" => wrap_block(out, "ol", node),
        "listItem" | "taskItem" => wrap_block(out, "li", node),
        "taskList" => wrap_block(out, "ul", node),
        "horizontalRule" => out.push_str("<hr>"),
        "image" => {
            out.push_str(&format!(
                "<img alt=\"{}\" data-hash=\"{}\">",
                escape(&attr_str(node, "alt")),
                escape(&attr_str(node, "hash"))
            ));
        }
        "noteRef" => {
            out.push_str(&format!("<p>{}</p>", escape(&attr_str(node, "title"))));
        }
        "table" => wrap_block(out, "table", node),
        "tableRow" => wrap_block(out, "tr", node),
        "tableHeader" => wrap(out, "th", node),
        "tableCell" => wrap(out, "td", node),
        _ => {
            if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
                for child in content {
                    render_html_node(child, out);
                }
            }
        }
    }
}

fn wrap(out: &mut String, tag: &str, node: &Value) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    render_inline_html(node, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn wrap_block(out: &mut String, tag: &str, node: &Value) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
        for child in content {
            render_html_node(child, out);
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn render_inline_html(node: &Value, out: &mut String) {
    let Some(content) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for child in content {
        let kind = child.get("type").and_then(|value| value.as_str()).unwrap_or("");
        if kind == "hardBreak" {
            out.push_str("<br>");
            continue;
        }
        if kind != "text" {
            render_html_node(child, out);
            continue;
        }
        let mut text = escape(child.get("text").and_then(|value| value.as_str()).unwrap_or(""));
        if let Some(marks) = child.get("marks").and_then(|value| value.as_array()) {
            for mark in marks.iter().rev() {
                let mark_type = mark.get("type").and_then(|value| value.as_str()).unwrap_or("");
                text = match mark_type {
                    "bold" => format!("<strong>{text}</strong>"),
                    "italic" => format!("<em>{text}</em>"),
                    "underline" => format!("<u>{text}</u>"),
                    "strike" => format!("<s>{text}</s>"),
                    "code" => format!("<code>{text}</code>"),
                    "highlight" => format!("<mark>{text}</mark>"),
                    "link" => {
                        let href = escape(
                            mark.get("attrs")
                                .and_then(|attrs| attrs.get("href"))
                                .and_then(|value| value.as_str())
                                .unwrap_or(""),
                        );
                        format!("<a href=\"{href}\">{text}</a>")
                    }
                    _ => text,
                };
            }
        }
        out.push_str(&text);
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("note");
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    fs::write(&tmp, bytes).map_err(|err| err.to_string())?;
    replace_file(&tmp, path)
}

fn replace_file(tmp: &Path, dest: &Path) -> Result<(), String> {
    if fs::rename(tmp, dest).is_ok() {
        return Ok(());
    }
    let backup = dest.with_extension("replacing");
    let _ = fs::remove_file(&backup);
    if dest.exists() {
        fs::rename(dest, &backup).map_err(|err| err.to_string())?;
    }
    if let Err(err) = fs::rename(tmp, dest) {
        let _ = fs::rename(&backup, dest);
        return Err(err.to_string());
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

pub fn copy_backup(data_dir: &Path, snapshot: &Path, previous: Option<&Path>) -> Result<(), String> {
    let notes_src = data_dir.join("notes");
    if notes_src.is_dir() {
        let dest = snapshot.join("notes");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        let previous_notes = previous.map(|dir| dir.join("notes"));
        for entry in fs::read_dir(&notes_src).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name();
            let label = name.to_string_lossy();
            if label.starts_with('.') || !label.ends_with(".json") {
                continue;
            }
            let src = entry.path();
            if !src.is_file() {
                continue;
            }
            let previous_file = previous_notes.as_ref().map(|dir| dir.join(&name));
            place_unchanged(&src, previous_file.as_deref(), &dest.join(&name))?;
        }
    }
    let attachments = data_dir.join("attachments");
    if attachments.is_dir() {
        let dest = snapshot.join("attachments");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        for entry in fs::read_dir(&attachments).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || entry.path().is_dir() {
                continue;
            }
            link_or_copy(&entry.path(), &dest.join(&name))?;
        }
    }
    Ok(())
}

fn place_unchanged(live: &Path, previous: Option<&Path>, dest: &Path) -> Result<(), String> {
    if let Some(previous) = previous {
        if previous.is_file() && !newer(live, previous) {
            return link_or_copy(previous, dest);
        }
    }
    fs::copy(live, dest)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn newer(live: &Path, previous: &Path) -> bool {
    let Ok(live_meta) = live.metadata() else {
        return true;
    };
    let Ok(prev_meta) = previous.metadata() else {
        return true;
    };
    if live_meta.len() != prev_meta.len() {
        return true;
    }
    match (live_meta.modified(), prev_meta.modified()) {
        (Ok(live_time), Ok(prev_time)) => live_time > prev_time,
        _ => true,
    }
}

fn link_or_copy(source: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        return Ok(());
    }
    if fs::hard_link(source, dest).is_ok() {
        return Ok(());
    }
    fs::copy(source, dest)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub fn restore_backup(data_dir: &Path, snapshot: &Path) -> Result<(), String> {
    let src_notes = snapshot.join("notes");
    if src_notes.is_dir() {
        let dest = data_dir.join("notes");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        for entry in fs::read_dir(&dest).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name();
            let label = name.to_string_lossy();
            if label.ends_with(".json") && !src_notes.join(&name).is_file() {
                let _ = fs::remove_file(entry.path());
            }
        }
        for entry in fs::read_dir(&src_notes).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            if !entry.path().is_file() {
                continue;
            }
            fs::copy(entry.path(), dest.join(entry.file_name())).map_err(|err| err.to_string())?;
        }
    }
    let src_attachments = snapshot.join("attachments");
    if src_attachments.is_dir() {
        let dest = data_dir.join("attachments");
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        for entry in fs::read_dir(&src_attachments).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            if entry.path().is_dir() {
                continue;
            }
            link_or_copy(&entry.path(), &dest.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> NotesStore {
        let dir = std::env::temp_dir().join(format!("daodian-notes-{}", new_id()));
        NotesStore::open(&dir).unwrap()
    }

    fn sample_doc(text: &str) -> Value {
        serde_json::json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": text }]
            }]
        })
    }

    #[test]
    fn save_roundtrip_and_conflict() {
        let mut store = temp_store();
        let created = {
            let now = now_ms();
            let note = Note {
                id: "n_test".into(),
                title: "提测清单".into(),
                doc: sample_doc("登录和支付"),
                doc_version: 1,
                tags: vec!["版本".into()],
                due_at: Some(50),
                pinned: true,
                archived: false,
                reminded: false,
                thumb: None,
                attachments: Vec::new(),
                summary: String::new(),
                text: String::new(),
                created_at: now,
                updated_at: now,
            };
            store.upsert(note).unwrap()
        };
        let saved = store
            .save(NoteSave {
                id: "n_test".into(),
                title: "提测清单".into(),
                doc: sample_doc("登录和支付"),
                doc_version: 1,
                tags: vec!["#版本".into(), "版本".into()],
                due_at: Some(50),
                pinned: true,
                archived: false,
                thumb: None,
                attachments: Vec::new(),
                text: String::new(),
                summary: String::new(),
                base_updated_at: created.updated_at,
            })
            .unwrap();
        assert_eq!(saved.tags, vec!["版本".to_string()]);
        assert_eq!(saved.summary, "登录和支付");
        let err = store
            .save(NoteSave {
                id: "n_test".into(),
                title: "过期".into(),
                doc: empty_doc(),
                doc_version: 1,
                tags: Vec::new(),
                due_at: None,
                pinned: false,
                archived: false,
                thumb: None,
                attachments: Vec::new(),
                text: String::new(),
                summary: String::new(),
                base_updated_at: created.updated_at,
            })
            .unwrap_err();
        assert_eq!(err, "这篇在别处被改过");
        let _ = fs::remove_dir_all(store.data_dir());
    }

    #[test]
    fn corrupt_index_rebuilds_from_notes() {
        let mut store = temp_store();
        let now = now_ms();
        store
            .upsert(Note {
                id: "n_keep".into(),
                title: "还在".into(),
                doc: sample_doc("正文还在"),
                doc_version: 1,
                tags: Vec::new(),
                due_at: None,
                pinned: false,
                archived: false,
                reminded: false,
                thumb: None,
                attachments: Vec::new(),
                summary: "正文还在".into(),
                text: "正文还在".into(),
                created_at: now,
                updated_at: now,
            })
            .unwrap();
        fs::write(&store.index_path, "{").unwrap();
        store.load_index().unwrap();
        assert_eq!(store.index.notes.len(), 1);
        assert_eq!(store.index.notes[0].title, "还在");
        let _ = fs::remove_dir_all(store.data_dir());
    }

    #[test]
    fn reminder_fires_once() {
        let notes = vec![NoteIndex {
            id: "n_due".into(),
            title: "提测清单".into(),
            summary: "回归登录".into(),
            text: String::new(),
            tags: Vec::new(),
            due_at: Some(10),
            pinned: false,
            archived: false,
            reminded: false,
            thumb: None,
            attachments: Vec::new(),
            created_at: 1,
            updated_at: 1,
        }];
        let actions = actions_for(&notes, 20);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "备忘录 · 提测清单");
        let mut quiet = notes.clone();
        quiet[0].reminded = true;
        assert!(actions_for(&quiet, 20).is_empty());
        let mut later = notes;
        later[0].due_at = Some(30);
        assert!(actions_for(&later, 20).is_empty());
    }

    #[test]
    fn export_markdown_includes_body() {
        let note = Note {
            id: "n_export".into(),
            title: "周报".into(),
            doc: sample_doc("三段式"),
            doc_version: 1,
            tags: vec!["会议".into()],
            due_at: None,
            pinned: false,
            archived: false,
            reminded: false,
            thumb: None,
            attachments: Vec::new(),
            summary: String::new(),
            text: String::new(),
            created_at: 1,
            updated_at: 1,
        };
        let markdown = render_markdown(&note);
        assert!(markdown.contains("# 周报"));
        assert!(markdown.contains("#会议"));
        assert!(markdown.contains("三段式"));
    }

    #[test]
    fn gc_drops_old_orphans_only() {
        let mut store = temp_store();
        let hash = "a".repeat(64);
        let orphan = store.attachments_dir.join(format!("{hash}.png"));
        fs::write(&orphan, b"png").unwrap();
        {
            let file = File::options().write(true).open(&orphan).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(10 * 24 * 3600))
                .unwrap();
        }
        let fresh = store.attachments_dir.join(format!("{}.png", "b".repeat(64)));
        fs::write(&fresh, b"png").unwrap();
        let kept = "c".repeat(64);
        fs::write(store.attachments_dir.join(format!("{kept}.png")), b"png").unwrap();
        store.index.notes.push(NoteIndex {
            id: "n_img".into(),
            title: "有图".into(),
            summary: String::new(),
            text: String::new(),
            tags: Vec::new(),
            due_at: None,
            pinned: false,
            archived: false,
            reminded: false,
            thumb: Some(kept.clone()),
            attachments: vec![kept],
            created_at: 1,
            updated_at: 1,
        });
        let removed = store.gc_with_grace(Duration::from_secs(1)).unwrap();
        assert!(removed >= 1);
        assert!(!orphan.exists());
        assert!(fresh.exists());
        assert!(store.attachments_dir.join(format!("{}.png", "c".repeat(64))).exists());
        let _ = fs::remove_dir_all(store.data_dir());
    }
}
