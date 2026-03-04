// Decision: enable http, git, python by default for CLI users.
// Provide --no-http, --no-git, --no-python to disable individually.

//! Bashkit CLI - Command line interface for virtual bash execution
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

/// Bashkit - Virtual bash interpreter
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

    /// Disable HTTP builtins (curl/wget)
    #[arg(long)]
    no_http: bool,

    /// Disable git builtin
    #[arg(long)]
    no_git: bool,

    /// Disable python builtin (monty backend)
    #[arg(long)]
    no_python: bool,

    #[command(subcommand)]
    subcommand: Option<SubCmd>,
}

#[derive(Subcommand, Debug)]
enum SubCmd {
    /// Run as MCP (Model Context Protocol) server
    Mcp,
}

fn build_bash(args: &Args) -> bashkit::Bash {
    let mut builder = bashkit::Bash::builder();

    if !args.no_http {
        builder = builder.network(bashkit::NetworkAllowlist::allow_all());
    }

    if !args.no_git {
        builder = builder.git(bashkit::GitConfig::new());
    }

    if !args.no_python {
        builder = builder.python();
    }

    builder.build()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands first
    if let Some(SubCmd::Mcp) = args.subcommand {
        return mcp::run().await;
    }

    let mut bash = build_bash(&args);

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_disable_flags() {
        let args = Args::parse_from([
            "bashkit",
            "--no-http",
            "--no-git",
            "--no-python",
            "-c",
            "echo hi",
        ]);
        assert!(args.no_http);
        assert!(args.no_git);
        assert!(args.no_python);
    }

    #[test]
    fn defaults_all_enabled() {
        let args = Args::parse_from(["bashkit", "-c", "echo hi"]);
        assert!(!args.no_http);
        assert!(!args.no_git);
        assert!(!args.no_python);
    }

    #[tokio::test]
    async fn python_enabled_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "python --version"]);
        let mut bash = build_bash(&args);
        let result = bash.exec("python --version").await.expect("exec");
        assert_ne!(result.stderr, "python: command not found\n");
    }

    #[tokio::test]
    async fn python_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-python", "-c", "python --version"]);
        let mut bash = build_bash(&args);
        let result = bash.exec("python --version").await.expect("exec");
        assert!(result.stderr.contains("command not found"));
    }

    #[tokio::test]
    async fn git_enabled_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "git init /repo"]);
        let mut bash = build_bash(&args);
        let result = bash.exec("git init /repo").await.expect("exec");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn git_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-git", "-c", "git init /repo"]);
        let mut bash = build_bash(&args);
        let result = bash.exec("git init /repo").await.expect("exec");
        assert!(result.stderr.contains("not configured"));
    }

    #[tokio::test]
    async fn http_enabled_by_default() {
        // curl should be recognized (not "command not found") even if network fails
        let args = Args::parse_from(["bashkit", "-c", "curl --help"]);
        let mut bash = build_bash(&args);
        let result = bash.exec("curl --help").await.expect("exec");
        assert!(!result.stderr.contains("command not found"));
    }

    #[tokio::test]
    async fn http_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-http", "-c", "curl https://example.com"]);
        let mut bash = build_bash(&args);
        let result = bash.exec("curl https://example.com").await.expect("exec");
        assert!(result.stderr.contains("not configured"));
    }

    #[tokio::test]
    async fn all_disabled_still_runs_basic_commands() {
        let args = Args::parse_from([
            "bashkit",
            "--no-http",
            "--no-git",
            "--no-python",
            "-c",
            "echo works",
        ]);
        let mut bash = build_bash(&args);
        let result = bash.exec("echo works").await.expect("exec");
        assert_eq!(result.stdout, "works\n");
        assert_eq!(result.exit_code, 0);
    }
}
