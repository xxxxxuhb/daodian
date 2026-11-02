use serde::Serialize;
use serde_json::{Value, json};

use crate::cloud::state::{Tomb, Tombstones, prune_tombs};
use crate::domain::{Document, SCHEMA_VERSION};
use crate::notes::Note;
use crate::store::templates::PlanTemplate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: String,
    pub id: String,
    pub local: Value,
    pub remote: Value,
}

pub struct MergedLibrary {
    pub document: Document,
    pub templates: Vec<PlanTemplate>,
    pub quick_ids: Vec<String>,
    pub tombs: Tombstones,
    pub conflicts: Vec<Conflict>,
}

pub fn merge_library(
    local: Document,
    remote: Document,
    local_templates: Vec<PlanTemplate>,
    remote_templates: Vec<PlanTemplate>,
    local_quick: Vec<String>,
    remote_quick: Vec<String>,
    mut tombs: Tombstones,
    last_pull_at: i64,
    synced_before: bool,
    now: i64,
) -> MergedLibrary {
    prune_tombs(&mut tombs, now);
    let mut conflicts = Vec::new();
    let todos = merge_items(
        local.todos,
        remote.todos,
        &mut tombs.todos,
        last_pull_at,
        synced_before,
        now,
        "todo",
        &mut conflicts,
        |item| item.id.clone(),
        |item| item.updated_at,
    );
    let plans = merge_items(
        local.plans,
        remote.plans,
        &mut tombs.plans,
        last_pull_at,
        synced_before,
        now,
        "plan",
        &mut conflicts,
        |item| item.id.clone(),
        |item| item.updated_at,
    );
    let templates = merge_items(
        local_templates
            .into_iter()
            .filter(|item| !is_builtin(&item.id))
            .collect(),
        remote_templates
            .into_iter()
            .filter(|item| !is_builtin(&item.id))
            .collect(),
        &mut tombs.templates,
        last_pull_at,
        synced_before,
        now,
        "template",
        &mut conflicts,
        |item| item.id.clone(),
        |item| item.updated_at,
    );
    let mut quick_ids = Vec::new();
    for id in local_quick.into_iter().chain(remote_quick) {
        if templates.iter().any(|item| item.id == id) && !quick_ids.contains(&id) {
            quick_ids.push(id);
        }
    }
    MergedLibrary {
        document: Document {
            version: local.version.max(remote.version).max(SCHEMA_VERSION),
            todos,
            plans,
        },
        templates,
        quick_ids,
        tombs,
        conflicts,
    }
}

fn merge_items<T, Id, At>(
    local: Vec<T>,
    remote: Vec<T>,
    tombs: &mut Vec<Tomb>,
    last_pull_at: i64,
    synced_before: bool,
    now: i64,
    kind: &str,
    conflicts: &mut Vec<Conflict>,
    id_of: Id,
    updated_of: At,
) -> Vec<T>
where
    T: Clone + PartialEq + Serialize,
    Id: Fn(&T) -> String,
    At: Fn(&T) -> i64,
{
    let mut order = Vec::new();
    let mut local_map = index(local, &id_of, &mut order);
    let mut remote_map = index(remote, &id_of, &mut order);
    let mut out = Vec::new();
    for id in order {
        let left = local_map.remove(&id);
        let right = remote_map.remove(&id);
        match (left, right) {
            (Some(local), Some(remote)) => {
                tombs.retain(|item| item.id != id);
                let local_at = updated_of(&local);
                let remote_at = updated_of(&remote);
                if local_at > remote_at {
                    out.push(local);
                } else if remote_at > local_at {
                    out.push(remote);
                } else if local != remote {
                    conflicts.push(Conflict {
                        kind: kind.into(),
                        id: id.clone(),
                        local: serde_json::to_value(&local).unwrap_or(Value::Null),
                        remote: serde_json::to_value(&remote).unwrap_or(Value::Null),
                    });
                    out.push(local);
                } else {
                    out.push(local);
                }
            }
            (Some(local), None) => {
                if tombs.iter().any(|item| item.id == id) {
                    continue;
                }
                if !synced_before || updated_of(&local) > last_pull_at {
                    out.push(local);
                } else {
                    tombs.push(Tomb {
                        id,
                        deleted_at: now,
                    });
                }
            }
            (None, Some(remote)) => {
                if let Some(tomb) = tombs.iter().find(|item| item.id == id) {
                    if tomb.deleted_at >= updated_of(&remote) {
                        continue;
                    }
                }
                tombs.retain(|item| item.id != id);
                out.push(remote);
            }
            (None, None) => {}
        }
    }
    out
}

fn index<T, Id>(items: Vec<T>, id_of: &Id, order: &mut Vec<String>) -> std::collections::HashMap<String, T>
where
    Id: Fn(&T) -> String,
{
    let mut map = std::collections::HashMap::new();
    for item in items {
        let id = id_of(&item);
        if id.is_empty() {
            continue;
        }
        if !order.contains(&id) {
            order.push(id.clone());
        }
        map.insert(id, item);
    }
    map
}

fn is_builtin(id: &str) -> bool {
    matches!(id, "release" | "meeting" | "blank")
}

pub fn merge_notes(
    local: Vec<Note>,
    remote: Vec<Note>,
    mut tombs: Tombstones,
    last_pull_at: i64,
    synced_before: bool,
    now: i64,
) -> (Vec<Note>, Tombstones, Vec<Conflict>) {
    prune_tombs(&mut tombs, now);
    let mut conflicts = Vec::new();
    let notes = merge_items(
        local,
        remote,
        &mut tombs.notes,
        last_pull_at,
        synced_before,
        now,
        "note",
        &mut conflicts,
        |item| item.id.clone(),
        |item| item.updated_at,
    );
    (notes, tombs, conflicts)
}

pub fn conflict_document(conflicts: &[Conflict]) -> Value {
    let pick = |kind: &str| {
        conflicts
            .iter()
            .filter(|item| item.kind == kind)
            .map(|item| json!({ "id": item.id, "local": item.local, "remote": item.remote }))
            .collect::<Vec<_>>()
    };
    json!({
        "todos": pick("todo"),
        "plans": pick("plan"),
        "templates": pick("template"),
        "notes": pick("note"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Todo;

    fn todo(id: &str, title: &str, updated_at: i64) -> Todo {
        Todo {
            id: id.into(),
            title: title.into(),
            note: String::new(),
            due_at: None,
            priority: "normal".into(),
            done: false,
            created_at: 1,
            updated_at,
            reminded: false,
            repeat: None,
            lead_ms: 0,
            lead_reminded: false,
            series_id: None,
            skipped: false,
        }
    }

    fn doc(todos: Vec<Todo>) -> Document {
        Document {
            version: SCHEMA_VERSION,
            todos,
            plans: Vec::new(),
        }
    }

    fn merged(local: Vec<Todo>, remote: Vec<Todo>, tombs: Tombstones, synced_before: bool) -> MergedLibrary {
        merge_library(
            doc(local),
            doc(remote),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            tombs,
            100,
            synced_before,
            1_000,
        )
    }

    #[test]
    fn newer_copy_wins_and_tombstone_blocks_revival() {
        let result = merged(
            vec![todo("a", "本地新", 200)],
            vec![todo("a", "远端旧", 150), todo("b", "远端还在", 120)],
            Tombstones {
                todos: vec![Tomb {
                    id: "b".into(),
                    deleted_at: 180,
                }],
                plans: Vec::new(),
                templates: Vec::new(),
                notes: Vec::new(),
            },
            true,
        );
        assert_eq!(result.document.todos.len(), 1);
        assert_eq!(result.document.todos[0].title, "本地新");
    }

    #[test]
    fn remote_delete_drops_stale_local_and_keeps_fresh_edits() {
        let result = merged(
            vec![todo("old", "没再改", 50), todo("fresh", "刚改过", 300)],
            Vec::new(),
            Tombstones::default(),
            true,
        );
        assert_eq!(result.document.todos.len(), 1);
        assert_eq!(result.document.todos[0].id, "fresh");
        assert!(result.tombs.todos.iter().any(|item| item.id == "old"));
    }

    #[test]
    fn first_sync_keeps_local_only_items() {
        let result = merged(vec![todo("a", "只有本地", 1)], Vec::new(), Tombstones::default(), false);
        assert_eq!(result.document.todos.len(), 1);
    }

    #[test]
    fn same_time_different_content_keeps_local_and_records_conflict() {
        let result = merged(
            vec![todo("a", "左边", 100)],
            vec![todo("a", "右边", 100)],
            Tombstones::default(),
            true,
        );
        assert_eq!(result.document.todos[0].title, "左边");
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].id, "a");
    }
}
