//! Integration-level unit tests for the agnes-mcp crate.

use agnes_mcp::config::AppConfig;

#[test]
fn default_config_has_agnes_models() {
    let cfg = AppConfig::default();
    assert_eq!(cfg.agnes.base_url, "https://apihub.agnes-ai.com");
    assert!(cfg.agnes.api_key.is_none());
    assert_eq!(cfg.server.transport_mode, "stdio");
}

#[test]
fn validate_requires_key() {
    let mut cfg = AppConfig::default();
    assert!(cfg.validate().is_err());
    cfg.agnes.api_key = Some("sk-x".to_string());
    assert!(cfg.validate().is_ok());
}

#[test]
fn user_agent_contains_name_and_version() {
    let ua = agnes_mcp::user_agent();
    assert!(ua.starts_with("AgnesMCP/"));
}
