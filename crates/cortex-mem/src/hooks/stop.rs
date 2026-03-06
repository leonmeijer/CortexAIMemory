//! Stop hook handler.

use super::{HookInput, HookOutput};

pub async fn handle(input: Option<HookInput>, worker_url: &str) -> HookOutput {
    let session_id = input
        .as_ref()
        .and_then(|i| i.session_id.as_deref())
        .unwrap_or("");

    // Phase 1: Summarize
    let _ = summarize(worker_url, session_id).await;
    // Phase 2: Complete
    let _ = complete(worker_url, session_id).await;

    HookOutput {
        continue_processing: true,
        suppress_output: Some(true),
        exit_code: 0,
        hook_specific_output: None,
    }
}

async fn summarize(worker_url: &str, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    client
        .post(format!("{}/api/sessions/summarize", worker_url))
        .json(&serde_json::json!({ "contentSessionId": session_id }))
        .send()
        .await?;

    Ok(())
}

async fn complete(worker_url: &str, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    client
        .post(format!("{}/api/sessions/complete", worker_url))
        .json(&serde_json::json!({ "contentSessionId": session_id }))
        .send()
        .await?;

    Ok(())
}
