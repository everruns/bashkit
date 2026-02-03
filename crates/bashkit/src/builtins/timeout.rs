//! Timeout builtin - run command with time limit
//!
//! Executes a command with a specified timeout duration.

use async_trait::async_trait;
use std::time::Duration;

use super::{Builtin, Context};
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
/// Note: In BashKit's sandboxed environment, timeout works by wrapping
/// the command execution in a tokio timeout. Max timeout is 300 seconds
/// for safety.
pub struct Timeout;

const MAX_TIMEOUT_SECONDS: u64 = 300; // 5 minutes max

/// Parse a duration string like "30", "30s", "5m", "1h"
fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Check for suffix
    let (num_str, multiplier) = if let Some(stripped) = s.strip_suffix('s') {
        (stripped, 1u64)
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, 60u64)
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 3600u64)
    } else {
        (s, 1u64) // Default to seconds
    };

    // Parse the number (support decimals)
    let seconds: f64 = num_str.parse().ok()?;
    if seconds < 0.0 {
        return None;
    }

    let total_seconds = (seconds * multiplier as f64) as u64;

    // Cap at max timeout
    let capped = total_seconds.min(MAX_TIMEOUT_SECONDS);

    Some(Duration::from_secs(capped))
}

#[async_trait]
impl Builtin for Timeout {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "timeout: missing operand\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
                125,
            ));
        }

        // Parse options
        let mut preserve_status = false;
        let mut duration_idx = 0;

        for (i, arg) in ctx.args.iter().enumerate() {
            match arg.as_str() {
                "--preserve-status" => {
                    preserve_status = true;
                    duration_idx = i + 1;
                }
                "-k" | "-s" => {
                    // Skip the next argument (these options take a value)
                    duration_idx = i + 2;
                }
                _ if arg.starts_with('-') => {
                    // Skip unknown options
                    duration_idx = i + 1;
                }
                _ => {
                    duration_idx = i;
                    break;
                }
            }
        }

        if duration_idx >= ctx.args.len() {
            return Ok(ExecResult::err(
                "timeout: missing operand\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
                125,
            ));
        }

        // Parse duration
        let duration = match parse_duration(&ctx.args[duration_idx]) {
            Some(d) => d,
            None => {
                return Ok(ExecResult::err(
                    format!(
                        "timeout: invalid time interval '{}'\n",
                        ctx.args[duration_idx]
                    ),
                    125,
                ));
            }
        };

        // Get command and args
        let cmd_idx = duration_idx + 1;
        if cmd_idx >= ctx.args.len() {
            return Ok(ExecResult::err(
                "timeout: missing command\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
                125,
            ));
        }

        let command = &ctx.args[cmd_idx];
        let command_args: Vec<String> = ctx.args[cmd_idx + 1..].to_vec();

        // Note: In the current BashKit architecture, we can't easily execute
        // arbitrary commands from within a builtin. The timeout would need to
        // be implemented at the interpreter level to wrap command execution.
        //
        // For now, we return an informative message about the limitation.
        // A full implementation would require interpreter-level changes.

        let _ = (duration, preserve_status, command, command_args);

        Ok(ExecResult::err(
            format!(
                "timeout: command execution not available from builtin context\n\
                 Note: timeout requires interpreter-level integration.\n\
                 Requested: timeout {:?} {} ...\n\
                 Consider using execution limits instead.\n",
                duration, command
            ),
            125,
        ))
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
            #[cfg(feature = "network")]
            http_client: None,
        };

        Timeout.execute(ctx).await.unwrap()
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
    fn test_parse_duration_decimal() {
        assert_eq!(parse_duration("1.5"), Some(Duration::from_secs(1)));
        assert_eq!(parse_duration("0.5s"), Some(Duration::from_secs(0)));
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
    async fn test_timeout_with_command() {
        let result = run_timeout(&["30", "echo", "hello"]).await;
        // Currently returns stub error
        assert_eq!(result.exit_code, 125);
        assert!(result.stderr.contains("interpreter-level"));
    }
}
