//! Prompt enhancement helper — expands a simple prompt into a rich, detailed
//! prompt suitable for image or video generation.
//!
//! This is an internal helper used by `agnes_generate_image` and
//! `agnes_generate_video` when their `enhance_prompt` parameter is set. It is
//! NOT exposed as a standalone MCP tool.

#![allow(clippy::missing_docs_in_private_items)]

use crate::error::Result;
use crate::tools::agnes_client::{extract_chat_text, AgnesClient, ChatMessage, ChatRequest};

/// Target modality for prompt enhancement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptTarget {
    /// Image generation prompt.
    Image,
    /// Video generation prompt.
    Video,
}

/// Expand a simple prompt into a rich, detailed generation prompt.
///
/// Uses the Agnes chat model to add subject, scene, style, lighting,
/// composition, and quality details.
///
/// # Errors
///
/// Returns an error if the chat completion request fails or returns an
/// unexpected response.
pub(crate) async fn enhance_prompt(
    client: &AgnesClient,
    prompt: &str,
    target: PromptTarget,
) -> Result<String> {
    let system = build_system_prompt(target);
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: serde_json::Value::String(system),
        },
        ChatMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(format!(
                "Expand this {target} prompt into a single rich, detailed generation prompt. \
                 Output ONLY the prompt text, no preamble:\n\n{prompt}",
            )),
        },
    ];

    let request = ChatRequest {
        model: client.text_model().to_string(),
        messages,
        temperature: Some(0.8),
        top_p: None,
        max_tokens: Some(512),
    };
    let body = serde_json::to_value(&request)?;

    let response = client.chat_completions(&body).await?;

    let text = extract_chat_text(&response).map_or_else(
        || serde_json::to_string_pretty(&response).unwrap_or_default(),
        |t| t.trim().to_string(),
    );

    Ok(text)
}

impl PromptTarget {
    /// Lowercase label used in the user-facing instruction.
    fn label(self) -> &'static str {
        match self {
            PromptTarget::Image => "image",
            PromptTarget::Video => "video",
        }
    }
}

/// Render the target label for use in `format!`.
impl std::fmt::Display for PromptTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Build the system prompt that guides the chat model to expand prompts.
fn build_system_prompt(target: PromptTarget) -> String {
    match target {
        PromptTarget::Video => concat!(
            "You are an expert AI video-generation prompt engineer. ",
            "Given a simple idea, produce ONE vivid, detailed video prompt covering: ",
            "subject, action, scene/environment, camera movement, lighting, mood, and style. ",
            "Keep it under 120 words. Output only the prompt."
        )
        .to_string(),
        PromptTarget::Image => concat!(
            "You are an expert AI image-generation prompt engineer. ",
            "Given a simple idea, produce ONE vivid, detailed image prompt covering: ",
            "subject, scene/environment, style, lighting, composition, and quality requirements. ",
            "Keep it under 100 words. Output only the prompt."
        )
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_image_mentions_key_aspects() {
        let s = build_system_prompt(PromptTarget::Image);
        assert!(s.contains("image-generation"));
        assert!(s.contains("lighting"));
        assert!(s.contains("composition"));
    }

    #[test]
    fn system_prompt_video_mentions_key_aspects() {
        let s = build_system_prompt(PromptTarget::Video);
        assert!(s.contains("video-generation"));
        assert!(s.contains("camera movement"));
        assert!(s.contains("lighting"));
    }

    #[test]
    fn target_label_roundtrip() {
        assert_eq!(PromptTarget::Image.to_string(), "image");
        assert_eq!(PromptTarget::Video.to_string(), "video");
    }
}
