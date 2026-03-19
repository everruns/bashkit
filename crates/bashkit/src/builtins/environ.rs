//! Environment builtins - env, printenv, history

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The env builtin - run command in modified environment or print environment.
///
/// Usage: env [-i] [NAME=VALUE]... [COMMAND [ARG]...]
///
/// Options:
///   -i   Start with empty environment
///
/// If no COMMAND is given, prints the environment.
pub struct Env;

#[async_trait]
impl Builtin for Env {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut ignore_env = false;
        let mut env_vars: Vec<(String, String)> = Vec::new();
        let mut command_start = 0;

        // Parse arguments
        for (i, arg) in ctx.args.iter().enumerate() {
            if arg == "-i" || arg == "--ignore-environment" {
                ignore_env = true;
            } else if arg == "-u" {
                // -u NAME would unset a variable, but we'll skip for simplicity
                return Ok(ExecResult::err(
                    "env: -u option not supported\n".to_string(),
                    1,
                ));
            } else if let Some((name, value)) = arg.split_once('=') {
                env_vars.push((name.to_string(), value.to_string()));
            } else {
                // This is the start of the command
                command_start = i;
                break;
            }
        }

        // If no command, print environment
        if command_start == 0 || command_start == ctx.args.len() {
            let mut output = String::new();

            // If not ignoring environment, print existing env vars
            if !ignore_env {
                let mut pairs: Vec<_> = ctx.env.iter().collect();
                pairs.sort_by_key(|(k, _)| *k);
                for (key, value) in pairs {
                    output.push_str(&format!("{}={}\n", key, value));
                }
            }

            // Print specified env vars
            for (key, value) in env_vars {
                output.push_str(&format!("{}={}\n", key, value));
            }

            return Ok(ExecResult::ok(output));
        }

        // We have a command - but since we're in a virtual environment, we can't execute arbitrary commands
        // Return an error indicating this
        Ok(ExecResult::err(
            "env: executing commands not supported in virtual mode\n".to_string(),
            126,
        ))
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
            stdout: output,
            stderr: String::new(),
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
        let now = chrono::Utc::now().timestamp();

        // Apply filters
        let filtered: Vec<(usize, &crate::interpreter::HistoryEntry)> = history
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
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
            })
            .collect();

        // Apply count limit (last N entries)
        let entries: &[(usize, &crate::interpreter::HistoryEntry)] = if let Some(n) = count {
            let start = filtered.len().saturating_sub(n);
            &filtered[start..]
        } else {
            &filtered
        };

        // Format output: bash-style numbered listing
        let mut output = String::new();
        for (idx, entry) in entries {
            use std::fmt::Write;
            // 1-indexed like bash
            let _ = writeln!(output, "  {}  {}", idx + 1, entry.command);
        }

        Ok(ExecResult::ok(output))
    }
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
            shell: None,
        };

        let result = Env.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("FOO=bar"));
        assert!(result.stdout.contains("BAZ=qux"));
    }

    #[tokio::test]
    async fn test_env_command_not_supported() {
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
            shell: None,
        };

        let result = Env.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 126);
        assert!(result.stderr.contains("not supported"));
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
            shell: None,
        };

        let result = History.execute(ctx).await.unwrap();
        // No shell state → graceful no-op
        assert_eq!(result.exit_code, 0);
    }
}
