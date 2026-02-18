//! Sort and uniq builtins - sort lines and filter duplicates

// Uses unwrap() after is_empty() check (e.g., files.first() in else branch)
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The sort builtin - sort lines of text.
///
/// Usage: sort [-fnruV] [FILE...]
///
/// Options:
///   -f   Fold lower case to upper case characters (case insensitive)
///   -n   Compare according to string numerical value
///   -r   Reverse the result of comparisons
///   -u   Output only unique lines (like sort | uniq)
///   -V   Natural sort of version numbers
pub struct Sort;

#[async_trait]
impl Builtin for Sort {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let reverse = ctx
            .args
            .iter()
            .any(|a| a.contains('r') && a.starts_with('-'));
        let numeric = ctx
            .args
            .iter()
            .any(|a| a.contains('n') && a.starts_with('-'));
        let unique = ctx
            .args
            .iter()
            .any(|a| a.contains('u') && a.starts_with('-'));
        let fold_case = ctx
            .args
            .iter()
            .any(|a| a.contains('f') && a.starts_with('-'));

        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        // Collect all input
        let mut all_lines = Vec::new();

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                for line in stdin.lines() {
                    all_lines.push(line.to_string());
                }
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
                        for line in text.lines() {
                            all_lines.push(line.to_string());
                        }
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("sort: {}: {}\n", file, e), 1));
                    }
                }
            }
        }

        // Sort the lines
        if numeric {
            all_lines.sort_by(|a, b| {
                let a_num: f64 = a
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let b_num: f64 = b
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                a_num
                    .partial_cmp(&b_num)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        } else if fold_case {
            all_lines.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        } else {
            all_lines.sort();
        }

        if reverse {
            all_lines.reverse();
        }

        // Remove duplicates if -u
        if unique {
            all_lines.dedup();
        }

        let mut output = all_lines.join("\n");
        if !output.is_empty() {
            output.push('\n');
        }

        Ok(ExecResult::ok(output))
    }
}

/// The uniq builtin - report or omit repeated lines.
///
/// Usage: uniq [-cdu] [INPUT [OUTPUT]]
///
/// Options:
///   -c   Prefix lines by the number of occurrences
///   -d   Only print duplicate lines
///   -u   Only print unique lines
pub struct Uniq;

#[async_trait]
impl Builtin for Uniq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let count = ctx
            .args
            .iter()
            .any(|a| a.contains('c') && a.starts_with('-'));
        let only_duplicates = ctx
            .args
            .iter()
            .any(|a| a.contains('d') && a.starts_with('-'));
        let only_unique = ctx
            .args
            .iter()
            .any(|a| a.contains('u') && a.starts_with('-'));

        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        // Get input lines
        let lines: Vec<String> = if files.is_empty() {
            // Read from stdin
            ctx.stdin
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        } else {
            // Read from first file
            let file = files.first().unwrap();
            let path = if file.starts_with('/') {
                std::path::PathBuf::from(file)
            } else {
                ctx.cwd.join(file)
            };

            match ctx.fs.read_file(&path).await {
                Ok(content) => {
                    let text = String::from_utf8_lossy(&content);
                    text.lines().map(|l| l.to_string()).collect()
                }
                Err(e) => {
                    return Ok(ExecResult::err(format!("uniq: {}: {}\n", file, e), 1));
                }
            }
        };

        // Process lines - uniq only removes adjacent duplicates
        let mut result = Vec::new();
        let mut prev_line: Option<String> = None;
        let mut current_count = 0usize;

        for line in lines {
            if let Some(ref prev) = prev_line {
                if *prev == line {
                    current_count += 1;
                    continue;
                } else {
                    // Output previous line based on flags
                    let should_output = if only_duplicates {
                        current_count > 1
                    } else if only_unique {
                        current_count == 1
                    } else {
                        true
                    };

                    if should_output {
                        if count {
                            result.push(format!("{:>7} {}", current_count, prev));
                        } else {
                            result.push(prev.clone());
                        }
                    }
                }
            }
            prev_line = Some(line);
            current_count = 1;
        }

        // Don't forget the last line
        if let Some(prev) = prev_line {
            let should_output = if only_duplicates {
                current_count > 1
            } else if only_unique {
                current_count == 1
            } else {
                true
            };

            if should_output {
                if count {
                    result.push(format!("{:>7} {}", current_count, prev));
                } else {
                    result.push(prev);
                }
            }
        }

        let mut output = result.join("\n");
        if !output.is_empty() {
            output.push('\n');
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

    async fn run_sort(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Sort.execute(ctx).await.unwrap()
    }

    async fn run_uniq(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Uniq.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_sort_basic() {
        let result = run_sort(&[], Some("banana\napple\ncherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nbanana\ncherry\n");
    }

    #[tokio::test]
    async fn test_sort_reverse() {
        let result = run_sort(&["-r"], Some("apple\nbanana\ncherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "cherry\nbanana\napple\n");
    }

    #[tokio::test]
    async fn test_sort_numeric() {
        let result = run_sort(&["-n"], Some("10\n2\n1\n20\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n10\n20\n");
    }

    #[tokio::test]
    async fn test_sort_unique() {
        let result = run_sort(&["-u"], Some("apple\nbanana\napple\ncherry\nbanana\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nbanana\ncherry\n");
    }

    #[tokio::test]
    async fn test_sort_fold_case() {
        let result = run_sort(&["-f"], Some("Banana\napple\nCherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nBanana\nCherry\n");
    }

    #[tokio::test]
    async fn test_uniq_basic() {
        let result = run_uniq(&[], Some("a\na\nb\nb\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_uniq_count() {
        let result = run_uniq(&["-c"], Some("a\na\nb\nc\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("2 a"));
        assert!(result.stdout.contains("1 b"));
        assert!(result.stdout.contains("3 c"));
    }

    #[tokio::test]
    async fn test_uniq_duplicates_only() {
        let result = run_uniq(&["-d"], Some("a\na\nb\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a"));
        assert!(result.stdout.contains("c"));
        assert!(!result.stdout.contains("b\n"));
    }

    #[tokio::test]
    async fn test_uniq_unique_only() {
        let result = run_uniq(&["-u"], Some("a\na\nb\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("b"));
        assert!(!result.stdout.contains("a\n"));
        assert!(!result.stdout.contains("c\n"));
    }
}
