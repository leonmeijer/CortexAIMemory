//! SessionStart hook handler.
//!
//! Called when a Claude Code session starts. Fetches context from the worker
//! (or remote Cortex) and returns it as additionalContext.

use super::client::HookClient;
use super::{HookInput, HookOutput, HookSpecificOutput};

/// Handle SessionStart event.
pub async fn handle(input: Option<HookInput>, client: &HookClient) -> HookOutput {
    let cwd = input.as_ref().and_then(|i| i.cwd.as_deref()).unwrap_or(".");

    let context = match client.fetch_context(cwd).await {
        Ok(ctx) if !ctx.is_empty() => Some(ctx),
        Ok(_) => None,
        Err(e) => {
            super::log_error("failed to fetch context", e);
            None
        }
    };

    HookOutput {
        continue_processing: true,
        suppress_output: Some(true),
        exit_code: 0,
        hook_specific_output: context.map(|ctx| HookSpecificOutput {
            hook_event_name: "SessionStart".to_string(),
            additional_context: Some(ctx),
        }),
    }
}
