//! `agnes_generate_video` and `agnes_video_status` tools — asynchronous video
//! generation with `agnes-video-v2.0`.

#![allow(clippy::missing_docs_in_private_items)]

use crate::tools::agnes_client::{
    extract_task_id, extract_task_status, extract_video_url, AgnesClient, TaskStatus,
};
use crate::tools::prompt::{enhance_prompt, PromptTarget};
use crate::tools::Tool;
use crate::utils::validate_num_frames;
use async_trait::async_trait;
use rust_mcp_sdk::macros;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// agnes_generate_video
// ============================================================================

/// Parameters for the `agnes_generate_video` tool.
#[macros::mcp_tool(
    name = "agnes_generate_video",
    title = "Agnes Generate Video",
    description = "Generate video with the Agnes-Video-V2.0 model. Supports text-to-video, image-to-video (single image), multi-image, and keyframe animation. Generation is asynchronous: by default it returns a task id immediately; set wait=true to poll until completion and return the video URL. Optional enhance_prompt: when true, expand the prompt into a rich, detailed prompt before generation. INCREASES LATENCY by one extra chat-model call; leave false if your prompt is already detailed. On failure, falls back to the original prompt. Optional save_to: download the video to this local path once the task completes (only with wait=true; use agnes_video_status with save_to otherwise).",
    destructive_hint = false,
    idempotent_hint = false,
    open_world_hint = true,
    read_only_hint = false
)]
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct GenerateVideoToolParams {
    /// Video description: subject, action, scene, camera movement, lighting, style.
    #[serde(rename = "prompt")]
    pub prompt: String,

    /// Input image URL(s): one URL for image-to-video, multiple for multi-image or keyframe animation.
    #[serde(default, rename = "image_urls")]
    pub image_urls: Option<Vec<String>>,

    /// Generation mode: \"ti2vid\" (text/image-to-video), \"keyframes\", or \"multi-image\".
    #[serde(default, rename = "mode")]
    pub mode: Option<String>,

    /// Output width in pixels. Defaults to 1152.
    #[serde(default = "default_width", rename = "width")]
    pub width: u64,

    /// Output height in pixels. Defaults to 768.
    #[serde(default = "default_height", rename = "height")]
    pub height: u64,

    /// Number of frames. Must be <= 441 and satisfy 8n+1 (e.g. 81, 121, 161, 241, 441). Defaults to 121.
    #[serde(default = "default_num_frames", rename = "num_frames")]
    pub num_frames: i64,

    /// Frame rate (1–60). Defaults to 24.
    #[serde(default = "default_frame_rate", rename = "frame_rate")]
    pub frame_rate: f64,

    /// Optional negative prompt (what to avoid).
    #[serde(default, rename = "negative_prompt")]
    pub negative_prompt: Option<String>,

    /// Optional seed for reproducible results.
    #[serde(default, rename = "seed")]
    pub seed: Option<i64>,

    /// Optional number of inference steps.
    #[serde(default, rename = "num_inference_steps")]
    pub num_inference_steps: Option<i64>,

    /// If true, poll until the task completes (or fails/times out) and return the video URL. Defaults to false (return task id immediately).
    #[serde(default, rename = "wait")]
    pub wait: bool,

    /// When true, expand `prompt` into a rich, detailed prompt via the Agnes
    /// chat model before creating the video task. INCREASES TOTAL LATENCY by
    /// one extra chat-model round trip (~1-5s). Leave false if your prompt is
    /// already detailed. On enhancement failure, falls back to the original
    /// prompt. Defaults to false.
    #[serde(default, rename = "enhance_prompt")]
    pub enhance_prompt: bool,

    /// Optional local path to download the generated video to once the task
    /// completes. Only honored when `wait=true` (otherwise there is no video
    /// URL yet). Use `agnes_video_status` with `save_to` to download after
    /// polling. Defaults to no download.
    #[serde(default, rename = "save_to")]
    pub save_to: Option<String>,
}

fn default_width() -> u64 {
    1152
}
fn default_height() -> u64 {
    768
}
fn default_num_frames() -> i64 {
    121
}
fn default_frame_rate() -> f64 {
    24.0
}

/// `agnes_generate_video` tool implementation.
pub struct GenerateVideoTool {
    client: Arc<AgnesClient>,
}

impl GenerateVideoTool {
    /// Create a new video generation tool.
    #[must_use]
    pub fn new(client: Arc<AgnesClient>) -> Self {
        Self { client }
    }

    /// Build the request body from validated parameters.
    ///
    /// Callers must validate `num_frames` via [`crate::utils::validate_num_frames`]
    /// before calling this.
    fn build_body(
        params: &GenerateVideoToolParams,
        video_model: &str,
        prompt: &str,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": video_model,
            "prompt": prompt,
            "width": params.width,
            "height": params.height,
            "num_frames": params.num_frames,
            "frame_rate": params.frame_rate,
        });

        if let Some(neg) = &params.negative_prompt {
            body["negative_prompt"] = serde_json::Value::String(neg.clone());
        }
        if let Some(seed) = params.seed {
            body["seed"] = serde_json::json!(seed);
        }
        if let Some(steps) = params.num_inference_steps {
            body["num_inference_steps"] = serde_json::json!(steps);
        }

        let image_urls = params.image_urls.clone().unwrap_or_default();
        let mode = params.mode.as_deref();

        if image_urls.len() == 1 && !matches!(mode, Some("keyframes" | "multi-image")) {
            body["image"] = serde_json::Value::String(image_urls[0].clone());
        } else if !image_urls.is_empty() {
            let mut extra = serde_json::Map::new();
            extra.insert(
                "image".to_string(),
                serde_json::Value::Array(
                    image_urls
                        .iter()
                        .map(|u| serde_json::Value::String(u.clone()))
                        .collect(),
                ),
            );
            if mode == Some("keyframes") {
                extra.insert(
                    "mode".to_string(),
                    serde_json::Value::String("keyframes".to_string()),
                );
            }
            body["extra_body"] = serde_json::Value::Object(extra);
        }

        if let Some(m) = mode {
            if !matches!(m, "keyframes" | "multi-image") {
                body["mode"] = serde_json::Value::String(m.to_string());
            }
        }

        body
    }
}

#[async_trait]
impl Tool for GenerateVideoTool {
    fn definition(&self) -> McpTool {
        GenerateVideoToolParams::tool()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let params: GenerateVideoToolParams = serde_json::from_value(arguments).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_generate_video",
                Some(format!("invalid arguments: {e}")),
            )
        })?;

        validate_num_frames(params.num_frames).map_err(|e| {
            CallToolError::invalid_arguments("agnes_generate_video", Some(e.to_string()))
        })?;

        if let Some(mode) = &params.mode {
            if !matches!(mode.as_str(), "ti2vid" | "keyframes" | "multi-image") {
                return Err(CallToolError::invalid_arguments(
                    "agnes_generate_video",
                    Some(format!(
                        "invalid mode '{mode}'. Expected one of: ti2vid, keyframes, multi-image"
                    )),
                ));
            }
        }

        // Optional prompt enhancement (D3/D4 contract).
        let mut effective_prompt = params.prompt.clone();
        let mut enhanced_note: Option<String> = None;
        let mut warning: Option<String> = None;
        if params.enhance_prompt {
            match enhance_prompt(&self.client, &params.prompt, PromptTarget::Video).await {
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

        let body = Self::build_body(&params, self.client.video_model(), &effective_prompt);

        let (response, key_idx) = self.client.create_video_task(&body).await.map_err(|e| {
            CallToolError::from_message(format!("Agnes video creation failed: {e}"))
        })?;

        let Some(task_id) = extract_task_id(&response) else {
            let mut msg = format!(
                "Video task submitted, but no task id was found in the response.\n\n{}",
                serde_json::to_string_pretty(&response).unwrap_or_default()
            );
            if let Some(note) = &enhanced_note {
                let _ = write!(msg, "\nEnhanced prompt: {note}");
            }
            if let Some(w) = &warning {
                msg.push('\n');
                msg.push_str(w);
            }
            return Ok(CallToolResult::text_content(vec![msg.into()]));
        };

        // Pin this task to the key that created it, so all subsequent status
        // queries (via agnes_video_status or internal polling) reuse the same
        // key. Agnes ties task ownership to the creating key.
        self.client.record_task_key(&task_id, key_idx);

        let image_count = params.image_urls.as_ref().map_or(0, Vec::len);
        let mode_label = match (params.mode.as_deref(), image_count) {
            (Some(m), _) => m,
            (None, 1) => "image-to-video",
            (None, n) if n > 1 => "multi-image",
            (None, _) => "text-to-video",
        };

        if !params.wait {
            let mut msg = format!(
                "Video task created ({mode_label}).\nTask ID: {task_id}\n\nUse the `agnes_video_status` tool with this task id (and wait=true to poll) to retrieve the result.",
            );
            if let Some(note) = &enhanced_note {
                let _ = write!(msg, "\nEnhanced prompt: {note}");
            }
            if let Some(w) = &warning {
                msg.push('\n');
                msg.push_str(w);
            }
            return Ok(CallToolResult::text_content(vec![msg.into()]));
        }

        // Poll until completion.
        let final_response = poll_task(
            &self.client,
            &task_id,
            self.client.poll_interval(),
            self.client.poll_timeout(),
        )
        .await
        .map_err(|e| CallToolError::from_message(format!("polling task {task_id} failed: {e}")))?;

        let status = extract_task_status(&final_response);
        let video_url = extract_video_url(&final_response);
        let mut content =
            render_task_result(&final_response, &task_id, status, video_url.as_deref());
        if let Some(note) = &enhanced_note {
            let _ = writeln!(content, "Enhanced prompt: {note}");
        }
        if let Some(w) = &warning {
            content.push_str(w);
            content.push('\n');
        }
        if let (Some(save_to), Some(url)) = (&params.save_to, video_url.as_deref()) {
            if status == TaskStatus::Done {
                let report = save_video(&self.client, url, save_to).await;
                let _ = writeln!(content, "{report}");
            }
        }

        Ok(CallToolResult::text_content(vec![content.into()]))
    }
}

// ============================================================================
// agnes_video_status
// ============================================================================

/// Parameters for the `agnes_video_status` tool.
#[macros::mcp_tool(
    name = "agnes_video_status",
    title = "Agnes Video Status",
    description = "Retrieve the status (and result, when complete) of an Agnes video generation task by its task id. Set wait=true to poll until the task completes, fails, or times out. Optional save_to: download the video to this local path once the task is complete.",
    destructive_hint = false,
    idempotent_hint = true,
    open_world_hint = true,
    read_only_hint = true
)]
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct VideoStatusToolParams {
    /// The video task id returned by `agnes_generate_video`.
    #[serde(rename = "task_id")]
    pub task_id: String,

    /// If true, poll until the task completes, fails, or times out. Defaults to false (single status check).
    #[serde(default, rename = "wait")]
    pub wait: bool,

    /// Optional local path to download the video to once the task is complete.
    /// Ignored if the task has not finished or has no video URL. Defaults to
    /// no download.
    #[serde(default, rename = "save_to")]
    pub save_to: Option<String>,
}

/// `agnes_video_status` tool implementation.
pub struct VideoStatusTool {
    client: Arc<AgnesClient>,
}

impl VideoStatusTool {
    /// Create a new video status tool.
    #[must_use]
    pub fn new(client: Arc<AgnesClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for VideoStatusTool {
    fn definition(&self) -> McpTool {
        VideoStatusToolParams::tool()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let params: VideoStatusToolParams = serde_json::from_value(arguments).map_err(|e| {
            CallToolError::invalid_arguments(
                "agnes_video_status",
                Some(format!("invalid arguments: {e}")),
            )
        })?;

        let response = if params.wait {
            poll_task(
                &self.client,
                &params.task_id,
                self.client.poll_interval(),
                self.client.poll_timeout(),
            )
            .await
            .map_err(|e| {
                CallToolError::from_message(format!(
                    "polling task {} failed: {}",
                    params.task_id, e
                ))
            })?
        } else {
            self.client
                .get_video_task(&params.task_id)
                .await
                .map_err(|e| {
                    CallToolError::from_message(format!(
                        "retrieving task {} failed: {}",
                        params.task_id, e
                    ))
                })?
        };

        let status = extract_task_status(&response);
        let video_url = extract_video_url(&response);
        let mut content =
            render_task_result(&response, &params.task_id, status, video_url.as_deref());

        if let (Some(save_to), Some(url)) = (&params.save_to, video_url.as_deref()) {
            if status == TaskStatus::Done {
                let report = save_video(&self.client, url, save_to).await;
                let _ = writeln!(content, "{report}");
            }
        }

        Ok(CallToolResult::text_content(vec![content.into()]))
    }
}

// ============================================================================
// Shared helpers
// ============================================================================

/// Poll a video task until it reaches a terminal state or times out.
///
/// # Errors
///
/// Returns an error if the task fails or the timeout elapses.
pub async fn poll_task(
    client: &AgnesClient,
    task_id: &str,
    interval: Duration,
    timeout: Duration,
) -> crate::error::Result<serde_json::Value> {
    let deadline = Instant::now() + timeout;

    loop {
        let response = client.get_video_task(task_id).await?;
        let status = extract_task_status(&response);
        let progress = response
            .get("progress")
            .or_else(|| response.get("data").and_then(|d| d.get("progress")))
            .and_then(serde_json::Value::as_f64);

        tracing::info!(
            "video task {task_id}: status={:?} progress={progress:?}",
            status
        );

        match status {
            TaskStatus::Done => return Ok(response),
            TaskStatus::Failed => {
                let raw = response
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("failed");
                return Err(crate::error::Error::api(format!(
                    "video task {task_id} failed with status '{raw}'"
                )));
            }
            TaskStatus::Running | TaskStatus::Unknown => {}
        }

        if Instant::now() >= deadline {
            return Err(crate::error::Error::api(format!(
                "timed out waiting for video task {task_id}"
            )));
        }
        tokio::time::sleep(interval).await;
    }
}

/// Render a human-readable summary of a video task response.
#[allow(clippy::too_many_lines)]
fn render_task_result(
    response: &serde_json::Value,
    task_id: &str,
    status: TaskStatus,
    video_url: Option<&str>,
) -> String {
    let status_label = match status {
        TaskStatus::Running => "running",
        TaskStatus::Done => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Unknown => "unknown",
    };

    let mut out = format!("Task {task_id}: {status_label}\n");
    if let Some(url) = video_url {
        let _ = writeln!(out, "Video URL: {url}");
    }
    if let Some(progress) = response
        .get("progress")
        .or_else(|| response.get("data").and_then(|d| d.get("progress")))
        .and_then(serde_json::Value::as_f64)
    {
        let _ = writeln!(out, "Progress: {progress}");
    }
    if let Some(seconds) = response
        .get("seconds")
        .or_else(|| response.get("data").and_then(|d| d.get("seconds")))
        .and_then(serde_json::Value::as_f64)
    {
        let _ = writeln!(out, "Duration: {seconds}s");
    }
    out
}

/// Download a video URL to `save_to` (a file path or directory).
async fn save_video(client: &AgnesClient, url: &str, save_to: &str) -> String {
    use crate::utils::derive_filename;
    use std::path::PathBuf;

    let dest = PathBuf::from(save_to);
    let target = if dest.is_dir() {
        let filename = derive_filename(url, 0, None, "video", "mp4");
        dest.join(filename)
    } else {
        dest
    };
    match client.download_url(url, &target).await {
        Ok(abs) => format!("Saved to: {}", abs.display()),
        Err(e) => format!("[download failed: {e}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_body_text_to_video() {
        let params = GenerateVideoToolParams {
            prompt: "a cat".to_string(),
            image_urls: None,
            mode: None,
            width: 1152,
            height: 768,
            num_frames: 121,
            frame_rate: 24.0,
            negative_prompt: None,
            seed: None,
            num_inference_steps: None,
            wait: false,
            enhance_prompt: false,
            save_to: None,
        };
        let body = GenerateVideoTool::build_body(&params, "test-video-model", "a cat");
        assert_eq!(body["model"], "test-video-model");
        assert_eq!(body["prompt"], "a cat");
        assert!(body.get("image").is_none());
        assert!(body.get("extra_body").is_none());
    }

    #[test]
    fn build_body_image_to_video_single() {
        let params = GenerateVideoToolParams {
            prompt: "animate".to_string(),
            image_urls: Some(vec!["https://example.com/i.png".to_string()]),
            mode: None,
            width: 1152,
            height: 768,
            num_frames: 121,
            frame_rate: 24.0,
            negative_prompt: None,
            seed: None,
            num_inference_steps: None,
            wait: false,
            enhance_prompt: false,
            save_to: None,
        };
        let body = GenerateVideoTool::build_body(&params, "test-video-model", "a cat");
        assert_eq!(body["image"], "https://example.com/i.png");
    }

    #[test]
    fn build_body_keyframes() {
        let params = GenerateVideoToolParams {
            prompt: "transition".to_string(),
            image_urls: Some(vec![
                "https://example.com/k1.png".to_string(),
                "https://example.com/k2.png".to_string(),
            ]),
            mode: Some("keyframes".to_string()),
            width: 1152,
            height: 768,
            num_frames: 121,
            frame_rate: 24.0,
            negative_prompt: None,
            seed: None,
            num_inference_steps: None,
            wait: false,
            enhance_prompt: false,
            save_to: None,
        };
        let body = GenerateVideoTool::build_body(&params, "test-video-model", "a cat");
        assert_eq!(body["extra_body"]["mode"], "keyframes");
        assert_eq!(body["extra_body"]["image"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn render_completed_task() {
        let resp = serde_json::json!({"status": "completed", "video_url": "https://example.com/v.mp4", "seconds": 5.0});
        let content = render_task_result(
            &resp,
            "task_1",
            TaskStatus::Done,
            Some("https://example.com/v.mp4"),
        );
        assert!(content.contains("completed"));
        assert!(content.contains("https://example.com/v.mp4"));
    }
}
