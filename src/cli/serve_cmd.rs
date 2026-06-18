//! Implementation of the `serve`, `health`, and `config` commands.

use crate::cli::commands::{ConfigArgs, HealthArgs, ServeArgs};
use crate::config::AppConfig;
use crate::tools::create_default_registry;
use crate::AgnesServer;
use std::path::Path;

/// Apply CLI argument overrides onto the loaded configuration.
fn apply_cli_overrides(config: &mut AppConfig, args: &ServeArgs) {
    if let Some(base) = &args.base_url {
        config.agnes.base_url = base.trim_end_matches('/').to_string();
    }
    if let Some(key) = &args.api_key {
        if !key.is_empty() {
            config.agnes.api_key = Some(key.clone());
        }
    }
    if let Some(mode) = &args.mode {
        config.server.transport_mode.clone_from(mode);
    }
    if let Some(host) = &args.host {
        config.server.host.clone_from(host);
    }
    if let Some(port) = args.port {
        config.server.port = port;
    }
}

/// Load configuration from file, env, and CLI overrides.
fn load_config(args: &ServeArgs) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let mut config = if args.config.exists() {
        tracing::info!("loading configuration from {}", args.config.display());
        AppConfig::from_file(&args.config)?
    } else {
        tracing::warn!(
            "config file '{}' not found, using defaults",
            args.config.display()
        );
        AppConfig::default()
    };

    config.apply_env();
    apply_cli_overrides(&mut config, args);
    config.validate()?;
    Ok(config)
}

/// Run the `serve` command.
///
/// # Errors
///
/// Returns an error if configuration loading, server creation, or serving fails.
pub async fn run_serve_command(args: ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&args)?;

    crate::init_logging(&config.logging.level)?;

    tracing::info!("starting Agnes MCP server v{}", crate::VERSION);
    tracing::info!(
        "agnes base_url: {}, transport: {}",
        config.agnes.base_url,
        config.server.transport_mode
    );

    let server = AgnesServer::new(config.clone())?;
    server.run_with_configured_mode().await?;
    Ok(())
}

/// Run the `health` command.
///
/// # Errors
///
/// Returns an error if configuration loading or the health check fails.
pub async fn run_health_command(args: HealthArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&args.serve)?;
    crate::init_logging("warn")?;

    // Build a registry purely to obtain a client-backed health check.
    let registry = create_default_registry(&config.agnes)?;
    let tools = registry.get_tools();
    println!(
        "Agnes MCP server: {} tools registered ({}).",
        tools.len(),
        tools
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    );

    match registry
        .execute_tool(
            "health_check",
            serde_json::json!({ "check_type": "all", "verbose": args.verbose }),
        )
        .await
    {
        Ok(result) => {
            for block in result.content {
                if let Ok(text) = block.as_text_content() {
                    println!("{}", text.text);
                }
            }
        }
        Err(e) => {
            eprintln!("health check error: {e}");
        }
    }
    Ok(())
}

/// Run the `config` command: write an example configuration file.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn run_config_command(args: &ConfigArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.output.exists() && !args.force {
        return Err(format!(
            "file '{}' already exists; use --force to overwrite",
            args.output.display()
        )
        .into());
    }
    write_example_config(&args.output)?;
    println!("example configuration written to {}", args.output.display());
    Ok(())
}

/// The bundled example configuration text.
pub const EXAMPLE_CONFIG: &str = include_str!("../../config.example.toml");

/// Write the example configuration to a path.
fn write_example_config(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, EXAMPLE_CONFIG)
}
