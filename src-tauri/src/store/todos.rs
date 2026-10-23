use tauri::{AppHandle, State};

use crate::domain::{
    Document, Repeat, Todo, TodoInput, clean_note, local_offset_secs, new_id, next_due,
    normalize_priority, now_ms, require_title,
};
use crate::remind;

use super::{SharedStore, lock};

#[tauri::command]
pub fn save_todo(
    app: AppHandle,
    store: State<SharedStore>,
    input: TodoInput,
) -> Result<Todo, String> {
    let saved = {
        let mut guard = lock(&store)?;
        let title = require_title(&input.title, 80)?;
        let note = clean_note(&input.note)?;
        if let Some(at) = input.due_at {
            if at < 0 {
                return Err("时间不对".into());
            }
        }
        let priority = normalize_priority(&input.priority);
        let repeat = clean_repeat(input.repeat)?;
        if repeat.is_some() && input.due_at.is_none() {
            return Err("重复任务需要时间点".into());
        }
        let lead_ms = clean_lead(input.lead_ms)?;
        let now = now_ms();

        if let Some(id) = input.id.filter(|id| !id.is_empty()) {
            let todo = guard
                .doc
                .todos
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or("这条待办已经不在了")?;
            let same_time = todo.due_at == input.due_at;
            todo.title = title;
            todo.note = note;
            todo.due_at = input.due_at;
            todo.priority = priority;
            todo.repeat = repeat;
            todo.series_id = todo
                .repeat
                .as_ref()
                .map(|_| todo.series_id.clone().unwrap_or_else(|| todo.id.clone()));
            if !same_time || todo.lead_ms != lead_ms {
                todo.reminded = false;
                todo.lead_reminded = false;
            }
            todo.lead_ms = lead_ms;
            todo.updated_at = now;
            let saved = todo.clone();
            guard.persist()?;
            saved
        } else {
            let id = new_id();
            let series_id = repeat.as_ref().map(|_| id.clone());
            let todo = Todo {
                id: id.clone(),
                title,
                note,
                due_at: input.due_at,
                priority,
                done: false,
                created_at: now,
                updated_at: now,
                reminded: false,
                repeat,
                lead_ms,
                lead_reminded: false,
                series_id,
                skipped: false,
            };
            guard.doc.todos.push(todo.clone());
            guard.persist()?;
            todo
        }
    };

    remind::check(&app);
    Ok(saved)
}

#[tauri::command]
pub fn set_todo_done(
    app: AppHandle,
    store: State<SharedStore>,
    id: String,
    done: bool,
) -> Result<Todo, String> {
    let saved = {
        let mut guard = lock(&store)?;
        let now = now_ms();
        let saved = if done {
            finish_occurrence(&mut guard.doc, &id, now, local_offset_secs(), false)?
        } else {
            reopen_occurrence(&mut guard.doc, &id, now)?
        };
        guard.persist()?;
        saved
    };
    remind::check(&app);
    Ok(saved)
}

#[tauri::command]
pub fn skip_todo_occurrence(
    app: AppHandle,
    store: State<SharedStore>,
    id: String,
) -> Result<Todo, String> {
    let saved = {
        let mut guard = lock(&store)?;
        let saved = finish_occurrence(&mut guard.doc, &id, now_ms(), local_offset_secs(), true)?;
        guard.persist()?;
        saved
    };
    remind::check(&app);
    Ok(saved)
}

#[tauri::command]
pub fn delete_todo(store: State<SharedStore>, id: String) -> Result<(), String> {
    let mut guard = lock(&store)?;
    let before = guard.doc.todos.len();
    guard.doc.todos.retain(|item| item.id != id);
    if guard.doc.todos.len() == before {
        return Err("这条待办已经不在了".into());
    }
    guard.persist()?;
    Ok(())
}

fn clean_repeat(repeat: Option<Repeat>) -> Result<Option<Repeat>, String> {
    let Some(repeat) = repeat else {
        return Ok(None);
    };
    if !matches!(
        repeat.rule.as_str(),
        "daily" | "weekly" | "weekdays" | "monthEnd" | "custom"
    ) {
        return Err("重复规则不对".into());
    }
    if let Some(day) = repeat.weekday {
        if day > 6 {
            return Err("重复规则不对".into());
        }
    }
    if repeat.rule == "custom" && !matches!(repeat.interval_days, Some(1..=3650)) {
        return Err("自定义间隔应为 1 到 3650 天".into());
    }
    if repeat.until.is_some_and(|until| until < 0) {
        return Err("重复结束时间不对".into());
    }
    Ok(Some(repeat))
}

fn clean_lead(lead_ms: i64) -> Result<i64, String> {
    if !(0..=7 * 86_400_000).contains(&lead_ms) {
        return Err("提前量不对".into());
    }
    Ok(lead_ms)
}

fn finish_occurrence(
    doc: &mut Document,
    id: &str,
    now: i64,
    offset_secs: i64,
    skipped: bool,
) -> Result<Todo, String> {
    let index = doc
        .todos
        .iter()
        .position(|item| item.id == id)
        .ok_or("这条待办已经不在了")?;
    if doc.todos[index].done {
        return Ok(doc.todos[index].clone());
    }
    if skipped && doc.todos[index].repeat.is_none() {
        return Err("只有重复任务可以跳过本次".into());
    }

    let source = doc.todos[index].clone();
    let series_id = source
        .series_id
        .clone()
        .unwrap_or_else(|| source.id.clone());
    doc.todos[index].series_id = source.repeat.as_ref().map(|_| series_id.clone());
    doc.todos[index].done = true;
    doc.todos[index].skipped = skipped;
    doc.todos[index].updated_at = now;

    let has_active = source.repeat.is_some()
        && doc.todos.iter().enumerate().any(|(other_index, item)| {
            other_index != index
                && !item.done
                && item.series_id.as_deref() == Some(series_id.as_str())
        });
    if !has_active {
        if let (Some(repeat), Some(due)) = (&source.repeat, source.due_at) {
            if let Some(next) = next_due(repeat, due, offset_secs) {
                doc.todos.push(Todo {
                    id: new_id(),
                    title: source.title,
                    note: source.note,
                    due_at: Some(next),
                    priority: source.priority,
                    done: false,
                    created_at: now,
                    updated_at: now,
                    reminded: false,
                    repeat: Some(repeat.clone()),
                    lead_ms: source.lead_ms,
                    lead_reminded: false,
                    series_id: Some(series_id),
                    skipped: false,
                });
            }
        }
    }
    Ok(doc.todos[index].clone())
}

fn reopen_occurrence(doc: &mut Document, id: &str, now: i64) -> Result<Todo, String> {
    let source = doc
        .todos
        .iter()
        .find(|item| item.id == id)
        .cloned()
        .ok_or("这条待办已经不在了")?;
    let series_id = source.series_id.clone();
    if let Some(series_id) = &series_id {
        doc.todos.retain(|item| {
            item.id == id || item.done || item.series_id.as_deref() != Some(series_id.as_str())
        });
    }
    let todo = doc
        .todos
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or("这条待办已经不在了")?;
    todo.done = false;
    todo.skipped = false;
    todo.updated_at = now;
    Ok(todo.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeating_todo() -> Todo {
        Todo {
            id: "first".into(),
            title: "站会".into(),
            note: String::new(),
            due_at: Some(1_000_000),
            priority: "normal".into(),
            done: false,
            created_at: 1,
            updated_at: 1,
            reminded: false,
            repeat: Some(Repeat {
                rule: "daily".into(),
                weekday: None,
                interval_days: None,
                until: None,
                skip_weekends: false,
            }),
            lead_ms: 0,
            lead_reminded: false,
            series_id: None,
            skipped: false,
        }
    }

    #[test]
    fn completing_twice_only_creates_one_active_instance() {
        let mut doc = Document {
            todos: vec![repeating_todo()],
            ..Document::default()
        };
        finish_occurrence(&mut doc, "first", 2, 0, false).unwrap();
        finish_occurrence(&mut doc, "first", 3, 0, false).unwrap();
        assert_eq!(doc.todos.iter().filter(|item| !item.done).count(), 1);
        assert_eq!(doc.todos.len(), 2);
    }

    #[test]
    fn reopening_removes_the_generated_active_instance() {
        let mut doc = Document {
            todos: vec![repeating_todo()],
            ..Document::default()
        };
        finish_occurrence(&mut doc, "first", 2, 0, false).unwrap();
        reopen_occurrence(&mut doc, "first", 3).unwrap();
        assert_eq!(doc.todos.iter().filter(|item| !item.done).count(), 1);
        assert_eq!(doc.todos.len(), 1);
        assert!(!doc.todos[0].done);
    }

    #[test]
    fn skipping_marks_history_and_rolls_forward() {
        let mut doc = Document {
            todos: vec![repeating_todo()],
            ..Document::default()
        };
        let saved = finish_occurrence(&mut doc, "first", 2, 0, true).unwrap();
        assert!(saved.done);
        assert!(saved.skipped);
        assert_eq!(doc.todos.iter().filter(|item| !item.done).count(), 1);
    }
}
