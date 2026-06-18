//! MCP tool definitions for the Agnes AI models.
//!
//! Tools provided:
//! - [`chat`] — `agnes_chat` text/chat completions
//! - [`image`] — `agnes_generate_image` (text-to-image & image-to-image)
//! - [`image_recognition`] — `agnes_image_recognition` (vision / image understanding)
//! - [`video`] — `agnes_generate_video` and `agnes_video_status`
//! - [`prompt`] — internal prompt-enhancement helper (NOT an MCP tool; used by
//!   `agnes_generate_image` / `agnes_generate_video` when `enhance_prompt=true`)
//! - [`health`] — CLI-only health check (NOT an MCP tool)

pub mod agnes_client;
pub mod chat;
pub mod health;
pub mod image;
pub mod image_recognition;
pub mod prompt;
pub mod video;

use async_trait::async_trait;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::AgnesConfig;
use crate::error::Result;
use agnes_client::AgnesClient;

/// Trait implemented by every MCP tool.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Return the MCP tool definition (name, description, input schema).
    fn definition(&self) -> McpTool;

    /// Execute the tool with the given JSON arguments.
    ///
    /// # Errors
    ///
    /// Returns a [`CallToolError`] when the arguments are invalid or the tool
    /// fails to execute.
    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError>;
}

/// Registry mapping tool names to tool implementations.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool, keyed by its definition name.
    #[must_use]
    pub fn register<T: Tool + 'static>(mut self, tool: T) -> Self {
        let name = tool.definition().name.clone();
        self.tools.insert(name, Box::new(tool));
        self
    }

    /// Return all registered tool definitions.
    #[must_use]
    pub fn get_tools(&self) -> Vec<McpTool> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    /// Execute the named tool.
    ///
    /// # Errors
    ///
    /// Returns [`CallToolError::unknown_tool`] if the tool is not registered.
    pub async fn execute_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        match self.tools.get(name) {
            Some(tool) => tool.execute(arguments).await,
            None => Err(CallToolError::unknown_tool(name.to_string())),
        }
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default tool registry from the Agnes configuration.
///
/// # Errors
///
/// Returns an error if the HTTP client cannot be constructed.
pub fn create_default_registry(config: &AgnesConfig) -> Result<ToolRegistry> {
    let client = Arc::new(AgnesClient::new(config)?);

    Ok(ToolRegistry::new()
        .register(chat::ChatTool::new(client.clone()))
        .register(image_recognition::ImageRecognitionTool::new(client.clone()))
        .register(image::GenerateImageTool::new(client.clone()))
        .register(video::GenerateVideoTool::new(client.clone()))
        .register(video::VideoStatusTool::new(client.clone())))
}
