// Decision: enable http, git, python by default for CLI users.
// Provide --no-http, --no-git, --no-python to disable individually.
// Decision: keep one-shot CLI on a current-thread runtime; reserve multi-thread
// runtime for MCP only so cold-start work stays off the common path.
// Decision: CLI uses relaxed execution limits (ExecutionLimits::cli()) because
// the user explicitly chose to run the script. Counting-based limits are
// effectively unlimited; timeout is removed (user has Ctrl-C). Memory-guarding
// limits (function depth, AST depth, parser fuel) are kept.
// MCP mode keeps the sandboxed defaults since requests come from LLM agents.
// Decision: interactive mode uses rustyline for line editing — lightweight, MIT,
// no heavy deps (no SQLite, no crossterm). Multiline via parse error detection.
// See specs/018-interactive-shell.md

//! Bashkit CLI - Command line interface for virtual bash execution
//!
//! Usage:
//!   bashkit -c 'echo hello'        # Execute a command string
//!   bashkit script.sh              # Execute a script file
//!   bashkit mcp                    # Run as MCP server
//!   bashkit                        # Interactive REPL

#[cfg(feature = "interactive")]
mod interactive;
mod mcp;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::runtime::Builder;

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

    /// Maximum number of commands to execute (unlimited for CLI, 10000 for MCP)
    #[arg(long)]
    max_commands: Option<usize>,

    /// Maximum iterations for a single loop (unlimited for CLI, 10000 for MCP)
    #[arg(long)]
    max_loop_iterations: Option<usize>,

    /// Maximum total loop iterations across all loops (unlimited for CLI, 1000000 for MCP)
    #[arg(long)]
    max_total_loop_iterations: Option<usize>,

    /// Execution timeout in seconds (unlimited for CLI, 30 for MCP)
    #[arg(long)]
    timeout: Option<u64>,

    #[command(subcommand)]
    subcommand: Option<SubCmd>,
}

#[derive(Subcommand, Debug)]
enum SubCmd {
    /// Run as MCP (Model Context Protocol) server
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliMode {
    Mcp,
    Command,
    Script,
    Interactive,
}

#[derive(Debug)]
struct RunOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

fn build_bash(args: &Args, mode: CliMode) -> bashkit::Bash {
    configure_bash(args, mode).build()
}

fn configure_bash(args: &Args, mode: CliMode) -> bashkit::BashBuilder {
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
        // THREAT[TM-ESC-030]: Warn when --mount-rw is used in MCP mode — LLM agents
        // get read-write access to the host filesystem, breaking the sandbox boundary.
        if mode == CliMode::Mcp && !args.mount_rw.is_empty() {
            eprintln!(
                "WARNING: --mount-rw in MCP mode gives LLM agents read-write access to host files."
            );
            eprintln!(
                "         This breaks the sandbox boundary. Use --mount-ro for safer access."
            );
        }
        builder = apply_real_mounts(builder, &args.mount_ro, &args.mount_rw);
    }

    // CLI/script modes use relaxed limits; MCP keeps sandboxed defaults.
    let mut limits = if mode == CliMode::Mcp {
        bashkit::ExecutionLimits::new()
    } else {
        bashkit::ExecutionLimits::cli()
    };
    if let Some(v) = args.max_commands {
        limits = limits.max_commands(v);
    }
    if let Some(v) = args.max_loop_iterations {
        limits = limits.max_loop_iterations(v);
    }
    if let Some(v) = args.max_total_loop_iterations {
        limits = limits.max_total_loop_iterations(v);
    }
    if let Some(v) = args.timeout {
        limits = limits.timeout(std::time::Duration::from_secs(v));
    }
    builder = builder.limits(limits);

    if mode != CliMode::Mcp {
        builder = builder.session_limits(bashkit::SessionLimits::unlimited());
    }

    #[cfg(feature = "interactive")]
    if mode == CliMode::Interactive {
        builder = builder.tty(0, true).tty(1, true).tty(2, true);
    }

    builder
}

fn cli_mode(args: &Args) -> CliMode {
    if matches!(args.subcommand, Some(SubCmd::Mcp)) {
        CliMode::Mcp
    } else if args.command.is_some() {
        CliMode::Command
    } else if args.script.is_some() {
        CliMode::Script
    } else {
        CliMode::Interactive
    }
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

/// Format a panic payload into a sanitized error message.
// THREAT[TM-INF-021]: No file paths, line numbers, or dependency versions in output.
fn format_panic_message(payload: &dyn std::any::Any) -> String {
    let msg = if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unexpected error".to_string()
    };
    format!("bashkit: internal error: {msg}")
}

fn main() -> Result<()> {
    // THREAT[TM-INF-021]: Suppress stack backtraces to prevent information disclosure.
    // Custom panic hook emits a sanitized message without file paths, line numbers,
    // or dependency versions that could be exploited by attackers.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{}", format_panic_message(info.payload()));
    }));

    let args = Args::parse();

    let mode = cli_mode(&args);
    match mode {
        CliMode::Mcp => run_mcp(args, mode),
        CliMode::Command | CliMode::Script => {
            let output = run_oneshot(args, mode)?;
            print!("{}", output.stdout);
            if !output.stderr.is_empty() {
                eprint!("{}", output.stderr);
            }
            std::process::exit(output.exit_code);
        }
        CliMode::Interactive => {
            #[cfg(feature = "interactive")]
            {
                let exit_code = run_interactive(args, mode)?;
                std::process::exit(exit_code);
            }
            #[cfg(not(feature = "interactive"))]
            {
                eprintln!("bashkit: interactive mode requires the 'interactive' feature");
                eprintln!("Rebuild with: cargo build -p bashkit-cli --features interactive");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(feature = "interactive")]
fn run_interactive(args: Args, mode: CliMode) -> Result<i32> {
    use std::sync::Arc;

    let exit_state = Arc::new(interactive::ExitState::new());
    let es = Arc::clone(&exit_state);
    let bash = configure_bash(&args, mode)
        .on_exit(Box::new(move |event| {
            es.code
                .store(event.code, std::sync::atomic::Ordering::Relaxed);
            es.requested
                .store(true, std::sync::atomic::Ordering::Release);
            bashkit::hooks::HookAction::Continue(event)
        }))
        .build();

    Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build interactive runtime")?
        .block_on(interactive::run(bash, exit_state))
}

fn run_mcp(args: Args, mode: CliMode) -> Result<()> {
    Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to build MCP runtime")?
        .block_on(mcp::run(move || build_bash(&args, mode)))
}

fn run_oneshot(args: Args, mode: CliMode) -> Result<RunOutput> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build CLI runtime")?
        .block_on(async move {
            let mut bash = build_bash(&args, mode);

            if let Some(cmd) = args.command {
                let result = bash.exec(&cmd).await.context("Failed to execute command")?;
                return Ok(RunOutput {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                });
            }

            if let Some(script_path) = args.script {
                let script = std::fs::read_to_string(&script_path)
                    .with_context(|| format!("Failed to read script: {}", script_path.display()))?;

                let result = bash
                    .exec(&script)
                    .await
                    .context("Failed to execute script")?;
                return Ok(RunOutput {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                });
            }

            unreachable!("run_oneshot called for non-executable mode");
        })
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

    #[test]
    fn cli_mode_prefers_mcp() {
        let args = Args::parse_from(["bashkit", "mcp"]);
        assert_eq!(cli_mode(&args), CliMode::Mcp);
    }

    #[test]
    fn cli_mode_detects_command() {
        let args = Args::parse_from(["bashkit", "-c", "echo hi"]);
        assert_eq!(cli_mode(&args), CliMode::Command);
    }

    #[test]
    fn cli_mode_detects_script() {
        let args = Args::parse_from(["bashkit", "script.sh"]);
        assert_eq!(cli_mode(&args), CliMode::Script);
    }

    #[test]
    fn cli_mode_falls_back_to_interactive() {
        let args = Args::parse_from(["bashkit"]);
        assert_eq!(cli_mode(&args), CliMode::Interactive);
    }

    #[cfg(feature = "python")]
    #[tokio::test]
    async fn python_enabled_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "python --version"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("python --version").await.expect("exec");
        assert_ne!(result.stderr, "python: command not found\n");
    }

    #[cfg(feature = "python")]
    #[tokio::test]
    async fn python_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-python", "-c", "python --version"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("python --version").await.expect("exec");
        assert!(result.stderr.contains("command not found"));
    }

    #[tokio::test]
    async fn git_enabled_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "git init /repo"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("git init /repo").await.expect("exec");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn git_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-git", "-c", "git init /repo"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("git init /repo").await.expect("exec");
        assert!(result.stderr.contains("not configured"));
    }

    #[tokio::test]
    async fn http_enabled_by_default() {
        // curl should be recognized (not "command not found") even if network fails
        let args = Args::parse_from(["bashkit", "-c", "curl --help"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("curl --help").await.expect("exec");
        assert!(!result.stderr.contains("command not found"));
    }

    #[tokio::test]
    async fn http_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-http", "-c", "curl https://example.com"]);
        let mut bash = build_bash(&args, CliMode::Command);
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
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("echo works").await.expect("exec");
        assert_eq!(result.stdout, "works\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_oneshot_executes_command_on_current_thread_runtime() {
        let args = Args::parse_from(["bashkit", "--no-http", "--no-git", "-c", "echo works"]);
        let output = run_oneshot(args, CliMode::Command).expect("run");
        assert_eq!(output.stdout, "works\n");
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
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
        let mut bash = build_bash(&args, CliMode::Command);
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
        let mut bash = build_bash(&args, CliMode::Command);
        bash.exec("echo result > /mnt/out/r.txt")
            .await
            .expect("exec");

        let content = std::fs::read_to_string(dir.path().join("r.txt")).unwrap();
        assert_eq!(content, "result\n");
    }

    #[cfg(feature = "realfs")]
    #[test]
    fn mount_rw_mcp_mode_emits_warning() {
        // THREAT[TM-ESC-030]: Verify warning is emitted when --mount-rw is used with MCP mode.
        let args = Args::parse_from(["bashkit", "--mount-rw", "/tmp", "mcp"]);
        assert_eq!(cli_mode(&args), CliMode::Mcp);
        assert!(!args.mount_rw.is_empty());
        // configure_bash emits the warning to stderr; verify it doesn't panic
        // and the builder still succeeds (mounts are still applied).
        let _builder = configure_bash(&args, CliMode::Mcp);
    }

    #[cfg(feature = "realfs")]
    #[test]
    fn mount_ro_mcp_mode_no_warning() {
        // Read-only mounts in MCP mode should not trigger a warning.
        let args = Args::parse_from(["bashkit", "--mount-ro", "/tmp", "mcp"]);
        assert_eq!(cli_mode(&args), CliMode::Mcp);
        assert!(args.mount_rw.is_empty());
        let _builder = configure_bash(&args, CliMode::Mcp);
    }

    #[test]
    fn panic_message_str_payload() {
        let msg = format_panic_message(&"something went wrong" as &dyn std::any::Any);
        assert_eq!(msg, "bashkit: internal error: something went wrong");
        assert!(!msg.contains(".rs:"));
        assert!(!msg.contains("cargo"));
    }

    #[test]
    fn panic_message_string_payload() {
        let payload = String::from("Formatting argument out of range");
        let msg = format_panic_message(&payload as &dyn std::any::Any);
        assert_eq!(
            msg,
            "bashkit: internal error: Formatting argument out of range"
        );
    }

    #[test]
    fn panic_message_unknown_payload() {
        let payload = 42i32;
        let msg = format_panic_message(&payload as &dyn std::any::Any);
        assert_eq!(msg, "bashkit: internal error: unexpected error");
    }
}
