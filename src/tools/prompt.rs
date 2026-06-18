//! `agnes_enhance_prompt` tool — expand a simple prompt into a rich, detailed
//! prompt suitable for image/video generation.

#![allow(clippy::missing_docs_in_private_items)]

use crate::tools::agnes_client::{extract_chat_text, ChatMessage, ChatRequest};
use crate::tools::{agnes_client::AgnesClient, Tool};
use async_trait::async_trait;
use rust_mcp_sdk::macros;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Parameters for the `agnes_enhance_prompt` tool.
#[macros::mcp_tool(
    name = "agnes_enhance_prompt",
    title = "Agnes Enhance Prompt",
    description = "Expand a short, simple prompt into a rich, detailed prompt optimized for Agnes image or video generation. Uses the Agnes chat model to add subject, scene, style, lighting, composition, and quality details. Target: \"image\" or \"video\".",
    destructive_hint = false,
    idempotent_hint = false,
    open_world_hint = true,
    read_only_hint = true
)]
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct EnhancePromptToolParams {
    /// The simple prompt to expand (e.g. \"a cat on the beach\").
    #[serde(rename = "prompt")]
    pub prompt: String,

    /// Target modality: \"image\" or \"video\". Defaults to \"image\".
    #[serde(default = "default_target", rename = "target")]
    pub target: String,
}

fn default_target() -> String {
    "image".to_string()
}

/// `agnes_enhance_prompt` tool implementation.
pub struct EnhancePromptTool {
    client: Arc<AgnesClient>,
}

impl EnhancePromptTool {
    /// Create a new prompt-enhancement tool.
    #[must_use]
    pub fn new(client: Arc<AgnesClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for EnhancePromptTool {
    fn definition(&self) -> McpTool {
        EnhancePromptToolParams::tool()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let params: EnhancePromptToolParams = serde_json::from_value(arguments).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_enhance_prompt",
                Some(format!("invalid arguments: {e}")),
            )
        })?;

        let target = params.target.trim().to_ascii_lowercase();
        if target != "image" && target != "video" {
            return Err(CallToolError::invalid_arguments(
                "agnes_enhance_prompt",
                Some(format!(
                    "target must be 'image' or 'video', got: {}",
                    params.target
                )),
            ));
        }

        let system = build_system_prompt(&target);
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: serde_json::Value::String(system),
            },
            ChatMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(format!(
                    "Expand this {target} prompt into a single rich, detailed generation prompt. Output ONLY the prompt text, no preamble:\n\n{}",
                    params.prompt
                )),
            },
        ];

        let request = ChatRequest {
            model: AgnesClient::text_model().to_string(),
            messages,
            temperature: Some(0.8),
            top_p: None,
            max_tokens: Some(512),
        };
        let body = serde_json::to_value(&request).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_enhance_prompt",
                Some(format!("serialization error: {e}")),
            )
        })?;

        let response = self.client.chat_completions(&body).await.map_err(|e| {
            CallToolError::from_message(format!("Agnes prompt enhancement failed: {e}"))
        })?;

        let text = extract_chat_text(&response).map_or_else(
            || serde_json::to_string_pretty(&response).unwrap_or_default(),
            |t| t.trim().to_string(),
        );

        Ok(CallToolResult::text_content(vec![text.into()]))
    }
}

/// Build the system prompt that guides the chat model to expand prompts.
fn build_system_prompt(target: &str) -> String {
    match target {
        "video" => concat!(
            "You are an expert AI video-generation prompt engineer. ",
            "Given a simple idea, produce ONE vivid, detailed video prompt covering: ",
            "subject, action, scene/environment, camera movement, lighting, mood, and style. ",
            "Keep it under 120 words. Output only the prompt."
        )
        .to_string(),
        _ => concat!(
            "You are an expert AI image-generation prompt engineer. ",
            "Given a simple idea, produce ONE vivid, detailed image prompt covering: ",
            "subject, scene/environment, style, lighting, composition, and quality requirements. ",
            "Keep it under 100 words. Output only the prompt."
        )
        .to_string(),
    }
}
