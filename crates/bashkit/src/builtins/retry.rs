//! retry builtin - parse retry options and report planned behavior
//!
//! Non-standard builtin. Cannot actually re-execute commands in VFS,
//! so parses options and prints what it would do.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Retry builtin - parses retry configuration and reports planned behavior.
///
/// Usage: retry [OPTIONS] -- command [args...]
///
/// Options:
///   -n NUM       Max retry attempts (default: 3)
///   -d SECONDS   Delay between retries (default: 1)
///   --backoff    Enable exponential backoff
///   -q           Quiet mode (suppress retry messages)
///   -v           Verbose mode (show detailed retry info)
pub struct Retry;

struct RetryConfig {
    max_attempts: u32,
    delay_secs: f64,
    backoff: bool,
    quiet: bool,
    verbose: bool,
    command: Vec<String>,
}

fn parse_retry_args(args: &[String]) -> std::result::Result<RetryConfig, String> {
    let mut max_attempts: u32 = 3;
    let mut delay_secs: f64 = 1.0;
    let mut backoff = false;
    let mut quiet = false;
    let mut verbose = false;
    let mut command = Vec::new();

    let mut i = 0;
    let mut found_separator = false;

    while i < args.len() {
        let arg = &args[i];
        if found_separator {
            command.push(arg.clone());
            i += 1;
            continue;
        }
        match arg.as_str() {
            "--" => {
                found_separator = true;
            }
            "-n" => {
                i += 1;
                if i >= args.len() {
                    return Err("retry: -n requires an argument".to_string());
                }
                max_attempts = args[i]
                    .parse()
                    .map_err(|_| format!("retry: invalid number '{}'", args[i]))?;
                if max_attempts == 0 {
                    return Err("retry: -n must be at least 1".to_string());
                }
            }
            "-d" => {
                i += 1;
                if i >= args.len() {
                    return Err("retry: -d requires an argument".to_string());
                }
                delay_secs = args[i]
                    .parse()
                    .map_err(|_| format!("retry: invalid delay '{}'", args[i]))?;
                if delay_secs < 0.0 {
                    return Err("retry: delay must be non-negative".to_string());
                }
            }
            "--backoff" => {
                backoff = true;
            }
            "-q" => {
                quiet = true;
            }
            "-v" => {
                verbose = true;
            }
            other => {
                return Err(format!("retry: unknown option '{other}'"));
            }
        }
        i += 1;
    }

    Ok(RetryConfig {
        max_attempts,
        delay_secs,
        backoff,
        quiet,
        verbose,
        command,
    })
}

#[async_trait]
impl Builtin for Retry {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "retry: usage: retry [OPTIONS] -- command [args...]\n".to_string(),
                1,
            ));
        }

        let config = match parse_retry_args(ctx.args) {
            Ok(c) => c,
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
        };

        let mut output = String::new();

        if !config.quiet {
            output.push_str(&format!(
                "retry: would retry {} time(s) with {:.1}s delay",
                config.max_attempts, config.delay_secs,
            ));
            if config.backoff {
                output.push_str(" (exponential backoff)");
            }
            output.push('\n');

            if !config.command.is_empty() {
                output.push_str(&format!("retry: command: {}\n", config.command.join(" ")));
            }

            if config.verbose {
                for attempt in 1..=config.max_attempts {
                    let delay = if config.backoff {
                        config.delay_secs * 2.0_f64.powi((attempt as i32) - 1)
                    } else {
                        config.delay_secs
                    };
                    output.push_str(&format!(
                        "retry: attempt {attempt}/{} delay {delay:.1}s\n",
                        config.max_attempts,
                    ));
                }
            }

            output.push_str("retry: not supported in virtual environment\n");
        }

        Ok(ExecResult::ok(output))
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

    async fn run_retry(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Retry.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_no_args() {
        let result = run_retry(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_defaults_with_separator() {
        let result = run_retry(&["--", "echo", "hello"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("3 time(s)"));
        assert!(result.stdout.contains("1.0s delay"));
        assert!(result.stdout.contains("command: echo hello"));
    }

    #[tokio::test]
    async fn test_custom_attempts_and_delay() {
        let result = run_retry(&["-n", "5", "-d", "2.5", "--", "curl", "http://x"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("5 time(s)"));
        assert!(result.stdout.contains("2.5s delay"));
    }

    #[tokio::test]
    async fn test_backoff_flag() {
        let result = run_retry(&["--backoff", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("exponential backoff"));
    }

    #[tokio::test]
    async fn test_quiet_mode() {
        let result = run_retry(&["-q", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_verbose_mode() {
        let result = run_retry(&["-v", "-n", "3", "--backoff", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("attempt 1/3"));
        assert!(result.stdout.contains("attempt 2/3"));
        assert!(result.stdout.contains("attempt 3/3"));
    }

    #[tokio::test]
    async fn test_invalid_n() {
        let result = run_retry(&["-n", "abc", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid number"));
    }

    #[tokio::test]
    async fn test_missing_n_arg() {
        let result = run_retry(&["-n"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("-n requires an argument"));
    }

    #[tokio::test]
    async fn test_zero_attempts() {
        let result = run_retry(&["-n", "0", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("must be at least 1"));
    }

    #[tokio::test]
    async fn test_unknown_option() {
        let result = run_retry(&["--foo", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unknown option"));
    }
}
