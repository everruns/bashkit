//! Head and tail builtins - output first/last lines of input

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Default number of lines to output
const DEFAULT_LINES: usize = 10;

/// The head builtin - output the first N lines of input.
///
/// Usage: head [-n NUM] [FILE...]
///
/// Options:
///   -n NUM   Output the first NUM lines (default: 10)
///   -NUM     Shorthand for -n NUM
pub struct Head;

#[async_trait]
impl Builtin for Head {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (num_lines, files) = parse_head_tail_args(ctx.args, DEFAULT_LINES)?;

        let mut output = String::new();

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                output = take_first_lines(stdin, num_lines);
            }
        } else {
            // Read from files
            let multiple_files = files.len() > 1;
            for (i, file) in files.iter().enumerate() {
                if multiple_files {
                    if i > 0 {
                        output.push('\n');
                    }
                    output.push_str(&format!("==> {} <==\n", file));
                }

                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        output.push_str(&take_first_lines(&text, num_lines));
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("head: {}: {}\n", file, e), 1));
                    }
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// The tail builtin - output the last N lines of input.
///
/// Usage: tail [-n NUM] [FILE...]
///
/// Options:
///   -n NUM   Output the last NUM lines (default: 10)
///   -NUM     Shorthand for -n NUM
pub struct Tail;

#[async_trait]
impl Builtin for Tail {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (num_lines, files) = parse_head_tail_args(ctx.args, DEFAULT_LINES)?;

        let mut output = String::new();

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                output = take_last_lines(stdin, num_lines);
            }
        } else {
            // Read from files
            let multiple_files = files.len() > 1;
            for (i, file) in files.iter().enumerate() {
                if multiple_files {
                    if i > 0 {
                        output.push('\n');
                    }
                    output.push_str(&format!("==> {} <==\n", file));
                }

                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        output.push_str(&take_last_lines(&text, num_lines));
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("tail: {}: {}\n", file, e), 1));
                    }
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// Parse arguments for head/tail commands
/// Returns (num_lines, file_list)
fn parse_head_tail_args(args: &[String], default: usize) -> Result<(usize, Vec<String>)> {
    let mut num_lines = default;
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "-n" {
            // -n NUM
            i += 1;
            if i < args.len() {
                num_lines = args[i].parse().unwrap_or(default);
            }
        } else if let Some(num_str) = arg.strip_prefix("-n") {
            // -nNUM (no space)
            num_lines = num_str.parse().unwrap_or(default);
        } else if let Some(num_str) = arg.strip_prefix('-') {
            // -NUM shorthand
            if let Ok(n) = num_str.parse::<usize>() {
                num_lines = n;
            }
            // Ignore unknown options
        } else {
            // File argument
            files.push(arg.clone());
        }
        i += 1;
    }

    Ok((num_lines, files))
}

/// Take the first N lines from text
fn take_first_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().take(n).collect();
    if lines.is_empty() {
        String::new()
    } else {
        let mut result = lines.join("\n");
        // Preserve trailing newline if original had one
        if text.ends_with('\n') || !text.is_empty() {
            result.push('\n');
        }
        result
    }
}

/// Take the last N lines from text
fn take_last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    let selected: Vec<&str> = lines[start..].to_vec();

    if selected.is_empty() {
        String::new()
    } else {
        let mut result = selected.join("\n");
        // Preserve trailing newline if original had one
        if text.ends_with('\n') || !text.is_empty() {
            result.push('\n');
        }
        result
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

    async fn run_head(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            #[cfg(feature = "network")]
            http_client: None,
        };

        Head.execute(ctx).await.unwrap()
    }

    async fn run_tail(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            #[cfg(feature = "network")]
            http_client: None,
        };

        Tail.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_head_default() {
        let input = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n";
        let result = run_head(&[], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    }

    #[tokio::test]
    async fn test_head_n_flag() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_head(&["-n", "3"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_head_shorthand() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_head(&["-2"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_tail_default() {
        let input = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n";
        let result = run_tail(&[], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n");
    }

    #[tokio::test]
    async fn test_tail_n_flag() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_tail(&["-n", "3"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "c\nd\ne\n");
    }

    #[tokio::test]
    async fn test_tail_shorthand() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_tail(&["-2"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "d\ne\n");
    }

    #[tokio::test]
    async fn test_head_empty_input() {
        let result = run_head(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_tail_empty_input() {
        let result = run_tail(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_head_fewer_lines_than_requested() {
        let input = "a\nb\n";
        let result = run_head(&["-n", "10"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_tail_fewer_lines_than_requested() {
        let input = "a\nb\n";
        let result = run_tail(&["-n", "10"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }
}
