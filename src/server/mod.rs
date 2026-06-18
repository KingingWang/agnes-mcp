//! Agnes MCP server: configuration, tool registry, and transport orchestration.

pub mod handler;
pub mod transport;

use crate::config::AppConfig;
use crate::error::Result;
use crate::tools::{create_default_registry, ToolRegistry};
use rust_mcp_sdk::schema::{
    Implementation, InitializeResult, ProtocolVersion, ServerCapabilities, ServerCapabilitiesTools,
};
use std::sync::Arc;

pub use handler::AgnesHandler;
pub use transport::{run_hyper_server, run_stdio_server, HyperServerConfig, TransportMode};

/// The Agnes MCP server.
#[derive(Clone)]
pub struct AgnesServer {
    config: AppConfig,
    tool_registry: Arc<ToolRegistry>,
}

impl AgnesServer {
    /// Create a new server from configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the tool registry (and its HTTP client) cannot be
    /// constructed.
    pub fn new(config: AppConfig) -> Result<Self> {
        let tool_registry = Arc::new(create_default_registry(&config.agnes)?);
        Ok(Self {
            config,
            tool_registry,
        })
    }

    /// Borrow the application configuration.
    #[must_use]
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Borrow the tool registry.
    #[must_use]
    pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    /// Build the MCP initialization result with server metadata and capabilities.
    #[must_use]
    pub fn server_info(&self) -> InitializeResult {
        InitializeResult {
            server_info: Implementation {
                name: self.config.server.name.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("Agnes AI MCP Server".to_string()),
                description: Some(
                    "MCP server for Agnes AI: image recognition, text-to-image, text-to-video, and more."
                        .to_string(),
                ),
                icons: vec![],
                website_url: None,
            },
            capabilities: ServerCapabilities {
                tools: Some(ServerCapabilitiesTools { list_changed: None }),
                resources: None,
                prompts: None,
                experimental: None,
                completions: None,
                logging: None,
                tasks: None,
            },
            protocol_version: ProtocolVersion::V2025_11_25.into(),
            instructions: Some(
                "Use this server to access Agnes AI free models: agnes_chat (text), \
                 agnes_image_recognition (vision), agnes_generate_image (text-to-image & \
                 image-to-image), agnes_generate_video (text-to-video & image-to-video & \
                 keyframes), agnes_video_status, agnes_enhance_prompt, and health_check."
                    .to_string(),
            ),
            meta: None,
        }
    }

    /// Run with stdio transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the server fails to start.
    pub async fn run_stdio(&self) -> Result<()> {
        run_stdio_server(self).await
    }

    /// Run with HTTP transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the server fails to start.
    pub async fn run_http(&self) -> Result<()> {
        run_hyper_server(self, HyperServerConfig::http()).await
    }

    /// Run with SSE transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the server fails to start.
    pub async fn run_sse(&self) -> Result<()> {
        run_hyper_server(self, HyperServerConfig::sse()).await
    }

    /// Run with the configured transport mode from the application config.
    ///
    /// # Errors
    ///
    /// Returns an error if the transport mode is unknown or the server fails to start.
    pub async fn run_with_configured_mode(&self) -> Result<()> {
        let mode: TransportMode = self
            .config
            .server
            .transport_mode
            .parse()
            .map_err(|e: String| crate::error::Error::config(e))?;
        match mode {
            TransportMode::Stdio => self.run_stdio().await,
            TransportMode::Http | TransportMode::Sse | TransportMode::Hybrid => {
                let cfg = mode
                    .to_hyper_config()
                    .expect("hyper config exists for http/sse/hybrid");
                run_hyper_server(self, cfg).await
            }
        }
    }
}
