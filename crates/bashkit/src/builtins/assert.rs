//! assert builtin - assertion testing for scripts
//!
//! Non-standard builtin. Evaluates test expressions and fails with
//! a message if the assertion is false.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

/// Assert builtin - evaluate test expressions with failure messages.
///
/// Usage: assert <test-expression> [message]
///
/// Supports:
///   String:  -z, -n, =, !=
///   Numeric: -eq, -ne, -lt, -gt, -le, -ge
///   File:    -f, -d, -e
///
/// If the expression is true, exits 0 silently.
/// If false, prints "assertion failed: <message>" to stderr and exits 1.
pub struct Assert;

/// Evaluate a unary file/string test.
async fn eval_unary(op: &str, arg: &str, fs: &Arc<dyn FileSystem>, cwd: &Path) -> bool {
    match op {
        "-z" => arg.is_empty(),
        "-n" => !arg.is_empty(),
        "-e" => {
            let path = super::resolve_path(cwd, arg);
            fs.exists(&path).await.unwrap_or(false)
        }
        "-f" => {
            let path = super::resolve_path(cwd, arg);
            if let Ok(meta) = fs.stat(&path).await {
                meta.file_type.is_file()
            } else {
                false
            }
        }
        "-d" => {
            let path = super::resolve_path(cwd, arg);
            if let Ok(meta) = fs.stat(&path).await {
                meta.file_type.is_dir()
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Evaluate a binary comparison.
fn eval_binary(left: &str, op: &str, right: &str) -> Option<bool> {
    match op {
        "=" | "==" => Some(left == right),
        "!=" => Some(left != right),
        "-eq" => Some(parse_int(left) == parse_int(right)),
        "-ne" => Some(parse_int(left) != parse_int(right)),
        "-lt" => Some(parse_int(left) < parse_int(right)),
        "-gt" => Some(parse_int(left) > parse_int(right)),
        "-le" => Some(parse_int(left) <= parse_int(right)),
        "-ge" => Some(parse_int(left) >= parse_int(right)),
        _ => None,
    }
}

fn parse_int(s: &str) -> i64 {
    s.trim().parse().unwrap_or(0)
}

/// Extract the assertion message from args that come after the test expression.
/// Returns (test_args, message).
fn split_args(args: &[String]) -> (&[String], Option<String>) {
    // If args start with "[", find the matching "]" and take the rest as message
    if args.first().map(|s| s.as_str()) == Some("[")
        && let Some(pos) = args.iter().position(|s| s == "]")
    {
        let test_args = &args[1..pos];
        let msg = if pos + 1 < args.len() {
            Some(args[pos + 1..].join(" "))
        } else {
            None
        };
        return (test_args, msg);
    }

    // Heuristic: look for known operators to determine where test expr ends.
    // For unary: 2 args = operator + operand, rest is message
    // For binary: 3 args = left op right, rest is message
    if args.len() >= 3 && is_binary_op(&args[1]) {
        let msg = if args.len() > 3 {
            Some(args[3..].join(" "))
        } else {
            None
        };
        return (&args[..3], msg);
    }

    if args.len() >= 2 && is_unary_op(&args[0]) {
        let msg = if args.len() > 2 {
            Some(args[2..].join(" "))
        } else {
            None
        };
        return (&args[..2], msg);
    }

    // Single arg: non-empty string test
    if !args.is_empty() {
        let msg = if args.len() > 1 {
            Some(args[1..].join(" "))
        } else {
            None
        };
        return (&args[..1], msg);
    }

    (args, None)
}

fn is_unary_op(s: &str) -> bool {
    matches!(s, "-z" | "-n" | "-e" | "-f" | "-d")
}

fn is_binary_op(s: &str) -> bool {
    matches!(
        s,
        "=" | "==" | "!=" | "-eq" | "-ne" | "-lt" | "-gt" | "-le" | "-ge"
    )
}

#[async_trait]
impl Builtin for Assert {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "assert: usage: assert <test-expression> [message]\n".to_string(),
                1,
            ));
        }

        let (test_args, message) = split_args(ctx.args);
        let cwd = ctx.cwd.clone();

        let passed = match test_args.len() {
            0 => false,
            1 => {
                // Single arg: true if non-empty
                !test_args[0].is_empty()
            }
            2 => {
                // Unary operator
                eval_unary(&test_args[0], &test_args[1], &ctx.fs, &cwd).await
            }
            3 => {
                // Binary operator
                eval_binary(&test_args[0], &test_args[1], &test_args[2]).unwrap_or(false)
            }
            _ => false,
        };

        if passed {
            Ok(ExecResult::ok(String::new()))
        } else {
            let msg = message.unwrap_or_else(|| {
                test_args
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            Ok(ExecResult::err(format!("assertion failed: {msg}\n"), 1))
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

    async fn run_assert(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Assert.execute(ctx).await.unwrap()
    }

    async fn run_assert_with_fs(args: &[&str], fs: Arc<InMemoryFs>) -> ExecResult {
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Assert.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_no_args() {
        let result = run_assert(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_string_equal_pass() {
        let result = run_assert(&["hello", "=", "hello"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_string_equal_fail() {
        let result = run_assert(&["hello", "=", "world"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("assertion failed"));
    }

    #[tokio::test]
    async fn test_string_not_equal() {
        let result = run_assert(&["a", "!=", "b"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_numeric_eq_pass() {
        let result = run_assert(&["42", "-eq", "42"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_numeric_lt_pass() {
        let result = run_assert(&["1", "-lt", "10"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_numeric_gt_fail() {
        let result = run_assert(&["1", "-gt", "10"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("assertion failed"));
    }

    #[tokio::test]
    async fn test_z_empty_string() {
        let result = run_assert(&["-z", ""]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_n_nonempty_string() {
        let result = run_assert(&["-n", "value"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_file_exists() {
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(Path::new("/test.txt"), b"data")
            .await
            .unwrap();
        let result = run_assert_with_fs(&["-f", "/test.txt"], fs).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_file_not_exists() {
        let result = run_assert(&["-f", "/nope.txt"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("assertion failed"));
    }

    #[tokio::test]
    async fn test_dir_exists() {
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(Path::new("/mydir"), true).await.unwrap();
        let result = run_assert_with_fs(&["-d", "/mydir"], fs).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_bracket_syntax_pass() {
        let result = run_assert(&["[", "x", "=", "x", "]"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_bracket_syntax_fail_with_message() {
        let result = run_assert(&["[", "a", "=", "b", "]", "values", "differ"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("assertion failed: values differ"));
    }

    #[tokio::test]
    async fn test_custom_message() {
        let result = run_assert(&["1", "-eq", "2", "expected", "equal"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("assertion failed: expected equal"));
    }

    #[tokio::test]
    async fn test_e_exists() {
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(Path::new("/exists.txt"), b"x").await.unwrap();
        let result = run_assert_with_fs(&["-e", "/exists.txt"], fs).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_numeric_le() {
        let result = run_assert(&["5", "-le", "5"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_numeric_ge() {
        let result = run_assert(&["10", "-ge", "5"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_numeric_ne() {
        let result = run_assert(&["1", "-ne", "2"]).await;
        assert_eq!(result.exit_code, 0);
    }
}
