//! Sleep builtin - pause execution for specified duration

use async_trait::async_trait;
use std::time::Duration;

#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    not(feature = "wasm_js")
))]
use super::ExecutionDeadline;
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
        // GNU sleep: only `--` ends options; every operand is a C strtod
        // number (blanks, hex floats, `inf` allowed) plus an optional single
        // s/m/h/d suffix, and the durations are summed.
        // getopt permutes, so any `-X` before `--` (even `-1`) is an option.
        let mut operands: Vec<&String> = Vec::new();
        let mut args = ctx.args.iter();
        while let Some(arg) = args.next() {
            if arg == "--" {
                operands.extend(args.by_ref());
            } else if arg.len() > 1 && arg.starts_with('-') {
                return Ok(super::invalid_option("sleep", arg, 1));
            } else {
                operands.push(arg);
            }
        }
        if operands.is_empty() {
            return Ok(Self::err("missing operand", 1));
        }
        let mut seconds = 0.0f64;
        for arg in operands {
            match parse_interval(arg) {
                Some(s) => seconds += s,
                None => {
                    return Ok(Self::err(format!("invalid time interval '{}'", arg), 1));
                }
            }
        }
        let seconds = seconds.min(MAX_SLEEP_SECONDS);

        if seconds > 0.0 {
            let duration = Duration::from_secs_f64(seconds);
            #[cfg(all(
                target_arch = "wasm32",
                target_os = "unknown",
                not(feature = "wasm_js")
            ))]
            let duration = if let Some(deadline) = ctx.execution_extension::<ExecutionDeadline>() {
                // THREAT[TM-DOS-057]: the non-JS timer spins synchronously, so
                // cap it before polling can block the outer timeout future.
                if let Ok(remaining) = deadline.try_with(ExecutionDeadline::remaining) {
                    effective_sleep_duration(duration, remaining)
                } else {
                    duration
                }
            } else {
                duration
            };
            crate::time_compat::sleep(duration).await;
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// Parse one GNU sleep operand into seconds; `None` when invalid or negative.
fn parse_interval(arg: &str) -> Option<f64> {
    let (value, used) = super::sortuniq::strtod(arg.as_bytes())?;
    let multiplier = match &arg[used..] {
        "" | "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        _ => return None,
    };
    if value.is_nan() || value < 0.0 {
        return None;
    }
    Some(value * multiplier)
}

#[cfg(any(
    test,
    all(
        target_arch = "wasm32",
        target_os = "unknown",
        not(feature = "wasm_js")
    )
))]
fn effective_sleep_duration(requested: Duration, remaining: Duration) -> Duration {
    requested.min(remaining)
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

    #[test]
    fn non_js_wasm_sleep_is_capped_by_execution_budget() {
        assert_eq!(
            effective_sleep_duration(Duration::from_secs(60), Duration::from_millis(10)),
            Duration::from_millis(10)
        );
        assert_eq!(
            effective_sleep_duration(Duration::from_millis(10), Duration::from_secs(60)),
            Duration::from_millis(10)
        );
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
        assert!(result.stderr.contains("invalid option -- '1'"));
    }

    #[test]
    fn test_parse_interval_gnu_forms() {
        assert_eq!(parse_interval("1"), Some(1.0));
        assert_eq!(parse_interval("1.5m"), Some(90.0));
        assert_eq!(parse_interval("2h"), Some(7200.0));
        assert_eq!(parse_interval("1d"), Some(86400.0));
        assert_eq!(parse_interval("0x10"), Some(16.0));
        assert_eq!(parse_interval("0x.8p1"), Some(1.0));
        assert_eq!(parse_interval(" 3"), Some(3.0));
        assert_eq!(parse_interval(".5"), Some(0.5));
        assert_eq!(parse_interval("inf"), Some(f64::INFINITY));
        assert_eq!(parse_interval("1x"), None);
        assert_eq!(parse_interval("1ss"), None);
        assert_eq!(parse_interval("3 "), None);
        assert_eq!(parse_interval("nan"), None);
        assert_eq!(parse_interval(""), None);
    }

    #[tokio::test]
    async fn test_sleep_sums_operands_and_dash_dash() {
        let result = run_sleep(&["--", "0", "0s", "0m"]).await;
        assert_eq!(result.exit_code, 0);
        let result = run_sleep(&["0", "bad"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid time interval 'bad'"));
    }
}
