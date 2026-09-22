//! UserPromptSubmit hook handler.

use super::client::HookClient;
use super::{HookInput, HookOutput};

pub async fn handle(input: Option<HookInput>, client: &HookClient) -> HookOutput {
    if let Some(ref inp) = input {
        let session_id = inp.session_id.as_deref().unwrap_or("");
        let cwd = inp.cwd.as_deref().unwrap_or(".");
        if let Err(e) = client.init_session(session_id, cwd).await {
            super::log_error("failed to init session", e);
        }
    }

    HookOutput {
        continue_processing: true,
        suppress_output: Some(true),
        exit_code: 0,
        hook_specific_output: None,
    }
}
