//! CLI command definitions.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Agnes AI MCP server.
#[derive(Parser, Debug)]
#[command(
    name = "agnes-mcp",
    version,
    about = "Agnes AI MCP server: image recognition, text-to-image, text-to-video, and more",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the MCP server.
    Serve(ServeArgs),

    /// Check the health of the Agnes API.
    Health(HealthArgs),

    /// Generate an example configuration file.
    Config(ConfigArgs),
}

/// Arguments shared by `serve` and `health`.
#[derive(Args, Debug, Clone)]
pub struct ServeArgs {
    /// Path to the TOML configuration file.
    #[arg(short, long, env = "AGNES_MCP_CONFIG", default_value = "config.toml")]
    pub config: PathBuf,

    /// Agnes API base URL.
    #[arg(long, env = "AGNES_BASE_URL")]
    pub base_url: Option<String>,

    /// Agnes API key (prefer the AGNES_API_KEY env var).
    #[arg(long, env = "AGNES_API_KEY")]
    pub api_key: Option<String>,

    /// Transport mode: stdio, http, sse, hybrid.
    #[arg(short, long, env = "AGNES_MCP_TRANSPORT")]
    pub mode: Option<String>,

    /// Listen host (http/sse/hybrid).
    #[arg(long, env = "AGNES_MCP_HOST")]
    pub host: Option<String>,

    /// Listen port (http/sse/hybrid).
    #[arg(short, long, env = "AGNES_MCP_PORT")]
    pub port: Option<u16>,

    /// Agnes chat/text model identifier.
    #[arg(long, env = "AGNES_MODEL_TEXT")]
    pub model_text: Option<String>,

    /// Agnes image generation model identifier.
    #[arg(long, env = "AGNES_MODEL_IMAGE")]
    pub model_image: Option<String>,

    /// Agnes video generation model identifier.
    #[arg(long, env = "AGNES_MODEL_VIDEO")]
    pub model_video: Option<String>,
}

/// Arguments for the `health` command.
#[derive(Args, Debug, Clone)]
pub struct HealthArgs {
    #[command(flatten)]
    pub serve: ServeArgs,

    /// Verbose output.
    #[arg(long, default_value_t = false)]
    pub verbose: bool,
}

/// Arguments for the `config` command.
#[derive(Args, Debug, Clone)]
pub struct ConfigArgs {
    /// Output file path.
    #[arg(short, long, default_value = "config.toml")]
    pub output: PathBuf,

    /// Overwrite an existing file.
    #[arg(short, long, default_value_t = false)]
    pub force: bool,
}
