use super::model::{Document, Plan, Stage, Todo};

/// 刚到点的阶段都响；睡着错过的一串里只响最近一次。
pub const FRESH_MS: i64 = 90_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemindTarget {
    Todo { id: String },
    Stage { plan_id: String, stage_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemindAction {
    pub target: RemindTarget,
    pub at: i64,
    /// 事项名，直接当通知标题。
    pub title: String,
    /// 一行上下文，当通知正文。
    pub detail: String,
    pub notify: bool,
    /// 提前提醒，记账时只标 leadReminded，到点那一次还要再响。
    pub lead: bool,
}

/// 最近一个还没到的时间点，用来算下一次醒来。
pub fn next_due_at(doc: &Document, now: i64) -> Option<i64> {
    let mut soonest: Option<i64> = None;
    let mut take = |at: i64| {
        if at > now {
            soonest = Some(soonest.map_or(at, |current| current.min(at)));
        }
    };
    for todo in &doc.todos {
        if todo.done {
            continue;
        }
        if let Some(at) = todo.due_at {
            if todo.lead_ms > 0 && !todo.lead_reminded {
                take(at - todo.lead_ms);
            }
            take(at);
        }
    }
    for plan in &doc.plans {
        if plan.done {
            continue;
        }
        for stage in &plan.stages {
            if stage.done {
                continue;
            }
            if let Some(at) = stage.at {
                take(at);
            }
        }
    }
    soonest
}

pub fn plan_actions(doc: &Document, now: i64) -> Vec<RemindAction> {
    let mut actions = Vec::new();
    for todo in &doc.todos {
        actions.extend(todo_actions(todo, now));
    }
    for plan in &doc.plans {
        actions.extend(stage_actions(plan, now));
    }
    actions
}

fn todo_actions(todo: &Todo, now: i64) -> Vec<RemindAction> {
    if todo.done || todo.reminded {
        return Vec::new();
    }
    let Some(at) = todo.due_at else {
        return Vec::new();
    };
    if todo.lead_ms > 0 && !todo.lead_reminded && now < at && now >= at - todo.lead_ms {
        return vec![RemindAction {
            target: RemindTarget::Todo {
                id: todo.id.clone(),
            },
            at,
            title: format!("还有 {} · {}", lead_label(todo.lead_ms), todo.title),
            detail: note_preview(&todo.note),
            notify: true,
            lead: true,
        }];
    }
    if now < at {
        return Vec::new();
    }
    vec![RemindAction {
        target: RemindTarget::Todo {
            id: todo.id.clone(),
        },
        at,
        title: todo.title.clone(),
        detail: note_preview(&todo.note),
        notify: true,
        lead: false,
    }]
}

fn lead_label(ms: i64) -> String {
    if ms % 86_400_000 == 0 {
        format!("{} 天", ms / 86_400_000)
    } else if ms % 3_600_000 == 0 {
        format!("{} 小时", ms / 3_600_000)
    } else {
        format!("{} 分钟", (ms / 60_000).max(1))
    }
}

fn note_preview(note: &str) -> String {
    let note = note.trim();
    if note.is_empty() {
        return "只提醒这一次".to_string();
    }
    let mut preview: String = note.chars().take(24).collect();
    if note.chars().count() > 24 {
        preview.push('…');
    }
    preview
}

fn stage_actions(plan: &Plan, now: i64) -> Vec<RemindAction> {
    if plan.done {
        return Vec::new();
    }
    let total = plan.stages.len();
    let mut pending: Vec<(usize, &Stage)> = plan
        .stages
        .iter()
        .enumerate()
        .filter(|(_, stage)| stage_is_due(stage, now))
        .collect();
    pending.sort_by_key(|(_, stage)| stage.at.unwrap_or(0));
    if pending.is_empty() {
        return Vec::new();
    }
    let fresh_from = now.saturating_sub(FRESH_MS);
    let any_fresh = pending
        .iter()
        .any(|(_, stage)| stage.at.unwrap_or(0) >= fresh_from);
    let last = pending.len() - 1;
    pending
        .into_iter()
        .enumerate()
        .map(|(index, (position, stage))| {
            let at = stage.at.unwrap_or(0);
            let notify = if any_fresh {
                at >= fresh_from
            } else {
                index == last
            };
            RemindAction {
                target: RemindTarget::Stage {
                    plan_id: plan.id.clone(),
                    stage_id: stage.id.clone(),
                },
                at,
                title: stage.title.clone(),
                detail: format!("{} · 阶段 {}/{}", plan.title, position + 1, total),
                notify,
                lead: false,
            }
        })
        .collect()
}

fn stage_is_due(stage: &Stage, now: i64) -> bool {
    if stage.done || stage.reminded {
        return false;
    }
    matches!(stage.at, Some(at) if at <= now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo(id: &str, due: i64) -> Todo {
        Todo {
            id: id.to_string(),
            title: "临时对一下".to_string(),
            note: String::new(),
            due_at: Some(due),
            priority: "normal".to_string(),
            done: false,
            created_at: 1,
            updated_at: 1,
            reminded: false,
            repeat: None,
            lead_ms: 0,
            lead_reminded: false,
        }
    }

    fn stage(id: &str, title: &str, at: i64) -> Stage {
        Stage {
            id: id.to_string(),
            title: title.to_string(),
            at: Some(at),
            done: false,
            reminded: false,
        }
    }

    fn plan(stages: Vec<Stage>) -> Plan {
        Plan {
            id: "v1".to_string(),
            title: "1.2 版本".to_string(),
            note: String::new(),
            priority: "normal".to_string(),
            stages,
            done: false,
            created_at: 1,
            updated_at: 1,
            template_id: None,
        }
    }

    #[test]
    fn todo_rings_once_at_its_time() {
        let now = 1_000_000;
        let mut waiting = todo("later", now + 60_000);
        waiting.id = "later".to_string();
        let due = todo("due", now - 1_000);
        let mut done = todo("done", now - 1_000);
        done.done = true;
        let mut reminded = todo("reminded", now - 1_000);
        reminded.reminded = true;
        let doc = Document {
            version: 2,
            todos: vec![waiting, due, done, reminded],
            plans: vec![],
        };
        let actions = plan_actions(&doc, now);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "临时对一下");
        assert_eq!(actions[0].detail, "只提醒这一次");
        assert!(actions[0].notify);
    }

    #[test]
    fn note_becomes_the_detail_line() {
        let now = 1_000_000;
        let mut with_note = todo("note", now - 1_000);
        with_note.note = "  顺手把压测结论一起附上  ".to_string();
        let doc = Document {
            version: 2,
            todos: vec![with_note],
            plans: vec![],
        };
        let actions = plan_actions(&doc, now);
        assert_eq!(actions[0].detail, "顺手把压测结论一起附上");
    }

    #[test]
    fn plan_stages_ring_on_their_own_times() {
        let now = 5_000_000;
        let fresh = plan(vec![
            stage("test", "提测", now - 20_000),
            stage("ship", "发布", now - 10_000),
            stage("later", "复盘", now + 60_000),
        ]);
        let mut stale = plan(vec![
            stage("a", "提测", now - 200_000),
            stage("b", "发布", now - 100_000),
        ]);
        stale.id = "old".to_string();
        let mut finished = fresh.clone();
        finished.id = "finished".to_string();
        finished.done = true;

        let doc = Document {
            version: 2,
            todos: vec![],
            plans: vec![fresh, stale, finished],
        };
        let actions = plan_actions(&doc, now);
        let fresh_hits: Vec<_> = actions
            .iter()
            .filter(|action| matches!(&action.target, RemindTarget::Stage { plan_id, .. } if plan_id == "v1"))
            .collect();
        assert_eq!(fresh_hits.len(), 2);
        assert!(fresh_hits.iter().all(|action| action.notify));
        assert_eq!(fresh_hits[0].title, "提测");
        assert_eq!(fresh_hits[0].detail, "1.2 版本 · 阶段 1/3");
        assert_eq!(fresh_hits[1].title, "发布");
        assert_eq!(fresh_hits[1].detail, "1.2 版本 · 阶段 2/3");

        let stale_hits: Vec<_> = actions
            .iter()
            .filter(|action| matches!(&action.target, RemindTarget::Stage { plan_id, .. } if plan_id == "old"))
            .collect();
        assert_eq!(stale_hits.len(), 2);
        assert!(!stale_hits[0].notify);
        assert!(stale_hits[1].notify);
        assert!(actions.iter().all(|action| {
            !matches!(&action.target, RemindTarget::Stage { plan_id, .. } if plan_id == "finished")
        }));
    }

    #[test]
    fn next_due_skips_done_and_past() {
        let now = 1_000_000;
        let mut done = todo("done", now + 10_000);
        done.done = true;
        let doc = Document {
            version: 2,
            todos: vec![todo("past", now - 5_000), todo("soon", now + 30_000), done],
            plans: vec![plan(vec![stage("s", "提测", now + 90_000)])],
        };
        assert_eq!(next_due_at(&doc, now), Some(now + 30_000));
        assert_eq!(next_due_at(&Document::default(), now), None);
    }
}
