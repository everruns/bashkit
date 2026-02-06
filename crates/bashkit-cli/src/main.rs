//! Bashkit CLI - Command line interface for sandboxed bash execution
//!
//! Usage:
//!   bashkit -c 'echo hello'        # Execute a command string
//!   bashkit script.sh              # Execute a script file
//!   bashkit mcp                    # Run as MCP server
//!   bashkit                        # Interactive REPL (not yet implemented)

mod mcp;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Bashkit - Sandboxed bash interpreter
#[derive(Parser, Debug)]
#[command(name = "bashkit")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Execute the given command string
    #[arg(short = 'c')]
    command: Option<String>,

    /// Script file to execute
    #[arg()]
    script: Option<PathBuf>,

    /// Arguments to pass to the script
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,

    #[command(subcommand)]
    subcommand: Option<SubCmd>,
}

#[derive(Subcommand, Debug)]
enum SubCmd {
    /// Run as MCP (Model Context Protocol) server
    Mcp,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands first
    if let Some(SubCmd::Mcp) = args.subcommand {
        return mcp::run().await;
    }

    let mut bash = bashkit::Bash::new();

    // Execute command string if provided
    if let Some(cmd) = args.command {
        let result = bash.exec(&cmd).await.context("Failed to execute command")?;
        print!("{}", result.stdout);
        if !result.stderr.is_empty() {
            eprint!("{}", result.stderr);
        }
        std::process::exit(result.exit_code);
    }

    // Execute script file if provided
    if let Some(script_path) = args.script {
        let script = std::fs::read_to_string(&script_path)
            .with_context(|| format!("Failed to read script: {}", script_path.display()))?;

        let result = bash
            .exec(&script)
            .await
            .context("Failed to execute script")?;
        print!("{}", result.stdout);
        if !result.stderr.is_empty() {
            eprint!("{}", result.stderr);
        }
        std::process::exit(result.exit_code);
    }

    // Interactive REPL (not yet implemented)
    eprintln!("bashkit: interactive mode not yet implemented");
    eprintln!("Usage: bashkit -c 'command' or bashkit script.sh or bashkit mcp");
    std::process::exit(1);
}
