//! Hook dispatch module.
//!
//! The `cortex-mem-hook` binary reads JSON from stdin (provided by Claude Code),
//! determines the event type from the CLI subcommand, and dispatches to the
//! appropriate handler.

pub mod post_tool_use;
pub mod prompt_submit;
pub mod session_start;
pub mod stop;

use serde::{Deserialize, Serialize};

/// Hook output returned to Claude Code via stdout.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    /// Whether to continue processing
    #[serde(rename = "continue")]
    pub continue_processing: bool,
    /// Suppress the hook's output in the conversation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    /// Exit code for the hook
    pub exit_code: i32,
    /// Hook-specific output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

/// Hook-specific output for SessionStart events.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    pub hook_event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

/// Generic hook input from stdin (Claude Code sends snake_case).
#[derive(Debug, Deserialize)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_response: Option<serde_json::Value>,
    #[serde(default)]
    pub user_prompt: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
}

/// Read and parse hook input from stdin.
pub fn read_stdin() -> Option<HookInput> {
    let mut input = String::new();
    if std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).is_ok() && !input.is_empty()
    {
        serde_json::from_str(&input).ok()
    } else {
        None
    }
}

/// Get the worker URL from config.
pub fn worker_url() -> String {
    let config = crate::config::MemConfig::load();
    config.worker_url()
}
