//! Agnes API health check — used by the CLI `health` command.
//!
//! Note: this is NOT registered as an MCP tool. AI agents diagnose service
//! issues from tool-call errors directly. This module exists solely for human
//! operators running `agnes-mcp health` to verify configuration, connectivity,
//! and authentication before starting the server.

use crate::tools::agnes_client::AgnesClient;
use chrono::Utc;
use serde::Serialize;
use std::time::{Duration, Instant};

/// Run an Agnes API health check and return a human-readable report.
///
/// Probes the Agnes API with a tiny chat completion, exercising auth, the base
/// URL, and model availability end-to-end.
pub async fn run_health_check(client: &AgnesClient, verbose: bool) -> String {
    let agnes_check = check_agnes(client).await;

    let overall = if agnes_check.status == "healthy" {
        "healthy"
    } else {
        "unhealthy"
    };

    let mut report = format!(
        "Status: {overall}\nTimestamp: {}\n\nCheck Results:\n- agnes: {}",
        Utc::now().to_rfc3339(),
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
            "timestamp": Utc::now().to_rfc3339(),
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

    report
}

/// Probe the Agnes API endpoint.
async fn check_agnes(client: &AgnesClient) -> HealthCheck {
    // A lightweight probe: attempt a tiny chat completion. This exercises
    // auth, the base URL, and model availability end-to-end.
    let body = serde_json::json!({
        "model": client.text_model(),
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

/// Result of the Agnes API health check.
#[derive(Debug, Clone, Serialize)]
struct HealthCheck {
    name: String,
    status: String,
    duration_ms: Option<u128>,
    message: Option<String>,
    error: Option<String>,
}
