pub mod plans;
pub mod templates;
pub mod todos;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, State};

use crate::domain::{
    Document, RemindAction, RemindTarget, SCHEMA_VERSION, migrate_legacy_json, next_due_at,
    plan_actions, stage_has_valid_schedule,
};
use crate::logging;

pub type SharedStore = Mutex<Library>;

pub struct Library {
    path: PathBuf,
    pub(crate) doc: Document,
    pub(crate) templates_path: PathBuf,
    pub(crate) custom_templates: Vec<templates::PlanTemplate>,
    pub(crate) quick_template_ids: Vec<String>,
}

impl Library {
    pub fn open(dir: &Path) -> Result<Self, std::io::Error> {
        logging::info(format_args!("opening library: {}", dir.display()));
        // 目录已存在就不要再 create_dir_all：某些安全策略会直接拒绝「创建目录」这个动作，
        // 哪怕目录本来就在，会让应用起不来。
        if !dir.exists() {
            fs::create_dir_all(dir)?;
        }
        let path = dir.join("store.json");
        let templates_path = Self::templates_path(dir);
        let (custom_templates, quick_template_ids) = templates::load(&templates_path);
        if path.exists() {
            let raw = fs::read_to_string(&path)?;
            let mut doc: Document = serde_json::from_str(&raw).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("数据读不出来: {err}"),
                )
            })?;
            if doc.version < SCHEMA_VERSION {
                let backup = dir.join("store.v1.bak.json");
                // 备份是尽力而为：有些安全策略不允许新建文件，不能因此起不来。
                if !backup.exists() {
                    if let Err(err) = fs::copy(&path, &backup) {
                        logging::warn(format_args!(
                            "v1 backup failed; continuing migration: {err}"
                        ));
                    }
                }
                doc.version = SCHEMA_VERSION;
            }
            let library = Self {
                path,
                doc,
                templates_path,
                custom_templates,
                quick_template_ids,
            };
            library
                .persist()
                .map_err(|err| std::io::Error::other(err))?;
            logging::info(format_args!(
                "library loaded; todos={}; plans={}",
                library.doc.todos.len(),
                library.doc.plans.len()
            ));
            return Ok(library);
        }

        let legacy = dir.join("todos.json");
        let mut doc = if legacy.exists() {
            let raw = fs::read_to_string(&legacy)?;
            migrate_legacy_json(&raw)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?
        } else {
            Document::default()
        };
        doc.version = SCHEMA_VERSION;
        let library = Self {
            path,
            doc,
            templates_path,
            custom_templates,
            quick_template_ids,
        };
        library
            .persist()
            .map_err(|err| std::io::Error::other(err))?;
        logging::info(format_args!(
            "new library created: {}",
            library.path.display()
        ));
        Ok(library)
    }

    pub(crate) fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
        }
        let raw = serde_json::to_string_pretty(&self.doc).map_err(|err| err.to_string())?;
        fs::write(&self.path, raw).map_err(|err| err.to_string())
    }

    pub(crate) fn data_dir(&self) -> Result<&Path, String> {
        self.path
            .parent()
            .ok_or_else(|| "数据目录不存在".to_string())
    }

    pub(crate) fn reload_from_disk(&mut self) -> Result<(), String> {
        let dir = self.data_dir()?.to_path_buf();
        *self = Self::open(&dir).map_err(|err| err.to_string())?;
        Ok(())
    }
}

pub(crate) fn lock(store: &SharedStore) -> Result<MutexGuard<'_, Library>, String> {
    store.lock().map_err(|err| err.to_string())
}

pub(crate) fn notify_changed(app: &AppHandle) {
    let _ = app.emit("data-changed", ());
}

pub fn plan_from_store(store: &SharedStore, now: i64) -> Result<Vec<RemindAction>, String> {
    let guard = lock(store)?;
    Ok(plan_actions(&guard.doc, now))
}

/// 下一个还没到的时间点，用来决定多久醒一次。
pub fn next_due(store: &SharedStore, now: i64) -> Option<i64> {
    lock(store)
        .ok()
        .and_then(|guard| next_due_at(&guard.doc, now))
}

/// 到点了但还没做完的条数：托盘角标就看这个。
pub fn due_open_count(store: &SharedStore, now: i64) -> Result<usize, String> {
    let guard = lock(store)?;
    let mut count = 0;
    for todo in &guard.doc.todos {
        if !todo.done && matches!(todo.due_at, Some(at) if at <= now) {
            count += 1;
        }
    }
    for plan in &guard.doc.plans {
        if plan.done {
            continue;
        }
        for stage in &plan.stages {
            if !stage.done
                && stage_has_valid_schedule(plan, stage)
                && matches!(stage.at, Some(at) if at <= now)
            {
                count += 1;
            }
        }
    }
    Ok(count)
}

pub fn mark_reminded(
    store: &SharedStore,
    sent: &[(RemindTarget, i64, bool)],
    now: i64,
) -> Result<(), String> {
    if sent.is_empty() {
        return Ok(());
    }
    let mut guard = lock(store)?;
    let mut changed = false;
    for (target, at, lead) in sent {
        match target {
            RemindTarget::Todo { id } => {
                if let Some(todo) = guard.doc.todos.iter_mut().find(|item| &item.id == id) {
                    if todo.due_at != Some(*at) {
                        continue;
                    }
                    if *lead {
                        if !todo.lead_reminded {
                            todo.lead_reminded = true;
                            todo.updated_at = now;
                            changed = true;
                        }
                    } else if !todo.reminded {
                        todo.reminded = true;
                        todo.updated_at = now;
                        changed = true;
                    }
                }
            }
            RemindTarget::Stage { plan_id, stage_id } => {
                if let Some(plan) = guard.doc.plans.iter_mut().find(|item| &item.id == plan_id) {
                    if let Some(stage) = plan.stages.iter_mut().find(|item| &item.id == stage_id) {
                        if stage.at == Some(*at) && !stage.reminded {
                            stage.reminded = true;
                            plan.updated_at = now;
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    if changed {
        guard.persist()?;
    }
    Ok(())
}

#[tauri::command]
pub fn load_library(store: State<SharedStore>) -> Result<Document, String> {
    let guard = lock(&store)?;
    Ok(guard.doc.clone())
}
