//! Lightweight OpenAI-compatible LLM client for summarization.
//!
//! Works with any OpenAI-compat endpoint: Ollama, vLLM, OpenRouter, LiteLLM proxy, Anthropic.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// OpenAI-compatible chat completion client.
pub struct LlmClient {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

impl LlmClient {
    /// Create a new client. Returns None if base_url or model is empty.
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Option<Self> {
        if base_url.is_empty() || model.is_empty() {
            return None;
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: if api_key.is_empty() {
                None
            } else {
                Some(api_key.to_string())
            },
        })
    }

    /// Send a chat completion request and return the assistant's response.
    pub async fn chat(&self, system: &str, user: &str) -> Result<String> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                Message {
                    role: "system".into(),
                    content: system.into(),
                },
                Message {
                    role: "user".into(),
                    content: user.into(),
                },
            ],
            max_tokens: 1024,
        };

        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self.client.post(&url).json(&body);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req
            .send()
            .await
            .context("LLM request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM returned {}: {}", status, text);
        }

        let chat_resp: ChatResponse = resp
            .json()
            .await
            .context("Failed to parse LLM response")?;

        chat_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("LLM returned no choices")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_none_when_empty() {
        assert!(LlmClient::new("", "", "").is_none());
        assert!(LlmClient::new("http://localhost:11434/v1", "", "").is_none());
        assert!(LlmClient::new("", "llama3.2", "").is_none());
    }

    #[test]
    fn test_client_some_when_configured() {
        let client = LlmClient::new("http://localhost:11434/v1", "llama3.2", "");
        assert!(client.is_some());
    }
}
