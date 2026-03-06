//! UserPromptSubmit hook handler.

use super::{HookInput, HookOutput};

pub async fn handle(input: Option<HookInput>, worker_url: &str) -> HookOutput {
    if let Some(ref inp) = input {
        let session_id = inp.session_id.as_deref().unwrap_or("");
        let cwd = inp.cwd.as_deref().unwrap_or(".");

        let _ = init_session(worker_url, session_id, cwd).await;
    }

    HookOutput {
        continue_processing: true,
        suppress_output: Some(true),
        exit_code: 0,
        hook_specific_output: None,
    }
}

async fn init_session(
    worker_url: &str,
    session_id: &str,
    cwd: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    client
        .post(format!("{}/api/sessions/init", worker_url))
        .json(&serde_json::json!({
            "contentSessionId": session_id,
            "cwd": cwd,
        }))
        .send()
        .await?;

    Ok(())
}
