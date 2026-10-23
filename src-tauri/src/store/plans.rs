use tauri::{AppHandle, State};

use crate::domain::{
    clean_note, new_id, normalize_priority, now_ms, require_title, Plan, PlanInput, Stage, StageInput,
};
use crate::remind;

use super::{lock, SharedStore};

const MAX_STAGES: usize = 20;

#[tauri::command]
pub fn save_plan(
    app: AppHandle,
    store: State<SharedStore>,
    input: PlanInput,
) -> Result<Plan, String> {
    let saved = {
        let mut guard = lock(&store)?;
        let title = require_title(&input.title, 80)?;
        let note = clean_note(&input.note)?;
        let priority = normalize_priority(&input.priority);
        let now = now_ms();

        if let Some(id) = input.id.clone().filter(|id| !id.is_empty()) {
            let existing = guard
                .doc
                .plans
                .iter()
                .find(|item| item.id == id)
                .ok_or("这份计划已经不在了")?
                .stages
                .clone();
            let stages = merge_stages(&existing, &input.stages)?;
            let plan = guard
                .doc
                .plans
                .iter_mut()
                .find(|item| item.id == id)
                .expect("计划刚找到");
            plan.title = title;
            plan.note = note;
            plan.priority = priority;
            plan.stages = stages;
            plan.template_id = input.template_id.clone();
            plan.updated_at = now;
            let saved = plan.clone();
            guard.persist()?;
            saved
        } else {
            let stages = merge_stages(&[], &input.stages)?;
            let plan = Plan {
                id: new_id(),
                title,
                note,
                priority,
                stages,
                done: false,
                created_at: now,
                updated_at: now,
                template_id: input.template_id.clone(),
            };
            guard.doc.plans.push(plan.clone());
            guard.persist()?;
            plan
        }
    };

    remind::check(&app);
    Ok(saved)
}

#[tauri::command]
pub fn set_plan_done(
    app: AppHandle,
    store: State<SharedStore>,
    id: String,
    done: bool,
) -> Result<Plan, String> {
    let saved = {
        let mut guard = lock(&store)?;
        let plan = guard
            .doc
            .plans
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or("这份计划已经不在了")?;
        plan.done = done;
        plan.updated_at = now_ms();
        let saved = plan.clone();
        guard.persist()?;
        saved
    };
    remind::check(&app);
    Ok(saved)
}

#[tauri::command]
pub fn set_stage_done(
    app: AppHandle,
    store: State<SharedStore>,
    plan_id: String,
    stage_id: String,
    done: bool,
) -> Result<Plan, String> {
    let saved = {
        let mut guard = lock(&store)?;
        let plan = guard
            .doc
            .plans
            .iter_mut()
            .find(|item| item.id == plan_id)
            .ok_or("这份计划已经不在了")?;
        let stage = plan
            .stages
            .iter_mut()
            .find(|item| item.id == stage_id)
            .ok_or("这个阶段已经不在了")?;
        stage.done = done;
        plan.updated_at = now_ms();
        let saved = plan.clone();
        guard.persist()?;
        saved
    };
    remind::check(&app);
    Ok(saved)
}

#[tauri::command]
pub fn delete_plan(store: State<SharedStore>, id: String) -> Result<(), String> {
    let mut guard = lock(&store)?;
    let before = guard.doc.plans.len();
    guard.doc.plans.retain(|item| item.id != id);
    if guard.doc.plans.len() == before {
        return Err("这份计划已经不在了".into());
    }
    guard.persist()?;
    Ok(())
}

fn merge_stages(existing: &[Stage], input: &[StageInput]) -> Result<Vec<Stage>, String> {
    if input.len() > MAX_STAGES {
        return Err("最多 20 个阶段".into());
    }
    let mut out = Vec::new();
    for item in input {
        let title = item.title.trim().to_string();
        if title.is_empty() {
            return Err("阶段要有个名字".into());
        }
        if title.chars().count() > 40 {
            return Err("阶段名称最多 40 个字".into());
        }
        if let Some(at) = item.at {
            if at < 0 {
                return Err("阶段时间不对".into());
            }
        }
        let mut id = item
            .id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(new_id);
        if out.iter().any(|stage: &Stage| stage.id == id) {
            id = new_id();
        }
        let reminded = existing
            .iter()
            .find(|stage| stage.id == id)
            .is_some_and(|stage| stage.at == item.at && stage.reminded);
        out.push(Stage {
            id,
            title,
            at: item.at,
            done: item.done,
            reminded,
        });
    }
    Ok(out)
}
