//! cortex-mem-hook — Claude Code hook binary.
//!
//! Reads JSON from stdin, dispatches to the appropriate handler based on
//! the subcommand, and outputs JSON to stdout.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cortex-mem-hook")]
#[command(about = "Claude Code hook handler for cortex-mem")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Handle SessionStart event
    SessionStart,
    /// Handle UserPromptSubmit event
    PromptSubmit,
    /// Handle PostToolUse event
    PostToolUse,
    /// Handle Stop event
    Stop,
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let input = cortex_mem::hooks::read_stdin();
    let client = cortex_mem::hooks::hook_client();

    let output = match cli.command {
        Commands::SessionStart => cortex_mem::hooks::session_start::handle(input, &client).await,
        Commands::PromptSubmit => cortex_mem::hooks::prompt_submit::handle(input, &client).await,
        Commands::PostToolUse => cortex_mem::hooks::post_tool_use::handle(input, &client).await,
        Commands::Stop => cortex_mem::hooks::stop::handle(input, &client).await,
    };

    // Output JSON to stdout
    if let Ok(json) = serde_json::to_string(&output) {
        println!("{}", json);
    }
}
