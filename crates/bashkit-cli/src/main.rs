mod python;
    /// Disable HTTP builtins (curl/wget)
    #[arg(long)]
    no_http: bool,

    /// Disable git builtin
    #[arg(long)]
    no_git: bool,

    /// Disable python builtin (monty backend)
    #[arg(long)]
    no_python: bool,

#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeConfig {
    enable_http: bool,
    enable_git: bool,
    enable_python: bool,
}

impl RuntimeConfig {
    fn from_args(args: &Args) -> Self {
        Self {
            enable_http: !args.no_http,
            enable_git: !args.no_git,
            enable_python: !args.no_python,
        }
    }
}

pub(crate) fn build_bash(config: RuntimeConfig) -> bashkit::Bash {
    let mut builder = bashkit::Bash::builder();

    if config.enable_http {
        builder = builder.network(bashkit::NetworkAllowlist::allow_all());
    }

    if config.enable_git {
        builder = builder.git(bashkit::GitConfig::new());
    }

    if config.enable_python {
        builder = builder.builtin("python", Box::new(python::PythonBuiltin::new()));
    }

    builder.build()
}

    let config = RuntimeConfig::from_args(&args);

        return mcp::run(config).await;
    let mut bash = build_bash(config);

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

    #[tokio::test]
    async fn python_enabled_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "python --version"]);
        let mut bash = build_bash(RuntimeConfig::from_args(&args));
        let result = bash.exec("python --version").await.expect("exec");
        assert_ne!(result.stderr, "python: command not found\n");
    }

    #[tokio::test]
    async fn python_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-python", "-c", "python --version"]);
        let mut bash = build_bash(RuntimeConfig::from_args(&args));
        let result = bash.exec("python --version").await.expect("exec");
        assert!(result.stderr.contains("python: command not found"));
    }
}
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
