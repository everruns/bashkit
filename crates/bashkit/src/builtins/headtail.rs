//! Head and tail builtins - output first/last lines of input

use async_trait::async_trait;

use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Default number of lines to output
const DEFAULT_LINES: usize = 10;

/// The head builtin - output the first N lines or bytes of input.
///
/// Usage: head [-n NUM | -c NUM] [FILE...]
///
/// Options:
///   -n NUM   Output the first NUM lines (default: 10)
///   -c NUM   Output the first NUM bytes
///   -NUM     Shorthand for -n NUM
pub struct Head;

#[async_trait]
impl Builtin for Head {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (count, byte_mode, files) = parse_head_args(ctx.args, DEFAULT_LINES)?;

        let mut output = String::new();

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                if byte_mode {
                    output = take_first_bytes(stdin, count);
                } else {
                    output = take_first_lines(stdin, count);
                }
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
                        if byte_mode {
                            // Byte mode: take first N bytes, preserve raw byte values
                            let bytes = &content[..content.len().min(count)];
                            let s: String = bytes.iter().map(|&b| b as char).collect();
                            output.push_str(&s);
                        } else {
                            let text: String = content.iter().map(|&b| b as char).collect();
                            output.push_str(&take_first_lines(&text, count));
                        }
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
///   -n +NUM  Output starting from line NUM (1-indexed)
///   -NUM     Shorthand for -n NUM
pub struct Tail;

#[async_trait]
impl Builtin for Tail {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (num_lines, from_start, files) = parse_tail_args(ctx.args, DEFAULT_LINES)?;

        let mut output = String::new();

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                output = if from_start {
                    take_from_line(stdin, num_lines)
                } else {
                    take_last_lines(stdin, num_lines)
                };
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

                let text = match read_text_file(&*ctx.fs, &path, "tail").await {
                    Ok(t) => t,
                    Err(e) => return Ok(e),
                };
                let selected = if from_start {
                    take_from_line(&text, num_lines)
                } else {
                    take_last_lines(&text, num_lines)
                };
                output.push_str(&selected);
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// Parse arguments for head command, including -c (byte count) mode.
/// Returns (count, byte_mode, file_list)
fn parse_head_args(args: &[String], default: usize) -> Result<(usize, bool, Vec<String>)> {
    let mut count = default;
    let mut byte_mode = false;
    let mut files = Vec::new();
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if let Some(val) = p.flag_value_opt("-n") {
            count = val.parse().unwrap_or(default);
            byte_mode = false;
        } else if let Some(val) = p.flag_value_opt("-c") {
            count = val.parse().unwrap_or(default);
            byte_mode = true;
        } else if let Some(arg) = p.current().filter(|a| a.starts_with('-')) {
            if let Some(num_str) = arg.strip_prefix('-')
                && let Ok(n) = num_str.parse::<usize>()
            {
                count = n;
            }
            p.advance();
        } else if let Some(arg) = p.positional() {
            files.push(arg.to_string());
        }
    }

    Ok((count, byte_mode, files))
}

/// Take the first N bytes from text.
/// Uses char-level truncation so that Latin-1 encoded binary data
/// (e.g. from /dev/urandom where each byte maps to one char) is
/// counted correctly — each char represents one original byte.
fn take_first_bytes(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// Parse arguments for tail command, including +N "from start" syntax.
/// Returns (num_lines, from_start, file_list)
fn parse_tail_args(args: &[String], default: usize) -> Result<(usize, bool, Vec<String>)> {
    let mut num_lines = default;
    let mut from_start = false;
    let mut files = Vec::new();
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if let Some(val) = p.flag_value_opt("-n") {
            if let Some(pos_str) = val.strip_prefix('+') {
                from_start = true;
                num_lines = pos_str.parse().unwrap_or(default);
            } else {
                from_start = false;
                num_lines = val.parse().unwrap_or(default);
            }
        } else if let Some(arg) = p.current().filter(|a| a.starts_with('-')) {
            if let Some(num_str) = arg.strip_prefix('-')
                && let Ok(n) = num_str.parse::<usize>()
            {
                num_lines = n;
            }
            p.advance();
        } else if let Some(arg) = p.positional() {
            files.push(arg.to_string());
        }
    }

    Ok((num_lines, from_start, files))
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

/// Take lines starting from line N (1-indexed, like `tail -n +N`)
fn take_from_line(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = if n == 0 { 0 } else { n - 1 };
    let selected: Vec<&str> = lines.into_iter().skip(start).collect();

    if selected.is_empty() {
        String::new()
    } else {
        let mut result = selected.join("\n");
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
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

    #[tokio::test]
    async fn test_tail_plus_n_from_start() {
        let input = "header\nline1\nline2\nline3\n";
        let result = run_tail(&["-n", "+2"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "line1\nline2\nline3\n");
    }

    #[tokio::test]
    async fn test_tail_plus_1_all_lines() {
        let input = "a\nb\nc\n";
        let result = run_tail(&["-n", "+1"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }
}
