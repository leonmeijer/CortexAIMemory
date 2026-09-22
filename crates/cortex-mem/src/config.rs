//! Configuration for cortex-mem worker and hooks.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the cortex-mem worker daemon and Claude Code hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemConfig {
    /// Worker HTTP port (default: 19090)
    pub worker_port: u16,
    /// Worker bind host (default: 127.0.0.1)
    pub worker_host: String,
    /// Full base URL. When set, overrides `http://{host}:{port}`.
    /// Example: `https://devtest.using.indentia.ai/cortex`
    pub worker_url: String,
    /// Optional Bearer token (CORTEX_API_KEY / CLAUDE_MEM_AUTH_TOKEN).
    pub auth_token: String,
    /// Optional `indentia_session_0` cookie for the using-zone Kong gate.
    pub auth_cookie: String,
    /// Tenant slug sent as `X-Tenant-ID` / `X-Tenant-Handle`.
    pub tenant_id: String,
    /// API dialect: `auto`, `worker`, or `claude-memory`.
    pub api_style: String,

    /// SurrealDB connection URL
    pub surrealdb_url: String,
    /// SurrealDB namespace
    pub surrealdb_namespace: String,
    /// SurrealDB database
    pub surrealdb_database: String,
    /// SurrealDB username
    pub surrealdb_username: String,
    /// SurrealDB password
    pub surrealdb_password: String,

    /// Number of recent observations to include in context injection
    pub context_observations: usize,
    /// Number of recent sessions to show in context
    pub context_session_count: usize,
    /// Show last session summary in context
    pub context_show_last_summary: bool,

    /// Tools to skip when capturing observations
    pub skip_tools: Vec<String>,
    /// Excluded project paths
    pub excluded_projects: Vec<String>,

    /// Data directory (default: ~/.claude-mem)
    pub data_dir: PathBuf,
    /// Log level
    pub log_level: String,

    /// LLM base URL for summarization (OpenAI-compatible, e.g. "http://localhost:11434/v1")
    /// Empty string = disabled (rule-based fallback only)
    pub llm_base_url: String,
    /// LLM model name (e.g. "llama3.2", "claude-haiku-4-5-20251001")
    pub llm_model: String,
    /// LLM API key (optional, not needed for Ollama)
    pub llm_api_key: String,

    /// Max tokens for context injection (approximate: 4 chars = 1 token)
    pub context_max_tokens: usize,
}

impl Default for MemConfig {
    fn default() -> Self {
        Self {
            worker_port: 19090,
            worker_host: "127.0.0.1".into(),
            worker_url: String::new(),
            auth_token: String::new(),
            auth_cookie: String::new(),
            tenant_id: String::new(),
            api_style: "auto".into(),
            surrealdb_url: "ws://localhost:8000".into(),
            surrealdb_namespace: "cortex".into(),
            surrealdb_database: "memory".into(),
            surrealdb_username: "root".into(),
            surrealdb_password: "root".into(),
            context_observations: 50,
            context_session_count: 10,
            context_show_last_summary: true,
            skip_tools: vec![
                "ListMcpResourcesTool".into(),
                "SlashCommand".into(),
                "Skill".into(),
                "TodoWrite".into(),
                "AskUserQuestion".into(),
                "ToolSearch".into(),
                "TaskCreate".into(),
                "TaskUpdate".into(),
                "TaskGet".into(),
                "TaskList".into(),
                "TaskOutput".into(),
                "TaskStop".into(),
                "ExitPlanMode".into(),
                "EnterPlanMode".into(),
                "EnterWorktree".into(),
                "CronCreate".into(),
                "CronDelete".into(),
                "CronList".into(),
                "LSP".into(),
            ],
            excluded_projects: vec![],
            data_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".claude-mem"),
            log_level: "INFO".into(),
            llm_base_url: String::new(),
            llm_model: String::new(),
            llm_api_key: String::new(),
            context_max_tokens: 4000,
        }
    }
}

impl MemConfig {
    /// Load config from settings.json file, env vars override.
    pub fn load() -> Self {
        let mut config = Self::default();

        // Try loading from ~/.claude-mem/settings.json
        let settings_path = config.data_dir.join("settings.json");
        if let Ok(contents) = std::fs::read_to_string(&settings_path) {
            if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&contents) {
                apply_settings_object(&mut config, &settings);
            }
        }

        apply_env_overrides(&mut config);
        config.auth_cookie = resolve_auth_cookie(&config);
        config
    }

    /// Worker / remote Cortex base URL, without a trailing slash.
    pub fn worker_url(&self) -> String {
        let explicit = self.worker_url.trim().trim_end_matches('/');
        if !explicit.is_empty() {
            return explicit.to_string();
        }
        format!("http://{}:{}", self.worker_host, self.worker_port)
    }

    /// Whether hooks should speak the deployed Cortex `/api/v1/claude-memory` contract.
    pub fn uses_claude_memory_api(&self) -> bool {
        match self.api_style.trim().to_ascii_lowercase().as_str() {
            "claude-memory" | "cortex" | "remote" => true,
            "worker" | "local" => false,
            _ => {
                let url = self.worker_url();
                url.contains("using.indentia.ai")
                    || url.contains("/cortex")
                    || url.contains("claude-memory")
            }
        }
    }

    pub fn skip_tool(&self, tool_name: &str) -> bool {
        self.skip_tools.iter().any(|t| t == tool_name)
    }
}

fn apply_settings_object(config: &mut MemConfig, settings: &serde_json::Value) {
    if let Some(port) = json_string(settings.get("CLAUDE_MEM_WORKER_PORT")) {
        if let Ok(p) = port.parse() {
            config.worker_port = p;
        }
    }
    if let Some(host) = json_string(settings.get("CLAUDE_MEM_WORKER_HOST")) {
        config.worker_host = host;
    }
    if let Some(url) = json_string(settings.get("CLAUDE_MEM_WORKER_URL")) {
        config.worker_url = url;
    }
    if let Some(token) = json_string(settings.get("CLAUDE_MEM_AUTH_TOKEN"))
        .or_else(|| json_string(settings.get("CORTEX_API_KEY")))
    {
        config.auth_token = token;
    }
    if let Some(cookie) = json_string(settings.get("CLAUDE_MEM_AUTH_COOKIE")) {
        config.auth_cookie = cookie;
    }
    if let Some(tenant) = json_string(settings.get("CLAUDE_MEM_TENANT"))
        .or_else(|| json_string(settings.get("X_TENANT_ID")))
    {
        config.tenant_id = tenant;
    }
    if let Some(style) = json_string(settings.get("CLAUDE_MEM_API")) {
        config.api_style = style;
    }
    if let Some(obs) = json_string(settings.get("CLAUDE_MEM_CONTEXT_OBSERVATIONS")) {
        if let Ok(n) = obs.parse() {
            config.context_observations = n;
        }
    }
    if let Some(skip) = json_string(settings.get("CLAUDE_MEM_SKIP_TOOLS")) {
        config.skip_tools = skip.split(',').map(|s| s.trim().to_string()).collect();
    }
    if let Some(dir) = json_string(settings.get("CLAUDE_MEM_DATA_DIR")) {
        config.data_dir = PathBuf::from(shellexpand(&dir));
    }
}

fn apply_env_overrides(config: &mut MemConfig) {
    if let Ok(port) = std::env::var("CLAUDE_MEM_WORKER_PORT") {
        if let Ok(p) = port.parse() {
            config.worker_port = p;
        }
    }
    if let Ok(host) = std::env::var("CLAUDE_MEM_WORKER_HOST") {
        config.worker_host = host;
    }
    if let Ok(url) = std::env::var("CLAUDE_MEM_WORKER_URL") {
        if !url.trim().is_empty() {
            config.worker_url = url;
        }
    }
    if let Ok(token) = first_nonempty_env(&["CLAUDE_MEM_AUTH_TOKEN", "CORTEX_API_KEY"]) {
        config.auth_token = token;
    }
    if let Ok(cookie) = std::env::var("CLAUDE_MEM_AUTH_COOKIE") {
        if !cookie.trim().is_empty() {
            config.auth_cookie = cookie;
        }
    }
    if let Ok(tenant) = first_nonempty_env(&["CLAUDE_MEM_TENANT", "X_TENANT_ID"]) {
        config.tenant_id = tenant;
    }
    if let Ok(style) = std::env::var("CLAUDE_MEM_API") {
        if !style.trim().is_empty() {
            config.api_style = style;
        }
    }
    if let Ok(url) = std::env::var("SURREALDB_URL") {
        config.surrealdb_url = url;
    }
    if let Ok(ns) = std::env::var("SURREALDB_NAMESPACE") {
        config.surrealdb_namespace = ns;
    }
    if let Ok(db) = std::env::var("SURREALDB_DATABASE") {
        config.surrealdb_database = db;
    }
    if let Ok(user) = std::env::var("SURREALDB_USERNAME") {
        config.surrealdb_username = user;
    }
    if let Ok(pass) = std::env::var("SURREALDB_PASSWORD") {
        config.surrealdb_password = pass;
    }
    if let Ok(url) = std::env::var("CORTEX_MEM_LLM_BASE_URL") {
        config.llm_base_url = url;
    }
    if let Ok(model) = std::env::var("CORTEX_MEM_LLM_MODEL") {
        config.llm_model = model;
    }
    if let Ok(key) = std::env::var("CORTEX_MEM_LLM_API_KEY") {
        config.llm_api_key = key;
    }
    if let Ok(tokens) = std::env::var("CORTEX_MEM_CONTEXT_MAX_TOKENS") {
        if let Ok(n) = tokens.parse() {
            config.context_max_tokens = n;
        }
    }
}

fn resolve_auth_cookie(config: &MemConfig) -> String {
    let explicit = config.auth_cookie.trim();
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    let cookie_path = config.data_dir.join("session.cookie");
    if let Ok(contents) = std::fs::read_to_string(&cookie_path) {
        let trimmed = contents.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    String::new()
}

fn first_nonempty_env(keys: &[&str]) -> Result<String, std::env::VarError> {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return Ok(value);
            }
        }
    }
    Err(std::env::VarError::NotPresent)
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn shellexpand(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{}", home.display(), &path[1..]);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_hook_env() {
        for key in [
            "CLAUDE_MEM_WORKER_URL",
            "CLAUDE_MEM_WORKER_HOST",
            "CLAUDE_MEM_WORKER_PORT",
            "CLAUDE_MEM_AUTH_TOKEN",
            "CLAUDE_MEM_AUTH_COOKIE",
            "CLAUDE_MEM_TENANT",
            "CLAUDE_MEM_API",
            "CORTEX_API_KEY",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn default_worker_url_is_localhost() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_hook_env();
        let cfg = MemConfig::default();
        assert_eq!(cfg.worker_url(), "http://127.0.0.1:19090");
        assert!(!cfg.uses_claude_memory_api());
    }

    #[test]
    fn full_url_overrides_host_port_and_strips_slash() {
        let mut cfg = MemConfig::default();
        cfg.worker_url = "https://devtest.using.indentia.ai/cortex/".into();
        assert_eq!(
            cfg.worker_url(),
            "https://devtest.using.indentia.ai/cortex"
        );
        assert!(cfg.uses_claude_memory_api());
    }

    #[test]
    fn explicit_worker_style_wins_over_url_heuristic() {
        let mut cfg = MemConfig::default();
        cfg.worker_url = "https://devtest.using.indentia.ai/cortex".into();
        cfg.api_style = "worker".into();
        assert!(!cfg.uses_claude_memory_api());
        cfg.api_style = "claude-memory".into();
        cfg.worker_url = "http://127.0.0.1:7337".into();
        assert!(cfg.uses_claude_memory_api());
    }

    #[test]
    fn env_worker_url_overrides_default() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_hook_env();
        std::env::set_var(
            "CLAUDE_MEM_WORKER_URL",
            "https://devtest.using.indentia.ai/cortex",
        );
        std::env::set_var("CLAUDE_MEM_TENANT", "devtest");
        std::env::set_var("CLAUDE_MEM_API", "claude-memory");
        let cfg = {
            let mut c = MemConfig::default();
            apply_env_overrides(&mut c);
            c
        };
        clear_hook_env();
        assert_eq!(
            cfg.worker_url(),
            "https://devtest.using.indentia.ai/cortex"
        );
        assert_eq!(cfg.tenant_id, "devtest");
        assert!(cfg.uses_claude_memory_api());
    }
}
