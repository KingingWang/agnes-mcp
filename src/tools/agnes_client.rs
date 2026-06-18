//! HTTP client for the Agnes AI API.
//!
//! Wraps [`reqwest`] with the Agnes base URL, API key, and timeouts, and
//! normalizes error responses returned by the API.

use crate::config::AgnesConfig;
use crate::error::{Error, Result};
use reqwest::{Client, Method};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Asynchronous client for the Agnes AI API.
#[derive(Clone)]
pub struct AgnesClient {
    base_url: String,
    api_key: String,
    http: Client,
    poll_interval: Duration,
    poll_timeout: Duration,
    model_text: String,
    model_image: String,
    model_video: String,
}

impl AgnesClient {
    /// Create a new client from the Agnes configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is missing or the HTTP client cannot be
    /// built.
    pub fn new(config: &AgnesConfig) -> Result<Self> {
        let api_key = config.require_api_key()?;
        let http = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .connect_timeout(Duration::from_secs(15))
            .user_agent(crate::user_agent())
            .build()
            .map_err(|e| Error::http(format!("failed to build http client: {e}")))?;

        Ok(Self {
            base_url: config.base_url.clone(),
            api_key,
            http,
            poll_interval: Duration::from_secs_f64(config.poll_interval_secs.max(1.0)),
            poll_timeout: Duration::from_secs_f64(config.poll_timeout_secs.max(1.0)),
            model_text: config.model_text.clone(),
            model_image: config.model_image.clone(),
            model_video: config.model_video.clone(),
        })
    }

    /// The configured chat/text model identifier.
    #[must_use]
    pub fn text_model(&self) -> &str {
        &self.model_text
    }

    /// The configured image model identifier.
    #[must_use]
    pub fn image_model(&self) -> &str {
        &self.model_image
    }

    /// The configured video model identifier.
    #[must_use]
    pub fn video_model(&self) -> &str {
        &self.model_video
    }

    /// The configured poll interval for async tasks.
    #[must_use]
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// The configured poll timeout for async tasks.
    #[must_use]
    pub fn poll_timeout(&self) -> Duration {
        self.poll_timeout
    }

    /// Issue an authenticated request and return the parsed JSON response.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, non-2xx status, or a response body
    /// that is not a JSON object.
    pub async fn request_json(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}{path}", self.base_url);
        tracing::debug!(%url, ?method, "agnes request");

        let mut req = self
            .http
            .request(method.clone(), &url)
            .bearer_auth(&self.api_key);
        if let Some(json) = body {
            req = req.json(json);
        }

        let response = req
            .send()
            .await
            .map_err(|e| Error::http(format!("request to {url} failed: {e}")))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| Error::http(format!("failed to read response body: {e}")))?;

        if !status.is_success() {
            return Err(api_error(status.as_u16(), &text));
        }

        let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
            Error::api(format!(
                "expected JSON response, got: {}",
                truncate(&text, 300)
            ))
        })?;

        // Surface a top-level API error object even on a 2xx response.
        if let Some(msg) = extract_error_message(&value) {
            return Err(Error::api(msg));
        }
        Ok(value)
    }

    /// POST `/v1/chat/completions`.
    ///
    /// # Errors
    ///
    /// See [`request_json`](Self::request_json).
    pub async fn chat_completions(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.request_json(Method::POST, "/v1/chat/completions", Some(body))
            .await
    }

    /// POST `/v1/images/generations`.
    ///
    /// # Errors
    ///
    /// See [`request_json`](Self::request_json).
    pub async fn images_generations(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.request_json(Method::POST, "/v1/images/generations", Some(body))
            .await
    }

    /// POST `/v1/videos` (create an async video task).
    ///
    /// # Errors
    ///
    /// See [`request_json`](Self::request_json).
    pub async fn create_video_task(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.request_json(Method::POST, "/v1/videos", Some(body))
            .await
    }

    /// GET `/v1/videos/{task_id}` (retrieve a video task).
    ///
    /// # Errors
    ///
    /// See [`request_json`](Self::request_json).
    pub async fn get_video_task(&self, task_id: &str) -> Result<serde_json::Value> {
        let path = urlencoding::encode(task_id);
        let path = format!("/v1/videos/{path}");
        self.request_json(Method::GET, &path, None).await
    }

    /// Download a URL to a local file. Returns the absolute path written.
    ///
    /// Reuses the configured HTTP client (timeouts, user agent). Creates the
    /// parent directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is not `http(s)://`, the request fails, the
    /// status is not 2xx, or the file cannot be written.
    pub async fn download_url(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(Error::config(format!(
                "download_url requires an http(s) URL, got: {url}"
            )));
        }
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    Error::config(format!("failed to create dir {}: {e}", parent.display()))
                })?;
            }
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::http(format!("download from {url} failed: {e}")))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| Error::http(format!("failed to read download body: {e}")))?;
        if !status.is_success() {
            return Err(Error::http(format!(
                "download from {url} returned HTTP {status}"
            )));
        }
        tokio::fs::write(dest, &bytes)
            .await
            .map_err(|e| Error::config(format!("failed to write {}: {e}", dest.display())))?;
        let abs = dest.canonicalize().map_err(|e| {
            Error::config(format!("failed to canonicalize {}: {e}", dest.display()))
        })?;
        Ok(abs)
    }
}

/// Build a normalized API error from a non-2xx response body.
fn api_error(status: u16, body: &str) -> Error {
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let message = extract_error_message(&parsed)
        .unwrap_or_else(|| format!("HTTP {status}: {}", truncate(body, 300)));
    Error::api(message)
}

/// Extract a human-readable message from an Agnes/OpenAI-style error object.
///
/// Returns `None` when there is no error, including the common case where the
/// Agnes API includes an `"error": null` field in successful responses.
fn extract_error_message(value: &serde_json::Value) -> Option<String> {
    let obj = value.as_object()?;
    if let Some(err) = obj.get("error") {
        // The Agnes API returns `"error": null` on success — not an error.
        if err.is_null() {
            return None;
        }
        if let Some(m) = err.get("message").and_then(serde_json::Value::as_str) {
            return Some(m.to_string());
        }
        if let Some(m) = err.get("type").and_then(serde_json::Value::as_str) {
            return Some(m.to_string());
        }
        if let Some(s) = err.as_str() {
            return Some(s.to_string());
        }
        return Some(err.to_string());
    }
    // Fall back to a top-level `message` field (some error payloads use it).
    obj.get("message")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

/// Truncate a string to `max` characters, appending an ellipsis.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Chat completions request body (OpenAI-compatible).
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

/// A single chat message with flexible OpenAI-style content.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
}

/// Extract the assistant text from an OpenAI-style chat completion response.
///
/// Returns `None` if the response shape is unexpected.
#[must_use]
pub fn extract_chat_text(response: &serde_json::Value) -> Option<String> {
    let choice = response.get("choices")?.as_array()?.first()?;
    let message = choice.get("message")?;
    if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
        return Some(content.to_string());
    }
    choice
        .get("text")
        .and_then(|v| v.as_str())
        .map(String::from)
}


/// Recursively collect all `http(s)://` URLs found under `url`/`image_url`
/// keys in a JSON value.
#[must_use]
pub fn collect_urls(value: &serde_json::Value) -> Vec<String> {
    let mut urls = Vec::new();
    collect_urls_inner(value, &mut urls);
    urls
}

fn collect_urls_inner(value: &serde_json::Value, urls: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let is_url_key = key == "url" || key == "image_url";
                if let Some(s) = val.as_str().filter(|s| {
                    is_url_key && (s.starts_with("http://") || s.starts_with("https://"))
                }) {
                    urls.push(s.to_string());
                } else {
                    collect_urls_inner(val, urls);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_urls_inner(item, urls);
            }
        }
        _ => {}
    }
}

/// Status of an asynchronous video task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Still running (queued / in_progress / processing / submitted / pending).
    Running,
    /// Completed successfully.
    Done,
    /// Failed / cancelled / errored.
    Failed,
    /// Unknown status string.
    Unknown,
}

impl TaskStatus {
    /// Classify a raw status string into a [`TaskStatus`].
    #[must_use]
    pub fn classify(status: &str) -> Self {
        match status.to_ascii_lowercase().as_str() {
            "queued" | "in_progress" | "processing" | "submitted" | "pending" => {
                TaskStatus::Running
            }
            "completed" | "succeeded" | "success" => TaskStatus::Done,
            "failed" | "cancelled" | "canceled" | "error" => TaskStatus::Failed,
            _ => TaskStatus::Unknown,
        }
    }
}

/// Extract a task id from a video creation or status response.
#[must_use]
pub fn extract_task_id(response: &serde_json::Value) -> Option<String> {
    for key in ["id", "task_id"] {
        if let Some(id) = response.get(key).and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
    }
    response.get("data").and_then(|d| {
        ["id", "task_id"]
            .iter()
            .find_map(|k| d.get(k).and_then(|v| v.as_str()).map(String::from))
    })
}

/// Extract the classified status of a video task.
#[must_use]
pub fn extract_task_status(response: &serde_json::Value) -> TaskStatus {
    let raw = response.get("status").and_then(|v| v.as_str()).or_else(|| {
        response
            .get("data")
            .and_then(|d| d.get("status"))
            .and_then(|v| v.as_str())
    });
    match raw {
        Some(s) => TaskStatus::classify(s),
        None => TaskStatus::Unknown,
    }
}

/// Extract the video URL from a completed task response.
///
/// The Agnes API exposes the downloadable video under `video_url`, `url`, or
/// (observed in practice) `remixed_from_video_id` for completed tasks.
#[must_use]
pub fn extract_video_url(response: &serde_json::Value) -> Option<String> {
    for key in ["video_url", "url", "remixed_from_video_id"] {
        if let Some(url) = response
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        {
            return Some(url.to_string());
        }
    }
    response.get("data").and_then(extract_video_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_chat_text_works() {
        let resp = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hello"}}]
        });
        assert_eq!(extract_chat_text(&resp).as_deref(), Some("hello"));
    }

    #[test]
    fn extract_chat_text_missing() {
        assert!(extract_chat_text(&serde_json::json!({})).is_none());
    }

    #[test]
    fn collect_urls_nested() {
        let resp = serde_json::json!({
            "data": [{"url": "https://example.com/a.png"}, {"image_url": "https://example.com/b.png"}]
        });
        let urls = collect_urls(&resp);
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/a.png".to_string()));
    }

    #[test]
    fn task_status_classification() {
        assert_eq!(TaskStatus::classify("in_progress"), TaskStatus::Running);
        assert_eq!(TaskStatus::classify("Completed"), TaskStatus::Done);
        assert_eq!(TaskStatus::classify("failed"), TaskStatus::Failed);
        assert_eq!(TaskStatus::classify("???"), TaskStatus::Unknown);
    }

    #[test]
    fn extract_task_id_top_level() {
        let resp = serde_json::json!({"id": "task_123"});
        assert_eq!(extract_task_id(&resp).as_deref(), Some("task_123"));
    }

    #[test]
    fn extract_task_id_nested() {
        let resp = serde_json::json!({"data": {"task_id": "task_456"}});
        assert_eq!(extract_task_id(&resp).as_deref(), Some("task_456"));
    }

    #[test]
    fn extract_video_url_works() {
        let resp = serde_json::json!({"video_url": "https://example.com/v.mp4"});
        assert_eq!(
            extract_video_url(&resp).as_deref(),
            Some("https://example.com/v.mp4")
        );
    }

    #[test]
    fn extract_video_url_remixed_field() {
        let resp = serde_json::json!({
            "status": "completed",
            "remixed_from_video_id": "https://example.com/video.mp4"
        });
        assert_eq!(
            extract_video_url(&resp).as_deref(),
            Some("https://example.com/video.mp4")
        );
    }

    #[test]
    fn extract_error_message_variants() {
        let v = serde_json::json!({"error": {"message": "bad"}});
        assert_eq!(extract_error_message(&v).as_deref(), Some("bad"));
        let v = serde_json::json!({"message": "oops"});
        assert_eq!(extract_error_message(&v).as_deref(), Some("oops"));
        // Agnes returns `"error": null` on success — must NOT be treated as an error.
        let v = serde_json::json!({"error": null, "status": "completed"});
        assert!(extract_error_message(&v).is_none());
        let v = serde_json::json!({"error": "rate limited"});
        assert_eq!(extract_error_message(&v).as_deref(), Some("rate limited"));
    }

}
