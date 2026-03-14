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
    #[cfg_attr(not(feature = "python"), arg(long, hide = true))]
    #[cfg_attr(feature = "python", arg(long))]
    no_python: bool,

    /// Mount a host directory as readonly in the VFS (format: HOST_PATH or HOST_PATH:VFS_PATH)
    ///
    /// Examples:
    ///   --mount-ro /path/to/project           # overlay at VFS root
    ///   --mount-ro /path/to/data:/mnt/data    # mount at /mnt/data
    #[cfg_attr(not(feature = "realfs"), arg(long, hide = true))]
    #[cfg_attr(feature = "realfs", arg(long, value_name = "PATH"))]
    mount_ro: Vec<String>,

    /// Mount a host directory as read-write in the VFS (format: HOST_PATH or HOST_PATH:VFS_PATH)
    ///
    /// WARNING: This breaks the sandbox boundary. Scripts can modify host files.
    ///
    /// Examples:
    ///   --mount-rw /path/to/workspace           # overlay at VFS root
    ///   --mount-rw /path/to/output:/mnt/output  # mount at /mnt/output
    #[cfg_attr(not(feature = "realfs"), arg(long, hide = true))]
    #[cfg_attr(feature = "realfs", arg(long, value_name = "PATH"))]
    mount_rw: Vec<String>,

    /// Maximum number of commands to execute (default: 10000)
    #[arg(long)]
    max_commands: Option<usize>,

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

    #[cfg(feature = "python")]
    if !args.no_python {
        builder = builder.python();
    }

    #[cfg(feature = "realfs")]
    {
        builder = apply_real_mounts(builder, &args.mount_ro, &args.mount_rw);
    }

    if let Some(max_cmds) = args.max_commands {
        builder = builder.limits(bashkit::ExecutionLimits::new().max_commands(max_cmds));
    }

    builder.build()
}

/// Parse mount specs (HOST_PATH or HOST_PATH:VFS_PATH) and apply to builder.
#[cfg(feature = "realfs")]
fn apply_real_mounts(
    mut builder: bashkit::BashBuilder,
    ro_mounts: &[String],
    rw_mounts: &[String],
) -> bashkit::BashBuilder {
    for spec in ro_mounts {
        if let Some((host, vfs)) = spec.split_once(':') {
            builder = builder.mount_real_readonly_at(host, vfs);
        } else {
            builder = builder.mount_real_readonly(spec);
        }
    }
    for spec in rw_mounts {
        if let Some((host, vfs)) = spec.split_once(':') {
            builder = builder.mount_real_readwrite_at(host, vfs);
        } else {
            builder = builder.mount_real_readwrite(spec);
        }
    }
    builder
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

    #[cfg(feature = "python")]
    #[tokio::test]
    async fn python_enabled_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "python --version"]);
        let mut bash = build_bash(&args);
        let result = bash.exec("python --version").await.expect("exec");
        assert_ne!(result.stderr, "python: command not found\n");
    }

    #[cfg(feature = "python")]
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

    #[cfg(feature = "realfs")]
    #[test]
    fn parse_mount_flags() {
        let args = Args::parse_from([
            "bashkit",
            "--mount-ro",
            "/tmp/data:/mnt/data",
            "--mount-rw",
            "/tmp/out",
            "-c",
            "echo hi",
        ]);
        assert_eq!(args.mount_ro, vec!["/tmp/data:/mnt/data"]);
        assert_eq!(args.mount_rw, vec!["/tmp/out"]);
    }

    #[cfg(feature = "realfs")]
    #[tokio::test]
    async fn mount_ro_reads_host_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "from host\n").unwrap();
        let spec = format!("{}:/mnt/data", dir.path().display());

        let args = Args::parse_from([
            "bashkit",
            "--mount-ro",
            &spec,
            "-c",
            "cat /mnt/data/test.txt",
        ]);
        let mut bash = build_bash(&args);
        let result = bash.exec("cat /mnt/data/test.txt").await.expect("exec");
        assert_eq!(result.stdout, "from host\n");
    }

    #[cfg(feature = "realfs")]
    #[tokio::test]
    async fn mount_rw_writes_host_files() {
        let dir = tempfile::tempdir().unwrap();
        let spec = format!("{}:/mnt/out", dir.path().display());

        let args = Args::parse_from([
            "bashkit",
            "--mount-rw",
            &spec,
            "-c",
            "echo result > /mnt/out/r.txt",
        ]);
        let mut bash = build_bash(&args);
        bash.exec("echo result > /mnt/out/r.txt")
            .await
            .expect("exec");

        let content = std::fs::read_to_string(dir.path().join("r.txt")).unwrap();
        assert_eq!(content, "result\n");
    }
}
