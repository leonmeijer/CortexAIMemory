//! HTTP client used by `cortex-mem-hook`.
//!
//! Talks either to the local `cortex-mem` worker or to a remote Cortex
//! instance's `/api/v1/claude-memory` surface (the contract behind
//! `https://<tenant>.using.indentia.ai/cortex`).

use super::HookInput;
use crate::config::MemConfig;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    Worker,
    ClaudeMemory,
}

#[derive(Debug, Clone)]
pub struct HookClient {
    http: Client,
    base_url: String,
    auth_token: String,
    auth_cookie: String,
    tenant_id: String,
    style: ApiStyle,
    skip_tools: Vec<String>,
}

impl HookClient {
    pub fn load() -> Self {
        Self::from_config(MemConfig::load())
    }

    pub fn from_config(config: MemConfig) -> Self {
        let style = if config.uses_claude_memory_api() {
            ApiStyle::ClaudeMemory
        } else {
            ApiStyle::Worker
        };
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url: config.worker_url(),
            auth_token: config.auth_token.trim().to_string(),
            auth_cookie: config.auth_cookie.trim().to_string(),
            tenant_id: config.tenant_id.trim().to_string(),
            style,
            skip_tools: config.skip_tools,
        }
    }

    pub fn style(&self) -> ApiStyle {
        self.style
    }

    pub fn skip_tool(&self, tool_name: &str) -> bool {
        self.skip_tools.iter().any(|t| t == tool_name)
    }

    pub async fn fetch_context(&self, cwd: &str) -> Result<String, Box<dyn std::error::Error>> {
        let project = match self.resolve_project(cwd).await {
            Some(slug) => slug,
            None => dir_name(cwd),
        };
        let (path, query): (&str, Vec<(&str, &str)>) = match self.style {
            ApiStyle::ClaudeMemory => (
                "/api/v1/claude-memory/context",
                vec![("cwd", cwd)],
            ),
            ApiStyle::Worker => ("/api/context/inject", vec![("projects", project.as_str())]),
        };
        let resp = self
            .get(path)
            .query(&query)
            .timeout(Duration::from_secs(4))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(format!("Worker returned {}", resp.status()).into());
        }
        let body: Value = resp.json().await?;
        Ok(body
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    pub async fn resolve_project(&self, cwd: &str) -> Option<String> {
        match self.style {
            ApiStyle::ClaudeMemory => {
                let resp = self
                    .post("/api/v1/claude-memory/projects/upsert")
                    .json(&json!({ "cwd": cwd }))
                    .timeout(Duration::from_secs(4))
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let body: Value = resp.json().await.ok()?;
                body.get("project_slug")
                    .or_else(|| body.get("projectSlug"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| Some(dir_name(cwd)))
            }
            ApiStyle::Worker => {
                let resp = self
                    .post("/api/sessions/resolve-project")
                    .json(&json!({ "cwd": cwd }))
                    .timeout(Duration::from_secs(4))
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let body: Value = resp.json().await.ok()?;
                body.get("projectSlug")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
        }
    }

    pub async fn init_session(
        &self,
        session_id: &str,
        cwd: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (path, body) = match self.style {
            ApiStyle::ClaudeMemory => (
                "/api/v1/claude-memory/sessions/init",
                json!({
                    "content_session_id": session_id,
                    "cwd": cwd,
                }),
            ),
            ApiStyle::Worker => (
                "/api/sessions/init",
                json!({
                    "contentSessionId": session_id,
                    "cwd": cwd,
                }),
            ),
        };
        self.send_checked(
            self.post(path)
                .json(&body)
                .timeout(Duration::from_secs(5)),
        )
        .await
    }

    pub async fn send_observation(
        &self,
        input: &HookInput,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");
        if self.skip_tool(tool_name) {
            return Ok(());
        }
        let path = match self.style {
            ApiStyle::ClaudeMemory => "/api/v1/claude-memory/sessions/observations",
            ApiStyle::Worker => "/api/sessions/observations",
        };
        let body = json!({
            "content_session_id": input.session_id.as_deref().unwrap_or(""),
            "contentSessionId": input.session_id.as_deref().unwrap_or(""),
            "tool_name": tool_name,
            "tool_input": input.tool_input.clone().unwrap_or(Value::Null),
            "tool_response": input.tool_response.clone().unwrap_or(Value::Null),
            "cwd": input.cwd.as_deref().unwrap_or("."),
        });
        self.send_checked(
            self.post(path)
                .json(&body)
                .timeout(Duration::from_secs(8)),
        )
        .await
    }

    pub async fn summarize(&self, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (path, body) = match self.style {
            ApiStyle::ClaudeMemory => (
                "/api/v1/claude-memory/sessions/summarize",
                json!({ "content_session_id": session_id }),
            ),
            ApiStyle::Worker => (
                "/api/sessions/summarize",
                json!({ "contentSessionId": session_id }),
            ),
        };
        self.send_checked(
            self.post(path)
                .json(&body)
                .timeout(Duration::from_secs(8)),
        )
        .await
    }

    pub async fn complete(&self, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (path, body) = match self.style {
            ApiStyle::ClaudeMemory => (
                "/api/v1/claude-memory/sessions/complete",
                json!({ "content_session_id": session_id }),
            ),
            ApiStyle::Worker => (
                "/api/sessions/complete",
                json!({ "contentSessionId": session_id }),
            ),
        };
        self.send_checked(
            self.post(path)
                .json(&body)
                .timeout(Duration::from_secs(5)),
        )
        .await
    }

    async fn send_checked(
        &self,
        req: RequestBuilder,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let resp = req.send().await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if is_session_expired(status, &body) {
            return Err(
                "using-zone session expired; run python3 scripts/indentia-session-cookie.py > ~/.claude-mem/session.cookie"
                    .into(),
            );
        }
        Err(format!("Worker returned {status}").into())
    }

    fn get(&self, path: &str) -> RequestBuilder {
        self.apply_auth(self.http.get(self.url(path)))
    }

    fn post(&self, path: &str) -> RequestBuilder {
        self.apply_auth(self.http.post(self.url(path)))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn apply_auth(&self, mut req: RequestBuilder) -> RequestBuilder {
        if !self.auth_token.is_empty() {
            req = req.bearer_auth(&self.auth_token);
        }
        if !self.auth_cookie.is_empty() {
            req = req.header(
                "Cookie",
                cookie_header(&self.auth_cookie),
            );
        }
        if !self.tenant_id.is_empty() {
            req = req
                .header("X-Tenant-ID", &self.tenant_id)
                .header("X-Tenant-Handle", &self.tenant_id);
        }
        req.header("Accept", "application/json")
    }
}

fn dir_name(cwd: &str) -> String {
    std::path::Path::new(cwd)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_string()
}

fn cookie_header(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains('=') {
        trimmed.to_string()
    } else {
        format!("indentia_session_0={trimmed}")
    }
}

/// True when a response is the Kong using-zone login wall.
pub fn is_session_expired(status: StatusCode, body: &Value) -> bool {
    status == StatusCode::UNAUTHORIZED
        && body
            .get("detail")
            .and_then(|v| v.as_str())
            .is_some_and(|d| d.contains("session expired"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_header_prefixes_bare_sid() {
        assert_eq!(
            cookie_header("abc-123"),
            "indentia_session_0=abc-123"
        );
        assert_eq!(
            cookie_header("indentia_session_0=abc-123"),
            "indentia_session_0=abc-123"
        );
    }

    #[test]
    fn claude_memory_style_from_using_zone_url() {
        let mut cfg = MemConfig::default();
        cfg.worker_url = "https://devtest.using.indentia.ai/cortex".into();
        let client = HookClient::from_config(cfg);
        assert_eq!(client.style(), ApiStyle::ClaudeMemory);
        assert_eq!(
            client.url("/api/v1/claude-memory/health"),
            "https://devtest.using.indentia.ai/cortex/api/v1/claude-memory/health"
        );
    }
}
