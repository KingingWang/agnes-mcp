//! Error types for the Agnes MCP server.

use thiserror::Error;

/// Result type alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the Agnes MCP server.
#[derive(Debug, Error)]
pub enum Error {
    /// Configuration error (missing/invalid value).
    #[error("configuration error: {0}")]
    Config(String),

    /// Agnes API returned an error response.
    #[error("agnes api error: {0}")]
    Api(String),

    /// Agnes API returned a non-2xx response with a known HTTP status code.
    ///
    /// Used internally by the retry layer to inspect the status (429 rate
    /// limit, 401/403 auth failure) and apply per-key cooldowns.
    #[error("agnes api error (HTTP {status}): {message}")]
    ApiStatus {
        /// HTTP status code returned by the API.
        status: u16,
        /// Human-readable error message extracted from the response body.
        message: String,
    },

    /// HTTP / network transport error talking to the Agnes API.
    #[error("http error: {0}")]
    Http(String),

    /// JSON serialization or deserialization error.
    #[error("json error: {0}")]
    Json(String),

    /// Initialization error (logging, global state, etc.).
    #[error("initialization error ({context}): {message}")]
    Initialization {
        /// Subsystem that failed to initialize.
        context: String,
        /// Human-readable failure detail.
        message: String,
    },

    /// An MCP SDK error wrapping the underlying SDK message.
    #[error("mcp error ({context}): {message}")]
    Mcp {
        /// Where the MCP error originated.
        context: String,
        /// The underlying SDK error message.
        message: String,
    },

    /// Catch-all for errors not covered by a more specific variant.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    /// Create a configuration error.
    #[must_use]
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create an Agnes API error.
    #[must_use]
    pub fn api(msg: impl Into<String>) -> Self {
        Self::Api(msg.into())
    }

    /// Create an Agnes API error with a known HTTP status code.
    ///
    /// The status is used by the multi-key retry layer to decide whether to
    /// cool the offending key down (401/403/429) or fail fast (other 4xx).
    #[must_use]
    pub fn api_status(status: u16, msg: impl Into<String>) -> Self {
        Self::ApiStatus {
            status,
            message: msg.into(),
        }
    }

    /// Create an HTTP/network error.
    #[must_use]
    pub fn http(msg: impl Into<String>) -> Self {
        Self::Http(msg.into())
    }

    /// Create a JSON error.
    #[must_use]
    pub fn json(msg: impl Into<String>) -> Self {
        Self::Json(msg.into())
    }

    /// Create an initialization error.
    #[must_use]
    pub fn initialization(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Initialization {
            context: context.into(),
            message: message.into(),
        }
    }

    /// Create an MCP error.
    #[must_use]
    pub fn mcp(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Mcp {
            context: context.into(),
            message: message.into(),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Other(anyhow::anyhow!(e))
    }
}

impl From<toml::de::Error> for Error {
    fn from(e: toml::de::Error) -> Self {
        Self::Config(e.to_string())
    }
}
