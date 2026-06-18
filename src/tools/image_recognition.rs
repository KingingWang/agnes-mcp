//! `agnes_image_recognition` tool — multimodal image understanding via the
//! OpenAI-compatible chat endpoint with vision content.

#![allow(clippy::missing_docs_in_private_items)]

use crate::tools::agnes_client::{extract_chat_text, ChatMessage, ChatRequest};
use crate::tools::{agnes_client::AgnesClient, Tool};
use crate::utils::resolve_image_input;
use async_trait::async_trait;
use rust_mcp_sdk::macros;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Parameters for the `agnes_image_recognition` tool.
#[macros::mcp_tool(
    name = "agnes_image_recognition",
    title = "Agnes Image Recognition",
    description = "Recognize, describe, analyze, and answer questions about images using the Agnes-2.0-Flash vision model. Pass an image (URL, local file path, or base64 data) and a question/instruction. Returns a textual description or answer.",
    destructive_hint = false,
    idempotent_hint = true,
    open_world_hint = true,
    read_only_hint = true
)]
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct ImageRecognitionToolParams {
    /// The image to analyze: an http(s) URL, a local file path, a `data:` URI, or raw base64 text.
    #[serde(rename = "image")]
    pub image: String,

    /// The question or instruction about the image (e.g. \"Describe this image in detail\", \"What objects are present?\", \"Extract all text\").
    #[serde(rename = "prompt")]
    pub prompt: String,

    /// Optional system prompt (e.g. \"You are an expert image analyst.\").
    #[serde(default, rename = "system")]
    pub system: Option<String>,

    /// Optional detail level for vision processing: `low`, `high`, or `auto`.
    #[serde(default, rename = "detail")]
    pub detail: Option<String>,
}

/// `agnes_image_recognition` tool implementation.
pub struct ImageRecognitionTool {
    client: Arc<AgnesClient>,
}

impl ImageRecognitionTool {
    /// Create a new image recognition tool.
    #[must_use]
    pub fn new(client: Arc<AgnesClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ImageRecognitionTool {
    fn definition(&self) -> McpTool {
        ImageRecognitionToolParams::tool()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let params: ImageRecognitionToolParams =
            serde_json::from_value(arguments).map_err(|e| {
                CallToolError::invalid_arguments(
                    "agnes_image_recognition",
                    Some(format!("invalid arguments: {e}")),
                )
            })?;

        let image_url = resolve_image_input(&params.image).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_image_recognition",
                Some(format!("could not load image: {e}")),
            )
        })?;

        let mut messages: Vec<ChatMessage> = Vec::new();
        if let Some(system) = &params.system {
            if !system.trim().is_empty() {
                messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: serde_json::Value::String(system.clone()),
                });
            }
        }

        let detail = params
            .detail
            .as_deref()
            .map(|d| match d.to_ascii_lowercase().as_str() {
                "low" | "high" | "auto" => d.to_ascii_lowercase(),
                other => other.to_string(),
            });

        let user_content = build_vision_content(&params.prompt, &image_url, detail);
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_content,
        });

        let request = ChatRequest {
            model: AgnesClient::text_model().to_string(),
            messages,
            temperature: None,
            top_p: None,
            max_tokens: None,
        };
        let body = serde_json::to_value(&request).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_image_recognition",
                Some(format!("serialization error: {e}")),
            )
        })?;

        let response = self.client.chat_completions(&body).await.map_err(|e| {
            CallToolError::from_message(format!("Agnes image recognition failed: {e}"))
        })?;

        let text = extract_chat_text(&response)
            .unwrap_or_else(|| serde_json::to_string_pretty(&response).unwrap_or_default());

        Ok(CallToolResult::text_content(vec![text.into()]))
    }
}

/// Build the OpenAI vision-style content array (a text part + an image_url part).
fn build_vision_content(
    prompt: &str,
    image_url: &str,
    detail: Option<String>,
) -> serde_json::Value {
    let mut image_obj = serde_json::Map::new();
    image_obj.insert(
        "url".to_string(),
        serde_json::Value::String(image_url.to_string()),
    );
    if let Some(detail) = detail {
        image_obj.insert("detail".to_string(), serde_json::Value::String(detail));
    }

    serde_json::json!([
        { "type": "text", "text": prompt },
        { "type": "image_url", "image_url": serde_json::Value::Object(image_obj) }
    ])
}
