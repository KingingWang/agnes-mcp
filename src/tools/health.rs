//! `health_check` tool — verify connectivity to the Agnes API.

#![allow(clippy::missing_docs_in_private_items)]

use crate::tools::Tool;
use async_trait::async_trait;
use rust_mcp_sdk::macros;
use rust_mcp_sdk::schema::{CallToolError, CallToolResult, Tool as McpTool};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::agnes_client::AgnesClient;

const VALID_CHECK_TYPES: &[&str] = &["all", "agnes"];

/// Parameters for the `health_check` tool.
#[macros::mcp_tool(
    name = "health_check",
    title = "Health Check",
    description = "Check the health status of the Agnes MCP server and the Agnes API endpoint. Used for diagnosing connectivity issues and monitoring availability.",
    destructive_hint = false,
    idempotent_hint = true,
    open_world_hint = false,
    read_only_hint = true
)]
#[derive(Debug, Clone, Deserialize, Serialize, macros::JsonSchema)]
pub struct HealthCheckToolParams {
    /// Type of health check: \"all\" (default) or \"agnes\".
    #[serde(default, rename = "check_type")]
    pub check_type: Option<String>,

    /// Whether to show detailed output including response time.
    #[serde(default, rename = "verbose")]
    pub verbose: Option<bool>,
}

/// `health_check` tool implementation.
pub struct HealthCheckTool {
    client: Option<Arc<AgnesClient>>,
}

impl HealthCheckTool {
    /// Create a new health check tool, optionally bound to an Agnes client.
    #[must_use]
    pub fn new(client: Arc<AgnesClient>) -> Self {
        Self {
            client: Some(client),
        }
    }
}

#[async_trait]
impl Tool for HealthCheckTool {
    fn definition(&self) -> McpTool {
        HealthCheckToolParams::tool()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let params: HealthCheckToolParams = serde_json::from_value(arguments).map_err(|e| {
            CallToolError::invalid_arguments(
                "health_check",
                Some(format!("invalid arguments: {e}")),
            )
        })?;

        let check_type = params.check_type.unwrap_or_else(|| "all".to_string());
        if !VALID_CHECK_TYPES.contains(&check_type.as_str()) {
            return Err(CallToolError::invalid_arguments(
                "health_check",
                Some(format!(
                    "invalid check_type '{check_type}'. Expected one of: {}",
                    VALID_CHECK_TYPES.join(", ")
                )),
            ));
        }
        let verbose = params.verbose.unwrap_or(false);

        let agnes_check = self.check_agnes().await;

        let overall = if agnes_check.status == "healthy" {
            "healthy"
        } else {
            "unhealthy"
        };

        let mut report = format!(
            "Status: {overall}\nTimestamp: {}\n\nCheck Results:\n- agnes: {}",
            chrono::Utc::now().to_rfc3339(),
            agnes_check.status
        );
        if let Some(d) = agnes_check.duration_ms {
            let _ = std::fmt::Write::write_fmt(&mut report, format_args!(" ({d}ms)"));
        }
        if let Some(msg) = &agnes_check.message {
            let _ = std::fmt::Write::write_fmt(&mut report, format_args!(" - {msg}"));
        }
        if let Some(err) = &agnes_check.error {
            let _ = std::fmt::Write::write_fmt(&mut report, format_args!(" [Error: {err}]"));
        }
        report.push('\n');

        if verbose {
            let json = serde_json::json!({
                "status": overall,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "checks": [agnes_check],
            });
            let _ = std::fmt::Write::write_fmt(
                &mut report,
                format_args!(
                    "\n{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                ),
            );
        }

        Ok(CallToolResult::text_content(vec![report.into()]))
    }
}

impl HealthCheckTool {
    /// Probe the Agnes API endpoint.
    async fn check_agnes(&self) -> HealthCheck {
        let Some(client) = &self.client else {
            return HealthCheck {
                name: "agnes".to_string(),
                status: "unhealthy".to_string(),
                duration_ms: None,
                message: None,
                error: Some("Agnes client not configured".to_string()),
            };
        };

        // A lightweight probe: attempt a tiny chat completion. This exercises
        // auth, the base URL, and model availability end-to-end.
        let body = serde_json::json!({
            "model": AgnesClient::text_model(),
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1,
        });

        let start = Instant::now();
        match tokio::time::timeout(Duration::from_secs(20), client.chat_completions(&body)).await {
            Ok(Ok(_)) => HealthCheck {
                name: "agnes".to_string(),
                status: "healthy".to_string(),
                duration_ms: Some(start.elapsed().as_millis()),
                message: Some("Agnes API reachable and authenticated".to_string()),
                error: None,
            },
            Ok(Err(e)) => HealthCheck {
                name: "agnes".to_string(),
                status: "unhealthy".to_string(),
                duration_ms: Some(start.elapsed().as_millis()),
                message: None,
                error: Some(e.to_string()),
            },
            Err(_) => HealthCheck {
                name: "agnes".to_string(),
                status: "unhealthy".to_string(),
                duration_ms: Some(start.elapsed().as_millis()),
                message: None,
                error: Some("Agnes API check timed out (20s)".to_string()),
            },
        }
    }
}

/// Result of a single health check.
#[derive(Debug, Clone, Serialize)]
struct HealthCheck {
    name: String,
    status: String,
    duration_ms: Option<u128>,
    message: Option<String>,
    error: Option<String>,
}
