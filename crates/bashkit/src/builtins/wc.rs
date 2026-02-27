//! Word count builtin - count lines, words, bytes, and characters

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The wc builtin - print newline, word, and byte counts.
///
/// Usage: wc [-lwcmL] [FILE...]
///
/// Options:
///   -l, --lines   Print the newline count
///   -w, --words   Print the word count
///   -c, --bytes   Print the byte count
///   -m, --chars   Print the character count
///   -L, --max-line-length  Print the maximum line length
///
/// With no options, prints lines, words, and bytes.
pub struct Wc;

/// Parsed wc flags
struct WcFlags {
    lines: bool,
    words: bool,
    bytes: bool,
    chars: bool,
    max_line_length: bool,
}

impl WcFlags {
    fn parse(args: &[String]) -> Self {
        let mut lines = false;
        let mut words = false;
        let mut bytes = false;
        let mut chars = false;
        let mut max_line_length = false;

        for arg in args {
            if !arg.starts_with('-') {
                continue;
            }
            match arg.as_str() {
                "--lines" => lines = true,
                "--words" => words = true,
                "--bytes" => bytes = true,
                "--chars" => chars = true,
                "--max-line-length" => max_line_length = true,
                _ if arg.starts_with('-') && !arg.starts_with("--") => {
                    for ch in arg[1..].chars() {
                        match ch {
                            'l' => lines = true,
                            'w' => words = true,
                            'c' => bytes = true,
                            'm' => chars = true,
                            'L' => max_line_length = true,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Default: show lines, words, bytes if no flags
        if !lines && !words && !bytes && !chars && !max_line_length {
            lines = true;
            words = true;
            bytes = true;
        }

        Self {
            lines,
            words,
            bytes,
            chars,
            max_line_length,
        }
    }

    /// Number of active count fields
    fn active_count(&self) -> usize {
        [self.lines, self.words, self.bytes, self.chars, self.max_line_length]
            .iter()
            .filter(|&&b| b)
            .count()
    }
}

#[async_trait]
impl Builtin for Wc {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let flags = WcFlags::parse(ctx.args);

        let files: Vec<_> = ctx
            .args
            .iter()
            .filter(|a| !a.starts_with('-') || a.as_str() == "-")
            .collect();

        let mut output = String::new();
        let mut total_lines = 0usize;
        let mut total_words = 0usize;
        let mut total_bytes = 0usize;
        let mut total_chars = 0usize;
        let mut total_max_line = 0usize;

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                let counts = count_text(stdin);
                // Real bash: no padding for single-value stdin, padded for multiple values
                let padded = flags.active_count() > 1;
                output.push_str(&format_counts(&counts, &flags, None, padded));
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
                        let counts = count_text(&text);

                        total_lines += counts.lines;
                        total_words += counts.words;
                        total_bytes += counts.bytes;
                        total_chars += counts.chars;
                        if counts.max_line_length > total_max_line {
                            total_max_line = counts.max_line_length;
                        }

                        output.push_str(&format_counts(&counts, &flags, Some(file), true));
                        output.push('\n');
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("wc: {}: {}\n", file, e), 1));
                    }
                }
            }

            // Print total if multiple files
            if files.len() > 1 {
                let totals = TextCounts {
                    lines: total_lines,
                    words: total_words,
                    bytes: total_bytes,
                    chars: total_chars,
                    max_line_length: total_max_line,
                };
                output.push_str(&format_counts(&totals, &flags, Some(&"total".to_string()), true));
                output.push('\n');
            }
        }

        Ok(ExecResult::ok(output))
    }
}

struct TextCounts {
    lines: usize,
    words: usize,
    bytes: usize,
    chars: usize,
    max_line_length: usize,
}

/// Count lines, words, bytes, characters, and max line length in text
fn count_text(text: &str) -> TextCounts {
    let lines = text.lines().count();
    let words = text.split_whitespace().count();
    let bytes = text.len();
    let chars = text.chars().count();
    let max_line_length = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    TextCounts {
        lines,
        words,
        bytes,
        chars,
        max_line_length,
    }
}

/// Format counts for output.
/// When `padded` is true, right-align numbers in 8-char fields (used for file output).
/// When `padded` is false, use minimal formatting like real bash stdin output.
fn format_counts(
    counts: &TextCounts,
    flags: &WcFlags,
    filename: Option<&String>,
    padded: bool,
) -> String {
    let mut values: Vec<usize> = Vec::new();

    if flags.lines {
        values.push(counts.lines);
    }
    if flags.words {
        values.push(counts.words);
    }
    if flags.bytes {
        values.push(counts.bytes);
    }
    if flags.chars {
        values.push(counts.chars);
    }
    if flags.max_line_length {
        values.push(counts.max_line_length);
    }

    let result = if padded {
        // Real bash uses 7-char wide fields separated by a space
        let parts: Vec<String> = values.iter().map(|v| format!("{:>7}", v)).collect();
        parts.join(" ")
    } else {
        let parts: Vec<String> = values.iter().map(|v| v.to_string()).collect();
        parts.join(" ")
    };

    if let Some(name) = filename {
        format!("{} {}", result, name)
    } else {
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
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

    #[tokio::test]
    async fn test_wc_chars() {
        let result = run_wc(&["-m"], Some("hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("5"));
    }

    #[tokio::test]
    async fn test_wc_chars_unicode() {
        // héllo: 5 chars but 6 bytes (é is 2 bytes in UTF-8)
        let result = run_wc(&["-m"], Some("héllo")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("5"));
    }

    #[tokio::test]
    async fn test_wc_max_line_length() {
        let result = run_wc(&["-L"], Some("short\nlongerline\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("10"));
    }

    #[tokio::test]
    async fn test_wc_long_flags() {
        let result = run_wc(&["--bytes"], Some("hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("5"));

        let result = run_wc(&["--lines"], Some("a\nb\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("2"));

        let result = run_wc(&["--words"], Some("one two three")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().contains("3"));
    }
}
