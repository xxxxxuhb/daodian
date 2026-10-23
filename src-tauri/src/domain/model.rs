use serde::{Deserialize, Serialize};

fn normal_priority() -> String {
    "normal".to_string()
}

/// 重复规则。`dueAt` 始终是下一次，完成这一次后再滚到后面。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Repeat {
    pub rule: String,
    #[serde(default)]
    pub weekday: Option<u8>,
    #[serde(default)]
    pub interval_days: Option<u32>,
    #[serde(default)]
    pub until: Option<i64>,
    #[serde(default)]
    pub skip_weekends: bool,
}

/// 临时事项。只有一个时间点，到点提醒一次；重复事项完成这次后生成下一次。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub due_at: Option<i64>,
    #[serde(default = "normal_priority")]
    pub priority: String,
    #[serde(default)]
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub reminded: bool,
    #[serde(default)]
    pub repeat: Option<Repeat>,
    #[serde(default)]
    pub lead_ms: i64,
    #[serde(default)]
    pub lead_reminded: bool,
    /// 同一重复任务各次实例共享。旧数据在第一次滚动或保存时自动补上。
    #[serde(default)]
    pub series_id: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

/// 计划里的一个阶段，例如提测、发布。每个阶段自己的时间。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StageDependency {
    pub stage_id: String,
    #[serde(default)]
    pub delay_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub at: Option<i64>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub reminded: bool,
    #[serde(default)]
    pub depends_on: Option<StageDependency>,
    #[serde(default)]
    pub completed_at: Option<i64>,
}

/// 一件要分阶段推进的事，例如一个项目版本。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default = "normal_priority")]
    pub priority: String,
    #[serde(default)]
    pub stages: Vec<Stage>,
    #[serde(default)]
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// 缺这个字段的是 v1。读到之后备份成 store.v1.bak.json，再写成 2。
    #[serde(default = "schema_v1")]
    pub version: u32,
    #[serde(default)]
    pub todos: Vec<Todo>,
    #[serde(default)]
    pub plans: Vec<Plan>,
}

fn schema_v1() -> u32 {
    1
}

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoInput {
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub note: String,
    pub due_at: Option<i64>,
    pub priority: String,
    #[serde(default)]
    pub repeat: Option<Repeat>,
    #[serde(default)]
    pub lead_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageInput {
    pub id: Option<String>,
    pub title: String,
    pub at: Option<i64>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub depends_on: Option<StageDependency>,
    #[serde(default)]
    pub completed_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub note: String,
    pub priority: String,
    #[serde(default)]
    pub stages: Vec<StageInput>,
    #[serde(default)]
    pub template_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{Repeat, Stage};

    #[test]
    fn old_repeat_json_gets_safe_defaults() {
        let repeat: Repeat = serde_json::from_str(r#"{"rule":"weekly","weekday":5}"#).unwrap();
        assert_eq!(repeat.interval_days, None);
        assert_eq!(repeat.until, None);
        assert!(!repeat.skip_weekends);
    }

    #[test]
    fn old_stage_json_gets_dependency_defaults() {
        let stage: Stage = serde_json::from_str(
            r#"{"id":"a","title":"提测","at":1000,"done":false,"reminded":false}"#,
        )
        .unwrap();
        assert_eq!(stage.depends_on, None);
        assert_eq!(stage.completed_at, None);
    }
}
