// Decision: keep network default-deny in CLI mode. --http-allow-all is explicit
// opt-in for trusted scripts only; --no-http remains for symmetry with other
// feature toggles.
// Decision: keep one-shot CLI on a current-thread runtime for lower cold-start.
// Decision: interactive mode uses relaxed execution limits (ExecutionLimits::cli())
// because users have terminal control (Ctrl-C) and expect long-running sessions.
// One-shot command/script mode uses sandboxed defaults unless explicitly overridden
// by flags to avoid unbounded hangs for wrapper/automation usage.
// Decision: interactive mode uses rustyline for line editing — lightweight, MIT,
// no heavy deps (no SQLite, no crossterm). Multiline via parse error detection.
// See knowledge/integrations/interactive-shell.md
// Decision: CLI enables embedded Python by default when compiled in. This is an
// explicit CLI builder opt-in, not ambient process environment inheritance.

//! Bashkit CLI - Command line interface for virtual bash execution
//!
//! Usage:
//!   bashkit -c 'echo hello'        # Execute a command string
//!   bashkit script.sh              # Execute a script file
//!   bashkit                        # Interactive REPL

#[cfg(feature = "interactive")]
mod interactive;

use anyhow::{Context, Result, bail};
use clap::Parser;
use std::{
    ffi::OsStr,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};
use tokio::runtime::Builder;

/// Cap on stdin slurped from the host before execution. Large inputs belong in
/// a mounted file; holding an unbounded stream in memory is a DoS surface.
const MAX_STDIN_BYTES: usize = 10 * 1024 * 1024;

#[cfg(feature = "python")]
const PYTHON_INPROCESS_OPT_IN_ENV: &str = "BASHKIT_ALLOW_INPROCESS_PYTHON";

#[cfg(feature = "sqlite")]
const SQLITE_INPROCESS_OPT_IN_ENV: &str = "BASHKIT_ALLOW_INPROCESS_SQLITE";

const REMOVED_MCP_COMMAND: &str = "mcp";

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

    /// Enable HTTP builtins with unrestricted outbound access (trusted scripts only)
    #[arg(long, conflicts_with = "no_http")]
    http_allow_all: bool,

    /// Disable git builtin
    #[arg(long)]
    no_git: bool,

    /// Disable python builtin (monty backend)
    #[cfg_attr(not(feature = "python"), arg(long, hide = true))]
    #[cfg_attr(feature = "python", arg(long))]
    no_python: bool,

    /// Disable sqlite/sqlite3 builtin (turso backend, BETA)
    #[cfg_attr(not(feature = "sqlite"), arg(long, hide = true))]
    #[cfg_attr(feature = "sqlite", arg(long))]
    no_sqlite: bool,

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

    /// Maximum number of commands to execute (unlimited for interactive mode)
    #[arg(long)]
    max_commands: Option<usize>,

    /// Maximum iterations for a single loop (unlimited for interactive mode)
    #[arg(long)]
    max_loop_iterations: Option<usize>,

    /// Maximum total loop iterations across all loops (unlimited for interactive mode)
    #[arg(long)]
    max_total_loop_iterations: Option<usize>,

    /// Execution timeout in seconds (unlimited for interactive mode)
    #[arg(long)]
    timeout: Option<u64>,

    /// Do not forward the host's stdin to the script
    ///
    /// One-shot mode reads stdin to EOF before execution when it is not a
    /// terminal. Use this when stdin is an inherited pipe that stays open.
    #[arg(long)]
    no_stdin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliMode {
    Command,
    Script,
    Interactive,
}

fn build_bash(args: &Args, mode: CliMode) -> bashkit::Bash {
    configure_bash(args, mode).build()
}

fn configure_bash(args: &Args, mode: CliMode) -> bashkit::BashBuilder {
    let mut builder = bashkit::Bash::builder();

    if args.http_allow_all && !args.no_http {
        builder = builder.network(bashkit::NetworkAllowlist::allow_all());
    }

    if !args.no_git {
        builder = builder.git(bashkit::GitConfig::new());
    }

    #[cfg(feature = "python")]
    if !args.no_python {
        builder = builder.python();
        builder = builder.env(PYTHON_INPROCESS_OPT_IN_ENV, "1");
    }

    #[cfg(feature = "sqlite")]
    if !args.no_sqlite {
        builder = builder.sqlite();
        builder = builder.env(SQLITE_INPROCESS_OPT_IN_ENV, "1");
    }

    #[cfg(feature = "realfs")]
    {
        builder = apply_real_mounts(builder, &args.mount_ro, &args.mount_rw);
    }

    // Interactive mode uses relaxed limits; one-shot keeps sandboxed defaults.
    let mut limits = if mode == CliMode::Interactive {
        bashkit::ExecutionLimits::cli()
    } else {
        bashkit::ExecutionLimits::new()
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

    if mode == CliMode::Interactive {
        builder = builder.session_limits(bashkit::SessionLimits::unlimited());
    }

    #[cfg(feature = "interactive")]
    if mode == CliMode::Interactive {
        builder = builder.tty(0, true).tty(1, true).tty(2, true);
    }

    builder
}

fn cli_mode(args: &Args) -> CliMode {
    if args.command.is_some() {
        CliMode::Command
    } else if args.script.is_some() {
        CliMode::Script
    } else {
        CliMode::Interactive
    }
}

fn validate_cli_args(args: &Args) -> Result<()> {
    if args.command.is_none()
        && args
            .script
            .as_deref()
            .is_some_and(is_bare_removed_mcp_command)
    {
        bail!(
            "the `bashkit mcp` MCP server command was removed; use `bashkit ./mcp` to execute a script named `mcp`"
        );
    }

    Ok(())
}

fn is_bare_removed_mcp_command(script: &Path) -> bool {
    script.as_os_str() == OsStr::new(REMOVED_MCP_COMMAND)
}

/// Parse mount specs (HOST_PATH or HOST_PATH:VFS_PATH) and apply to builder.
#[cfg(feature = "realfs")]
fn apply_real_mounts(
    mut builder: bashkit::BashBuilder,
    ro_mounts: &[String],
    rw_mounts: &[String],
) -> bashkit::BashBuilder {
    // Important decision: explicit CLI mount flags are the user's allowlist.
    // The library keeps sensitive host paths closed by default for embedders;
    // the CLI turns each requested host path into an audit-visible allow entry.
    let allowed_mount_paths = ro_mounts
        .iter()
        .chain(rw_mounts)
        .map(|spec| spec.split_once(':').map_or(spec.as_str(), |(host, _)| host))
        .collect::<Vec<_>>();
    if !allowed_mount_paths.is_empty() {
        builder = builder.allowed_mount_paths(allowed_mount_paths);
    }

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
    validate_cli_args(&args)?;

    let mode = cli_mode(&args);
    match mode {
        CliMode::Command | CliMode::Script => {
            let exit_code = run_oneshot(args, mode)?;
            std::process::exit(exit_code);
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

/// Read the host's stdin for the script to consume.
///
/// Only called when stdin is not a terminal, so an interactive
/// `bashkit -c 'echo hi'` never blocks waiting for input. Bash reads stdin
/// lazily, when a command asks for it; the interpreter takes its stdin up
/// front, so this reads to EOF before execution starts (L-CLI-002).
/// `--no-stdin` opts out for callers that inherit a pipe nobody will close.
///
// THREAT[TM-DOS-095]: bounded read; an unbounded producer cannot grow the
// buffer past MAX_STDIN_BYTES.
fn read_host_stdin() -> Result<Vec<u8>> {
    use std::io::Read;

    let mut buf = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_STDIN_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .context("Failed to read stdin")?;
    if buf.len() > MAX_STDIN_BYTES {
        bail!(
            "stdin exceeds the {} MiB limit; redirect from a file inside a mount instead",
            MAX_STDIN_BYTES / (1024 * 1024)
        );
    }
    Ok(buf)
}

/// Split the parsed arguments into `$0` and the positional parameters.
///
/// Matches bash: after `-c 'script'` the first trailing argument becomes `$0`
/// and the rest are `$1`…; for a script file, `$0` is the script path and every
/// trailing argument is positional. Both cases read the same two clap slots,
/// because the first non-flag token always lands in `script` — the script path
/// in script mode, the `$0` override in command mode — and everything after it
/// lands in `args`.
fn split_invocation_args(args: &Args) -> (Option<String>, Vec<String>) {
    (
        args.script.as_ref().map(|p| p.display().to_string()),
        args.args.clone(),
    )
}

/// Build the per-execution options: positional parameters, `$0`, and stdin.
fn exec_options(args: &Args) -> Result<bashkit::ExecOptions> {
    let mut options = bashkit::ExecOptions::new().streaming(Box::new(|stdout, stderr| {
        // Write through as chunks arrive so long scripts are observable and a
        // run aborted by a resource limit keeps what it already produced.
        if !stdout.is_empty() {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(stdout.as_bytes());
            let _ = out.flush();
        }
        if !stderr.is_empty() {
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(stderr.as_bytes());
            let _ = err.flush();
        }
    }));

    // Left unset when there is nothing to install, so `$0` keeps its default
    // instead of being overwritten with an empty name.
    let (arg0, positional) = split_invocation_args(args);
    if let Some(arg0) = arg0 {
        options = options.arg0(arg0);
    }
    if !positional.is_empty() {
        options = options.positional(positional);
    }

    if !args.no_stdin && !std::io::stdin().is_terminal() {
        options = options.stdin(read_host_stdin()?);
    }

    Ok(options)
}

fn run_oneshot(args: Args, mode: CliMode) -> Result<i32> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build CLI runtime")?
        .block_on(async move {
            let mut bash = build_bash(&args, mode);
            let options = exec_options(&args)?;

            let (script, context) = match (&args.command, &args.script) {
                (Some(cmd), _) => (cmd.clone(), "Failed to execute command"),
                (None, Some(path)) => {
                    let script = std::fs::read_to_string(path)
                        .with_context(|| format!("Failed to read script: {}", path.display()))?;
                    (script, "Failed to execute script")
                }
                (None, None) => unreachable!("run_oneshot called for non-executable mode"),
            };

            let result = bash
                .exec_with_options(&script, options)
                .await
                .context(context)?;

            // Output already reached the terminal through the streaming
            // callback — printing `result.stdout` here would duplicate it.
            Ok(result.exit_code)
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
        assert!(!args.http_allow_all);
        assert!(args.no_git);
        assert!(args.no_python);
    }

    #[test]
    fn defaults_keep_http_disabled() {
        let args = Args::parse_from(["bashkit", "-c", "echo hi"]);
        assert!(!args.no_http);
        assert!(!args.http_allow_all);
        assert!(!args.no_git);
        assert!(!args.no_python);
    }

    #[test]
    fn parse_http_allow_all_flag() {
        let args = Args::parse_from(["bashkit", "--http-allow-all", "-c", "curl --help"]);
        assert!(args.http_allow_all);
        assert!(!args.no_http);
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
    fn validate_cli_args_rejects_removed_mcp_command() {
        let args = Args::parse_from(["bashkit", "mcp"]);
        let err = validate_cli_args(&args).expect_err("bare mcp should be rejected");
        assert!(
            err.to_string().contains("MCP server command was removed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_cli_args_allows_explicit_mcp_script_path() {
        let args = Args::parse_from(["bashkit", "./mcp"]);
        validate_cli_args(&args).expect("explicit mcp path is a script");
        assert_eq!(cli_mode(&args), CliMode::Script);
        assert_eq!(args.script, Some(PathBuf::from("./mcp")));
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
    async fn python_code_runs_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "python -c 'print(2 + 2)'"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("python -c 'print(2 + 2)'").await.expect("exec");
        assert_eq!(result.stdout, "4\n");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);
    }

    #[cfg(feature = "python")]
    #[tokio::test]
    async fn python_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-python", "-c", "python --version"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("python --version").await.expect("exec");
        assert!(result.stderr.contains("command not found"));
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn sqlite_enabled_by_default() {
        // The CLI default-builds with `sqlite` and auto-injects the runtime
        // opt-in env var, so a fresh `sqlite :memory: 'SELECT 1'` works
        // without further configuration.
        let args = Args::parse_from(["bashkit", "-c", "sqlite :memory: 'SELECT 1'"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("sqlite :memory: 'SELECT 1'").await.expect("exec");
        assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
        assert_eq!(result.stdout.trim(), "1");
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn sqlite_can_be_disabled() {
        let args = Args::parse_from(["bashkit", "--no-sqlite", "-c", "sqlite :memory: 'SELECT 1'"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("sqlite :memory: 'SELECT 1'").await.expect("exec");
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
    async fn http_disabled_by_default() {
        let args = Args::parse_from(["bashkit", "-c", "curl https://example.com"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("curl https://example.com").await.expect("exec");
        assert!(result.stderr.contains("network access not configured"));
    }

    #[tokio::test]
    async fn http_can_be_enabled_explicitly() {
        let args = Args::parse_from([
            "bashkit",
            "--http-allow-all",
            "-c",
            "curl https://example.com",
        ]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = bash.exec("curl https://example.com").await.expect("exec");
        assert!(!result.stderr.contains("not configured"));
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
        // Output goes straight to the process streams now, so the assertion
        // here is the exit code; `tests/cli_oneshot.rs` covers what is printed.
        let args = Args::parse_from([
            "bashkit",
            "--no-http",
            "--no-git",
            "--no-stdin",
            "-c",
            "echo works",
        ]);
        assert_eq!(run_oneshot(args, CliMode::Command).expect("run"), 0);
    }

    #[test]
    fn command_mode_takes_arg0_from_the_first_trailing_arg() {
        let args = Args::parse_from(["bashkit", "-c", "echo hi", "myname", "a", "b"]);
        let (arg0, positional) = split_invocation_args(&args);
        assert_eq!(arg0.as_deref(), Some("myname"));
        assert_eq!(positional, vec!["a", "b"]);
    }

    #[test]
    fn command_mode_without_trailing_args_leaves_arg0_unset() {
        let args = Args::parse_from(["bashkit", "-c", "echo hi"]);
        let (arg0, positional) = split_invocation_args(&args);
        assert_eq!(arg0, None);
        assert!(positional.is_empty());
    }

    #[test]
    fn script_mode_uses_the_script_path_as_arg0() {
        let args = Args::parse_from(["bashkit", "run.sh", "a", "b"]);
        let (arg0, positional) = split_invocation_args(&args);
        assert_eq!(arg0.as_deref(), Some("run.sh"));
        assert_eq!(positional, vec!["a", "b"]);
    }

    #[test]
    fn parse_no_stdin_flag() {
        let args = Args::parse_from(["bashkit", "--no-stdin", "-c", "cat"]);
        assert!(args.no_stdin);
        assert!(!Args::parse_from(["bashkit", "-c", "cat"]).no_stdin);
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

    #[tokio::test]
    async fn command_mode_enforces_default_loop_limit() {
        let args = Args::parse_from(["bashkit", "-c", "while true; do :; done"]);
        let mut bash = build_bash(&args, CliMode::Command);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            bash.exec("while true; do :; done"),
        )
        .await
        .expect("command mode should enforce bounded limits");
        let err = result.expect_err("exec should fail with a resource limit");
        let msg = err.to_string();
        assert!(
            msg.contains("maximum command count")
                || msg.contains("maximum loop iterations")
                || msg.contains("maximum total loop iterations")
        );
    }
}
