//! `agnes_generate_image` tool — text-to-image and image-to-image with
//! `agnes-image-2.1-flash`.

#![allow(clippy::missing_docs_in_private_items)]

use crate::tools::agnes_client::{collect_urls, AgnesClient};
use crate::tools::prompt::{enhance_prompt, PromptTarget};
use crate::tools::Tool;
use crate::utils::{derive_filename, resolve_image_input, validate_size};
use async_trait::async_trait;
use rust_mcp_sdk::macros;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;

/// Parameters for the `agnes_generate_image` tool.
#[macros::mcp_tool(
    name = "agnes_generate_image",
    title = "Agnes Generate Image",
    description = "Generate images with the Agnes-Image-2.1-Flash model. Supports text-to-image and image-to-image (transformation/editing). For image-to-image, pass one or more reference images as `image_urls`: each entry may be an http(s) URL, a local file path, a `data:` URI, or raw base64 text (local files and base64 are encoded as `data:` URIs and sent inline). Returns generated image URLs. Use a rich prompt: [Subject] + [Scene/Environment] + [Style] + [Lighting] + [Composition] + [Quality]. Optional enhance_prompt: when true, expand the prompt into a rich, detailed prompt before generation. INCREASES LATENCY by one extra chat-model call; leave false if your prompt is already detailed. On failure, falls back to the original prompt. Optional save_to: a local directory or file path; when set, the generated image(s) are downloaded there.",
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

    /// Optional reference image(s) for image-to-image generation. Each entry
    /// may be an http(s) URL, a local file path, a `data:` URI, or raw base64
    /// text. Local files and base64 inputs are encoded as `data:` URIs and
    /// sent inline (no public hosting required).
    #[serde(default, rename = "image_urls")]
    pub image_urls: Option<Vec<String>>,

    /// When true, expand `prompt` into a rich, detailed prompt via the Agnes
    /// chat model before generation. INCREASES TOTAL LATENCY by one extra
    /// chat-model round trip (~1-5s). Leave false if your prompt is already
    /// detailed. On enhancement failure, generation falls back to the
    /// original prompt. Defaults to false.
    #[serde(default, rename = "enhance_prompt")]
    pub enhance_prompt: bool,

    /// Optional local path to download the generated image(s) to. If a single
    /// image is returned and the path is an existing directory, the file is
    /// written inside it; otherwise the path is treated as a target file.
    /// For multiple images, the path must be a directory (created if absent).
    /// When unset (default), only image URLs are returned.
    #[serde(default, rename = "save_to")]
    pub save_to: Option<String>,
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

/// Resolve `image_urls` entries into Agnes-API-acceptable form.
///
/// Each entry may be an http(s) URL (passed through unchanged), a local
/// file path, a `data:` URI, or raw base64 text; the latter three are
/// normalized to `data:` URIs so the API can receive them inline.
fn resolve_image_inputs(image_urls: Option<&[String]>) -> Result<Vec<String>, CallToolError> {
    let raw = image_urls.unwrap_or_default();
    let mut out = Vec::with_capacity(raw.len());
    for src in raw {
        let resolved = resolve_image_input(src).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_generate_image",
                Some(format!("invalid image input '{src}': {e}")),
            )
        })?;
        out.push(resolved);
    }
    Ok(out)
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

        // Optional prompt enhancement. On failure, fall back to the original
        // prompt and append a warning to the output (D3/D4 contract).
        let mut effective_prompt = params.prompt.clone();
        let mut enhanced_note: Option<String> = None;
        let mut warning: Option<String> = None;
        if params.enhance_prompt {
            match enhance_prompt(&self.client, &params.prompt, PromptTarget::Image).await {
                Ok(enhanced) => {
                    enhanced_note = Some(enhanced.clone());
                    effective_prompt = enhanced;
                }
                Err(e) => {
                    warning = Some(format!(
                        "[prompt enhancement failed: {e}], using original prompt"
                    ));
                }
            }
        }

        // Resolve each reference image into something the Agnes API accepts:
        // http(s) URLs pass through unchanged; local file paths, raw base64,
        // and `data:` URIs are normalized to `data:` URIs and sent inline.
        let image_urls = resolve_image_inputs(params.image_urls.as_deref())?;

        let mut extra_body = serde_json::Map::new();
        extra_body.insert(
            "response_format".to_string(),
            serde_json::Value::String("url".to_string()),
        );

        let mut body = serde_json::json!({
            "model": self.client.image_model(),
            "prompt": effective_prompt,
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
            {
                let mut out = format!(
                    "Image generated ({mode}), but no downloadable URL was returned.\n\nRaw response:\n{}",
                    serde_json::to_string_pretty(&response).unwrap_or_default()
                );
                if let Some(note) = &enhanced_note {
                    let _ = writeln!(out, "\nEnhanced prompt: {note}");
                }
                if let Some(w) = &warning {
                    out.push_str(w);
                    out.push('\n');
                }
                out
            }
        } else {
            let mut out = format!("Image generated ({mode}):\n");
            for url in &urls {
                out.push_str("- ");
                out.push_str(url);
                out.push('\n');
            }
            if let Some(note) = &enhanced_note {
                let _ = writeln!(out, "Enhanced prompt: {note}");
            }
            if let Some(w) = &warning {
                out.push_str(w);
                out.push('\n');
            }
            if let Some(save_to) = &params.save_to {
                let report = save_images(&self.client, &urls, save_to).await;
                let _ = writeln!(out, "{report}");
            }
            out
        };

        Ok(CallToolResult::text_content(vec![content.into()]))
    }
}

/// Download each URL into `save_to` (a directory, or a single file path).
///
/// Returns a single-line report: either `Saved to: <paths>` or, on failure,
/// `[download failed: <reason>]`.
async fn save_images(client: &AgnesClient, urls: &[String], save_to: &str) -> String {
    if urls.is_empty() {
        return String::new();
    }
    let dest = PathBuf::from(save_to);
    let is_dir_target = dest.is_dir() || urls.len() > 1 || save_to.ends_with('/');

    let mut saved: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (i, url) in urls.iter().enumerate() {
        let target = if is_dir_target {
            let filename = derive_filename(url, i, None, "image", "png");
            dest.join(filename)
        } else {
            dest.clone()
        };
        match client.download_url(url, &target).await {
            Ok(abs) => saved.push(abs.display().to_string()),
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }

    if saved.is_empty() && !errors.is_empty() {
        format!("[download failed: {}]", errors.join("; "))
    } else if errors.is_empty() {
        format!("Saved to: {}", saved.join(", "))
    } else {
        format!(
            "Saved to: {}; [partial failures: {}]",
            saved.join(", "),
            errors.join("; ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_image_inputs_passes_through_https_urls() {
        let inputs = vec![
            "https://example.com/a.png".to_string(),
            "http://example.com/b.jpg".to_string(),
        ];
        let resolved = resolve_image_inputs(Some(&inputs)).expect("resolve");
        assert_eq!(resolved, inputs);
    }

    #[test]
    fn resolve_image_inputs_passes_through_data_uri() {
        let inputs = vec!["data:image/png;base64,AAAA".to_string()];
        let resolved = resolve_image_inputs(Some(&inputs)).expect("resolve");
        assert_eq!(resolved, inputs);
    }

    #[test]
    fn resolve_image_inputs_wraps_raw_base64() {
        let inputs = vec!["AAAA".to_string()];
        let resolved = resolve_image_inputs(Some(&inputs)).expect("resolve");
        assert_eq!(resolved, vec!["data:image/png;base64,AAAA".to_string()]);
    }

    #[test]
    fn resolve_image_inputs_reads_local_file_as_data_uri() {
        use std::io::Write;
        let mut tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("tempfile");
        // Write a tiny valid byte sequence; content does not need to be a real PNG.
        tmp.write_all(&[0x89, 0x50, 0x4E, 0x47]).expect("write");
        let path = tmp.path().to_string_lossy().to_string();

        let resolved = resolve_image_inputs(Some(&[path])).expect("resolve");
        assert_eq!(resolved.len(), 1);
        assert!(
            resolved[0].starts_with("data:image/png;base64,"),
            "expected data URI, got {}",
            resolved[0]
        );
    }

    #[test]
    fn resolve_image_inputs_handles_none_and_empty() {
        assert!(resolve_image_inputs(None).expect("resolve").is_empty());
        assert!(resolve_image_inputs(Some(&[])).expect("resolve").is_empty());
    }
}
