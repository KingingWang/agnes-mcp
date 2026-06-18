//! MCP request handler implementation.

use async_trait::async_trait;
use rust_mcp_sdk::{
    mcp_server::ServerHandler,
    schema::{
        CallToolError, CallToolRequestParams, CallToolResult, GetPromptRequestParams,
        GetPromptResult, ListPromptsResult, ListResourcesResult, ListToolsResult,
        PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult, RpcError,
    },
    McpServer,
};
use std::sync::Arc;
use tracing::{info_span, Instrument};
use uuid::Uuid;

use crate::server::AgnesServer;

/// MCP server handler, routing requests to the tool registry.
pub struct AgnesHandler {
    server: Arc<AgnesServer>,
}

impl AgnesHandler {
    /// Create a new handler bound to a server.
    #[must_use]
    pub fn new(server: Arc<AgnesServer>) -> Self {
        Self { server }
    }

    /// Return the list of available tools.
    #[must_use]
    pub fn list_tools(&self) -> ListToolsResult {
        ListToolsResult {
            tools: self.server.tool_registry().get_tools(),
            meta: None,
            next_cursor: None,
        }
    }
}

#[async_trait]
impl ServerHandler for AgnesHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        let trace_id = Uuid::new_v4();
        let span = info_span!("list_tools", trace_id = %trace_id);
        async {
            tracing::debug!("listing available tools");
            let result = self.list_tools();
            tracing::debug!("found {} tools", result.tools.len());
            Ok(result)
        }
        .instrument(span)
        .await
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let trace_id = Uuid::new_v4();
        let tool_name = params.name.clone();
        let span = info_span!("execute_tool", trace_id = %trace_id, tool = %tool_name);

        async {
            tracing::info!("executing tool: {tool_name}");
            let start = std::time::Instant::now();

            // An omitted `arguments` field is valid per the MCP spec. Default
            // to an empty object so tools whose parameters are all optional
            // still deserialize with their defaults.
            let arguments = serde_json::Value::Object(params.arguments.unwrap_or_default());

            let result = self
                .server
                .tool_registry()
                .execute_tool(&tool_name, arguments)
                .await;

            match &result {
                Ok(_) => {
                    tracing::info!("tool {tool_name} succeeded in {:?}", start.elapsed());
                }
                Err(e) => {
                    tracing::error!("tool {tool_name} failed in {:?}: {e:?}", start.elapsed());
                }
            }
            result
        }
        .instrument(span)
        .await
    }

    async fn handle_list_resources_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListResourcesResult, RpcError> {
        Ok(ListResourcesResult {
            resources: vec![],
            meta: None,
            next_cursor: None,
        })
    }

    async fn handle_read_resource_request(
        &self,
        _params: ReadResourceRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ReadResourceResult, RpcError> {
        Err(RpcError::invalid_request().with_message("resource not found".to_string()))
    }

    async fn handle_list_prompts_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListPromptsResult, RpcError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            meta: None,
            next_cursor: None,
        })
    }

    async fn handle_get_prompt_request(
        &self,
        _params: GetPromptRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<GetPromptResult, RpcError> {
        Err(RpcError::invalid_request().with_message("prompt not found".to_string()))
    }
}
