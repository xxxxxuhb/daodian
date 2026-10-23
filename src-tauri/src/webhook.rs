use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::{SharedSettings, WebhookSettings};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_ATTEMPTS: u32 = 3;
const FIRST_BACKOFF_MS: u64 = 250;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Feishu,
    Wecom,
}

impl Provider {
    fn name(self) -> &'static str {
        match self {
            Self::Feishu => "feishu",
            Self::Wecom => "wecom",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookStatusEvent {
    provider: Provider,
    status: String,
}

struct Target {
    provider: Provider,
    url: String,
}

enum DeliveryError {
    Network,
    Http(u16),
    Api,
    Response,
}

impl DeliveryError {
    fn log_status(&self) -> &'static str {
        match self {
            Self::Network => "network_error",
            Self::Http(_) => "http_error",
            Self::Api => "api_error",
            Self::Response => "response_error",
        }
    }

    fn display(&self) -> String {
        match self {
            Self::Network => "网络错误".into(),
            Self::Http(status) => format!("HTTP {status}"),
            Self::Api => "机器人接口拒绝".into(),
            Self::Response => "响应格式错误".into(),
        }
    }
}

pub fn validate_settings(settings: &WebhookSettings, provider: Provider) -> Result<(), String> {
    if settings.webhook.trim().is_empty() {
        if settings.enabled {
            return Err(format!("{} Webhook URL 不能为空", provider_label(provider)));
        }
        return Ok(());
    }
    validate_url(provider, &settings.webhook)
}

pub fn spawn_reminder(app: AppHandle, title: String, body: String) {
    let targets = {
        let store = app.state::<SharedSettings>();
        match store.lock() {
            Ok(store) => {
                let (feishu, wecom) = store.webhook_settings();
                [(Provider::Feishu, feishu), (Provider::Wecom, wecom)]
                    .into_iter()
                    .filter_map(|(provider, settings)| {
                        settings.enabled.then_some(Target {
                            provider,
                            url: settings.webhook,
                        })
                    })
                    .collect::<Vec<_>>()
            }
            Err(_) => return,
        }
    };
    if targets.is_empty() {
        return;
    }

    for target in targets {
        let app = app.clone();
        let title = title.clone();
        let body = body.clone();
        std::thread::spawn(move || {
            let status = match validate_url(target.provider, &target.url) {
                Ok(()) => delivery_status(deliver_with_retry(
                    target.provider,
                    &target.url,
                    &title,
                    &body,
                    false,
                )),
                Err(_) => {
                    crate::logging::warn(format_args!(
                        "webhook provider={} status=invalid_url attempt=0",
                        target.provider.name()
                    ));
                    stamped("URL 无效")
                }
            };
            update_status(&app, target.provider, status);
        });
    }
}

#[tauri::command]
pub async fn test_webhook(
    app: AppHandle,
    provider: Provider,
    webhook: String,
) -> Result<String, String> {
    if let Err(message) = validate_url(provider, &webhook) {
        let status = stamped(&message);
        update_status(&app, provider, status.clone());
        return Err(status);
    }

    let result = tauri::async_runtime::spawn_blocking(move || {
        deliver_with_retry(
            provider,
            &webhook,
            "到点 Webhook 测试",
            "测试消息发送成功",
            true,
        )
    })
    .await
    .map_err(|_| "Webhook 测试任务异常结束".to_string())?;
    let succeeded = result.is_ok();
    let status = delivery_status(result);
    update_status(&app, provider, status.clone());
    if succeeded { Ok(status) } else { Err(status) }
}

fn delivery_status(result: Result<(), DeliveryError>) -> String {
    match result {
        Ok(()) => stamped("发送成功"),
        Err(error) => stamped(&format!("发送失败：{}", error.display())),
    }
}

fn stamped(message: &str) -> String {
    format!(
        "{} · {}",
        chrono::Local::now().format("%m-%d %H:%M"),
        message
    )
}

fn update_status(app: &AppHandle, provider: Provider, status: String) {
    let saved = {
        let store = app.state::<SharedSettings>();
        store
            .lock()
            .map_err(|_| ())
            .and_then(|mut store| store.set_webhook_status(provider, &status).map_err(|_| ()))
            .is_ok()
    };
    if saved {
        let _ = app.emit("webhook-status", WebhookStatusEvent { provider, status });
    }
}

fn deliver_with_retry(
    provider: Provider,
    url: &str,
    title: &str,
    body: &str,
    test_message: bool,
) -> Result<(), DeliveryError> {
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| DeliveryError::Network)?;
    let payload = payload(provider, title, body, test_message);
    let mut last = DeliveryError::Network;

    for attempt in 1..=MAX_ATTEMPTS {
        match deliver_once(&client, provider, url, &payload) {
            Ok(()) => {
                crate::logging::info(format_args!(
                    "webhook provider={} status=success attempt={attempt}",
                    provider.name()
                ));
                return Ok(());
            }
            Err(error) => {
                crate::logging::warn(format_args!(
                    "webhook provider={} status={} attempt={attempt}",
                    provider.name(),
                    error.log_status()
                ));
                last = error;
                if attempt < MAX_ATTEMPTS {
                    std::thread::sleep(backoff(attempt));
                }
            }
        }
    }
    Err(last)
}

fn deliver_once(
    client: &Client,
    provider: Provider,
    url: &str,
    payload: &Value,
) -> Result<(), DeliveryError> {
    let response = client
        .post(url)
        .json(payload)
        .send()
        .map_err(|_| DeliveryError::Network)?;
    let status = response.status();
    if !status.is_success() {
        return Err(DeliveryError::Http(status.as_u16()));
    }
    let body = response
        .json::<Value>()
        .map_err(|_| DeliveryError::Response)?;
    let accepted = match provider {
        Provider::Feishu => body.get("code").and_then(Value::as_i64) == Some(0),
        Provider::Wecom => body.get("errcode").and_then(Value::as_i64) == Some(0),
    };
    accepted.then_some(()).ok_or(DeliveryError::Api)
}

fn payload(provider: Provider, title: &str, body: &str, test_message: bool) -> Value {
    match (provider, test_message) {
        (Provider::Feishu, true) => json!({
            "msg_type": "text",
            "content": { "text": format!("{title}\n{body}") }
        }),
        (Provider::Feishu, false) => json!({
            "msg_type": "interactive",
            "card": {
                "header": {
                    "title": { "tag": "plain_text", "content": title }
                },
                "elements": [{
                    "tag": "div",
                    "text": { "tag": "lark_md", "content": body }
                }]
            }
        }),
        (Provider::Wecom, true) => json!({
            "msgtype": "text",
            "text": { "content": format!("{title}\n{body}") }
        }),
        (Provider::Wecom, false) => json!({
            "msgtype": "markdown",
            "markdown": { "content": format!("**{title}**\n> {body}") }
        }),
    }
}

fn validate_url(provider: Provider, raw: &str) -> Result<(), String> {
    let url = Url::parse(raw.trim()).map_err(|_| "Webhook URL 格式无效".to_string())?;
    let host = url.host_str().unwrap_or_default();
    let local_mock = cfg!(test) && matches!(host, "localhost" | "127.0.0.1" | "::1");
    if url.scheme() != "https" && !(local_mock && url.scheme() == "http") {
        return Err("Webhook URL 必须使用 HTTPS".into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("Webhook URL 不能包含账号信息或片段".into());
    }
    if local_mock {
        return Ok(());
    }

    match provider {
        Provider::Feishu => {
            if !matches!(host, "open.feishu.cn" | "open.larksuite.com") {
                return Err("飞书 Webhook 域名无效".into());
            }
            let token = url
                .path()
                .strip_prefix("/open-apis/bot/v2/hook/")
                .unwrap_or_default();
            if token.is_empty() || token.contains('/') || url.query().is_some() {
                return Err("飞书 Webhook 路径无效".into());
            }
        }
        Provider::Wecom => {
            if host != "qyapi.weixin.qq.com" || url.path() != "/cgi-bin/webhook/send" {
                return Err("企业微信 Webhook 域名或路径无效".into());
            }
            let pairs = url.query_pairs().collect::<Vec<_>>();
            if pairs.len() != 1 || pairs[0].0 != "key" || pairs[0].1.is_empty() {
                return Err("企业微信 Webhook key 无效".into());
            }
        }
    }
    Ok(())
}

fn backoff(failed_attempt: u32) -> Duration {
    Duration::from_millis(FIRST_BACKOFF_MS * 2_u64.pow(failed_attempt.saturating_sub(1)))
}

fn provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::Feishu => "飞书",
        Provider::Wecom => "企业微信",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_only_official_webhook_shapes() {
        assert!(
            validate_url(
                Provider::Feishu,
                "https://open.feishu.cn/open-apis/bot/v2/hook/token"
            )
            .is_ok()
        );
        assert!(
            validate_url(
                Provider::Wecom,
                "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=token"
            )
            .is_ok()
        );
        assert!(validate_url(Provider::Feishu, "https://example.com/hook/token").is_err());
        assert!(
            validate_url(
                Provider::Wecom,
                "http://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=token"
            )
            .is_err()
        );
        assert!(validate_url(Provider::Feishu, "http://127.0.0.1:1234/mock").is_ok());
    }

    #[test]
    fn builds_provider_specific_payloads() {
        let feishu = payload(Provider::Feishu, "两件到点", "写周报、评审", false);
        assert_eq!(feishu["msg_type"], "interactive");
        assert_eq!(feishu["card"]["header"]["title"]["content"], "两件到点");

        let wecom = payload(Provider::Wecom, "两件到点", "写周报、评审", false);
        assert_eq!(wecom["msgtype"], "markdown");
        assert_eq!(wecom["markdown"]["content"], "**两件到点**\n> 写周报、评审");
    }

    #[test]
    fn retries_use_exponential_backoff() {
        assert_eq!(backoff(1), Duration::from_millis(250));
        assert_eq!(backoff(2), Duration::from_millis(500));
        assert_eq!(backoff(3), Duration::from_millis(1_000));
        assert_eq!(MAX_ATTEMPTS, 3);
    }
}
