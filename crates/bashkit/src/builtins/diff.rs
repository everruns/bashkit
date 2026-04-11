//! diff builtin command - compare files line by line

use async_trait::async_trait;

use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The diff builtin - compare files line by line.
///
/// Usage: diff [-u] [-q] FILE1 FILE2
///
/// Options:
///   -u         Output in unified format
///   -q         Report only whether files differ
///   --brief    Same as -q
///
/// Default output is normal (ed-style) format, matching `/usr/bin/diff`.
pub struct Diff;

struct DiffOptions {
    unified: bool,
    brief: bool,
}

fn parse_diff_args(args: &[String]) -> (DiffOptions, Vec<String>) {
    let mut opts = DiffOptions {
        unified: false,
        brief: false,
    };
    let mut files = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-u" => opts.unified = true,
            "-q" | "--brief" => opts.brief = true,
            _ if !arg.starts_with('-') || arg == "-" => files.push(arg.clone()),
            _ => {} // ignore unknown options
        }
    }

    (opts, files)
}

/// Simple LCS-based diff algorithm
fn compute_diff<'a>(lines1: &'a [String], lines2: &'a [String]) -> Vec<DiffLine<'a>> {
    // Build LCS table
    let m = lines1.len();
    let n = lines2.len();

    // Cap at reasonable size to prevent DoS
    if m * n > 10_000_000 {
        // Fall back to simple diff for very large files
        return simple_diff(lines1, lines2);
    }

    let mut dp = vec![vec![0u32; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if lines1[i - 1] == lines2[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to produce diff
    let mut result = Vec::new();
    let mut i = m;
    let mut j = n;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && lines1[i - 1] == lines2[j - 1] {
            result.push(DiffLine::Context(&lines1[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            result.push(DiffLine::Added(&lines2[j - 1]));
            j -= 1;
        } else if i > 0 {
            result.push(DiffLine::Removed(&lines1[i - 1]));
            i -= 1;
        }
    }

    result.reverse();
    result
}

/// Fallback for very large files
fn simple_diff<'a>(lines1: &'a [String], lines2: &'a [String]) -> Vec<DiffLine<'a>> {
    let mut result = Vec::new();
    for line in lines1 {
        result.push(DiffLine::Removed(line));
    }
    for line in lines2 {
        result.push(DiffLine::Added(line));
    }
    result
}

#[derive(Debug)]
enum DiffLine<'a> {
    Context(&'a str),
    Added(&'a str),
    Removed(&'a str),
}

/// Format diff output in normal (ed-style) format, the default for `/usr/bin/diff`.
///
/// Normal format uses commands like `1c1`, `2a3`, `3d2` followed by `< ` / `> ` lines.
fn format_normal(diff: &[DiffLine<'_>]) -> String {
    let mut output = String::new();

    let mut old_line: usize = 1;
    let mut new_line: usize = 1;
    let mut i = 0;

    while i < diff.len() {
        match diff[i] {
            DiffLine::Context(_) => {
                old_line += 1;
                new_line += 1;
                i += 1;
            }
            DiffLine::Removed(_) | DiffLine::Added(_) => {
                // Collect contiguous removed then added lines
                let old_start = old_line;
                let new_start = new_line;
                let mut removed = Vec::new();
                let mut added = Vec::new();

                // Collect removed lines first
                while i < diff.len() && matches!(diff[i], DiffLine::Removed(_)) {
                    if let DiffLine::Removed(line) = diff[i] {
                        removed.push(line);
                    }
                    old_line += 1;
                    i += 1;
                }
                // Then collect added lines
                while i < diff.len() && matches!(diff[i], DiffLine::Added(_)) {
                    if let DiffLine::Added(line) = diff[i] {
                        added.push(line);
                    }
                    new_line += 1;
                    i += 1;
                }

                // Determine command type
                let old_end = old_start + removed.len() - if removed.is_empty() { 0 } else { 1 };
                let new_end = new_start + added.len() - if added.is_empty() { 0 } else { 1 };

                if !removed.is_empty() && !added.is_empty() {
                    // Change
                    let old_range = if removed.len() == 1 {
                        format!("{}", old_start)
                    } else {
                        format!("{},{}", old_start, old_end)
                    };
                    let new_range = if added.len() == 1 {
                        format!("{}", new_start)
                    } else {
                        format!("{},{}", new_start, new_end)
                    };
                    output.push_str(&format!("{}c{}\n", old_range, new_range));
                    for line in &removed {
                        output.push_str(&format!("< {}\n", line));
                    }
                    output.push_str("---\n");
                    for line in &added {
                        output.push_str(&format!("> {}\n", line));
                    }
                } else if !removed.is_empty() {
                    // Delete
                    let old_range = if removed.len() == 1 {
                        format!("{}", old_start)
                    } else {
                        format!("{},{}", old_start, old_end)
                    };
                    output.push_str(&format!("{}d{}\n", old_range, new_start - 1));
                    for line in &removed {
                        output.push_str(&format!("< {}\n", line));
                    }
                } else if !added.is_empty() {
                    // Add
                    let new_range = if added.len() == 1 {
                        format!("{}", new_start)
                    } else {
                        format!("{},{}", new_start, new_end)
                    };
                    output.push_str(&format!("{}a{}\n", old_start - 1, new_range));
                    for line in &added {
                        output.push_str(&format!("> {}\n", line));
                    }
                }
            }
        }
    }

    output
}

fn format_unified(file1: &str, file2: &str, diff: &[DiffLine<'_>]) -> String {
    let mut output = String::new();

    // Check if there are any changes
    let has_changes = diff
        .iter()
        .any(|d| matches!(d, DiffLine::Added(_) | DiffLine::Removed(_)));
    if !has_changes {
        return output;
    }

    output.push_str(&format!("--- {}\n", file1));
    output.push_str(&format!("+++ {}\n", file2));

    // Generate hunks with context
    let context_lines = 3;
    let mut i = 0;

    while i < diff.len() {
        // Find next change
        let change_start = diff[i..]
            .iter()
            .position(|d| matches!(d, DiffLine::Added(_) | DiffLine::Removed(_)));

        let change_start = match change_start {
            Some(v) => i + v,
            None => break,
        };
        let hunk_start = change_start.saturating_sub(context_lines);

        // Find end of hunk (including context after changes)
        let mut hunk_end = change_start;
        let mut last_change = change_start;

        while hunk_end < diff.len() {
            if matches!(diff[hunk_end], DiffLine::Added(_) | DiffLine::Removed(_)) {
                last_change = hunk_end;
            }
            // If we're past the last change + context, stop
            if hunk_end > last_change + context_lines {
                break;
            }
            hunk_end += 1;
        }
        hunk_end = hunk_end.min(diff.len());

        // Count lines for hunk header
        let mut old_count = 0;
        let mut new_count = 0;
        let mut old_start = 1;
        let mut new_start = 1;
        let mut old_line = 1;
        let mut new_line = 1;

        for (idx, d) in diff.iter().enumerate().take(hunk_end) {
            if idx == hunk_start {
                old_start = old_line;
                new_start = new_line;
            }
            match d {
                DiffLine::Context(_) => {
                    if idx >= hunk_start {
                        old_count += 1;
                        new_count += 1;
                    }
                    old_line += 1;
                    new_line += 1;
                }
                DiffLine::Removed(_) => {
                    if idx >= hunk_start {
                        old_count += 1;
                    }
                    old_line += 1;
                }
                DiffLine::Added(_) => {
                    if idx >= hunk_start {
                        new_count += 1;
                    }
                    new_line += 1;
                }
            }
        }

        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            old_start, old_count, new_start, new_count
        ));

        for d in &diff[hunk_start..hunk_end] {
            match d {
                DiffLine::Context(line) => {
                    output.push(' ');
                    output.push_str(line);
                    output.push('\n');
                }
                DiffLine::Added(line) => {
                    output.push('+');
                    output.push_str(line);
                    output.push('\n');
                }
                DiffLine::Removed(line) => {
                    output.push('-');
                    output.push_str(line);
                    output.push('\n');
                }
            }
        }

        i = hunk_end;
    }

    output
}

#[async_trait]
impl Builtin for Diff {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: diff [OPTION]... FILE1 FILE2\nCompare files line by line.\n\n  -u\t\toutput in unified format\n  -q, --brief\treport only when files differ\n  --help\t\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("diff (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, files) = parse_diff_args(ctx.args);

        if files.len() < 2 {
            return Ok(ExecResult::err("diff: missing operand\n".to_string(), 1));
        }

        // Read file1
        let lines1: Vec<String> = if files[0] == "-" {
            ctx.stdin
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        } else {
            let path = if files[0].starts_with('/') {
                std::path::PathBuf::from(&files[0])
            } else {
                ctx.cwd.join(&files[0])
            };
            match read_text_file(&*ctx.fs, &path, "diff").await {
                Ok(text) => text.lines().map(|l| l.to_string()).collect(),
                Err(e) => return Ok(e),
            }
        };

        // Read file2
        let lines2: Vec<String> = if files[1] == "-" {
            ctx.stdin
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        } else {
            let path = if files[1].starts_with('/') {
                std::path::PathBuf::from(&files[1])
            } else {
                ctx.cwd.join(&files[1])
            };
            match read_text_file(&*ctx.fs, &path, "diff").await {
                Ok(text) => text.lines().map(|l| l.to_string()).collect(),
                Err(e) => return Ok(e),
            }
        };

        if lines1 == lines2 {
            return Ok(ExecResult::ok(String::new()));
        }

        if opts.brief {
            return Ok(ExecResult::with_code(
                format!("Files {} and {} differ\n", files[0], files[1]),
                1,
            ));
        }

        let diff = compute_diff(&lines1, &lines2);

        let output = if opts.unified {
            format_unified(&files[0], &files[1], &diff)
        } else {
            format_normal(&diff)
        };

        // diff returns exit code 1 when files differ, output goes to stdout
        Ok(ExecResult::with_code(output, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_diff(args: &[&str], stdin: Option<&str>, files: &[(&str, &[u8])]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            fs.write_file(std::path::Path::new(path), content)
                .await
                .unwrap();
        }
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Diff.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_diff_identical() {
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"hello\nworld\n"), ("/b.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_diff_different() {
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"hello\nworld\n"), ("/b.txt", b"hello\nearth\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        // Default is normal format
        assert!(result.stdout.contains("2c2"));
        assert!(result.stdout.contains("< world"));
        assert!(result.stdout.contains("> earth"));
    }

    #[tokio::test]
    async fn test_diff_added_lines() {
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\n"), ("/b.txt", b"a\nb\nc\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        // Normal format: add command
        assert!(result.stdout.contains("2a3"));
        assert!(result.stdout.contains("> c"));
    }

    #[tokio::test]
    async fn test_diff_removed_lines() {
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"a\nb\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        // Normal format: delete command
        assert!(result.stdout.contains("3d2"));
        assert!(result.stdout.contains("< c"));
    }

    #[tokio::test]
    async fn test_diff_brief() {
        let result = run_diff(
            &["-q", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"hello\n"), ("/b.txt", b"world\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("Files /a.txt and /b.txt differ"));
    }

    #[tokio::test]
    async fn test_diff_brief_identical() {
        let result = run_diff(
            &["-q", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"hello\n"), ("/b.txt", b"hello\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_diff_empty_vs_content() {
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b""), ("/b.txt", b"hello\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("> hello"));
    }

    #[tokio::test]
    async fn test_diff_missing_operand() {
        let result = run_diff(&["/a.txt"], None, &[("/a.txt", b"hello\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_diff_file_not_found() {
        let result = run_diff(&["/a.txt", "/b.txt"], None, &[("/a.txt", b"hello\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("diff:"));
    }

    #[tokio::test]
    async fn test_diff_unified_header() {
        let result = run_diff(
            &["-u", "/old.txt", "/new.txt"],
            None,
            &[("/old.txt", b"a\nb\nc\n"), ("/new.txt", b"a\nB\nc\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("--- /old.txt"));
        assert!(result.stdout.contains("+++ /new.txt"));
        assert!(result.stdout.contains("@@"));
    }

    #[tokio::test]
    async fn test_diff_hunk_format() {
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[
                ("/a.txt", b"line1\nline2\nline3\n"),
                ("/b.txt", b"line1\nmodified\nline3\n"),
            ],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        // Normal format: change command
        assert!(result.stdout.contains("2c2"));
        assert!(result.stdout.contains("< line2"));
        assert!(result.stdout.contains("> modified"));
    }

    #[tokio::test]
    async fn test_diff_stdin() {
        let result = run_diff(&["-", "/b.txt"], Some("hello\n"), &[("/b.txt", b"world\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("< hello"));
        assert!(result.stdout.contains("> world"));
    }

    #[tokio::test]
    async fn test_diff_multiple_changes() {
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[
                ("/a.txt", b"a\nb\nc\nd\ne\n"),
                ("/b.txt", b"a\nB\nc\nD\ne\n"),
            ],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        // Normal format
        assert!(result.stdout.contains("< b"));
        assert!(result.stdout.contains("> B"));
        assert!(result.stdout.contains("< d"));
        assert!(result.stdout.contains("> D"));
    }

    #[tokio::test]
    async fn test_diff_normal_format_default() {
        // Default format should be normal (ed-style), not unified
        let result = run_diff(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\n"), ("/b.txt", b"b\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.starts_with("1c1\n"));
        assert!(result.stdout.contains("< a"));
        assert!(result.stdout.contains("---\n"));
        assert!(result.stdout.contains("> b"));
        // Should NOT contain unified format markers
        assert!(!result.stdout.contains("---  "));
        assert!(!result.stdout.contains("+++"));
        assert!(!result.stdout.contains("@@"));
    }

    #[tokio::test]
    async fn test_diff_unified_with_flag() {
        // -u flag should produce unified format
        let result = run_diff(
            &["-u", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\n"), ("/b.txt", b"b\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("--- /a.txt"));
        assert!(result.stdout.contains("+++ /b.txt"));
        assert!(result.stdout.contains("@@"));
    }
}
