//! Timeout builtin - run command with time limit
//!
//! Executes a command with a specified timeout duration.
//! Returns an [`ExecutionPlan::Timeout`] for the interpreter to fulfill.

use async_trait::async_trait;
use std::time::Duration;

use super::{Builtin, Context, ExecutionPlan, SubCommand};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The timeout builtin - run command with time limit.
///
/// Usage: timeout DURATION COMMAND [ARGS...]
///
/// DURATION can be:
///   N     - N seconds
///   Ns    - N seconds
///   Nm    - N minutes
///   Nh    - N hours
///   Nd    - N days
///
/// Options:
///   -k DURATION  - Send KILL signal after DURATION if command still running
///   -s SIGNAL    - Signal to send (ignored, always uses timeout)
///   --preserve-status - Exit with command's status even on timeout
///
/// Exit codes:
///   124 - Command timed out
///   125 - Timeout command itself failed
///   126 - Command found but not executable
///   127 - Command not found
///   Otherwise, exit status of command
///
/// Note: In Bashkit's virtual environment, timeout works by wrapping
/// the command execution in a tokio timeout. Max timeout is 300 seconds
/// for safety.
pub struct Timeout;

const MAX_TIMEOUT_SECONDS: u64 = 300; // 5 minutes max

/// Parse a duration string like "30", "30s", "5m", "1h", "1d"
pub(crate) fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, multiplier) = if let Some(stripped) = s.strip_suffix('s') {
        (stripped, 1u64)
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, 60u64)
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 3600u64)
    } else if let Some(stripped) = s.strip_suffix('d') {
        (stripped, 86400u64)
    } else {
        (s, 1u64) // Default to seconds
    };

    let seconds: f64 = num_str.parse().ok()?;
    if seconds < 0.0 {
        return None;
    }

    let total_secs_f64 = seconds * multiplier as f64;
    // Cap at max while preserving subsecond precision
    let max = Duration::from_secs(MAX_TIMEOUT_SECONDS);
    let d = Duration::from_secs_f64(total_secs_f64);
    Some(if d > max { max } else { d })
}

/// Parse timeout arguments, returning (preserve_status, duration, cmd_name, cmd_args)
/// or an error ExecResult.
#[allow(clippy::result_large_err)]
fn parse_timeout_args(
    args: &[String],
) -> std::result::Result<(bool, Duration, String, Vec<String>), ExecResult> {
    if args.is_empty() {
        return Err(ExecResult::err(
            "timeout: missing operand\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
            125,
        ));
    }

    let mut preserve_status = false;
    let mut arg_idx = 0;

    while arg_idx < args.len() {
        let arg = &args[arg_idx];
        match arg.as_str() {
            "--preserve-status" => {
                preserve_status = true;
                arg_idx += 1;
            }
            "-k" | "-s" => {
                // These options take a value, skip it
                arg_idx += 2;
            }
            s if s.starts_with('-') && !s.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) => {
                arg_idx += 1;
            }
            _ => break, // Found duration
        }
    }

    if arg_idx >= args.len() {
        return Err(ExecResult::err(
            "timeout: missing operand\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
            125,
        ));
    }

    let duration_str = &args[arg_idx];
    let duration = match parse_duration(duration_str) {
        Some(d) => d,
        None => {
            return Err(ExecResult::err(
                format!("timeout: invalid time interval '{}'\n", duration_str),
                125,
            ));
        }
    };

    arg_idx += 1;

    if arg_idx >= args.len() {
        return Err(ExecResult::err(
            "timeout: missing command\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
            125,
        ));
    }

    let cmd_name = args[arg_idx].clone();
    let cmd_args = args[arg_idx + 1..].to_vec();

    Ok((preserve_status, duration, cmd_name, cmd_args))
}

#[async_trait]
impl Builtin for Timeout {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Validate arguments and return error for invalid input.
        // Actual command execution is handled by execution_plan().
        match parse_timeout_args(ctx.args) {
            Err(e) => Ok(e),
            Ok(_) => {
                // Valid args but no executor available (standalone builtin context).
                // This shouldn't normally happen since the interpreter uses execution_plan().
                Ok(ExecResult::ok(String::new()))
            }
        }
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        match parse_timeout_args(ctx.args) {
            Err(_) => Ok(None), // Let execute() handle the error
            Ok((preserve_status, duration, cmd_name, cmd_args)) => {
                Ok(Some(ExecutionPlan::Timeout {
                    duration,
                    preserve_status,
                    command: SubCommand {
                        name: cmd_name,
                        args: cmd_args,
                        stdin: ctx.stdin.map(|s| s.to_string()),
                    },
                }))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_timeout(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Timeout.execute(ctx).await.unwrap()
    }

    async fn get_plan(args: &[&str], stdin: Option<&str>) -> Option<ExecutionPlan> {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Timeout.execution_plan(&ctx).await.unwrap()
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("0"), Some(Duration::from_secs(0)));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1m"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_parse_duration_hours() {
        // Capped at MAX_TIMEOUT_SECONDS (300)
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(300)));
    }

    #[test]
    fn test_parse_duration_days() {
        // Capped at MAX_TIMEOUT_SECONDS (300)
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(300)));
    }

    #[test]
    fn test_parse_duration_decimal() {
        let d = parse_duration("1.5").unwrap();
        assert!(d.as_secs_f64() > 1.4 && d.as_secs_f64() < 1.6);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration("-5"), None);
    }

    #[tokio::test]
    async fn test_timeout_no_args() {
        let result = run_timeout(&[]).await;
        assert_eq!(result.exit_code, 125);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_timeout_no_command() {
        let result = run_timeout(&["30"]).await;
        assert_eq!(result.exit_code, 125);
        assert!(result.stderr.contains("missing command"));
    }

    #[tokio::test]
    async fn test_timeout_invalid_duration() {
        let result = run_timeout(&["abc", "echo", "hello"]).await;
        assert_eq!(result.exit_code, 125);
        assert!(result.stderr.contains("invalid time interval"));
    }

    #[tokio::test]
    async fn test_timeout_plan_basic() {
        let plan = get_plan(&["30", "echo", "hello"], None).await;
        match plan {
            Some(ExecutionPlan::Timeout {
                duration,
                preserve_status,
                command,
            }) => {
                assert_eq!(duration, Duration::from_secs(30));
                assert!(!preserve_status);
                assert_eq!(command.name, "echo");
                assert_eq!(command.args, vec!["hello"]);
                assert!(command.stdin.is_none());
            }
            _ => panic!("expected Timeout plan"),
        }
    }

    #[tokio::test]
    async fn test_timeout_plan_preserve_status() {
        let plan = get_plan(&["--preserve-status", "5", "sleep", "10"], None).await;
        match plan {
            Some(ExecutionPlan::Timeout {
                preserve_status, ..
            }) => {
                assert!(preserve_status);
            }
            _ => panic!("expected Timeout plan"),
        }
    }

    #[tokio::test]
    async fn test_timeout_plan_with_stdin() {
        let plan = get_plan(&["5", "cat"], Some("hello\n")).await;
        match plan {
            Some(ExecutionPlan::Timeout { command, .. }) => {
                assert_eq!(command.stdin.as_deref(), Some("hello\n"));
            }
            _ => panic!("expected Timeout plan"),
        }
    }

    #[tokio::test]
    async fn test_timeout_plan_invalid_returns_none() {
        let plan = get_plan(&[], None).await;
        assert!(plan.is_none());

        let plan = get_plan(&["abc", "echo"], None).await;
        assert!(plan.is_none());
    }
}
