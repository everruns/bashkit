//! Sleep builtin - pause execution for specified duration

use async_trait::async_trait;
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

use super::limits::SLEEP_MAX_SECONDS as MAX_SLEEP_SECONDS;
use super::{Builtin, BuiltinHelper, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The sleep builtin - pause execution for a specified number of seconds.
///
/// Usage: sleep SECONDS
///
/// SECONDS can be a floating-point number (e.g., 0.5 for half a second).
/// Maximum duration is capped at 60 seconds for safety.
pub struct Sleep;

impl BuiltinHelper for Sleep {
    const NAME: &'static str = "sleep";
}

#[async_trait]
impl Builtin for Sleep {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: sleep SECONDS\nPause for SECONDS seconds.\nSECONDS may be a floating-point number. Maximum duration is 60 seconds.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("sleep (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let seconds = match ctx.args.first() {
            Some(arg) => match arg.parse::<f64>() {
                Ok(s) if s < 0.0 => {
                    return Ok(Self::err(format!("invalid time interval '{}'", arg), 1));
                }
                Ok(s) => s.min(MAX_SLEEP_SECONDS),
                Err(_) => {
                    // Try parsing as integer for better error messages
                    if arg.parse::<i64>().is_ok() {
                        arg.parse::<f64>().unwrap_or(0.0).min(MAX_SLEEP_SECONDS)
                    } else {
                        return Ok(Self::err(format!("invalid time interval '{}'", arg), 1));
                    }
                }
            },
            None => {
                return Ok(Self::err("missing operand", 1));
            }
        };

        if seconds > 0.0 {
            // wasm32-unknown-unknown has no timer driver, so tokio::time::sleep
            // panics ("time not implemented"). The single-threaded sandbox has
            // no wall-clock semantics anyway (see specs/browser-package.md), so
            // on wasm the sleep elapses instantly.
            #[cfg(not(target_family = "wasm"))]
            tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
        }

        Ok(ExecResult::ok(String::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_sleep(args: &[&str]) -> ExecResult {
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Sleep.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_sleep_zero() {
        let start = crate::time_compat::Instant::now();
        let result = run_sleep(&["0"]).await;
        let elapsed = start.elapsed();

        assert_eq!(result.exit_code, 0);
        assert!(elapsed.as_millis() < 100); // Should be nearly instant
    }

    #[tokio::test]
    async fn test_sleep_fractional() {
        let start = crate::time_compat::Instant::now();
        let result = run_sleep(&["0.1"]).await;
        let elapsed = start.elapsed();

        assert_eq!(result.exit_code, 0);
        assert!(elapsed.as_millis() >= 90); // Allow some margin
        assert!(elapsed.as_millis() < 200);
    }

    #[tokio::test]
    async fn test_sleep_missing_operand() {
        let result = run_sleep(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_sleep_invalid_argument() {
        let result = run_sleep(&["abc"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid time interval"));
    }

    #[tokio::test]
    async fn test_sleep_negative() {
        let result = run_sleep(&["-1"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid time interval"));
    }
}
