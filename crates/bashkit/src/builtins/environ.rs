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

        // We have a command - but since we're in a sandbox, we can't execute arbitrary commands
        // Return an error indicating this
        Ok(ExecResult::err(
            "env: executing commands not supported in sandbox\n".to_string(),
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
        })
    }
}

/// The history builtin - display command history.
///
/// Usage: history [-c] [N]
///
/// Options:
///   -c   Clear the history
///   N    Show last N entries
///
/// In a sandboxed environment, history is limited to the current session.
/// Note: Full history tracking would require interpreter changes.
pub struct History;

#[async_trait]
impl Builtin for History {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut clear = false;
        let mut count: Option<usize> = None;

        for arg in ctx.args {
            if arg == "-c" {
                clear = true;
            } else if let Ok(n) = arg.parse::<usize>() {
                count = Some(n);
            } else if let Some(opt) = arg.strip_prefix('-') {
                return Ok(ExecResult::err(
                    format!("history: invalid option -- '{}'\n", opt),
                    1,
                ));
            }
        }

        if clear {
            // In a sandboxed environment, there's no persistent history to clear
            return Ok(ExecResult::ok(String::new()));
        }

        // In a sandboxed environment, we don't have access to shell history
        // Return an informational message
        let output = if let Some(_n) = count {
            // Would show last N entries
            String::new()
        } else {
            String::new()
        };

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
        };

        let result = History.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_history_invalid_option() {
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
        };

        let result = History.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
    }
}
