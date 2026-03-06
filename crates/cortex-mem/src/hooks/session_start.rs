//! SessionStart hook handler.
//!
//! Called when a Claude Code session starts. Fetches context from the worker
//! and returns it as additionalContext.

use super::{HookInput, HookOutput, HookSpecificOutput};

/// Handle SessionStart event.
///
/// Fetches context from the worker daemon and returns it to Claude Code.
pub async fn handle(input: Option<HookInput>, worker_url: &str) -> HookOutput {
    let cwd = input.as_ref().and_then(|i| i.cwd.as_deref()).unwrap_or(".");

    // Try to fetch context from worker
    let context = match fetch_context(worker_url, cwd).await {
        Ok(ctx) if !ctx.is_empty() => Some(ctx),
        Ok(_) => None,
        Err(e) => {
            eprintln!("cortex-mem: failed to fetch context: {}", e);
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

async fn fetch_context(worker_url: &str, cwd: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Extract project name from cwd
    let project = std::path::Path::new(cwd)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");

    let resp = client
        .get(format!("{}/api/context/inject", worker_url))
        .query(&[("projects", project)])
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(format!("Worker returned {}", resp.status()).into());
    }

    let body: serde_json::Value = resp.json().await?;
    Ok(body
        .get("context")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}
