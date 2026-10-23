use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::{new_id, require_title};

use super::{Library, SharedStore, lock};

const MAX_TEMPLATES: usize = 50;
const MAX_STAGES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateStage {
    pub title: String,
    #[serde(default)]
    pub day: Option<i32>,
    #[serde(default)]
    pub hour: Option<u8>,
    #[serde(default)]
    pub minute: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanTemplate {
    pub id: String,
    pub label: String,
    pub stages: Vec<TemplateStage>,
    pub built_in: bool,
    #[serde(default)]
    pub quick: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInput {
    pub id: Option<String>,
    pub label: String,
    #[serde(default)]
    pub stages: Vec<TemplateStage>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TemplateDocument {
    #[serde(default = "template_version")]
    version: u32,
    #[serde(default)]
    templates: Vec<PlanTemplate>,
    #[serde(default)]
    quick_template_ids: Vec<String>,
}

fn template_version() -> u32 {
    1
}

pub(crate) fn load(path: &Path) -> (Vec<PlanTemplate>, Vec<String>) {
    let Ok(raw) = fs::read_to_string(path) else {
        return (Vec::new(), Vec::new());
    };
    serde_json::from_str::<TemplateDocument>(&raw)
        .map(|doc| {
            let templates = doc
                .templates
                .into_iter()
                .filter(|item| !is_builtin_id(&item.id))
                .map(|mut item| {
                    item.built_in = false;
                    item.quick = false;
                    item
                })
                .collect();
            (templates, doc.quick_template_ids)
        })
        .unwrap_or_default()
}

fn persist(
    path: &Path,
    templates: &[PlanTemplate],
    quick_template_ids: &[String],
) -> Result<(), String> {
    let document = TemplateDocument {
        version: template_version(),
        templates: templates.to_vec(),
        quick_template_ids: quick_template_ids.to_vec(),
    };
    let raw = serde_json::to_string_pretty(&document).map_err(|err| err.to_string())?;
    fs::write(path, raw).map_err(|err| err.to_string())
}

fn stage(title: &str, day: i32, hour: u8, minute: u8) -> TemplateStage {
    TemplateStage {
        title: title.into(),
        day: Some(day),
        hour: Some(hour),
        minute: Some(minute),
    }
}

fn builtins() -> Vec<PlanTemplate> {
    vec![
        PlanTemplate {
            id: "release".into(),
            label: "提测 → 发布 → 观察".into(),
            stages: vec![
                stage("提测", 0, 15, 0),
                stage("发布", 1, 10, 0),
                stage("观察", 3, 10, 0),
            ],
            built_in: true,
            quick: false,
        },
        PlanTemplate {
            id: "meeting".into(),
            label: "周会改版".into(),
            stages: vec![
                stage("议程", 0, 9, 0),
                stage("改版", 2, 14, 0),
                stage("定稿", 4, 16, 0),
            ],
            built_in: true,
            quick: false,
        },
        PlanTemplate {
            id: "blank".into(),
            label: "空计划".into(),
            stages: Vec::new(),
            built_in: true,
            quick: false,
        },
    ]
}

fn is_builtin_id(id: &str) -> bool {
    matches!(id, "release" | "meeting" | "blank")
}

fn validate_stages(stages: Vec<TemplateStage>) -> Result<Vec<TemplateStage>, String> {
    if stages.len() > MAX_STAGES {
        return Err("模板最多 20 个阶段".into());
    }
    stages
        .into_iter()
        .map(|mut item| {
            item.title = require_title(&item.title, 40)?;
            match (item.day, item.hour, item.minute) {
                (None, None, None) => {}
                (Some(day), Some(hour), Some(minute))
                    if (-365..=365).contains(&day) && hour < 24 && minute < 60 => {}
                _ => return Err("模板阶段时间不对".into()),
            }
            Ok(item)
        })
        .collect()
}

fn all_templates(library: &Library) -> Vec<PlanTemplate> {
    let mut result = builtins();
    result.extend(library.custom_templates.clone());
    for item in &mut result {
        item.quick = library.quick_template_ids.contains(&item.id);
    }
    result
}

#[tauri::command]
pub fn load_templates(store: State<SharedStore>) -> Result<Vec<PlanTemplate>, String> {
    let guard = lock(&store)?;
    Ok(all_templates(&guard))
}

#[tauri::command]
pub fn save_template(
    store: State<SharedStore>,
    input: TemplateInput,
) -> Result<PlanTemplate, String> {
    let mut guard = lock(&store)?;
    let label = require_title(&input.label, 40)?;
    let stages = validate_stages(input.stages)?;
    let id = input.id.filter(|id| !id.is_empty());

    let saved = if let Some(id) = id {
        if is_builtin_id(&id) {
            return Err("内置模板不能修改".into());
        }
        let item = guard
            .custom_templates
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or("这个模板已经不在了")?;
        item.label = label;
        item.stages = stages;
        item.clone()
    } else {
        if guard.custom_templates.len() >= MAX_TEMPLATES {
            return Err("最多保存 50 个自定义模板".into());
        }
        let item = PlanTemplate {
            id: format!("custom-{}", new_id()),
            label,
            stages,
            built_in: false,
            quick: false,
        };
        guard.custom_templates.push(item.clone());
        item
    };
    guard.persist_templates()?;
    Ok(saved)
}

#[tauri::command]
pub fn rename_template(
    store: State<SharedStore>,
    id: String,
    label: String,
) -> Result<PlanTemplate, String> {
    if is_builtin_id(&id) {
        return Err("内置模板不能重命名".into());
    }
    let mut guard = lock(&store)?;
    let label = require_title(&label, 40)?;
    let item = guard
        .custom_templates
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or("这个模板已经不在了")?;
    item.label = label;
    let saved = item.clone();
    guard.persist_templates()?;
    Ok(saved)
}

#[tauri::command]
pub fn delete_template(store: State<SharedStore>, id: String) -> Result<(), String> {
    if is_builtin_id(&id) {
        return Err("内置模板不能删除".into());
    }
    let mut guard = lock(&store)?;
    let before = guard.custom_templates.len();
    guard.custom_templates.retain(|item| item.id != id);
    if guard.custom_templates.len() == before {
        return Err("这个模板已经不在了".into());
    }
    guard.quick_template_ids.retain(|quick_id| quick_id != &id);
    guard.persist_templates()
}

#[tauri::command]
pub fn set_template_quick(
    store: State<SharedStore>,
    id: String,
    quick: bool,
) -> Result<(), String> {
    let mut guard = lock(&store)?;
    if !all_templates(&guard).iter().any(|item| item.id == id) {
        return Err("这个模板已经不在了".into());
    }
    guard.quick_template_ids.retain(|item| item != &id);
    if quick {
        guard.quick_template_ids.push(id);
    }
    guard.persist_templates()
}

impl Library {
    pub(crate) fn templates_path(dir: &Path) -> PathBuf {
        dir.join("templates.json")
    }

    pub(crate) fn persist_templates(&self) -> Result<(), String> {
        persist(
            &self.templates_path,
            &self.custom_templates,
            &self.quick_template_ids,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::current_dir()
            .expect("测试工作目录")
            .join("target")
            .join("template-tests");
        fs::create_dir_all(&dir).expect("创建模板测试目录");
        dir.join(format!("daodian-{name}-{}.json", new_id()))
    }

    #[test]
    fn missing_or_broken_file_falls_back_to_builtins() {
        let missing = temp_file("missing-templates");
        assert!(load(&missing).0.is_empty());

        let broken = temp_file("broken-templates");
        fs::write(&broken, "{not json").unwrap();
        assert!(load(&broken).0.is_empty());
        let _ = fs::remove_file(broken);
        assert_eq!(
            builtins()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["release", "meeting", "blank"]
        );
    }

    #[test]
    fn custom_templates_round_trip_without_builtin_override() {
        let path = temp_file("templates-round-trip");
        let custom = PlanTemplate {
            id: "custom-one".into(),
            label: "我的模板".into(),
            stages: vec![stage("开始", 0, 9, 30)],
            built_in: false,
            quick: false,
        };
        let fake_builtin = PlanTemplate {
            id: "release".into(),
            label: "假的内置模板".into(),
            stages: Vec::new(),
            built_in: false,
            quick: false,
        };
        persist(
            &path,
            &[custom.clone(), fake_builtin],
            &["custom-one".into()],
        )
        .unwrap();
        assert_eq!(load(&path), (vec![custom], vec!["custom-one".to_string()]));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_partial_or_invalid_stage_times() {
        assert!(
            validate_stages(vec![TemplateStage {
                title: "阶段".into(),
                day: Some(0),
                hour: None,
                minute: Some(0),
            }])
            .is_err()
        );
        assert!(validate_stages(vec![stage("阶段", 0, 24, 0)]).is_err());
    }
}
