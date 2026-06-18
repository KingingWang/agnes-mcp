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
    /// - `AGNES_API_KEY` / `AGNES_TOKEN` → `agnes.api_key`
    /// - `AGNES_BASE_URL`               → `agnes.base_url`
    /// - `AGNES_MCP_HOST`               → `server.host`
    /// - `AGNES_MCP_PORT`               → `server.port`
    /// - `AGNES_MCP_TRANSPORT`          → `server.transport_mode`
    /// - `AGNES_MCP_LOG_LEVEL`          → `logging.level`
    pub fn apply_env(&mut self) {
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
        if let Ok(level) = std::env::var("AGNES_MCP_LOG_LEVEL") {
            if !level.is_empty() {
                self.logging.level = level;
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
        if self
            .agnes
            .api_key
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            return Err(Error::config(
                "agnes.api_key is required. Set it in config.toml, the AGNES_API_KEY env var, \
                 or pass --api-key.",
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

    /// Agnes API key. Prefer setting via the `AGNES_API_KEY` environment
    /// variable rather than committing it to the config file.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Request timeout in seconds for synchronous endpoints (chat, image).
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// Default poll interval in seconds for asynchronous video tasks.
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: f64,

    /// Default poll timeout in seconds for asynchronous video tasks.
    #[serde(default = "default_poll_timeout")]
    pub poll_timeout_secs: f64,

    /// Directory for downloading generated assets (optional).
    #[serde(default)]
    pub output_dir: Option<String>,
}

const fn default_request_timeout() -> u64 {
    180
}

const fn default_poll_interval() -> f64 {
    10.0
}

const fn default_poll_timeout() -> f64 {
    900.0
}

fn default_base_url() -> String {
    DEFAULT_AGNES_BASE_URL.to_string()
}

impl Default for AgnesConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: None,
            request_timeout_secs: default_request_timeout(),
            poll_interval_secs: default_poll_interval(),
            poll_timeout_secs: default_poll_timeout(),
            output_dir: None,
        }
    }
}

impl AgnesConfig {
    /// Returns the configured API key, or an error if missing.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if no API key is set.
    pub fn require_api_key(&self) -> Result<String> {
        self.api_key
            .clone()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| Error::config("agnes.api_key is not set"))
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
        std::env::set_var("AGNES_API_KEY", "sk-env");
        std::env::set_var("AGNES_BASE_URL", "https://env.example.com/");
        let mut cfg = AppConfig::default();
        cfg.apply_env();
        assert_eq!(cfg.agnes.api_key.as_deref(), Some("sk-env"));
        assert_eq!(cfg.agnes.base_url, "https://env.example.com");
        std::env::remove_var("AGNES_API_KEY");
        std::env::remove_var("AGNES_BASE_URL");
    }
}
