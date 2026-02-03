//! Word count builtin - count lines, words, and bytes

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The wc builtin - print newline, word, and byte counts.
///
/// Usage: wc [-lwc] [FILE...]
///
/// Options:
///   -l   Print the newline count
///   -w   Print the word count
///   -c   Print the byte count
///
/// With no options, prints all three counts.
pub struct Wc;

#[async_trait]
impl Builtin for Wc {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let show_lines = ctx.args.iter().any(|a| a.contains('l'));
        let show_words = ctx.args.iter().any(|a| a.contains('w'));
        let show_bytes = ctx.args.iter().any(|a| a.contains('c'));

        // If no flags specified, show all
        let (show_lines, show_words, show_bytes) = if !show_lines && !show_words && !show_bytes {
            (true, true, true)
        } else {
            (show_lines, show_words, show_bytes)
        };

        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        let mut output = String::new();
        let mut total_lines = 0usize;
        let mut total_words = 0usize;
        let mut total_bytes = 0usize;

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                let (lines, words, bytes) = count_text(stdin);
                output.push_str(&format_counts(
                    lines, words, bytes, show_lines, show_words, show_bytes, None,
                ));
                output.push('\n');
            }
        } else {
            // Read from files
            for file in &files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        let (lines, words, bytes) = count_text(&text);

                        total_lines += lines;
                        total_words += words;
                        total_bytes += bytes;

                        output.push_str(&format_counts(
                            lines,
                            words,
                            bytes,
                            show_lines,
                            show_words,
                            show_bytes,
                            Some(file),
                        ));
                        output.push('\n');
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("wc: {}: {}\n", file, e), 1));
                    }
                }
            }

            // Print total if multiple files
            if files.len() > 1 {
                output.push_str(&format_counts(
                    total_lines,
                    total_words,
                    total_bytes,
                    show_lines,
                    show_words,
                    show_bytes,
                    Some(&"total".to_string()),
                ));
                output.push('\n');
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// Count lines, words, and bytes in text
fn count_text(text: &str) -> (usize, usize, usize) {
    let lines = text.lines().count();
    let words = text.split_whitespace().count();
    let bytes = text.len();
    (lines, words, bytes)
}

/// Format counts for output
fn format_counts(
    lines: usize,
    words: usize,
    bytes: usize,
    show_lines: bool,
    show_words: bool,
    show_bytes: bool,
    filename: Option<&String>,
) -> String {
    let mut parts = Vec::new();

    if show_lines {
        parts.push(format!("{:>8}", lines));
    }
    if show_words {
        parts.push(format!("{:>8}", words));
    }
    if show_bytes {
        parts.push(format!("{:>8}", bytes));
    }

    let mut result = parts.join("");
    if let Some(name) = filename {
        result.push(' ');
        result.push_str(name);
    }
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_wc(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Wc.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_wc_all() {
        let result = run_wc(&[], Some("one two three\nfour five\n")).await;
        assert_eq!(result.exit_code, 0);
        // 2 lines, 5 words, 25 bytes
        assert!(result.stdout.contains("2"));
        assert!(result.stdout.contains("5"));
    }

    #[tokio::test]
    async fn test_wc_lines_only() {
        let result = run_wc(&["-l"], Some("one\ntwo\nthree\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("3"));
    }

    #[tokio::test]
    async fn test_wc_words_only() {
        let result = run_wc(&["-w"], Some("one two three four five")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("5"));
    }

    #[tokio::test]
    async fn test_wc_bytes_only() {
        let result = run_wc(&["-c"], Some("hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("5"));
    }

    #[tokio::test]
    async fn test_wc_empty() {
        let result = run_wc(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("0"));
    }
}
