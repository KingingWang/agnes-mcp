//! Command-line interface.

pub mod commands;
mod serve_cmd;

use crate::cli::commands::{Cli, Commands};
use std::process::ExitCode;

/// Run the CLI.
///
/// # Errors
///
/// Returns an error if a command fails.
pub async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Serve(args) => serve_cmd::run_serve_command(args).await?,
        Commands::Health(args) => serve_cmd::run_health_command(args).await?,
        Commands::Config(args) => serve_cmd::run_config_command(&args)?,
    }
    Ok(())
}

/// Convenience wrapper that runs the CLI and maps errors to an exit code.
///
/// # Errors
///
/// Never returns `Err`; encodes failure in the returned [`ExitCode`].
pub async fn run_exit_code(cli: Cli) -> ExitCode {
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
