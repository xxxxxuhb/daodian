use tauri::{AppHandle, State};

use crate::domain::{
    clean_note, local_offset_secs, new_id, next_due, normalize_priority, now_ms, require_title, Repeat,
    Todo, TodoInput,
};
use crate::remind;

use super::{lock, SharedStore};

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
            let todo = Todo {
                id: new_id(),
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
        let index = guard
            .doc
            .todos
            .iter()
            .position(|item| item.id == id)
            .ok_or("这条待办已经不在了")?;
        let now = now_ms();
        let follow = if done {
            let todo = &guard.doc.todos[index];
            match (&todo.repeat, todo.due_at) {
                (Some(repeat), Some(due)) => next_due(repeat, due, local_offset_secs()).map(|next| Todo {
                    id: new_id(),
                    title: todo.title.clone(),
                    note: todo.note.clone(),
                    due_at: Some(next),
                    priority: todo.priority.clone(),
                    done: false,
                    created_at: now,
                    updated_at: now,
                    reminded: false,
                    repeat: Some(repeat.clone()),
                    lead_ms: todo.lead_ms,
                    lead_reminded: false,
                }),
                _ => None,
            }
        } else {
            None
        };
        let saved = {
            let todo = &mut guard.doc.todos[index];
            todo.done = done || follow.is_some();
            todo.updated_at = now;
            todo.clone()
        };
        if let Some(follow) = follow {
            guard.doc.todos.push(follow);
        }
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
    if !matches!(repeat.rule.as_str(), "weekly" | "weekdays" | "monthEnd") {
        return Err("重复规则不对".into());
    }
    if let Some(day) = repeat.weekday {
        if day > 6 {
            return Err("重复规则不对".into());
        }
    }
    Ok(Some(repeat))
}

fn clean_lead(lead_ms: i64) -> Result<i64, String> {
    if !(0..=7 * 86_400_000).contains(&lead_ms) {
        return Err("提前量不对".into());
    }
    Ok(lead_ms)
}
