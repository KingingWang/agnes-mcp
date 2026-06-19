//! Configuration management.
//!
//! Configuration sources, in priority order:
//! 1. Command-line arguments (highest)
//! 2. Environment variables (`AGNES_API_KEY`, `AGNES_BASE_URL`, `AGNES_MCP_*`)
//! 3. TOML configuration file
//! 4. Built-in defaults (lowest)

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Default Agnes API base URL.
pub const DEFAULT_AGNES_BASE_URL: &str = "https://apihub.agnes-ai.com";

/// Agnes chat/text model identifier.
pub const MODEL_TEXT: &str = "agnes-2.0-flash";
/// Agnes image generation model identifier.
pub const MODEL_IMAGE: &str = "agnes-image-2.1-flash";
/// Agnes video generation model identifier.
pub const MODEL_VIDEO: &str = "agnes-video-v2.0";

/// Top-level application configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AppConfig {
    /// Agnes API configuration.
    #[serde(default)]
    pub agnes: AgnesConfig,

    /// Server / transport configuration.
    #[serde(default)]
    pub server: ServerConfig,

    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl AppConfig {
    /// Load configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::config(format!("failed to read {}: {e}", path.display())))?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Apply environment-variable overrides in place.
    ///
    /// Recognized variables:
    /// - `AGNES_API_KEYS`               → `agnes.api_keys` (comma-separated multi-key pool)
    /// - `AGNES_API_KEY` / `AGNES_TOKEN` → `agnes.api_key`
    /// - `AGNES_BASE_URL`               → `agnes.base_url`
    /// - `AGNES_MCP_HOST`               → `server.host`
    /// - `AGNES_MCP_PORT`               → `server.port`
    /// - `AGNES_MCP_TRANSPORT`          → `server.transport_mode`
    /// - `AGNES_KEY_COOLDOWN_SECS`      → `agnes.key_cooldown_secs`
    /// - `AGNES_KEY_RATE_LIMIT_COOLDOWN_SECS` → `agnes.key_rate_limit_cooldown_secs`
    /// - `AGNES_MCP_LOG_LEVEL`          → `logging.level`
    /// - `AGNES_MODEL_TEXT`             → `agnes.model_text`
    /// - `AGNES_MODEL_IMAGE`            → `agnes.model_image`
    /// - `AGNES_MODEL_VIDEO`            → `agnes.model_video`
    /// - `AGNES_DISABLED_TOOLS`         → `agnes.disabled_tools` (comma-separated)
    pub fn apply_env(&mut self) {
        // Multi-key support: AGNES_API_KEYS (comma-separated) takes precedence for
        // the multi-key pool. AGNES_API_KEY still works for single-key setups.
        if let Ok(keys_str) = std::env::var("AGNES_API_KEYS") {
            if !keys_str.is_empty() {
                self.agnes.api_keys = Some(
                    keys_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
            }
        }
        if let Ok(key) = std::env::var("AGNES_API_KEY").or_else(|_| std::env::var("AGNES_TOKEN")) {
            if !key.is_empty() {
                self.agnes.api_key = Some(key);
            }
        }
        if let Ok(base) = std::env::var("AGNES_BASE_URL") {
            if !base.is_empty() {
                self.agnes.base_url = base.trim_end_matches('/').to_string();
            }
        }
        if let Ok(host) = std::env::var("AGNES_MCP_HOST") {
            if !host.is_empty() {
                self.server.host = host;
            }
        }
        if let Ok(port) = std::env::var("AGNES_MCP_PORT") {
            if let Ok(port) = port.trim().parse::<u16>() {
                self.server.port = port;
            }
        }
        if let Ok(mode) = std::env::var("AGNES_MCP_TRANSPORT") {
            if !mode.is_empty() {
                self.server.transport_mode = mode;
            }
        }
        if let Ok(secs) = std::env::var("AGNES_KEY_COOLDOWN_SECS") {
            if let Ok(parsed) = secs.trim().parse::<u64>() {
                self.agnes.key_cooldown_secs = parsed;
            }
        }
        if let Ok(secs) = std::env::var("AGNES_KEY_RATE_LIMIT_COOLDOWN_SECS") {
            if let Ok(parsed) = secs.trim().parse::<u64>() {
                self.agnes.key_rate_limit_cooldown_secs = parsed;
            }
        }
        if let Ok(level) = std::env::var("AGNES_MCP_LOG_LEVEL") {
            if !level.is_empty() {
                self.logging.level = level;
            }
        }
        if let Ok(m) = std::env::var("AGNES_MODEL_TEXT") {
            if !m.is_empty() {
                self.agnes.model_text = m;
            }
        }
        if let Ok(m) = std::env::var("AGNES_MODEL_IMAGE") {
            if !m.is_empty() {
                self.agnes.model_image = m;
            }
        }
        if let Ok(m) = std::env::var("AGNES_MODEL_VIDEO") {
            if !m.is_empty() {
                self.agnes.model_video = m;
            }
        }
        if let Ok(list) = std::env::var("AGNES_DISABLED_TOOLS") {
            if !list.is_empty() {
                self.agnes.disabled_tools = list
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }

    /// Validate the configuration, returning an error if a required field is
    /// missing or invalid.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is missing or the base URL is invalid.
    pub fn validate(&self) -> Result<()> {
        if self.agnes.effective_api_keys().is_empty() {
            return Err(Error::config(
                "agnes.api_key / agnes.api_keys is required. Set it in config.toml,                  the AGNES_API_KEY / AGNES_API_KEYS env var, or pass --api-key.",
            ));
        }
        if !self.agnes.base_url.starts_with("http://")
            && !self.agnes.base_url.starts_with("https://")
        {
            return Err(Error::config(format!(
                "agnes.base_url must start with http:// or https://, got: {}",
                self.agnes.base_url
            )));
        }
        Ok(())
    }
}

/// Agnes API configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgnesConfig {
    /// Agnes API base URL (no trailing slash).
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Agnes API key. Accepts a single key or a comma-separated list (e.g.
    /// `"sk-1, sk-2"`), which is split into multiple pool entries — see
    /// [`AgnesConfig::effective_api_keys`]. Prefer setting via the
    /// `AGNES_API_KEY` environment variable rather than committing it to the
    /// config file.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Multiple Agnes API keys used as a round-robin pool. Requests are
    /// distributed evenly across all keys. Set via the `api_keys` TOML field
    /// or the `AGNES_API_KEYS` environment variable (comma-separated).
    #[serde(default)]
    pub api_keys: Option<Vec<String>>,

    /// Request timeout in seconds for synchronous endpoints (chat, image).
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// Cooldown in seconds applied to an API key after an authentication
    /// failure (HTTP 401/403). The key is skipped by the round-robin selector
    /// until the cooldown elapses. Defaults to 600 seconds (10 minutes).
    #[serde(default = "default_key_cooldown")]
    pub key_cooldown_secs: u64,

    /// Cooldown in seconds applied to an API key after a rate-limit response
    /// (HTTP 429). Defaults to 60 seconds.
    #[serde(default = "default_key_rate_limit_cooldown")]
    pub key_rate_limit_cooldown_secs: u64,

    /// Agnes chat/text model identifier. Defaults to `MODEL_TEXT`.
    #[serde(default = "default_model_text")]
    pub model_text: String,

    /// Agnes image generation model identifier. Defaults to `MODEL_IMAGE`.
    #[serde(default = "default_model_image")]
    pub model_image: String,

    /// Agnes video generation model identifier. Defaults to `MODEL_VIDEO`.
    #[serde(default = "default_model_video")]
    pub model_video: String,

    /// MCP tool names to disable (not register with the server). Empty by
    /// default — every built-in tool is enabled. Names are matched
    /// case-sensitively against the canonical identifiers in
    /// [`crate::tools::AVAILABLE_TOOLS`]. Unknown names are ignored at
    /// registry-build time with a warning. Set via the `disabled_tools` TOML
    /// field, the `AGNES_DISABLED_TOOLS` env var (comma-separated), or the
    /// `--disable-tool` CLI flag (repeatable); all sources are merged.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
}

const fn default_request_timeout() -> u64 {
    180
}

const fn default_key_cooldown() -> u64 {
    600
}

const fn default_key_rate_limit_cooldown() -> u64 {
    60
}

fn default_base_url() -> String {
    DEFAULT_AGNES_BASE_URL.to_string()
}

/// Default chat/text model identifier.
fn default_model_text() -> String {
    MODEL_TEXT.to_string()
}

/// Default image generation model identifier.
fn default_model_image() -> String {
    MODEL_IMAGE.to_string()
}

/// Default video generation model identifier.
fn default_model_video() -> String {
    MODEL_VIDEO.to_string()
}

impl Default for AgnesConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: None,
            api_keys: None,
            request_timeout_secs: default_request_timeout(),
            key_cooldown_secs: default_key_cooldown(),
            key_rate_limit_cooldown_secs: default_key_rate_limit_cooldown(),
            model_text: default_model_text(),
            model_image: default_model_image(),
            model_video: default_model_video(),
            disabled_tools: Vec::new(),
        }
    }
}

impl AgnesConfig {
    /// Merge all configured API keys into a single deduped pool.
    ///
    /// Sources (in order, all merged — not override semantics):
    /// 1. `agnes.api_keys` (TOML array) or `AGNES_API_KEYS` env (comma-separated)
    /// 2. `agnes.api_key` (TOML scalar) or `AGNES_API_KEY` env, or CLI `--api-key`.
    ///    The scalar form also accepts comma-separated entries for convenience.
    ///
    /// Empty strings and duplicates are dropped; the first occurrence wins.
    #[must_use]
    pub fn effective_api_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        if let Some(list) = &self.api_keys {
            keys.extend(list.iter().cloned());
        }
        if let Some(single) = &self.api_key {
            keys.extend(single.split(',').map(|s| s.trim().to_string()));
        }
        let mut seen = std::collections::HashSet::new();
        // Trim each key, drop empties (including whitespace-only), dedup
        // preserving first-occurrence order.
        keys = keys
            .into_iter()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        keys.retain(|k| seen.insert(k.clone()));
        keys
    }

    /// Returns the merged, deduped list of API keys, or an error if empty.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if no API key is set.
    pub fn require_api_keys(&self) -> Result<Vec<String>> {
        let keys = self.effective_api_keys();
        if keys.is_empty() {
            return Err(Error::config("agnes.api_key / agnes.api_keys is not set"));
        }
        Ok(keys)
    }
}

/// Server / transport configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// Server name reported to MCP clients.
    #[serde(default = "default_server_name")]
    pub name: String,

    /// Bind host address.
    #[serde(default = "default_server_host")]
    pub host: String,

    /// Bind port (HTTP/SSE transports).
    #[serde(default = "default_server_port")]
    pub port: u16,

    /// Transport mode: `stdio`, `http`, `sse`, or `hybrid`.
    #[serde(default = "default_transport_mode")]
    pub transport_mode: String,
}

fn default_server_name() -> String {
    crate::NAME.to_string()
}

fn default_server_host() -> String {
    "127.0.0.1".to_string()
}

const fn default_server_port() -> u16 {
    8080
}

fn default_transport_mode() -> String {
    "stdio".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: default_server_name(),
            host: default_server_host(),
            port: default_server_port(),
            transport_mode: default_transport_mode(),
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    /// Log level: `trace`, `debug`, `info`, `warn`, or `error`.
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

/// Shared mutex used by tests that mutate process-wide environment
/// variables. Multiple `#[test]` functions in this crate set / unset
/// `AGNES_*` env vars and call [`AppConfig::apply_env`]; without
/// serialization those tests race when the test runner schedules them on
/// different threads. Acquiring this guard at the top of each
/// env-mutating test forces them to run one at a time.
#[cfg(test)]
fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock, PoisonError};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    // If a previous test panicked while holding the lock, recover so that
    // subsequent tests can still run.
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_config(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{content}").unwrap();
        f
    }

    #[test]
    fn parse_minimal_config() {
        let f = temp_config("[agnes]\napi_key = \"sk-test\"\n");
        let cfg = AppConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.agnes.api_key.as_deref(), Some("sk-test"));
        assert_eq!(cfg.agnes.base_url, DEFAULT_AGNES_BASE_URL);
        assert_eq!(cfg.server.transport_mode, "stdio");
    }

    #[test]
    fn parse_full_config() {
        let f = temp_config(concat!(
            "[agnes]\n",
            "base_url = \"https://example.com\"\n",
            "api_key = \"sk-test\"\n",
            "request_timeout_secs = 60\n\n",
            "[server]\n",
            "host = \"0.0.0.0\"\n",
            "port = 9000\n",
            "transport_mode = \"hybrid\"\n\n",
            "[logging]\n",
            "level = \"debug\"\n",
        ));
        let cfg = AppConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.agnes.base_url, "https://example.com");
        assert_eq!(cfg.server.port, 9000);
        assert_eq!(cfg.server.transport_mode, "hybrid");
        assert_eq!(cfg.logging.level, "debug");
    }

    #[test]
    fn validate_requires_api_key() {
        let mut cfg = AppConfig::default();
        assert!(cfg.validate().is_err());
        cfg.agnes.api_key = Some("sk-test".to_string());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_bad_base_url() {
        let mut cfg = AppConfig::default();
        cfg.agnes.api_key = Some("sk-test".to_string());
        cfg.agnes.base_url = "ftp://bad".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn env_overrides() {
        let _env = super::env_test_lock();
        std::env::set_var("AGNES_API_KEY", "sk-env");
        std::env::set_var("AGNES_BASE_URL", "https://env.example.com/");
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        assert_eq!(cfg.agnes.api_key.as_deref(), Some("sk-env"));
        assert_eq!(cfg.agnes.base_url, "https://env.example.com");
        std::env::remove_var("AGNES_API_KEY");
        std::env::remove_var("AGNES_BASE_URL");
    }

    #[test]
    fn default_models_match_constants() {
        let cfg = AgnesConfig::default();
        assert_eq!(cfg.model_text, MODEL_TEXT);
        assert_eq!(cfg.model_image, MODEL_IMAGE);
        assert_eq!(cfg.model_video, MODEL_VIDEO);
    }

    #[test]
    fn parse_config_with_custom_models() {
        let f = temp_config(concat!(
            "[agnes]\n",
            "api_key = \"sk-test\"\n",
            "model_text = \"custom-text\"\n",
            "model_image = \"custom-image\"\n",
            "model_video = \"custom-video\"\n",
        ));
        let cfg = AppConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.agnes.model_text, "custom-text");
        assert_eq!(cfg.agnes.model_image, "custom-image");
        assert_eq!(cfg.agnes.model_video, "custom-video");
    }

    #[test]
    fn env_overrides_models() {
        let _env = super::env_test_lock();
        std::env::set_var("AGNES_MODEL_TEXT", "env-text");
        std::env::set_var("AGNES_MODEL_IMAGE", "env-image");
        std::env::set_var("AGNES_MODEL_VIDEO", "env-video");
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        assert_eq!(cfg.agnes.model_text, "env-text");
        assert_eq!(cfg.agnes.model_image, "env-image");
        assert_eq!(cfg.agnes.model_video, "env-video");
        std::env::remove_var("AGNES_MODEL_TEXT");
        std::env::remove_var("AGNES_MODEL_IMAGE");
        std::env::remove_var("AGNES_MODEL_VIDEO");
    }

    #[test]
    fn effective_api_keys_single() {
        let cfg = AgnesConfig {
            api_key: Some("sk-1".to_string()),
            ..AgnesConfig::default()
        };
        assert_eq!(cfg.effective_api_keys(), vec!["sk-1".to_string()]);
    }

    #[test]
    fn effective_api_keys_comma_separated() {
        let cfg = AgnesConfig {
            api_key: Some("sk-1, sk-2 , sk-3".to_string()),
            ..AgnesConfig::default()
        };
        assert_eq!(
            cfg.effective_api_keys(),
            vec!["sk-1".to_string(), "sk-2".to_string(), "sk-3".to_string()]
        );
    }

    #[test]
    fn effective_api_keys_dedup_preserves_order() {
        let cfg = AgnesConfig {
            api_keys: Some(vec!["sk-1".to_string(), "sk-2".to_string()]),
            api_key: Some("sk-2, sk-3".to_string()),
            ..AgnesConfig::default()
        };
        assert_eq!(
            cfg.effective_api_keys(),
            vec!["sk-1".to_string(), "sk-2".to_string(), "sk-3".to_string()]
        );
    }

    #[test]
    fn effective_api_keys_filters_empty() {
        let cfg = AgnesConfig {
            api_keys: Some(vec![
                "sk-1".to_string(),
                String::new(),
                "  ".to_string(),
                "sk-2".to_string(),
            ]),
            ..AgnesConfig::default()
        };
        assert_eq!(
            cfg.effective_api_keys(),
            vec!["sk-1".to_string(), "sk-2".to_string()]
        );
    }

    #[test]
    fn effective_api_keys_empty_when_unconfigured() {
        let cfg = AgnesConfig::default();
        assert!(cfg.effective_api_keys().is_empty());
    }

    #[test]
    fn require_api_keys_errors_when_empty() {
        let cfg = AgnesConfig::default();
        assert!(cfg.require_api_keys().is_err());
    }

    #[test]
    fn require_api_keys_ok_with_pool() {
        let cfg = AgnesConfig {
            api_keys: Some(vec!["sk-1".to_string(), "sk-2".to_string()]),
            ..AgnesConfig::default()
        };
        assert_eq!(cfg.require_api_keys().unwrap().len(), 2);
    }

    #[test]
    fn validate_accepts_api_keys() {
        let mut cfg = AppConfig::default();
        cfg.agnes.api_keys = Some(vec!["sk-1".to_string(), "sk-2".to_string()]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn env_multi_keys_overrides() {
        let _env = super::env_test_lock();
        std::env::set_var("AGNES_API_KEYS", "sk-a, sk-b , sk-c");
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        let expected: Vec<String> = ["sk-a", "sk-b", "sk-c"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(cfg.agnes.api_keys, Some(expected));
        std::env::remove_var("AGNES_API_KEYS");
    }

    #[test]
    fn default_cooldowns_match_constants() {
        let cfg = AgnesConfig::default();
        assert_eq!(cfg.key_cooldown_secs, 600);
        assert_eq!(cfg.key_rate_limit_cooldown_secs, 60);
    }

    #[test]
    fn parse_config_with_cooldowns() {
        let f = temp_config(concat!(
            "[agnes]\n",
            "api_key = \"sk-test\"\n",
            "key_cooldown_secs = 120\n",
            "key_rate_limit_cooldown_secs = 30\n",
        ));
        let cfg = AppConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.agnes.key_cooldown_secs, 120);
        assert_eq!(cfg.agnes.key_rate_limit_cooldown_secs, 30);
    }

    #[test]
    fn env_overrides_cooldowns() {
        let _env = super::env_test_lock();
        std::env::set_var("AGNES_KEY_COOLDOWN_SECS", "999");
        std::env::set_var("AGNES_KEY_RATE_LIMIT_COOLDOWN_SECS", "111");
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        assert_eq!(cfg.agnes.key_cooldown_secs, 999);
        assert_eq!(cfg.agnes.key_rate_limit_cooldown_secs, 111);
        std::env::remove_var("AGNES_KEY_COOLDOWN_SECS");
        std::env::remove_var("AGNES_KEY_RATE_LIMIT_COOLDOWN_SECS");
    }
}

#[cfg(test)]
mod disabled_tools_tests {
    use super::*;
    use std::io::Write;

    fn temp_config(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{content}").unwrap();
        f
    }

    #[test]
    fn default_disabled_tools_empty() {
        let cfg = AgnesConfig::default();
        assert!(cfg.disabled_tools.is_empty());
    }

    #[test]
    fn parse_disabled_tools_from_toml() {
        let f = temp_config(concat!(
            "[agnes]\n",
            "api_key = \"sk-test\"\n",
            "disabled_tools = [\"agnes_generate_video\", \"agnes_video_status\"]\n",
        ));
        let cfg = AppConfig::from_file(f.path()).unwrap();
        assert_eq!(
            cfg.agnes.disabled_tools,
            vec![
                "agnes_generate_video".to_string(),
                "agnes_video_status".to_string()
            ]
        );
    }

    #[test]
    fn env_overrides_disabled_tools() {
        let _env = super::env_test_lock();
        std::env::set_var(
            "AGNES_DISABLED_TOOLS",
            "agnes_generate_image, agnes_video_status",
        );
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        assert_eq!(
            cfg.agnes.disabled_tools,
            vec![
                "agnes_generate_image".to_string(),
                "agnes_video_status".to_string()
            ]
        );
        std::env::remove_var("AGNES_DISABLED_TOOLS");
    }

    #[test]
    fn env_disabled_tools_empty_string_keeps_default() {
        let _env = super::env_test_lock();
        std::env::set_var("AGNES_DISABLED_TOOLS", "");
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        assert!(cfg.agnes.disabled_tools.is_empty());
        std::env::remove_var("AGNES_DISABLED_TOOLS");
    }

    #[test]
    fn env_disabled_tools_filters_empty_entries() {
        let _env = super::env_test_lock();
        std::env::set_var(
            "AGNES_DISABLED_TOOLS",
            "agnes_generate_image, ,  ,agnes_video_status",
        );
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        assert_eq!(
            cfg.agnes.disabled_tools,
            vec![
                "agnes_generate_image".to_string(),
                "agnes_video_status".to_string()
            ]
        );
        std::env::remove_var("AGNES_DISABLED_TOOLS");
    }
}
