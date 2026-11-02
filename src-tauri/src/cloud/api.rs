use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

const API: &str = "https://gitee.com/api/v5";

#[derive(Debug)]
pub struct ApiError {
    pub code: u16,
    pub kind: &'static str,
    pub message: String,
}

impl ApiError {
    fn offline() -> Self {
        Self {
            code: 0,
            kind: "offline",
            message: "网络不可用，这次先不同步".into(),
        }
    }

    fn status(code: u16, body: &str) -> Self {
        let raw = message_text(body);
        let lower = raw.to_lowercase();
        let (kind, message) = match code {
            401 => ("auth", "令牌无效或已过期，请重新填写".into()),
            403 => (
                "auth",
                "令牌权限不足，请重新生成并勾选 projects 和 user_info".into(),
            ),
            404 => ("missing", "找不到对应的仓库或文件".into()),
            409 => ("sha", "远端已经变了，需要先合并再推".into()),
            400 | 422 if lower.contains("sha") || lower.contains("blob") => {
                ("sha", "远端已经变了，需要先合并再推".into())
            }
            400 | 422 if lower.contains("branch") || raw.contains("分支") => {
                ("repo", "分支不存在，请留空以使用仓库的默认分支".into())
            }
            400 | 422
                if lower.contains("exist")
                    || lower.contains("taken")
                    || raw.contains("已存在")
                    || raw.contains("已被使用") =>
            {
                (
                    "name",
                    "这个名字已经有了，但当前令牌看不到它。请换一个带 projects 权限的令牌，或换个仓库名"
                        .into(),
                )
            }
            400 | 422
                if lower.contains("invalid")
                    || raw.contains("仓库名")
                    || raw.contains("只允许")
                    || raw.contains("非法") =>
            {
                ("name", "仓库名不可用，换一个只含字母、数字、- 和 _ 的名字".into())
            }
            400 | 422 if !raw.is_empty() => ("gitee", format!("Gitee 拒绝了这次请求：{raw}")),
            400 | 422 => ("gitee", "Gitee 拒绝了这次请求".into()),
            _ if (500..600).contains(&code) => ("gitee", "Gitee 暂时不可用，稍后重试".into()),
            _ => ("gitee", format!("同步失败（{code}）")),
        };
        Self {
            code,
            kind,
            message,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub private: bool,
    pub created: bool,
}

#[derive(Debug)]
pub struct RemoteFile {
    pub sha: String,
    pub bytes: Vec<u8>,
}

pub struct RemoteEntry {
    pub name: String,
    pub sha: String,
}

pub struct Gitee {
    http: reqwest::blocking::Client,
    token: String,
}

impl Gitee {
    pub fn new(token: &str) -> Result<Self, ApiError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("daodian/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| ApiError::offline())?;
        Ok(Self {
            http,
            token: token.trim().to_string(),
        })
    }

    pub fn login(&self) -> Result<String, ApiError> {
        let value = self.get("/user", &[])?;
        value
            .get("login")
            .and_then(|item| item.as_str())
            .map(str::to_string)
            .ok_or_else(|| ApiError {
                code: 200,
                kind: "auth",
                message: "令牌没有返回登录名，请确认勾选了 user_info".into(),
            })
    }

    pub fn repo(&self, owner: &str, repo: &str) -> Result<RepoInfo, ApiError> {
        let value = self.get(&format!("/repos/{owner}/{repo}"), &[])?;
        Ok(repo_info(&value, false))
    }

    pub fn create_repo(&self, name: &str) -> Result<RepoInfo, ApiError> {
        let value = self.post(
            "/user/repos",
            json!({
                "name": name,
                "private": true,
                "auto_init": true,
                "description": "到点同步",
            }),
        )?;
        Ok(repo_info(&value, true))
    }

    pub fn head(&self, owner: &str, repo: &str, branch: &str) -> Result<String, ApiError> {
        let value = self.get(
            &format!("/repos/{owner}/{repo}/commits"),
            &[("sha", branch), ("per_page", "1")],
        )?;
        value
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("sha"))
            .and_then(|item| item.as_str())
            .map(str::to_string)
            .ok_or_else(|| ApiError {
                code: 404,
                kind: "repo",
                message: "仓库还没有可用的分支".into(),
            })
    }

    pub fn list(&self, owner: &str, repo: &str, path: &str, branch: &str) -> Result<Vec<RemoteEntry>, ApiError> {
        let response = self.send(self.http.get(self.url(&format!("/repos/{owner}/{repo}/contents/{path}"))).query(&[
            ("access_token", self.token.as_str()),
            ("ref", branch),
        ]))?;
        if response.status().as_u16() == 404 {
            return Ok(Vec::new());
        }
        let value = decode_ok(response)?;
        let Some(items) = value.as_array() else {
            return Ok(Vec::new());
        };
        Ok(items
            .iter()
            .filter_map(|item| {
                let name = item.get("name")?.as_str()?.to_string();
                let sha = item.get("sha").and_then(|value| value.as_str()).unwrap_or("").to_string();
                let kind = item.get("type").and_then(|value| value.as_str()).unwrap_or("file");
                if kind == "file" { Some(RemoteEntry { name, sha }) } else { None }
            })
            .collect())
    }

    pub fn read(&self, owner: &str, repo: &str, path: &str, branch: &str) -> Result<Option<RemoteFile>, ApiError> {
        let response = self.send(self.http.get(self.url(&path_of(owner, repo, path))).query(&[
            ("access_token", self.token.as_str()),
            ("ref", branch),
        ]))?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        let value = decode_ok(response)?;
        let sha = value
            .get("sha")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string();
        let encoded = value
            .get("content")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        let compact: String = encoded.chars().filter(|ch| !ch.is_whitespace()).collect();
        let bytes = if compact.is_empty() {
            Vec::new()
        } else {
            base64::engine::general_purpose::STANDARD
                .decode(compact)
                .map_err(|_| ApiError {
                    code: 200,
                    kind: "gitee",
                    message: "远端文件内容读不出来".into(),
                })?
        };
        Ok(Some(RemoteFile { sha, bytes }))
    }

    pub fn write(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        branch: &str,
        sha: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ApiError> {
        let mut body = json!({
            "message": "到点同步",
            "content": base64::engine::general_purpose::STANDARD.encode(bytes),
            "branch": branch,
        });
        let value = if let Some(sha) = sha.filter(|value| !value.is_empty()) {
            body["sha"] = json!(sha);
            self.put(&path_of(owner, repo, path), body)?
        } else {
            // Gitee 新建文件是 POST；对还不存在的文件发 PUT 会返回 400。
            self.post(&path_of(owner, repo, path), body)?
        };
        Ok(commit_sha(&value))
    }

    pub fn remove(&self, owner: &str, repo: &str, path: &str, branch: &str, sha: &str) -> Result<(), ApiError> {
        let mut body = json!({
            "message": "到点同步",
            "sha": sha,
            "branch": branch,
        });
        body["access_token"] = json!(self.token);
        let response = self.send(self.http.delete(self.url(&path_of(owner, repo, path))).json(&body))?;
        if response.status().as_u16() == 404 {
            return Ok(());
        }
        decode_ok(response).map(|_| ())
    }

    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, ApiError> {
        let mut request = self.http.get(self.url(path)).query(&[("access_token", self.token.as_str())]);
        if !query.is_empty() {
            request = request.query(query);
        }
        decode_ok(self.send(request)?)
    }

    fn post(&self, path: &str, mut body: Value) -> Result<Value, ApiError> {
        body["access_token"] = json!(self.token);
        decode_ok(self.send(self.http.post(self.url(path)).json(&body))?)
    }

    fn put(&self, path: &str, mut body: Value) -> Result<Value, ApiError> {
        body["access_token"] = json!(self.token);
        decode_ok(self.send(self.http.put(self.url(path)).json(&body))?)
    }

    fn send(&self, request: reqwest::blocking::RequestBuilder) -> Result<reqwest::blocking::Response, ApiError> {
        request.send().map_err(|err| {
            if err.is_timeout() || err.is_connect() || err.is_request() {
                ApiError::offline()
            } else {
                ApiError {
                    code: 0,
                    kind: "gitee",
                    message: "同步请求没有完成".into(),
                }
            }
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{API}{path}")
    }
}

fn message_text(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return String::new();
    };
    let text = value
        .get("message")
        .map(|item| match item {
            Value::String(text) => text.clone(),
            Value::Array(items) => items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join("；"),
            _ => String::new(),
        })
        .filter(|text| !text.is_empty())
        .or_else(|| {
            value
                .get("error")
                .and_then(|item| item.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let text = text.replace('\n', " ");
    let lower = text.to_lowercase();
    if lower.contains("access_token") {
        return String::new();
    }
    text.chars().take(160).collect()
}

fn commit_sha(value: &Value) -> String {
    value
        .get("commit")
        .and_then(|item| item.get("sha"))
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

fn path_of(owner: &str, repo: &str, path: &str) -> String {
    format!("/repos/{owner}/{repo}/contents/{path}")
}

fn decode_ok(response: reqwest::blocking::Response) -> Result<Value, ApiError> {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::status(status.as_u16(), &body));
    }
    serde_json::from_str(&body).map_err(|_| ApiError {
        code: status.as_u16(),
        kind: "gitee",
        message: "Gitee 返回的内容无法解析".into(),
    })
}

fn repo_info(value: &Value, created: bool) -> RepoInfo {
    let owner = value
        .get("owner")
        .and_then(|item| item.get("login"))
        .and_then(|item| item.as_str())
        .or_else(|| {
            value
                .get("full_name")
                .and_then(|item| item.as_str())
                .and_then(|name| name.split('/').next())
        })
        .unwrap_or("")
        .to_string();
    let repo = value
        .get("name")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let branch = value
        .get("default_branch")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    RepoInfo {
        owner,
        repo,
        branch,
        private: value
            .get("private")
            .and_then(|item| item.as_bool())
            .unwrap_or(false),
        created,
    }
}

#[cfg(test)]
mod tests {
    use super::ApiError;

    #[test]
    fn blob_sha_mismatch_is_retryable() {
        let err = ApiError::status(400, r#"{"message":"Blob SHA does not match"}"#);
        assert_eq!(err.kind, "sha");
    }

    #[test]
    fn generic_bad_request_is_not_blamed_on_the_repo_name() {
        let err = ApiError::status(400, r#"{"message":"400 Bad request"}"#);
        assert_eq!(err.kind, "gitee");
        assert!(!err.message.contains("仓库名"));
    }

    #[test]
    fn name_taken_reads_a_message_array() {
        let err = ApiError::status(422, r#"{"message":["路径已被使用"]}"#);
        assert_eq!(err.kind, "name");
    }
}
