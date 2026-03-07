//! Configuration for cortex-mem worker and hooks.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the cortex-mem worker daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemConfig {
    /// Worker HTTP port (default: 37777)
    pub worker_port: u16,
    /// Worker bind host (default: 127.0.0.1)
    pub worker_host: String,

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
                if let Some(port) = settings
                    .get("CLAUDE_MEM_WORKER_PORT")
                    .and_then(|v| v.as_str())
                {
                    if let Ok(p) = port.parse() {
                        config.worker_port = p;
                    }
                }
                if let Some(host) = settings
                    .get("CLAUDE_MEM_WORKER_HOST")
                    .and_then(|v| v.as_str())
                {
                    config.worker_host = host.to_string();
                }
                if let Some(obs) = settings
                    .get("CLAUDE_MEM_CONTEXT_OBSERVATIONS")
                    .and_then(|v| v.as_str())
                {
                    if let Ok(n) = obs.parse() {
                        config.context_observations = n;
                    }
                }
                if let Some(skip) = settings
                    .get("CLAUDE_MEM_SKIP_TOOLS")
                    .and_then(|v| v.as_str())
                {
                    config.skip_tools = skip.split(',').map(|s| s.trim().to_string()).collect();
                }
                if let Some(dir) = settings.get("CLAUDE_MEM_DATA_DIR").and_then(|v| v.as_str()) {
                    config.data_dir = PathBuf::from(shellexpand(dir));
                }
            }
        }

        // Env var overrides
        if let Ok(port) = std::env::var("CLAUDE_MEM_WORKER_PORT") {
            if let Ok(p) = port.parse() {
                config.worker_port = p;
            }
        }
        if let Ok(host) = std::env::var("CLAUDE_MEM_WORKER_HOST") {
            config.worker_host = host;
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

        config
    }

    /// Worker base URL
    pub fn worker_url(&self) -> String {
        format!("http://{}:{}", self.worker_host, self.worker_port)
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
