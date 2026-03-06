//! PostToolUse hook handler.

use super::{HookInput, HookOutput};

pub async fn handle(input: Option<HookInput>, worker_url: &str) -> HookOutput {
    if let Some(ref inp) = input {
        let _ = send_observation(worker_url, inp).await;
    }

    HookOutput {
        continue_processing: true,
        suppress_output: Some(true),
        exit_code: 0,
        hook_specific_output: None,
    }
}

async fn send_observation(
    worker_url: &str,
    input: &HookInput,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    client
        .post(format!("{}/api/sessions/observations", worker_url))
        .json(&serde_json::json!({
            "content_session_id": input.session_id.as_deref().unwrap_or(""),
            "tool_name": input.tool_name.as_deref().unwrap_or(""),
            "tool_input": input.tool_input.clone().unwrap_or(serde_json::Value::Null),
            "tool_response": input.tool_response.clone().unwrap_or(serde_json::Value::Null),
            "cwd": input.cwd.as_deref().unwrap_or("."),
        }))
        .send()
        .await?;

    Ok(())
}
