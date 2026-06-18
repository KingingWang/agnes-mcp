//! `agnes_generate_image` tool — text-to-image and image-to-image with
//! `agnes-image-2.1-flash`.

#![allow(clippy::missing_docs_in_private_items)]

use crate::tools::agnes_client::{collect_urls, AgnesClient};
use crate::tools::Tool;
use crate::utils::validate_size;
use async_trait::async_trait;
use rust_mcp_sdk::macros;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Parameters for the `agnes_generate_image` tool.
#[macros::mcp_tool(
    name = "agnes_generate_image",
    title = "Agnes Generate Image",
    description = "Generate images with the Agnes-Image-2.1-Flash model. Supports text-to-image and image-to-image (transformation/editing) by passing one or more reference image URLs. Returns generated image URLs. Use a rich prompt: [Subject] + [Scene/Environment] + [Style] + [Lighting] + [Composition] + [Quality].",
    destructive_hint = false,
    idempotent_hint = false,
    open_world_hint = true,
    read_only_hint = false
)]
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct GenerateImageToolParams {
    /// Text instruction for generation or editing. Describe what to create, or for image-to-image what should change and what should stay the same.
    #[serde(rename = "prompt")]
    pub prompt: String,

    /// Output size as WIDTHxHEIGHT in pixels (e.g. \"1024x768\"). Defaults to \"1024x768\".
    #[serde(default = "default_size", rename = "size")]
    pub size: String,

    /// Optional reference image URL(s) for image-to-image generation. Publicly URL-accessible.
    #[serde(default, rename = "image_urls")]
    pub image_urls: Option<Vec<String>>,
}

fn default_size() -> String {
    "1024x768".to_string()
}

/// `agnes_generate_image` tool implementation.
pub struct GenerateImageTool {
    client: Arc<AgnesClient>,
}

impl GenerateImageTool {
    /// Create a new image generation tool.
    #[must_use]
    pub fn new(client: Arc<AgnesClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for GenerateImageTool {
    fn definition(&self) -> McpTool {
        GenerateImageToolParams::tool()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let params: GenerateImageToolParams = serde_json::from_value(arguments).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_generate_image",
                Some(format!("invalid arguments: {e}")),
            )
        })?;

        validate_size(&params.size).map_err(|e| {
            CallToolError::invalid_arguments("agnes_generate_image", Some(e.to_string()))
        })?;

        let image_urls = params.image_urls.unwrap_or_default();

        let mut extra_body = serde_json::Map::new();
        extra_body.insert(
            "response_format".to_string(),
            serde_json::Value::String("url".to_string()),
        );

        let mut body = serde_json::json!({
            "model": AgnesClient::image_model(),
            "prompt": params.prompt,
            "size": params.size,
        });

        if !image_urls.is_empty() {
            extra_body.insert(
                "image".to_string(),
                serde_json::Value::Array(
                    image_urls
                        .iter()
                        .map(|u| serde_json::Value::String(u.clone()))
                        .collect(),
                ),
            );
        }
        body["extra_body"] = serde_json::Value::Object(extra_body);

        let response = self.client.images_generations(&body).await.map_err(|e| {
            CallToolError::from_message(format!("Agnes image generation failed: {e}"))
        })?;

        let urls = collect_urls(&response);
        let mode = if image_urls.is_empty() {
            "text-to-image"
        } else {
            "image-to-image"
        };

        let content = if urls.is_empty() {
            format!(
                "Image generated ({mode}), but no downloadable URL was returned.\n\nRaw response:\n{}",
                serde_json::to_string_pretty(&response).unwrap_or_default()
            )
        } else {
            let mut out = format!("Image generated ({mode}):\n");
            for url in &urls {
                out.push_str("- ");
                out.push_str(url);
                out.push('\n');
            }
            out
        };

        Ok(CallToolResult::text_content(vec![content.into()]))
    }
}
