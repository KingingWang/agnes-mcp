//! HTTP client for the Agnes AI API.
//!
//! Wraps [`reqwest`] with the Agnes base URL, a round-robin pool of API
//! keys, timeouts, and normalizes error responses returned by the API.
//!
//! Video task affinity: Agnes ties each video `task_id` to the API key that
//! created it (querying with a different key is treated as a possible key
//! leak). The client records `task_id -> key_idx` on creation and reuses the
//! same key for every subsequent status query.
//!
//! Multi-key retry & cooldown: when an API key returns HTTP 429 (rate
//! limited) or 401/403 (auth failure), it is cooled down for a configurable
//! duration and the request is retried on the next healthy key. Video task
//! queries are pinned to the creating key and never retried on a different key.

use crate::config::AgnesConfig;
use crate::error::{Error, Result};
use reqwest::{Client, Method};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Per-key health state used by the retry/cooldown layer.
///
/// `cooldown_until` is a unix timestamp in seconds; `0` means the key is
/// healthy. Updates are atomic so concurrent requests can read/write safely
/// without locking.
#[derive(Debug, Default)]
struct KeyHealth {
    cooldown_until: AtomicU64,
}

impl KeyHealth {
    /// Returns `true` when the key is usable (cooldown expired or never set).
    fn is_available(&self) -> bool {
        let until = self.cooldown_until.load(Ordering::Relaxed);
        until == 0 || unix_now() >= until
    }

    /// Seconds remaining until the cooldown elapses, or `0` if available.
    fn secs_remaining(&self) -> u64 {
        let until = self.cooldown_until.load(Ordering::Relaxed);
        if until == 0 {
            return 0;
        }
        until.saturating_sub(unix_now())
    }
}

/// Current unix timestamp in seconds.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Asynchronous client for the Agnes AI API.
pub struct AgnesClient {
    base_url: String,
    api_keys: Vec<String>,
    next_key: AtomicUsize,
    /// task_id -> index into `api_keys`. Used to keep video task queries on
    /// the same key that created the task (Agnes requires this).
    task_keys: RwLock<HashMap<String, usize>>,
    http: Client,
    model_text: String,
    model_image: String,
    model_video: String,
    /// Per-key health state, parallel to `api_keys`.
    key_health: Vec<KeyHealth>,
    /// Cooldown (seconds) after HTTP 401/403 (auth failure / revoked key).
    auth_cooldown_secs: u64,
    /// Cooldown (seconds) after HTTP 429 (rate limited).
    rate_limit_cooldown_secs: u64,
}

impl AgnesClient {
    /// Maximum retry attempts in the round-robin path. Capped to avoid
    /// pathological looping when many keys are simultaneously rate-limited.
    const MAX_RETRY_ATTEMPTS: usize = 3;

    /// Create a new client from the Agnes configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is missing or the HTTP client cannot be
    /// built.
    pub fn new(config: &AgnesConfig) -> Result<Self> {
        let api_keys = config.require_api_keys()?;
        if api_keys.len() > 1 {
            tracing::info!(
                "agnes client configured with {} API keys (round-robin load balancing)",
                api_keys.len()
            );
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .connect_timeout(Duration::from_secs(15))
            .user_agent(crate::user_agent())
            .build()
            .map_err(|e| Error::http(format!("failed to build http client: {e}")))?;

        let key_count = api_keys.len();
        Ok(Self {
            base_url: config.base_url.clone(),
            api_keys,
            next_key: AtomicUsize::new(0),
            task_keys: RwLock::new(HashMap::new()),
            http,
            model_text: config.model_text.clone(),
            model_image: config.model_image.clone(),
            model_video: config.model_video.clone(),
            key_health: (0..key_count).map(|_| KeyHealth::default()).collect(),
            auth_cooldown_secs: config.key_cooldown_secs,
            rate_limit_cooldown_secs: config.key_rate_limit_cooldown_secs,
        })
    }

    /// Number of API keys in the round-robin pool.
    #[must_use]
    pub fn api_key_count(&self) -> usize {
        self.api_keys.len()
    }

    /// Return the next API key in round-robin order, ignoring cooldown state.
    ///
    /// **Test-only.** Production code must go through [`Self::select_healthy_key_idx`]
    /// / [`Self::request_with_retry`] which respect per-key cooldowns. This helper
    /// exists solely to verify the underlying atomic round-robin counter in unit
    /// tests without coupling to cooldown behaviour.
    #[cfg(test)]
    #[must_use]
    pub fn next_api_key(&self) -> &str {
        let idx = self.next_key.fetch_add(1, Ordering::Relaxed) % self.api_keys.len();
        &self.api_keys[idx]
    }

    /// Record that `task_id` was created with key at `key_idx`.
    ///
    /// Stored in-process only; lost on server restart. Subsequent calls to
    /// [`Self::key_index_for_task`] and [`Self::get_video_task`] pin requests
    /// for this task to the same key.
    pub fn record_task_key(&self, task_id: &str, key_idx: usize) {
        match self.task_keys.write() {
            Ok(mut map) => {
                map.insert(task_id.to_string(), key_idx);
                tracing::debug!(
                    task_id,
                    key_idx,
                    tracked_tasks = map.len(),
                    "recorded video task key affinity"
                );
            }
            // RwLock poisoned: a panic happened while holding the lock. Record
            // nothing and log loudly — every subsequent get_video_task for this
            // task will fail with the affinity error, so operators need a
            // breadcrumb explaining why.
            Err(_) => {
                tracing::error!(
                    task_id,
                    key_idx,
                    "task_keys RwLock poisoned; affinity recording disabled"
                );
            }
        }
    }

    /// Look up the key index bound to `task_id`, if any.
    ///
    /// Returns `None` for tasks created by a previous server session or by
    /// external tools; callers should treat this as "unknown task" rather than
    /// silently picking a random key (which Agnes would reject as a possible
    /// key leak).
    #[must_use]
    pub fn key_index_for_task(&self, task_id: &str) -> Option<usize> {
        self.task_keys.read().ok()?.get(task_id).copied()
    }

    /// Whether the key at `key_idx` is currently usable (cooldown expired
    /// or never set).
    fn key_available(&self, key_idx: usize) -> bool {
        self.key_health
            .get(key_idx)
            .is_some_and(KeyHealth::is_available)
    }

    /// Cool down the key at `key_idx` for `secs` seconds.
    fn mark_key_cooldown(&self, key_idx: usize, secs: u64) {
        if let Some(health) = self.key_health.get(key_idx) {
            let until_unix = unix_now().saturating_add(secs);
            health.cooldown_until.store(until_unix, Ordering::Relaxed);
            tracing::warn!(
                key_idx,
                cooldown_secs = secs,
                available_again_unix = until_unix,
                "API key cooled down due to rate-limit / auth failure"
            );
        }
    }

    /// Seconds remaining on the cooldown for `key_idx`, or `0` if available.
    fn key_cooldown_remaining(&self, key_idx: usize) -> u64 {
        self.key_health
            .get(key_idx)
            .map_or(0, KeyHealth::secs_remaining)
    }

    /// Pick the next round-robin key index, skipping keys currently in
    /// cooldown. If every key is cooled down, falls back to the next
    /// round-robin index anyway — the caller will likely fail and cool down
    /// further, which surfaces a clear error to the user.
    fn select_healthy_key_idx(&self) -> usize {
        let n = self.api_keys.len();
        let start = self.next_key.fetch_add(1, Ordering::Relaxed) % n;
        for offset in 0..n {
            let idx = (start + offset) % n;
            if self.key_available(idx) {
                return idx;
            }
        }
        // All keys in cooldown; return the next round-robin position so the
        // call still attempts (and likely fails with a clear cooldown error).
        start
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

    /// Issue an authenticated request using the next healthy round-robin key,
    /// with automatic retry across keys on HTTP 429/401/403.
    ///
    /// Thin wrapper around [`Self::request_with_retry`] that discards the
    /// `key_idx` actually used (callers that need it, like video task
    /// creation, should call [`Self::request_with_retry`] directly).
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
        let (value, _key_idx) = self.request_with_retry(method, path, body, None).await?;
        Ok(value)
    }

    /// Issue an authenticated request with multi-key retry and cooldown.
    ///
    /// Behaviour:
    /// - `fixed_key_idx = Some(idx)` (affinity case, e.g. video task status):
    ///   only that key is used, no retry, even if it is currently in cooldown.
    ///   The request is attempted once anyway — switching keys would break
    ///   Agnes task ownership and look like a key leak to the server, so a
    ///   cooled affinity key is preferable to a wrong-key request. If it
    ///   returns 429/401/403, the key is cooled down further and the error is
    ///   surfaced.
    /// - `fixed_key_idx = None` (round-robin case): up to
    ///   `min(api_keys.len(), MAX_RETRY_ATTEMPTS)` attempts, skipping keys
    ///   that are currently in cooldown. On 429, the key is cooled down for
    ///   `rate_limit_cooldown_secs`; on 401/403, for `auth_cooldown_secs`.
    ///   Other errors (network, other 4xx, 5xx, JSON parse) fail fast — they
    ///   are not caused by the key.
    ///
    /// Returns the parsed response together with the index of the key that
    /// actually served the request (so callers like `create_video_task` can
    /// record affinity correctly even after a retry).
    ///
    /// # Errors
    ///
    /// See [`request_json_with_key_idx`](Self::request_json_with_key_idx).
    async fn request_with_retry(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        fixed_key_idx: Option<usize>,
    ) -> Result<(serde_json::Value, usize)> {
        // Affinity-bound path: single key, no retry.
        if let Some(key_idx) = fixed_key_idx {
            let remaining = self.key_cooldown_remaining(key_idx);
            if remaining > 0 {
                tracing::warn!(
                    key_idx,
                    cooldown_remaining_secs = remaining,
                    "affinity-bound key is in cooldown; attempting once anyway (no fallback)"
                );
            }
            let value = self
                .request_json_with_key_idx(method, path, body, key_idx)
                .await?;
            return Ok((value, key_idx));
        }

        // Round-robin path: try up to `cap` *healthy* keys (skipping cooled-down
        // ones does NOT consume the attempt budget). We iterate the whole pool
        // once if needed, counting only actual requests so transiently-cooled
        // keys don't starve the retry budget.
        let n = self.api_keys.len();
        let cap = n.min(Self::MAX_RETRY_ATTEMPTS);
        let start_idx = self.select_healthy_key_idx();

        let mut attempts_made = 0usize;
        let mut last_error: Option<Error> = None;
        for offset in 0..n {
            if attempts_made >= cap {
                break;
            }
            let key_idx = (start_idx + offset) % n;
            if !self.key_available(key_idx) {
                // Skip cooled-down keys without consuming the budget.
                continue;
            }
            attempts_made += 1;
            match self
                .request_json_with_key_idx(method.clone(), path, body, key_idx)
                .await
            {
                Ok(value) => return Ok((value, key_idx)),
                Err(Error::ApiStatus { status, message }) if matches!(status, 429 | 401 | 403) => {
                    let cooldown = if status == 429 {
                        self.rate_limit_cooldown_secs
                    } else {
                        self.auth_cooldown_secs
                    };
                    tracing::warn!(
                        key_idx,
                        http_status = status,
                        cooldown_secs = cooldown,
                        "API key rejected; cooling down and trying next key"
                    );
                    self.mark_key_cooldown(key_idx, cooldown);
                    last_error = Some(Error::ApiStatus { status, message });
                }
                Err(other) => return Err(other),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::api(format!(
                "all {n} API keys are in cooldown; wait and retry, or add more keys"
            ))
        }))
    }

    /// Issue an authenticated request pinned to a specific key in the pool.
    ///
    /// Used internally for video task affinity: creation picks a key via
    /// [`Self::select_healthy_key_idx`], then status queries are pinned to the
    /// same index via [`Self::record_task_key`] / [`Self::key_index_for_task`].
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, non-2xx status, or a response body
    /// that is not a JSON object.
    ///
    /// # Panics
    ///
    /// Panics if `key_idx` is out of range for the pool — prevented by all
    /// call sites in this file, which only pass indices from
    /// [`Self::select_healthy_key_idx`] or [`Self::key_index_for_task`].
    async fn request_json_with_key_idx(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        key_idx: usize,
    ) -> Result<serde_json::Value> {
        let url = format!("{}{path}", self.base_url);
        let api_key = self
            .api_keys
            .get(key_idx)
            .expect("key_idx is always within the api_keys pool");
        tracing::debug!(%url, ?method, key_idx, "agnes request");

        let mut req = self.http.request(method.clone(), &url).bearer_auth(api_key);
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
    /// Returns the parsed response along with the index of the key used to
    /// create it. Callers should extract the `task_id` from the response and
    /// pass it together with the returned key index to
    /// [`Self::record_task_key`] so that subsequent status queries are pinned
    /// to the same key (Agnes requires this).
    ///
    /// # Errors
    ///
    /// See [`request_json_with_key_idx`](Self::request_json_with_key_idx).
    pub async fn create_video_task(
        &self,
        body: &serde_json::Value,
    ) -> Result<(serde_json::Value, usize)> {
        // No fixed key: the retry layer picks a healthy key and may rotate
        // on 429/401/403. The key actually used is returned so the caller can
        // record affinity for subsequent status queries.
        let (response, key_idx) = self
            .request_with_retry(Method::POST, "/v1/videos", Some(body), None)
            .await?;
        Ok((response, key_idx))
    }

    /// GET `/v1/videos/{task_id}` (retrieve a video task).
    ///
    /// Pinned to the API key that created the task (looked up via
    /// [`Self::key_index_for_task`]). Returns a clear error if the task is
    /// unknown to this server session — Agnes ties task ownership to the
    /// creating key, and querying with a different key is treated as a
    /// possible key leak, so we never silently pick a random key.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the task was not created by this
    /// session. See [`request_json_with_key_idx`](Self::request_json_with_key_idx)
    /// for transport errors.
    pub async fn get_video_task(&self, task_id: &str) -> Result<serde_json::Value> {
        let key_idx = self.key_index_for_task(task_id).ok_or_else(|| {
            Error::config(format!(
                "video task '{task_id}' was not created by this server session;                  Agnes ties task ownership to the creating API key, so its key cannot be                  determined. Re-create the task via agnes_generate_video, or query it from                  the original session."
            ))
        })?;
        let path = urlencoding::encode(task_id);
        let path = format!("/v1/videos/{path}");
        // Pinned to the bound key (Some(key_idx)) — no retry on a different key,
        // because Agnes would treat that as a possible key leak.
        let (response, _used_idx) = self
            .request_with_retry(Method::GET, &path, None, Some(key_idx))
            .await?;
        Ok(response)
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
    // Use ApiStatus so the multi-key retry layer can inspect the status code
    // and decide whether to cool the offending key down (429/401/403).
    Error::api_status(status, message)
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

    fn build_client_with_keys(keys: &[&str]) -> AgnesClient {
        let cfg = crate::config::AgnesConfig {
            api_keys: Some(keys.iter().map(|s| (*s).to_string()).collect()),
            ..crate::config::AgnesConfig::default()
        };
        AgnesClient::new(&cfg).expect("client builds with at least one key")
    }

    #[test]
    fn round_robin_cycles_through_keys() {
        let client = build_client_with_keys(&["sk-a", "sk-b", "sk-c"]);
        assert_eq!(client.api_key_count(), 3);
        // 6 picks should cycle through the pool exactly twice.
        let picks: Vec<&str> = (0..6).map(|_| client.next_api_key()).collect();
        assert_eq!(picks, vec!["sk-a", "sk-b", "sk-c", "sk-a", "sk-b", "sk-c"]);
    }

    #[test]
    fn round_robin_single_key_always_returns_it() {
        let client = build_client_with_keys(&["solo"]);
        for _ in 0..5 {
            assert_eq!(client.next_api_key(), "solo");
        }
    }

    #[test]
    fn record_and_lookup_task_key() {
        let client = build_client_with_keys(&["sk-a", "sk-b", "sk-c"]);
        // Unknown task before recording
        assert!(client.key_index_for_task("task-unknown").is_none());

        client.record_task_key("task-1", 1);
        client.record_task_key("task-2", 0);

        assert_eq!(client.key_index_for_task("task-1"), Some(1));
        assert_eq!(client.key_index_for_task("task-2"), Some(0));
        assert!(client.key_index_for_task("task-3").is_none());
    }

    #[test]
    fn record_task_key_overwrites_on_replay() {
        let client = build_client_with_keys(&["sk-a", "sk-b"]);
        client.record_task_key("task-x", 0);
        assert_eq!(client.key_index_for_task("task-x"), Some(0));
        // Re-creating a task with the same id rebinds to the new key.
        client.record_task_key("task-x", 1);
        assert_eq!(client.key_index_for_task("task-x"), Some(1));
    }

    #[tokio::test]
    async fn get_video_task_unknown_id_errors_without_network_call() {
        // Use a bogus base_url; if affinity lookup fails first (as designed),
        // no network call is attempted and we get the clear config error.
        let cfg = crate::config::AgnesConfig {
            api_keys: Some(vec!["sk-x".to_string()]),
            base_url: "http://0.0.0.0:0".to_string(), // never reached
            ..crate::config::AgnesConfig::default()
        };
        let client = AgnesClient::new(&cfg).expect("client builds");

        let err = client.get_video_task("external-task-id").await;
        assert!(err.is_err(), "expected error for unknown task");
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("was not created by this server session"),
            "error message should explain affinity failure, got: {msg}"
        );
    }

    #[test]
    fn key_health_available_by_default() {
        let client = build_client_with_keys(&["sk-a", "sk-b"]);
        assert!(client.key_available(0));
        assert!(client.key_available(1));
    }

    #[test]
    fn mark_cooldown_makes_key_unavailable() {
        let client = build_client_with_keys(&["sk-a", "sk-b"]);
        client.mark_key_cooldown(0, 600);
        assert!(
            !client.key_available(0),
            "cooled-down key should be unavailable"
        );
        assert!(client.key_available(1), "other keys should be unaffected");
    }

    #[test]
    fn cooldown_reports_remaining_seconds() {
        let client = build_client_with_keys(&["sk-a"]);
        // Fresh key: 0 remaining.
        assert_eq!(client.key_cooldown_remaining(0), 0);
        client.mark_key_cooldown(0, 600);
        let remaining = client.key_cooldown_remaining(0);
        assert!(
            (590..=600).contains(&remaining),
            "remaining should be ~600s right after a 600s cooldown, got {remaining}"
        );
    }

    #[test]
    fn select_healthy_key_skips_cooled_down() {
        let client = build_client_with_keys(&["sk-a", "sk-b", "sk-c"]);
        // Cool down keys 0 and 1.
        client.mark_key_cooldown(0, 600);
        client.mark_key_cooldown(1, 600);
        // The healthy selector must return 2 (the only available key). We try
        // multiple picks because round-robin starts at a random position; any
        // available pick must be key 2.
        for _ in 0..6 {
            let idx = client.select_healthy_key_idx();
            assert!(
                client.key_available(idx),
                "select_healthy_key_idx returned a cooled-down key {idx}"
            );
        }
    }

    #[test]
    fn select_healthy_returns_some_index_when_all_cooled() {
        // When every key is cooled down, the selector must still return a
        // valid index (callers will attempt the call and likely fail).
        let client = build_client_with_keys(&["sk-a", "sk-b"]);
        client.mark_key_cooldown(0, 600);
        client.mark_key_cooldown(1, 600);
        let idx = client.select_healthy_key_idx();
        assert!(
            idx < 2,
            "must return an in-range index even when all cooled"
        );
    }

    #[tokio::test]
    async fn retry_exhausts_keys_and_returns_status_error() {
        // Spin up a tiny mock that always returns HTTP 401, so every key gets
        // cooled down and the loop exhausts the pool. Using a real listener
        // lets us assert that retry actually rotates through keys.
        use std::sync::atomic::AtomicUsize as TestAtomic;
        use std::sync::Arc;

        let attempt_counter = Arc::new(TestAtomic::new(0));
        let counter_clone = attempt_counter.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{addr}");

        let server = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let body = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n";
                let _ = tokio::io::AsyncWriteExt::write_all(&mut sock, body).await;
            }
        });

        let cfg = crate::config::AgnesConfig {
            api_keys: Some(vec!["sk-a".to_string(), "sk-b".to_string()]),
            base_url: server_url,
            // Tiny cooldown so we can observe state quickly.
            key_cooldown_secs: 1,
            ..crate::config::AgnesConfig::default()
        };
        let client = AgnesClient::new(&cfg).expect("client builds");

        let result = client.request_json(Method::GET, "/x", None).await;
        assert!(result.is_err(), "expected error after exhausting keys");
        // Each key gets one attempt → 2 attempts total.
        let attempts = attempt_counter.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            attempts, 2,
            "retry should rotate through all keys, got {attempts} attempts"
        );
        // Both keys should now be in cooldown.
        assert!(!client.key_available(0));
        assert!(!client.key_available(1));

        server.abort();
    }

    /// Regression: cooled-down keys inside the round-robin scan window must
    /// NOT consume the retry budget. With 4 keys [A, B*, C*, D] (B, C pre-cooled)
    /// and MAX_RETRY_ATTEMPTS=3, the loop must try A then D — not stop after
    /// A + 2 skipped iterations. Verified by counting HTTP requests received
    /// by a mock that returns 429 for every request.
    #[tokio::test]
    async fn retry_budget_skips_cooled_keys_without_counting() {
        use std::sync::atomic::AtomicUsize as TestAtomic;
        use std::sync::Arc;

        let attempt_counter = Arc::new(TestAtomic::new(0));
        let counter_clone = attempt_counter.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{addr}");

        let server = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let body = b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n";
                let _ = tokio::io::AsyncWriteExt::write_all(&mut sock, body).await;
            }
        });

        let cfg = crate::config::AgnesConfig {
            api_keys: Some(vec![
                "sk-a".to_string(),
                "sk-b".to_string(),
                "sk-c".to_string(),
                "sk-d".to_string(),
            ]),
            base_url: server_url,
            key_cooldown_secs: 1,
            key_rate_limit_cooldown_secs: 1,
            ..crate::config::AgnesConfig::default()
        };
        let client = AgnesClient::new(&cfg).expect("client builds");

        // Pre-cool keys 1 and 2 (sk-b, sk-c). select_healthy_key_idx must
        // return 0 (sk-a) as the first healthy key.
        client.mark_key_cooldown(1, 600);
        client.mark_key_cooldown(2, 600);
        assert!(client.key_available(0), "sk-a should be healthy");
        assert!(!client.key_available(1), "sk-b should be cooled");
        assert!(!client.key_available(2), "sk-c should be cooled");
        assert!(client.key_available(3), "sk-d should be healthy");

        let result = client.request_json(Method::GET, "/x", None).await;
        assert!(result.is_err(), "expected error after exhausting retries");

        let attempts = attempt_counter.load(std::sync::atomic::Ordering::Relaxed);
        // cap = min(4, MAX_RETRY_ATTEMPTS=3) = 3 healthy attempts.
        // Healthy keys reachable in scan order from start_idx=0 are: A (0), D (3).
        // B and C are skipped without consuming budget. So 2 actual requests.
        assert_eq!(
            attempts, 2,
            "retry should make 2 real requests (sk-a then sk-d);              cooled sk-b/sk-c must not consume the budget, got {attempts}"
        );

        server.abort();
    }
}
