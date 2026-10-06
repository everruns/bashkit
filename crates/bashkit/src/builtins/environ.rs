//! Environment builtins - env, printenv, history

use async_trait::async_trait;

use super::{Builtin, Context, ExecutionPlan, SubCommand};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The env builtin - run command in modified environment or print environment.
///
/// Usage: env [-i] [-u NAME]... [-C DIR] [-0] [NAME=VALUE]... [COMMAND [ARG]...]
///
/// With a COMMAND, env returns an [`ExecutionPlan::Env`] and the interpreter
/// runs the command in a subshell-scoped copy of the environment, so the
/// changes (and anything the command does to shell state) never leak back.
pub struct Env;

#[derive(Debug, Default, PartialEq)]
struct EnvArgs {
    clear: bool,
    unset: Vec<String>,
    set: Vec<(String, String)>,
    chdir: Option<String>,
    null: bool,
    command: Vec<String>,
}

fn parse_env_args(args: &[String]) -> std::result::Result<EnvArgs, String> {
    let mut parsed = EnvArgs::default();
    let mut i = 0;
    let mut options_done = false;
    while i < args.len() {
        let arg = args[i].as_str();
        if !options_done && arg.starts_with('-') && arg != "-" && !arg.contains('=') {
            let mut value =
                |name: &str, inline: Option<&str>| -> std::result::Result<String, String> {
                    if let Some(v) = inline.filter(|v| !v.is_empty()) {
                        return Ok(v.to_string());
                    }
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| format!("env: option requires an argument -- '{name}'"))
                };
            match arg {
                "--" => options_done = true,
                "-i" | "--ignore-environment" => parsed.clear = true,
                "-0" | "--null" => parsed.null = true,
                "-u" | "--unset" => parsed.unset.push(value("u", None)?),
                "-C" | "--chdir" => parsed.chdir = Some(value("C", None)?),
                a if a.starts_with("--unset=") => parsed.unset.push(a[8..].to_string()),
                a if a.starts_with("--chdir=") => parsed.chdir = Some(a[8..].to_string()),
                a if a.starts_with("-u") => parsed.unset.push(a[2..].to_string()),
                a if a.starts_with("-C") => parsed.chdir = Some(a[2..].to_string()),
                a if a.starts_with("--") => return Err(format!("env: unrecognized option '{a}'")),
                a => return Err(format!("env: invalid option -- '{}'", &a[1..2])),
            }
            i += 1;
            continue;
        }
        if arg == "-" && !options_done {
            parsed.clear = true;
            i += 1;
            continue;
        }
        options_done = true;
        match arg.split_once('=') {
            Some((name, value)) if parsed.command.is_empty() && !name.is_empty() => {
                parsed.set.push((name.to_string(), value.to_string()));
            }
            _ => {
                parsed.command = args[i..].to_vec();
                break;
            }
        }
        i += 1;
    }
    if parsed.null && !parsed.command.is_empty() {
        return Err("env: cannot specify --null (-0) with command".to_string());
    }
    if parsed.chdir.is_some() && parsed.command.is_empty() {
        return Err("env: must specify command with --chdir (-C)".to_string());
    }
    Ok(parsed)
}

#[async_trait]
impl Builtin for Env {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: env [OPTION]... [-] [NAME=VALUE]... [COMMAND [ARG]...]\nSet each NAME to VALUE in the environment and run COMMAND.\n\n  -i, --ignore-environment\tstart with an empty environment\n  -0, --null\tend each output line with NUL, not newline\n  -u, --unset=NAME\tremove variable from the environment\n  -C, --chdir=DIR\tchange working directory to DIR\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("env (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let parsed = match parse_env_args(ctx.args) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("{e}\nTry 'env --help' for more information.\n"),
                    125,
                ));
            }
        };
        // A command is run through `execution_plan`; reaching here with one
        // means the plan path was unavailable.
        if !parsed.command.is_empty() {
            return Ok(ExecResult::err(
                format!("env: '{}': cannot run here\n", parsed.command[0]),
                126,
            ));
        }
        let mut vars: std::collections::BTreeMap<&str, &str> = if parsed.clear {
            Default::default()
        } else {
            ctx.env
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect()
        };
        for name in &parsed.unset {
            vars.remove(name.as_str());
        }
        for (k, v) in &parsed.set {
            vars.insert(k, v);
        }
        let end = if parsed.null { '\0' } else { '\n' };
        let output: String = vars.iter().map(|(k, v)| format!("{k}={v}{end}")).collect();
        Ok(ExecResult::ok(output))
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        let Ok(parsed) = parse_env_args(ctx.args) else {
            return Ok(None);
        };
        let Some((name, args)) = parsed.command.split_first() else {
            return Ok(None);
        };
        Ok(Some(ExecutionPlan::Env {
            command: SubCommand {
                name: name.clone(),
                args: args.to_vec(),
                stdin: ctx.stdin.cloned(),
                assignments: Vec::new(),
            },
            clear: parsed.clear,
            unset: parsed.unset,
            set: parsed.set,
            chdir: parsed.chdir,
        }))
    }
}

/// The printenv builtin - print environment variables.
///
/// Usage: printenv [VARIABLE...]
///
/// Prints the values of specified environment variables.
/// If no arguments given, prints all environment variables.
pub struct Printenv;

#[async_trait]
impl Builtin for Printenv {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: printenv [VARIABLE...]\nPrint the values of the specified environment variable(s).\nIf no VARIABLE is specified, print all.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("printenv (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        if ctx.args.is_empty() {
            // Print all environment variables
            let mut output = String::new();
            let mut pairs: Vec<_> = ctx.env.iter().collect();
            pairs.sort_by_key(|(k, _)| *k);
            for (key, value) in pairs {
                output.push_str(&format!("{}={}\n", key, value));
            }
            return Ok(ExecResult::ok(output));
        }

        // Print specified variables
        let mut output = String::new();
        let mut exit_code = 0;

        for var_name in ctx.args {
            if let Some(value) = ctx.env.get(var_name.as_str()) {
                output.push_str(value);
                output.push('\n');
            } else {
                // Variable not found - set exit code but continue
                exit_code = 1;
            }
        }

        Ok(ExecResult {
            stdout: output.into(),
            stderr: crate::StreamData::new(),
            exit_code,
            control_flow: crate::interpreter::ControlFlow::None,
            ..Default::default()
        })
    }
}

/// The history builtin — display and manage command history.
///
/// Usage: history [-c] [--grep PATTERN] [--cwd DIR] [--failed] [--since DURATION] [N]
///
/// Options:
///   -c          Clear the history
///   --grep PAT  Filter by command pattern
///   --cwd DIR   Filter by working directory prefix
///   --failed    Show only failed commands (non-zero exit)
///   --since DUR Show only entries within duration (e.g. 2d, 1h, 30m, 60s)
///   N           Show last N entries
///
/// Reads history from [`ShellRef`](super::ShellRef), clears via
/// [`BuiltinSideEffect::ClearHistory`](super::BuiltinSideEffect).
pub struct History;

impl History {
    /// Parse a human-readable duration string to seconds (e.g. "2d", "1h", "30m", "60s").
    fn parse_duration_to_secs(s: &str) -> Option<i64> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let (num_str, multiplier) = if let Some(stripped) = s.strip_suffix('d') {
            (stripped, 86400)
        } else if let Some(stripped) = s.strip_suffix('h') {
            (stripped, 3600)
        } else if let Some(stripped) = s.strip_suffix('m') {
            (stripped, 60)
        } else if let Some(stripped) = s.strip_suffix('s') {
            (stripped, 1)
        } else {
            (s, 1)
        };
        let num: i64 = num_str.parse().ok()?;
        Some(num * multiplier)
    }
}

#[async_trait]
impl Builtin for History {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            // No shell state — return empty (no-op for external builtins)
            return Ok(ExecResult::ok(String::new()));
        };

        let mut clear = false;
        let mut count: Option<usize> = None;
        let mut grep_pattern: Option<String> = None;
        let mut cwd_filter: Option<String> = None;
        let mut failed_only = false;
        let mut since_secs: Option<i64> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            match arg.as_str() {
                "-c" => clear = true,
                "--grep" => {
                    i += 1;
                    if i < ctx.args.len() {
                        grep_pattern = Some(ctx.args[i].clone());
                    } else {
                        return Ok(ExecResult::err(
                            "history: --grep requires an argument\n".to_string(),
                            1,
                        ));
                    }
                }
                "--cwd" => {
                    i += 1;
                    if i < ctx.args.len() {
                        cwd_filter = Some(ctx.args[i].clone());
                    } else {
                        return Ok(ExecResult::err(
                            "history: --cwd requires an argument\n".to_string(),
                            1,
                        ));
                    }
                }
                "--failed" => failed_only = true,
                "--since" => {
                    i += 1;
                    if i < ctx.args.len() {
                        match Self::parse_duration_to_secs(&ctx.args[i]) {
                            Some(secs) => since_secs = Some(secs),
                            None => {
                                return Ok(ExecResult::err(
                                    format!(
                                        "history: invalid duration '{}' (use e.g. 2d, 1h, 30m, 60s)\n",
                                        ctx.args[i]
                                    ),
                                    1,
                                ));
                            }
                        }
                    } else {
                        return Ok(ExecResult::err(
                            "history: --since requires an argument\n".to_string(),
                            1,
                        ));
                    }
                }
                _ => {
                    if let Some(opt) = arg.strip_prefix("--") {
                        return Ok(ExecResult::err(
                            format!("history: unrecognized option '--{}'\n", opt),
                            1,
                        ));
                    } else if let Some(opt) = arg.strip_prefix('-') {
                        // Allow -c, reject others
                        if opt != "c" {
                            return Ok(ExecResult::err(
                                format!("history: invalid option -- '{}'\n", opt),
                                1,
                            ));
                        }
                    } else if let Ok(n) = arg.parse::<usize>() {
                        count = Some(n);
                    }
                }
            }
            i += 1;
        }

        if clear {
            let mut result = ExecResult::ok(String::new());
            result
                .side_effects
                .push(super::BuiltinSideEffect::ClearHistory);
            return Ok(result);
        }

        let history = shell.history_entries();
        let limits = shell.limits();
        let now = crate::time_compat::now_utc().timestamp();
        let matches_entry = |entry: &crate::interpreter::HistoryEntry| {
            if let Some(ref pat) = grep_pattern
                && !entry.command.contains(pat.as_str())
            {
                return false;
            }
            if let Some(ref cwd) = cwd_filter
                && !entry.cwd.starts_with(cwd.as_str())
            {
                return false;
            }
            if failed_only && entry.exit_code == 0 {
                return false;
            }
            if let Some(secs) = since_secs
                && now - entry.timestamp > secs
            {
                return false;
            }
            true
        };

        // THREAT[TM-DOS-094]: Do not allocate a filtered copy of all history.
        // History is already session-capped; output is capped independently.
        let mut output = String::new();
        if let Some(n) = count {
            let mut entries = history
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, entry)| matches_entry(entry))
                .take(n)
                .collect::<Vec<_>>();
            entries.reverse();
            for (idx, entry) in entries {
                if !append_history_line(&mut output, limits.max_history_output_bytes, idx, entry) {
                    return Ok(history_output_capped(
                        output,
                        limits.max_history_output_bytes,
                    ));
                }
            }
        } else {
            for (idx, entry) in history
                .iter()
                .enumerate()
                .filter(|(_, entry)| matches_entry(entry))
            {
                if !append_history_line(&mut output, limits.max_history_output_bytes, idx, entry) {
                    return Ok(history_output_capped(
                        output,
                        limits.max_history_output_bytes,
                    ));
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// THREAT[TM-DOS-109]: a listing cut at `max_history_output_bytes` is reported.
fn history_output_capped(output: String, max_bytes: usize) -> ExecResult {
    super::limits::cap_exceeded("history", output, "output", format!("{max_bytes} bytes"))
}

/// Append one formatted history line. Returns `false` when the line does not
/// fit within `max_bytes`, signalling the caller to stop so the emitted output
/// is always a prefix of the full listing (never skipping an earlier entry to
/// fit a later, shorter one).
fn append_history_line(
    output: &mut String,
    max_bytes: usize,
    idx: usize,
    entry: &crate::interpreter::HistoryEntry,
) -> bool {
    use std::fmt::Write;
    let mut line = String::new();
    let _ = writeln!(line, "  {}  {}", idx + 1, entry.command);
    if output.len().saturating_add(line.len()) > max_bytes {
        return false;
    }
    output.push_str(&line);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    // ==================== env tests ====================

    #[tokio::test]
    async fn test_env_print_all() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), "/home/user".to_string());
        env.insert("PATH".to_string(), "/bin:/usr/bin".to_string());

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Env.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("HOME=/home/user"));
        assert!(result.stdout.contains("PATH=/bin:/usr/bin"));
    }

    #[tokio::test]
    async fn test_env_ignore_environment() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), "/home/user".to_string());

        let args = vec!["-i".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Env.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.contains("HOME"));
    }

    #[tokio::test]
    async fn test_env_add_vars() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["FOO=bar".to_string(), "BAZ=qux".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Env.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("FOO=bar"));
        assert!(result.stdout.contains("BAZ=qux"));
    }

    #[tokio::test]
    async fn test_env_command_outside_plan_is_rejected() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "FOO=bar".to_string(),
            "echo".to_string(),
            "hello".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Env.execute(ctx).await.unwrap();
        // Commands run via `execution_plan`; direct execute refuses them.
        assert_eq!(result.exit_code, 126);
        assert!(result.stderr.contains("cannot run here"));
    }

    // ==================== printenv tests ====================

    #[tokio::test]
    async fn test_printenv_all() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), "/home/user".to_string());
        env.insert("PATH".to_string(), "/bin".to_string());

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Printenv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("HOME=/home/user"));
        assert!(result.stdout.contains("PATH=/bin"));
    }

    #[tokio::test]
    async fn test_printenv_single_var() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), "/home/user".to_string());

        let args = vec!["HOME".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Printenv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "/home/user");
    }

    #[tokio::test]
    async fn test_printenv_multiple_vars() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), "/home/user".to_string());
        env.insert("PATH".to_string(), "/bin".to_string());

        let args = vec!["HOME".to_string(), "PATH".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Printenv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/home/user"));
        assert!(result.stdout.contains("/bin"));
    }

    #[tokio::test]
    async fn test_printenv_missing_var() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["NONEXISTENT".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Printenv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_printenv_mixed() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), "/home/user".to_string());

        let args = vec!["HOME".to_string(), "MISSING".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Printenv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1); // Non-zero because one var is missing
        assert!(result.stdout.contains("/home/user"));
    }

    // ==================== history tests ====================

    #[tokio::test]
    async fn test_history_empty() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = History.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_history_clear() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-c".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = History.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_history_count() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["10".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = History.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_history_no_shell_state() {
        // Without ShellRef, history is a no-op
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-z".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = History.execute(ctx).await.unwrap();
        // No shell state → graceful no-op
        assert_eq!(result.exit_code, 0);
    }
}

#[cfg(test)]
mod env_arg_tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_options_assignments_and_command() {
        let p = parse_env_args(&args(&["-i", "-u", "X", "-uY", "A=1", "sh", "-c", "B=2"])).unwrap();
        assert!(p.clear);
        assert_eq!(p.unset, vec!["X", "Y"]);
        assert_eq!(p.set, vec![("A".to_string(), "1".to_string())]);
        assert_eq!(p.command, args(&["sh", "-c", "B=2"]));
        let p = parse_env_args(&args(&["--", "-x"])).unwrap();
        assert_eq!(p.command, args(&["-x"]));
        let p = parse_env_args(&args(&["-", "env"])).unwrap();
        assert!(p.clear);
    }

    #[test]
    fn rejects_bad_option_combinations() {
        assert!(parse_env_args(&args(&["-z"])).is_err());
        assert!(parse_env_args(&args(&["-0", "true"])).is_err());
        assert!(parse_env_args(&args(&["-C", "/"])).is_err());
        assert!(parse_env_args(&args(&["-u"])).is_err());
    }
}
