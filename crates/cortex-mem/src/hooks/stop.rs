//! Stop hook handler.

use super::client::HookClient;
use super::{HookInput, HookOutput};

pub async fn handle(input: Option<HookInput>, client: &HookClient) -> HookOutput {
    let session_id = input
        .as_ref()
        .and_then(|i| i.session_id.as_deref())
        .unwrap_or("");

    if let Err(e) = client.summarize(session_id).await {
        super::log_error("failed to summarize session", e);
    }
    if let Err(e) = client.complete(session_id).await {
        super::log_error("failed to complete session", e);
    }

    HookOutput {
        continue_processing: true,
        suppress_output: Some(true),
        exit_code: 0,
        hook_specific_output: None,
    }
}
