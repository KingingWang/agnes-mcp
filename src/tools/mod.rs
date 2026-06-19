//! MCP tool definitions for the Agnes AI models.
//!
//! Tools provided:
//! - [`image`] — `agnes_generate_image` (text-to-image & image-to-image)
//! - [`image_recognition`] — `agnes_image_recognition` (vision / image understanding)
//! - [`video`] — `agnes_generate_video` and `agnes_video_status`
//! - [`prompt`] — internal prompt-enhancement helper (NOT an MCP tool; used by
//!   `agnes_generate_image` / `agnes_generate_video` when `enhance_prompt=true`)
//! - [`health`] — CLI-only health check (NOT an MCP tool)

pub mod agnes_client;
pub mod health;
pub mod image;
pub mod image_recognition;
pub mod prompt;
pub mod video;

// ---------------------------------------------------------------------------
// Built-in tool name constants
// ---------------------------------------------------------------------------
//
// Every MCP tool exposed by this server has a stable string identifier. They
// are centralized here so that:
//   - users can reference them in `disabled_tools` config / `--disable-tool`
//     CLI flag with the exact spelling,
//   - the registry builder can validate / skip disabled tools without
//     duplicating string literals,
//   - tests and docs can iterate over the canonical list via [`AVAILABLE_TOOLS`].

/// Tool name: Agnes image recognition (vision).
pub const TOOL_IMAGE_RECOGNITION: &str = "agnes_image_recognition";
/// Tool name: Agnes image generation (text-to-image & image-to-image).
pub const TOOL_GENERATE_IMAGE: &str = "agnes_generate_image";
/// Tool name: Agnes video generation (text-to-video & image-to-video).
pub const TOOL_GENERATE_VIDEO: &str = "agnes_generate_video";
/// Tool name: Agnes video task status polling.
pub const TOOL_VIDEO_STATUS: &str = "agnes_video_status";

/// All built-in MCP tool names exposed by this server, in registration order.
pub const AVAILABLE_TOOLS: &[&str] = &[
    TOOL_IMAGE_RECOGNITION,
    TOOL_GENERATE_IMAGE,
    TOOL_GENERATE_VIDEO,
    TOOL_VIDEO_STATUS,
];

/// Returns `true` if `name` matches a built-in tool name.
///
/// # Examples
///
/// ```
/// # use agnes_mcp::tools::is_tool_name;
/// assert!(is_tool_name("agnes_generate_image"));
/// assert!(!is_tool_name("agnes_unknown"));
/// ```
#[must_use]
pub fn is_tool_name(name: &str) -> bool {
    AVAILABLE_TOOLS.contains(&name)
}

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
/// Tools whose names appear in [`AgnesConfig::disabled_tools`] are skipped.
/// Unknown names in the disabled list are ignored with a `warn!` log line so
/// that a typo never silently disables the wrong tool. The disabled list is
/// matched case-sensitively against the canonical tool names listed in
/// [`AVAILABLE_TOOLS`].
///
/// # Errors
///
/// Returns an error if the HTTP client cannot be constructed.
pub fn create_default_registry(config: &AgnesConfig) -> Result<ToolRegistry> {
    let client = Arc::new(AgnesClient::new(config)?);

    // Normalize the disabled list once: trim, drop empties, dedupe in order.
    let mut disabled: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in &config.disabled_tools {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if !is_tool_name(name) {
            tracing::warn!(
                "ignoring unknown tool name in disabled_tools: {name:?}                  (known tools: {})",
                AVAILABLE_TOOLS.join(", ")
            );
            continue;
        }
        if seen.insert(name.to_string()) {
            disabled.push(name.to_string());
        }
    }

    if !disabled.is_empty() {
        tracing::info!(
            "disabling {} tool(s): {}",
            disabled.len(),
            disabled.join(", ")
        );
    }

    let mut registry = ToolRegistry::new();
    if !disabled.iter().any(|d| d == TOOL_IMAGE_RECOGNITION) {
        registry = registry.register(image_recognition::ImageRecognitionTool::new(client.clone()));
    }
    if !disabled.iter().any(|d| d == TOOL_GENERATE_IMAGE) {
        registry = registry.register(image::GenerateImageTool::new(client.clone()));
    }
    if !disabled.iter().any(|d| d == TOOL_GENERATE_VIDEO) {
        registry = registry.register(video::GenerateVideoTool::new(client.clone()));
    }
    if !disabled.iter().any(|d| d == TOOL_VIDEO_STATUS) {
        registry = registry.register(video::VideoStatusTool::new(client.clone()));
    }
    Ok(registry)
}
