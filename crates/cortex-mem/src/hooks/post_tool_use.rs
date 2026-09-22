//! PostToolUse hook handler.

use super::client::HookClient;
use super::{HookInput, HookOutput};

pub async fn handle(input: Option<HookInput>, client: &HookClient) -> HookOutput {
    if let Some(ref inp) = input {
        if let Err(e) = client.send_observation(inp).await {
            super::log_error("failed to store observation", e);
        }
    }

    HookOutput {
        continue_processing: true,
        suppress_output: Some(true),
        exit_code: 0,
        hook_specific_output: None,
    }
}
