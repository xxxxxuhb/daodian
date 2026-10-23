use std::collections::{HashMap, HashSet};
use tauri::{AppHandle, State};

use crate::domain::{
    Plan, PlanInput, Stage, StageInput, clean_note, new_id, normalize_priority, now_ms,
    require_title,
};
use crate::remind;

use super::{SharedStore, lock};

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
        let now = now_ms();
        apply_stage_done(plan, &stage_id, done, now)?;
        plan.updated_at = now;
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
        let previous = existing.iter().find(|stage| stage.id == id);
        let completed_at = if item.done {
            item.completed_at.or_else(|| {
                previous
                    .filter(|stage| stage.done)
                    .and_then(|stage| stage.completed_at)
            })
        } else {
            None
        };
        let reminded = previous.is_some_and(|stage| {
            stage.at == item.at && stage.depends_on == item.depends_on && stage.reminded
        });
        out.push(Stage {
            id,
            title,
            at: item.at,
            done: item.done,
            reminded,
            depends_on: item.depends_on.clone(),
            completed_at,
        });
    }
    validate_dependencies(&out)?;
    refresh_dependency_schedules(&mut out);
    Ok(out)
}

fn validate_dependencies(stages: &[Stage]) -> Result<(), String> {
    let ids: HashSet<&str> = stages.iter().map(|stage| stage.id.as_str()).collect();
    for stage in stages {
        let Some(dependency) = &stage.depends_on else {
            continue;
        };
        if dependency.delay_ms < 0 {
            return Err("依赖延迟不能小于 0".into());
        }
        if dependency.stage_id == stage.id {
            return Err("阶段不能依赖自己".into());
        }
        if !ids.contains(dependency.stage_id.as_str()) {
            return Err(format!("依赖的阶段不存在：{}", dependency.stage_id));
        }
    }

    let dependencies: HashMap<&str, &str> = stages
        .iter()
        .filter_map(|stage| {
            stage
                .depends_on
                .as_ref()
                .map(|dependency| (stage.id.as_str(), dependency.stage_id.as_str()))
        })
        .collect();
    for stage in stages {
        let mut seen = HashSet::new();
        let mut current = stage.id.as_str();
        while let Some(next) = dependencies.get(current).copied() {
            if !seen.insert(current) || next == stage.id {
                return Err("阶段依赖不能成环".into());
            }
            current = next;
        }
    }
    Ok(())
}

fn dependency_chain_ready(
    stage_id: &str,
    stages: &[Stage],
    visiting: &mut HashSet<String>,
) -> bool {
    if !visiting.insert(stage_id.to_string()) {
        return false;
    }
    let Some(stage) = stages.iter().find(|stage| stage.id == stage_id) else {
        return false;
    };
    let ready = match &stage.depends_on {
        None => true,
        Some(dependency) => stages
            .iter()
            .find(|candidate| candidate.id == dependency.stage_id)
            .is_some_and(|upstream| {
                upstream.done
                    && upstream.completed_at.is_some()
                    && dependency_chain_ready(&upstream.id, stages, visiting)
            }),
    };
    visiting.remove(stage_id);
    ready
}

fn refresh_dependency_schedules(stages: &mut [Stage]) {
    let snapshot = stages.to_vec();
    for stage in stages {
        let Some(dependency) = &stage.depends_on else {
            continue;
        };
        let next_at = snapshot
            .iter()
            .find(|candidate| candidate.id == dependency.stage_id)
            .filter(|upstream| {
                upstream.done
                    && upstream.completed_at.is_some()
                    && dependency_chain_ready(&upstream.id, &snapshot, &mut HashSet::new())
            })
            .and_then(|upstream| upstream.completed_at)
            .and_then(|completed_at| completed_at.checked_add(dependency.delay_ms));
        if stage.at != next_at {
            stage.at = next_at;
            stage.reminded = false;
        }
    }
}

fn apply_stage_done(plan: &mut Plan, stage_id: &str, done: bool, now: i64) -> Result<(), String> {
    let stage = plan
        .stages
        .iter_mut()
        .find(|item| item.id == stage_id)
        .ok_or("这个阶段已经不在了")?;
    stage.done = done;
    stage.completed_at = done.then_some(now);
    refresh_dependency_schedules(&mut plan.stages);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::StageDependency;

    fn input(id: &str, dependency: Option<(&str, i64)>) -> StageInput {
        StageInput {
            id: Some(id.into()),
            title: id.into(),
            at: Some(100),
            done: false,
            depends_on: dependency.map(|(stage_id, delay_ms)| StageDependency {
                stage_id: stage_id.into(),
                delay_ms,
            }),
            completed_at: None,
        }
    }

    fn plan_from(inputs: &[StageInput]) -> Plan {
        Plan {
            id: "p".into(),
            title: "计划".into(),
            note: String::new(),
            priority: "normal".into(),
            stages: merge_stages(&[], inputs).unwrap(),
            done: false,
            created_at: 0,
            updated_at: 0,
            template_id: None,
        }
    }

    #[test]
    fn rejects_missing_dependency() {
        let error = merge_stages(&[], &[input("b", Some(("missing", 0)))]).unwrap_err();
        assert!(error.contains("不存在"));
    }

    #[test]
    fn rejects_dependency_cycle() {
        let error = merge_stages(
            &[],
            &[input("a", Some(("b", 0))), input("b", Some(("a", 0)))],
        )
        .unwrap_err();
        assert!(error.contains("成环"));
    }

    #[test]
    fn completion_advances_multiple_levels_and_cancel_invalidates_them() {
        let mut plan = plan_from(&[
            input("a", None),
            input("b", Some(("a", 3_600_000))),
            input("c", Some(("b", 86_400_000))),
        ]);
        assert_eq!(plan.stages[1].at, None);
        assert_eq!(plan.stages[2].at, None);

        apply_stage_done(&mut plan, "a", true, 1_000).unwrap();
        assert_eq!(plan.stages[1].at, Some(3_601_000));
        apply_stage_done(&mut plan, "b", true, 5_000).unwrap();
        assert_eq!(plan.stages[2].at, Some(86_405_000));

        plan.stages[1].reminded = true;
        plan.stages[2].reminded = true;
        apply_stage_done(&mut plan, "a", false, 9_000).unwrap();
        assert_eq!(plan.stages[1].at, None);
        assert_eq!(plan.stages[2].at, None);
        assert!(!plan.stages[1].reminded);
        assert!(!plan.stages[2].reminded);
        assert_eq!(plan.stages[0].completed_at, None);
    }
}
