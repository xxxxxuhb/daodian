use std::io::Read;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager, State};

use crate::settings::{DeepSeekSettings, SharedSettings};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
const MIN_DATE_MS: i64 = 946_684_800_000; // 2000-01-01
const MAX_DATE_MS: i64 = 4_102_444_800_000; // 2100-01-01
const MAX_STAGES: usize = 20;
const MAX_RESPONSE_BYTES: u64 = 128 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParsedTodo {
    title: String,
    due_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedStage {
    title: String,
    at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedPlan {
    title: String,
    #[serde(default)]
    note: String,
    #[serde(default = "normal_priority")]
    priority: String,
    stages: Vec<GeneratedStage>,
}

fn normal_priority() -> String {
    "normal".into()
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: String,
}

pub fn validate_settings(settings: &DeepSeekSettings) -> Result<(), String> {
    if settings.base_url.trim().is_empty() {
        return if settings.enabled {
            Err("DeepSeek Base URL 不能为空".into())
        } else {
            Ok(())
        };
    }
    validate_base_url(&settings.base_url)?;
    if settings.enabled && settings.model.trim().is_empty() {
        return Err("DeepSeek 模型不能为空".into());
    }
    Ok(())
}

fn validate_base_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|_| "DeepSeek Base URL 格式无效".to_string())?;
    let host = url.host_str().unwrap_or_default();
    let localhost = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if url.scheme() != "https" && !(cfg!(test) && localhost && url.scheme() == "http") {
        return Err("DeepSeek Base URL 必须使用 HTTPS".into());
    }
    if host.is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("DeepSeek Base URL 格式无效".into());
    }
    Ok(url)
}

fn endpoint(base_url: &str) -> Result<Url, String> {
    let mut url = validate_base_url(base_url)?;
    let path = url.path().trim_end_matches('/');
    let next = if path.ends_with("/v1") {
        format!("{path}/chat/completions")
    } else {
        format!("{path}/v1/chat/completions")
    };
    url.set_path(&next);
    Ok(url)
}

fn credentials(store: &SharedSettings) -> Result<(DeepSeekSettings, Vec<u8>), String> {
    store
        .lock()
        .map_err(|_| "设置暂时不可用".to_string())?
        .deep_seek_credentials()
}

fn request(
    settings: &DeepSeekSettings,
    key: &[u8],
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let api_key = std::str::from_utf8(key).map_err(|_| "DeepSeek API Key 数据无效")?;
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "无法创建 DeepSeek 客户端")?;
    let response = client
        .post(endpoint(&settings.base_url)?)
        .bearer_auth(api_key)
        .json(&json!({
            "model": settings.model.trim(),
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": prompt }
            ],
            "response_format": { "type": "json_object" },
            "temperature": 0.1
        }))
        .send()
        .map_err(|_| "DeepSeek 请求失败或超时")?;
    if !response.status().is_success() {
        return Err(format!("DeepSeek 返回 HTTP {}", response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取 DeepSeek 响应失败")?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("DeepSeek 响应过大".into());
    }
    let body =
        serde_json::from_slice::<ChatResponse>(&bytes).map_err(|_| "DeepSeek 响应格式错误")?;
    body.choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| "DeepSeek 响应缺少结果".into())
}

fn json_content(raw: &str) -> Result<Value, String> {
    let trimmed = raw.trim();
    let clean = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(clean).map_err(|_| "DeepSeek 返回的 JSON 无效".into())
}

fn clean_title(value: String, max: usize, label: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() || value.chars().count() > max {
        return Err(format!("{label}必须为 1 到 {max} 个字"));
    }
    Ok(value)
}

fn validate_date(value: Option<i64>) -> Result<Option<i64>, String> {
    if value.is_some_and(|at| !(MIN_DATE_MS..MAX_DATE_MS).contains(&at)) {
        return Err("DeepSeek 返回的日期超出允许范围".into());
    }
    Ok(value)
}

fn parse_todo_response(raw: &str) -> Result<ParsedTodo, String> {
    let mut todo: ParsedTodo =
        serde_json::from_value(json_content(raw)?).map_err(|_| "DeepSeek 待办结构无效")?;
    todo.title = clean_title(todo.title, 80, "待办标题")?;
    todo.due_at = validate_date(todo.due_at)?;
    Ok(todo)
}

fn parse_plan_response(raw: &str) -> Result<GeneratedPlan, String> {
    let mut plan: GeneratedPlan =
        serde_json::from_value(json_content(raw)?).map_err(|_| "DeepSeek 计划结构无效")?;
    plan.title = clean_title(plan.title, 80, "计划标题")?;
    plan.note = plan.note.trim().to_string();
    if plan.note.chars().count() > 500 {
        return Err("计划备注最多 500 个字".into());
    }
    if !matches!(plan.priority.as_str(), "low" | "normal" | "high") {
        return Err("DeepSeek 返回的优先级无效".into());
    }
    if plan.stages.len() > MAX_STAGES {
        return Err("计划最多 20 个阶段".into());
    }
    for stage in &mut plan.stages {
        stage.title = clean_title(std::mem::take(&mut stage.title), 40, "阶段标题")?;
        stage.at = validate_date(stage.at)?;
    }
    Ok(plan)
}

#[tauri::command]
pub async fn test_deepseek(
    app: AppHandle,
    store: State<'_, SharedSettings>,
) -> Result<String, String> {
    let (settings, key) = credentials(&store)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        request(
            &settings,
            &key,
            "Reply with exactly one JSON object and no explanation.",
            r#"Return {"ok":true}."#,
        )
        .and_then(|raw| {
            let value = json_content(&raw)?;
            (value.get("ok").and_then(Value::as_bool) == Some(true))
                .then_some(())
                .ok_or_else(|| "DeepSeek 测试响应无效".into())
        })
    })
    .await
    .map_err(|_| "DeepSeek 测试任务异常结束".to_string())?;
    let status = stamped(if result.is_ok() {
        "连接成功"
    } else {
        "连接失败"
    });
    if let Ok(mut locked) = app.state::<SharedSettings>().lock() {
        let _ = locked.set_deep_seek_status(&status);
    }
    result
        .map(|_| status.clone())
        .map_err(|err| format!("{status}：{err}"))
}

#[tauri::command]
pub async fn ai_parse_todo(
    store: State<'_, SharedSettings>,
    text: String,
    now_ms: i64,
    timezone_offset_minutes: i32,
) -> Result<ParsedTodo, String> {
    let text = text.trim().to_string();
    if text.is_empty() || text.chars().count() > 500 {
        return Err("待解析内容必须为 1 到 500 个字".into());
    }
    let (settings, key) = credentials(&store)?;
    tauri::async_runtime::spawn_blocking(move || {
        let system = "Extract a todo. Return only JSON with exactly: title (string), dueAt (Unix milliseconds or null). Never add fields.";
        let context = format!(
            "Current Unix milliseconds: {now_ms}; local timezone offset from UTC in minutes: {timezone_offset_minutes}.\nInput:\n{text}"
        );
        request(&settings, &key, system, &context).and_then(|raw| parse_todo_response(&raw))
    })
    .await
    .map_err(|_| "DeepSeek 解析任务异常结束".to_string())?
}

#[tauri::command]
pub async fn ai_generate_plan(
    store: State<'_, SharedSettings>,
    text: String,
    now_ms: i64,
    timezone_offset_minutes: i32,
) -> Result<GeneratedPlan, String> {
    let text = text.trim().to_string();
    if text.is_empty() || text.chars().count() > 2_000 {
        return Err("计划描述必须为 1 到 2000 个字".into());
    }
    let (settings, key) = credentials(&store)?;
    tauri::async_runtime::spawn_blocking(move || {
        let system = "Create an editable plan. Return only JSON with exactly: title (string), note (string), priority (low|normal|high), stages (array of 0..20 objects, each exactly title string and at Unix milliseconds or null).";
        let context = format!(
            "Current Unix milliseconds: {now_ms}; local timezone offset from UTC in minutes: {timezone_offset_minutes}.\nInput:\n{text}"
        );
        request(&settings, &key, system, &context).and_then(|raw| parse_plan_response(&raw))
    })
    .await
    .map_err(|_| "DeepSeek 生成任务异常结束".to_string())?
}

fn stamped(message: &str) -> String {
    format!("{} · {message}", chrono::Local::now().format("%m-%d %H:%M"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_openai_compatible_endpoint_and_rejects_insecure_urls() {
        assert_eq!(
            endpoint("https://api.deepseek.com").unwrap().as_str(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            endpoint("https://example.com/v1").unwrap().as_str(),
            "https://example.com/v1/chat/completions"
        );
        assert!(validate_base_url("http://example.com").is_err());
        assert!(validate_base_url("http://localhost:1234").is_ok());
    }

    #[test]
    fn parses_and_validates_todo_response() {
        let parsed =
            parse_todo_response(r#"{"title":" 提交周报 ","dueAt":1792756800000}"#).unwrap();
        assert_eq!(parsed.title, "提交周报");
        assert_eq!(parsed.due_at, Some(1_792_756_800_000));
        assert!(parse_todo_response(r#"{"title":"x","dueAt":1}"#).is_err());
        assert!(parse_todo_response(r#"{"title":"x","dueAt":null,"extra":true}"#).is_err());
    }

    #[test]
    fn parses_plan_and_limits_stage_count_and_dates() {
        let parsed = parse_plan_response(
            r#"{"title":"发布","note":"","priority":"high","stages":[{"title":"提测","at":1792756800000}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.stages.len(), 1);

        let stages = (0..21)
            .map(|index| json!({ "title": format!("阶段{index}"), "at": null }))
            .collect::<Vec<_>>();
        let oversized = json!({
            "title": "发布",
            "note": "",
            "priority": "normal",
            "stages": stages
        });
        assert!(parse_plan_response(&oversized.to_string()).is_err());
    }
}
