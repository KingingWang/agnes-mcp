//! Agnes AI MCP Server
//!
//! A Model Context Protocol server exposing the Agnes AI free models:
//!
//! - **agnes-2.0-flash** — chat completions and multimodal image recognition
//!   (vision) via an OpenAI-compatible endpoint.
//! - **agnes-image-2.1-flash** — text-to-image and image-to-image generation.
//! - **agnes-video-v2.0** — text-to-video, image-to-video, multi-image and
//!   keyframe video generation (asynchronous tasks with polling).
//!
//! # Transport protocols
//!
//! - `stdio` — standard input/output (for MCP client integration)
//! - `http` — Streamable HTTP
//! - `sse`   — Server-Sent Events
//! - `hybrid`— HTTP + SSE
//!
//! # Example
//!
//! ```rust,no_run
//! use agnes_mcp::{AppConfig, AgnesServer};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = AppConfig::default();
//!     let server = AgnesServer::new(config)?;
//!     server.run_http().await?;
//!     Ok(())
//! }
//! ```

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::doc_markdown)]

pub mod cli;
pub mod config;
pub mod error;
pub mod server;
pub mod tools;
pub mod utils;

pub use crate::config::AppConfig;
pub use crate::error::{Error, Result};
pub use crate::server::AgnesServer;

/// Server version (from `CARGO_PKG_VERSION`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Server name.
pub const NAME: &str = "agnes-mcp";

/// Git commit short hash captured at build time.
pub const GIT_COMMIT: &str = env!("GIT_COMMIT");

/// Build timestamp captured at build time.
pub const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

/// Rust toolchain version captured at build time.
pub const RUST_VERSION: &str = env!("RUST_VERSION");

/// Build the `User-Agent` header sent to the Agnes API.
#[must_use]
pub fn user_agent() -> String {
    format!("AgnesMCP/{VERSION}")
}

/// Initialize the tracing logging system with the given level.
///
/// # Errors
///
/// Returns an error if the global tracing subscriber is already installed or
/// cannot be initialized.
pub fn init_logging(level: &str) -> Result<()> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let level = match level.to_ascii_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => level.to_string(),
        _ => "info".to_string(),
    };

    let filter = EnvFilter::new(level);

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr).with_target(true))
        .try_init()
        .map_err(|e| Error::initialization("logging", e.to_string()))?;

    Ok(())
}
