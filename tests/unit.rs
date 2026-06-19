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

#[test]
fn default_disabled_tools_is_empty() {
    let cfg = AppConfig::default();
    assert!(cfg.agnes.disabled_tools.is_empty());
}

#[test]
fn registry_skips_disabled_tools() {
    use agnes_mcp::tools::{create_default_registry, AVAILABLE_TOOLS};
    let mut cfg = AppConfig::default();
    cfg.agnes.api_key = Some("sk-test".to_string());

    // Default: every built-in tool registered.
    let registry = create_default_registry(&cfg.agnes).expect("registry");
    assert_eq!(registry.len(), AVAILABLE_TOOLS.len());

    // Disable two tools.
    cfg.agnes.disabled_tools = vec![
        "agnes_generate_video".to_string(),
        "agnes_video_status".to_string(),
    ];
    let registry = create_default_registry(&cfg.agnes).expect("registry");
    assert_eq!(registry.len(), AVAILABLE_TOOLS.len() - 2);

    let names: Vec<String> = registry
        .get_tools()
        .into_iter()
        .map(|t| t.name.clone())
        .collect();
    assert!(names.contains(&"agnes_image_recognition".to_string()));
    assert!(names.contains(&"agnes_generate_image".to_string()));
    assert!(!names.contains(&"agnes_generate_video".to_string()));
    assert!(!names.contains(&"agnes_video_status".to_string()));
}

#[test]
fn registry_ignores_unknown_disabled_names() {
    use agnes_mcp::tools::{create_default_registry, AVAILABLE_TOOLS};
    let mut cfg = AppConfig::default();
    cfg.agnes.api_key = Some("sk-test".to_string());
    cfg.agnes.disabled_tools = vec!["agnes_unknown".to_string(), "  ".to_string()];
    let registry = create_default_registry(&cfg.agnes).expect("registry");
    // Unknown names ignored; whitespace-only entries dropped.
    assert_eq!(registry.len(), AVAILABLE_TOOLS.len());
}
