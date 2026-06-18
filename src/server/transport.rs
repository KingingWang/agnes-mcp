//! Transport layer: stdio, HTTP, SSE, and hybrid modes.

use crate::error::{Error, Result};
use crate::server::handler::AgnesHandler;
use crate::server::AgnesServer;
use rust_mcp_sdk::{
    error::McpSdkError,
    event_store,
    mcp_server::{hyper_server, server_runtime, HyperServerOptions, McpServerOptions},
    McpServer, StdioTransport, ToMcpServerHandler, TransportOptions,
};
use std::sync::Arc;

/// Run the server over stdio.
///
/// # Errors
///
/// Returns an error if the server fails to start.
pub async fn run_stdio_server(server: &AgnesServer) -> Result<()> {
    tracing::info!("Starting Agnes MCP server (stdio transport)...");

    let server_info = server.server_info();
    let handler = AgnesHandler::new(Arc::new(server.clone()));

    let transport = StdioTransport::new(TransportOptions::default())
        .map_err(|e| Error::mcp("transport", e.to_string()))?;

    let mcp_server: Arc<rust_mcp_sdk::mcp_server::ServerRuntime> =
        server_runtime::create_server(McpServerOptions {
            server_details: server_info,
            transport,
            handler: handler.to_mcp_server_handler(),
            task_store: None,
            client_task_store: None,
            message_observer: None,
        });

    tracing::info!("Agnes MCP stdio server started, waiting for connections...");
    mcp_server
        .start()
        .await
        .map_err(|e: McpSdkError| Error::mcp("server_start", e.to_string()))?;

    Ok(())
}

/// Hyper server protocol configuration.
#[derive(Debug, Clone)]
pub struct HyperServerConfig {
    protocol_name: String,
    sse_support: bool,
}

impl HyperServerConfig {
    /// HTTP (Streamable HTTP) configuration.
    #[must_use]
    pub fn http() -> Self {
        Self {
            protocol_name: "HTTP".to_string(),
            sse_support: false,
        }
    }

    /// SSE (Server-Sent Events) configuration.
    #[must_use]
    pub fn sse() -> Self {
        Self {
            protocol_name: "SSE".to_string(),
            sse_support: true,
        }
    }

    /// Hybrid (HTTP + SSE) configuration.
    #[must_use]
    pub fn hybrid() -> Self {
        Self {
            protocol_name: "Hybrid".to_string(),
            sse_support: true,
        }
    }

    /// Protocol display name.
    #[must_use]
    pub fn protocol_name(&self) -> &str {
        &self.protocol_name
    }

    /// Whether SSE is supported.
    #[must_use]
    pub fn sse_support(&self) -> bool {
        self.sse_support
    }
}

/// Run a Hyper-based MCP server with the given configuration.
///
/// # Errors
///
/// Returns an error if the server fails to start.
pub async fn run_hyper_server(server: &AgnesServer, config: HyperServerConfig) -> Result<()> {
    let server_config = server.config();
    let server_info = server.server_info();
    let handler = AgnesHandler::new(Arc::new(server.clone()));

    tracing::info!(
        "Starting Agnes MCP {} server on {}:{}...",
        config.protocol_name(),
        server_config.server.host,
        server_config.server.port
    );

    let options = HyperServerOptions {
        host: server_config.server.host.clone(),
        port: server_config.server.port,
        transport_options: Arc::new(TransportOptions::default()),
        sse_support: config.sse_support(),
        event_store: Some(Arc::new(event_store::InMemoryEventStore::default())),
        task_store: None,
        client_task_store: None,
        allowed_hosts: Some(vec![]),
        allowed_origins: Some(vec![]),
        dns_rebinding_protection: false,
        health_endpoint: Some("/health".to_string()),
        ..Default::default()
    };

    let mcp_server =
        hyper_server::create_server(server_info, handler.to_mcp_server_handler(), options);

    let started = if config.sse_support() && config.protocol_name() != "SSE" {
        format!(
            "Agnes MCP {} server started on {}:{} (HTTP + SSE)",
            config.protocol_name(),
            server_config.server.host,
            server_config.server.port
        )
    } else {
        format!(
            "Agnes MCP {} server started on {}:{}",
            config.protocol_name(),
            server_config.server.host,
            server_config.server.port
        )
    };
    tracing::info!("{started}");

    mcp_server
        .start()
        .await
        .map_err(|e: McpSdkError| Error::mcp("server_start", e.to_string()))?;

    Ok(())
}

/// Transport mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum TransportMode {
    /// Standard input/output.
    Stdio,
    /// Streamable HTTP.
    Http,
    /// Server-Sent Events.
    Sse,
    /// HTTP + SSE.
    Hybrid,
}

impl std::str::FromStr for TransportMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "stdio" => Ok(Self::Stdio),
            "http" => Ok(Self::Http),
            "sse" => Ok(Self::Sse),
            "hybrid" => Ok(Self::Hybrid),
            _ => Err(format!("unknown transport mode: {s}")),
        }
    }
}

impl std::fmt::Display for TransportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio => write!(f, "stdio"),
            Self::Http => write!(f, "http"),
            Self::Sse => write!(f, "sse"),
            Self::Hybrid => write!(f, "hybrid"),
        }
    }
}

impl TransportMode {
    /// Convert to a [`HyperServerConfig`], or `None` for stdio.
    #[must_use]
    pub fn to_hyper_config(&self) -> Option<HyperServerConfig> {
        match self {
            Self::Stdio => None,
            Self::Http => Some(HyperServerConfig::http()),
            Self::Sse => Some(HyperServerConfig::sse()),
            Self::Hybrid => Some(HyperServerConfig::hybrid()),
        }
    }
}
