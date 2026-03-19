//! log builtin - structured logging for scripts
//!
//! Non-standard builtin. Outputs messages at specified levels with
//! optional key=value pairs. Respects LOG_LEVEL and LOG_FORMAT env vars.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Log builtin - structured logging with level filtering.
///
/// Usage: log <level> <message> [key=value...]
///
/// Levels: debug, info, warn, error
/// Env vars:
///   LOG_LEVEL  - minimum level to output (default: info)
///   LOG_FORMAT - "text" (default) or "json"
///
/// Text format: [LEVEL] message key=value...
/// JSON format: {"level":"info","message":"msg","key":"value",...}
pub struct Log;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

fn parse_level(s: &str) -> Option<Level> {
    match s.to_ascii_lowercase().as_str() {
        "debug" => Some(Level::Debug),
        "info" => Some(Level::Info),
        "warn" | "warning" => Some(Level::Warn),
        "error" | "err" => Some(Level::Error),
        _ => None,
    }
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Debug => "DEBUG",
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    }
}

/// Escape a string for JSON output without pulling in serde.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

#[async_trait]
impl Builtin for Log {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.len() < 2 {
            return Ok(ExecResult::err(
                "log: usage: log <level> <message> [key=value...]\n".to_string(),
                1,
            ));
        }

        let level = match parse_level(&ctx.args[0]) {
            Some(l) => l,
            None => {
                return Ok(ExecResult::err(
                    format!(
                        "log: unknown level '{}'. Use: debug, info, warn, error\n",
                        ctx.args[0]
                    ),
                    1,
                ));
            }
        };

        // Check LOG_LEVEL threshold
        let min_level = ctx
            .env
            .get("LOG_LEVEL")
            .and_then(|s| parse_level(s))
            .unwrap_or(Level::Info);

        if level < min_level {
            return Ok(ExecResult::ok(String::new()));
        }

        let message = &ctx.args[1];

        // Collect key=value pairs
        let mut kvs: Vec<(&str, &str)> = Vec::new();
        for arg in &ctx.args[2..] {
            if let Some(eq_pos) = arg.find('=') {
                kvs.push((&arg[..eq_pos], &arg[eq_pos + 1..]));
            }
        }

        let format = ctx
            .env
            .get("LOG_FORMAT")
            .map(|s| s.as_str())
            .unwrap_or("text");

        let output = if format == "json" {
            let mut json = format!(
                "{{\"level\":\"{}\",\"message\":\"{}\"",
                json_escape(level_name(level)),
                json_escape(message),
            );
            for (k, v) in &kvs {
                json.push_str(&format!(",\"{}\":\"{}\"", json_escape(k), json_escape(v),));
            }
            json.push_str("}\n");
            json
        } else {
            let mut line = format!("[{}] {}", level_name(level), message);
            for (k, v) in &kvs {
                line.push_str(&format!(" {k}={v}"));
            }
            line.push('\n');
            line
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

    use crate::fs::InMemoryFs;

    async fn run_log(args: &[&str], env: HashMap<String, String>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Log.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_no_args() {
        let result = run_log(&[], HashMap::new()).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_invalid_level() {
        let result = run_log(&["critical", "oh no"], HashMap::new()).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unknown level"));
    }

    #[tokio::test]
    async fn test_info_text_format() {
        let result = run_log(&["info", "server started"], HashMap::new()).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "[INFO] server started\n");
    }

    #[tokio::test]
    async fn test_text_with_kvs() {
        let result = run_log(
            &["warn", "high latency", "ms=250", "endpoint=/api"],
            HashMap::new(),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "[WARN] high latency ms=250 endpoint=/api\n");
    }

    #[tokio::test]
    async fn test_json_format() {
        let mut env = HashMap::new();
        env.insert("LOG_FORMAT".to_string(), "json".to_string());
        let result = run_log(&["error", "failed", "code=500"], env).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "{\"level\":\"ERROR\",\"message\":\"failed\",\"code\":\"500\"}\n"
        );
    }

    #[tokio::test]
    async fn test_level_filtering_suppresses() {
        let mut env = HashMap::new();
        env.insert("LOG_LEVEL".to_string(), "warn".to_string());
        let result = run_log(&["debug", "noisy"], env).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_level_filtering_passes() {
        let mut env = HashMap::new();
        env.insert("LOG_LEVEL".to_string(), "debug".to_string());
        let result = run_log(&["debug", "trace detail"], env).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "[DEBUG] trace detail\n");
    }

    #[tokio::test]
    async fn test_error_level() {
        let result = run_log(&["error", "disk full"], HashMap::new()).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "[ERROR] disk full\n");
    }

    #[tokio::test]
    async fn test_debug_suppressed_by_default() {
        let result = run_log(&["debug", "verbose"], HashMap::new()).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_json_escaping() {
        let mut env = HashMap::new();
        env.insert("LOG_FORMAT".to_string(), "json".to_string());
        let result = run_log(&["info", "line1\\nline2", "key=val\"ue"], env).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("\"message\":\"line1\\\\nline2\""));
    }
}
