use serde::Deserialize;

use super::model::{Document, Plan, Stage, Todo};
use super::time::normalize_priority;

/// 旧版「到点」只有待办。一条记录若设了多个提醒时间，导入成计划的多个阶段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyTodo {
    id: String,
    title: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    due_at: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    remind_before_minutes: i64,
    #[serde(default)]
    reminders: Vec<i64>,
    #[serde(default)]
    reminded_ats: Vec<i64>,
    #[serde(default = "schema_v1")]
    schema: u32,
    #[serde(default = "normal_priority")]
    priority: String,
    #[serde(default)]
    done: bool,
    created_at: i64,
    updated_at: i64,
    #[serde(default)]
    reminded_for: Option<i64>,
}

fn schema_v1() -> u32 {
    1
}

fn normal_priority() -> String {
    "normal".to_string()
}

pub fn migrate_legacy_json(raw: &str) -> Result<Document, String> {
    let items: Vec<LegacyTodo> =
        serde_json::from_str(raw).map_err(|err| format!("旧待办读不出来: {err}"))?;
    let mut doc = Document::default();
    for item in items {
        if item.schema >= 2 && item.reminders.len() > 1 {
            doc.plans.push(plan_from_legacy(item));
        } else {
            doc.todos.push(todo_from_legacy(item));
        }
    }
    Ok(doc)
}

fn todo_from_legacy(item: LegacyTodo) -> Todo {
    let due_at = item.due_at.or_else(|| {
        if item.reminders.len() == 1 {
            item.reminders.first().copied()
        } else {
            None
        }
    });
    let reminded = match due_at {
        Some(at) => item.reminded_for == Some(at) || item.reminded_ats.contains(&at),
        None => false,
    };
    Todo {
        id: item.id,
        title: item.title,
        note: item.note,
        due_at,
        priority: normalize_priority(&item.priority),
        done: item.done,
        created_at: item.created_at,
        updated_at: item.updated_at,
        reminded,
        repeat: None,
        lead_ms: 0,
        lead_reminded: false,
    }
}

fn plan_from_legacy(item: LegacyTodo) -> Plan {
    let mut times = item.reminders.clone();
    times.sort_unstable();
    times.dedup();
    let stages = times
        .into_iter()
        .enumerate()
        .map(|(index, at)| Stage {
            id: format!("{}-{}", item.id, index + 1),
            title: format!("阶段 {}", index + 1),
            at: Some(at),
            done: item.done,
            reminded: item.reminded_ats.contains(&at),
        })
        .collect();
    Plan {
        id: item.id,
        title: item.title,
        note: item.note,
        priority: normalize_priority(&item.priority),
        stages,
        done: item.done,
        created_at: item.created_at,
        updated_at: item.updated_at,
        template_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_single_time_point_on_old_todos() {
        let raw = r#"[
          {
            "id": "1",
            "title": "临时改需求",
            "dueAt": 1000,
            "remindBeforeMinutes": 15,
            "priority": "high",
            "done": false,
            "createdAt": 1,
            "updatedAt": 2,
            "remindedFor": 1000
          }
        ]"#;
        let doc = migrate_legacy_json(raw).unwrap();
        assert!(doc.plans.is_empty());
        assert_eq!(doc.todos.len(), 1);
        assert_eq!(doc.todos[0].due_at, Some(1000));
        assert!(doc.todos[0].reminded);
        assert_eq!(doc.todos[0].priority, "high");
    }

    #[test]
    fn turns_multiple_reminders_into_plan_stages() {
        let raw = r#"[
          {
            "id": "v12",
            "title": "1.2 版本",
            "note": "",
            "dueAt": 9000,
            "remindBeforeMinutes": 0,
            "reminders": [3000, 1000],
            "remindedAts": [1000],
            "schema": 2,
            "priority": "normal",
            "done": false,
            "createdAt": 1,
            "updatedAt": 2
          }
        ]"#;
        let doc = migrate_legacy_json(raw).unwrap();
        assert!(doc.todos.is_empty());
        let plan = &doc.plans[0];
        assert_eq!(plan.title, "1.2 版本");
        assert_eq!(plan.stages.len(), 2);
        assert_eq!(plan.stages[0].at, Some(1000));
        assert!(plan.stages[0].reminded);
        assert_eq!(plan.stages[1].at, Some(3000));
        assert!(!plan.stages[1].reminded);
        assert_eq!(plan.stages[0].title, "阶段 1");
    }
}
