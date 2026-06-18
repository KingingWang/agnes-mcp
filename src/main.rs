//! Agnes AI MCP server — main entry point.

use agnes_mcp::cli::{commands::Cli, run_exit_code};
use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Restore the default SIGPIPE disposition so piping output into tools like
    // `head` or `less` terminates cleanly instead of panicking on a broken pipe.
    reset_sigpipe();
    let cli = Cli::parse();
    run_exit_code(cli).await
}

/// Reset SIGPIPE to its default action on Unix.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: setting a signal handler to the default disposition is a simple,
    // well-defined libc call with no memory-safety implications.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
fn reset_sigpipe() {}
