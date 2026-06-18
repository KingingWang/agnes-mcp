//! `agnes_chat` tool — chat completions with `agnes-2.0-flash`.

#![allow(clippy::missing_docs_in_private_items)]

use crate::tools::agnes_client::{
    extract_chat_text, extract_usage_summary, ChatMessage, ChatRequest,
};
use crate::tools::Tool;
use async_trait::async_trait;
use rust_mcp_sdk::macros;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::agnes_client::AgnesClient;

/// Parameters for the `agnes_chat` tool.
#[macros::mcp_tool(
    name = "agnes_chat",
    title = "Agnes Chat",
    description = "Generate text with the Agnes-2.0-Flash chat model (OpenAI-compatible). Use for general text generation, answering questions, summarizing, coding help, and multi-turn conversations. Supports an optional system prompt and conversation history.",
    destructive_hint = false,
    idempotent_hint = false,
    open_world_hint = true,
    read_only_hint = true
)]
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct ChatToolParams {
    /// The user message / prompt to send.
    #[serde(rename = "prompt")]
    pub prompt: String,

    /// Optional system prompt to guide model behavior.
    #[serde(default, rename = "system")]
    pub system: Option<String>,

    /// Optional prior conversation turns as `[{\"role\": \"user|assistant|system\", \"content\": \"...\"}, ...]`. The `prompt` is appended as the final user message.
    #[serde(default, rename = "history")]
    pub history: Option<Vec<HistoryMessage>>,

    /// Sampling temperature (0.0–2.0). Higher is more random.
    #[serde(default, rename = "temperature")]
    pub temperature: Option<f64>,

    /// Nucleus sampling probability mass.
    #[serde(default, rename = "top_p")]
    pub top_p: Option<f64>,

    /// Maximum number of tokens to generate.
    #[serde(default, rename = "max_tokens")]
    pub max_tokens: Option<u64>,
}

/// A single message in optional conversation history.
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct HistoryMessage {
    /// Message role: `system`, `user`, or `assistant`.
    pub role: String,
    /// Message content.
    pub content: String,
}

/// `agnes_chat` tool implementation.
pub struct ChatTool {
    client: Arc<AgnesClient>,
}

impl ChatTool {
    /// Create a new chat tool backed by the given client.
    #[must_use]
    pub fn new(client: Arc<AgnesClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ChatTool {
    fn definition(&self) -> McpTool {
        ChatToolParams::tool()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let params: ChatToolParams = serde_json::from_value(arguments).map_err(|e| {
            CallToolError::invalid_arguments("agnes_chat", Some(format!("invalid arguments: {e}")))
        })?;

        if let Some(t) = params.temperature {
            if !(0.0..=2.0).contains(&t) {
                return Err(CallToolError::invalid_arguments(
                    "agnes_chat",
                    Some(format!("temperature must be in [0.0, 2.0], got {t}")),
                ));
            }
        }

        let mut messages: Vec<ChatMessage> = Vec::new();
        if let Some(system) = &params.system {
            if !system.trim().is_empty() {
                messages.push(text_message("system", system));
            }
        }
        if let Some(history) = &params.history {
            for m in history {
                let role = match m.role.as_str() {
                    "system" | "user" | "assistant" => m.role.clone(),
                    other => {
                        return Err(CallToolError::invalid_arguments(
                            "agnes_chat",
                            Some(format!("invalid history role: {other}")),
                        ));
                    }
                };
                messages.push(text_message(&role, &m.content));
            }
        }
        messages.push(text_message("user", &params.prompt));

        let request = ChatRequest {
            model: self.client.text_model().to_string(),
            messages,
            temperature: params.temperature,
            top_p: params.top_p,
            max_tokens: params.max_tokens,
        };
        let body = serde_json::to_value(&request).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_chat",
                Some(format!("serialization error: {e}")),
            )
        })?;

        let response = self
            .client
            .chat_completions(&body)
            .await
            .map_err(|e| CallToolError::from_message(format!("Agnes chat failed: {e}")))?;

        let text = extract_chat_text(&response)
            .unwrap_or_else(|| serde_json::to_string_pretty(&response).unwrap_or_default());

        let content = match extract_usage_summary(&response) {
            Some(usage) => format!("{text}\n\n---\n_usage: {usage}_"),
            None => text,
        };

        Ok(CallToolResult::text_content(vec![content.into()]))
    }
}

/// Build a simple text chat message.
fn text_message(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.to_string(),
        content: serde_json::Value::String(content.to_string()),
    }
}

#[cfg(test)]
mod tests {
    /// Document the accepted temperature range. The actual `execute` requires a
    /// live client; here we just assert the range constants used by validation.
    #[test]
    fn temperature_range_is_0_to_2() {
        // valid boundary values
        for t in [0.0_f64, 0.5, 1.0, 1.5, 2.0] {
            assert!((0.0..=2.0).contains(&t), "{t} should be valid");
        }
        // invalid values
        for t in [-0.1_f64, 2.1, 3.0, -1.0] {
            assert!(!(0.0..=2.0).contains(&t), "{t} should be invalid");
        }
    }
}
