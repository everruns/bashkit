//! patch - Apply unified diff patches to files
//!
//! Reads a unified diff from stdin and applies it to files in the VFS.
//!
//! Usage:
//!   patch [OPTIONS] [FILE]
//!   patch -p1 < diff.patch
//!   patch --dry-run < diff.patch
//!   patch -R < diff.patch         # reverse patch

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// patch command - apply unified diffs
pub struct Patch;

struct PatchOptions {
    strip: usize,
    dry_run: bool,
    reverse: bool,
    target_file: Option<String>,
}

fn parse_patch_args(args: &[String]) -> PatchOptions {
    let mut opts = PatchOptions {
        strip: 0,
        dry_run: false,
        reverse: false,
        target_file: None,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(rest) = arg.strip_prefix("-p") {
            if let Ok(n) = rest.parse::<usize>() {
                opts.strip = n;
            } else {
                // -p N as two args
                i += 1;
                if i < args.len() {
                    opts.strip = args[i].parse().unwrap_or(0);
                }
            }
        } else if arg == "--dry-run" {
            opts.dry_run = true;
        } else if arg == "-R" || arg == "--reverse" {
            opts.reverse = true;
        } else if !arg.starts_with('-') {
            opts.target_file = Some(arg.clone());
        }
        i += 1;
    }

    opts
}

/// A single hunk from a unified diff
#[derive(Debug)]
struct Hunk {
    old_start: usize,
    #[allow(dead_code)]
    old_count: usize,
    new_start: usize,
    #[allow(dead_code)]
    new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
}

/// A parsed file diff containing multiple hunks
#[derive(Debug)]
struct FileDiff {
    old_path: String,
    new_path: String,
    hunks: Vec<Hunk>,
}

/// Strip path components from a file path
fn strip_path(path: &str, strip: usize) -> String {
    if strip == 0 {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').collect();
    if strip >= parts.len() {
        parts.last().unwrap_or(&"").to_string()
    } else {
        parts[strip..].join("/")
    }
}

/// Parse unified diff text into file diffs
fn parse_unified_diff(input: &str) -> Vec<FileDiff> {
    let mut diffs = Vec::new();
    let lines: Vec<&str> = input.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Look for --- a/file header
        if lines[i].starts_with("--- ") && i + 1 < lines.len() && lines[i + 1].starts_with("+++ ") {
            let old_path = lines[i]
                .strip_prefix("--- ")
                .unwrap_or("")
                .split('\t')
                .next()
                .unwrap_or("")
                .to_string();
            let new_path = lines[i + 1]
                .strip_prefix("+++ ")
                .unwrap_or("")
                .split('\t')
                .next()
                .unwrap_or("")
                .to_string();
            i += 2;

            let mut hunks = Vec::new();

            // Parse hunks
            while i < lines.len() && lines[i].starts_with("@@ ") {
                if let Some(hunk) = parse_hunk_header(lines[i]) {
                    let mut hunk = hunk;
                    i += 1;

                    // Read hunk lines
                    while i < lines.len() {
                        let line = lines[i];
                        if line.starts_with("@@ ") || line.starts_with("--- ") {
                            break;
                        }
                        if let Some(rest) = line.strip_prefix('+') {
                            hunk.lines.push(HunkLine::Add(rest.to_string()));
                        } else if let Some(rest) = line.strip_prefix('-') {
                            hunk.lines.push(HunkLine::Remove(rest.to_string()));
                        } else if let Some(rest) = line.strip_prefix(' ') {
                            hunk.lines.push(HunkLine::Context(rest.to_string()));
                        } else if line == "\\ No newline at end of file" {
                            // skip
                        } else {
                            // Treat as context (some diffs omit the space prefix)
                            hunk.lines.push(HunkLine::Context(line.to_string()));
                        }
                        i += 1;
                    }
                    hunks.push(hunk);
                } else {
                    i += 1;
                }
            }

            diffs.push(FileDiff {
                old_path,
                new_path,
                hunks,
            });
        } else {
            i += 1;
        }
    }

    diffs
}

/// Parse a hunk header like @@ -1,3 +1,4 @@
fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let line = line.strip_prefix("@@ ")?;
    let line = line.split(" @@").next()?;
    let parts: Vec<&str> = line.split(' ').collect();
    if parts.len() < 2 {
        return None;
    }

    let old_part = parts[0].strip_prefix('-')?;
    let new_part = parts[1].strip_prefix('+')?;

    let (old_start, old_count) = parse_range(old_part);
    let (new_start, new_count) = parse_range(new_part);

    Some(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_range(s: &str) -> (usize, usize) {
    if let Some((start, count)) = s.split_once(',') {
        (start.parse().unwrap_or(1), count.parse().unwrap_or(1))
    } else {
        (s.parse().unwrap_or(1), 1)
    }
}

/// Apply hunks to file content, returning the patched content.
/// If reverse is true, swaps add/remove operations.
fn apply_hunks(
    content: &str,
    hunks: &[Hunk],
    reverse: bool,
) -> std::result::Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    // Track if original ended with newline
    let had_trailing_newline = content.ends_with('\n') || content.is_empty();

    // Apply hunks in reverse order to preserve line numbers
    for hunk in hunks.iter().rev() {
        let start = if reverse {
            hunk.new_start
        } else {
            hunk.old_start
        };
        // Convert 1-based to 0-based
        let start_idx = if start > 0 { start - 1 } else { 0 };

        // Build expected old lines and new lines based on direction
        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();

        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(l) => {
                    old_lines.push(l.clone());
                    new_lines.push(l.clone());
                }
                HunkLine::Add(l) => {
                    if reverse {
                        old_lines.push(l.clone());
                    } else {
                        new_lines.push(l.clone());
                    }
                }
                HunkLine::Remove(l) => {
                    if reverse {
                        new_lines.push(l.clone());
                    } else {
                        old_lines.push(l.clone());
                    }
                }
            }
        }

        // Verify context/old lines match (with fuzz tolerance)
        let end_idx = start_idx + old_lines.len();
        if end_idx > lines.len() {
            return Err(format!(
                "hunk at line {} does not match (file too short)",
                start
            ));
        }

        for (j, expected) in old_lines.iter().enumerate() {
            let actual_idx = start_idx + j;
            if actual_idx < lines.len() && lines[actual_idx] != *expected {
                return Err(format!(
                    "hunk at line {} does not match: expected '{}', got '{}'",
                    start, expected, lines[actual_idx]
                ));
            }
        }

        // Replace old lines with new lines
        lines.splice(start_idx..end_idx, new_lines);
    }

    let mut result = lines.join("\n");
    if had_trailing_newline && !result.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

#[async_trait]
impl Builtin for Patch {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let opts = parse_patch_args(ctx.args);

        let input = match ctx.stdin {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(ExecResult::err(
                    "patch: no input (expected unified diff on stdin)\n".to_string(),
                    1,
                ));
            }
        };

        let file_diffs = parse_unified_diff(&input);
        if file_diffs.is_empty() {
            return Ok(ExecResult::err(
                "patch: no valid diff found in input\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        let mut had_error = false;

        for diff in &file_diffs {
            // Determine target file path
            let target = if let Some(ref t) = opts.target_file {
                t.clone()
            } else {
                // Use new_path for forward patches, old_path for reverse
                let raw_path = if opts.reverse {
                    &diff.new_path
                } else {
                    // Prefer new_path unless it's /dev/null (file deletion)
                    if diff.new_path == "/dev/null" {
                        &diff.old_path
                    } else {
                        &diff.new_path
                    }
                };
                strip_path(raw_path, opts.strip)
            };

            let path = resolve_path(ctx.cwd, &target);

            // Read existing file (may not exist for new files)
            let content = if diff.old_path == "/dev/null" && !opts.reverse {
                // New file creation
                String::new()
            } else {
                match ctx.fs.read_file(&path).await {
                    Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                    Err(_) => {
                        // File doesn't exist - might be a new file
                        String::new()
                    }
                }
            };

            match apply_hunks(&content, &diff.hunks, opts.reverse) {
                Ok(patched) => {
                    if opts.dry_run {
                        output.push_str(&format!("checking file {}\n", target));
                    } else {
                        // Handle file deletion
                        if diff.new_path == "/dev/null" && !opts.reverse {
                            output.push_str(&format!("patching file {} (removed)\n", target));
                            ctx.fs.remove(&path, false).await?;
                        } else {
                            ctx.fs.write_file(&path, patched.as_bytes()).await?;
                            output.push_str(&format!("patching file {}\n", target));
                        }
                    }
                }
                Err(e) => {
                    output.push_str(&format!("patch: {}: {}\n", target, e));
                    had_error = true;
                }
            }
        }

        if had_error {
            Ok(ExecResult::err(output, 1))
        } else {
            Ok(ExecResult::ok(output))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_patch(
        args: &[&str],
        stdin: &str,
        files: &[(&str, &[u8])],
    ) -> (ExecResult, Arc<InMemoryFs>) {
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            let fs_trait = fs.clone() as Arc<dyn FileSystem>;
            fs_trait.write_file(Path::new(path), content).await.unwrap();
        }

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs_dyn = fs.clone() as Arc<dyn FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_dyn,
            stdin: Some(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Patch.execute(ctx).await.unwrap();
        (result, fs)
    }

    #[tokio::test]
    async fn test_patch_simple_change() {
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,3 @@
 line1
-line2
+modified
 line3
";
        let (result, fs) =
            run_patch(&["-p1"], diff, &[("/test.txt", b"line1\nline2\nline3\n")]).await;
        assert_eq!(result.exit_code, 0);
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait.read_file(Path::new("/test.txt")).await.unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("modified"));
        assert!(!text.contains("line2"));
    }

    #[tokio::test]
    async fn test_patch_add_lines() {
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,4 @@
 line1
+added1
+added2
 line2
";
        let (result, fs) = run_patch(&["-p1"], diff, &[("/test.txt", b"line1\nline2\n")]).await;
        assert_eq!(result.exit_code, 0);
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait.read_file(Path::new("/test.txt")).await.unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("added1"));
        assert!(text.contains("added2"));
    }

    #[tokio::test]
    async fn test_patch_remove_lines() {
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,1 @@
 line1
-line2
-line3
";
        let (result, fs) =
            run_patch(&["-p1"], diff, &[("/test.txt", b"line1\nline2\nline3\n")]).await;
        assert_eq!(result.exit_code, 0);
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait.read_file(Path::new("/test.txt")).await.unwrap();
        let text = String::from_utf8_lossy(&content);
        assert_eq!(text.trim(), "line1");
    }

    #[tokio::test]
    async fn test_patch_dry_run() {
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,2 @@
 line1
-line2
+changed
";
        let (result, fs) = run_patch(
            &["--dry-run", "-p1"],
            diff,
            &[("/test.txt", b"line1\nline2\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("checking file"));
        // File should NOT be modified
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait.read_file(Path::new("/test.txt")).await.unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("line2"));
    }

    #[tokio::test]
    async fn test_patch_reverse() {
        // First apply forward
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,2 @@
 line1
-original
+changed
";
        // File has "changed", we reverse-apply to get back "original"
        let (result, fs) =
            run_patch(&["-R", "-p1"], diff, &[("/test.txt", b"line1\nchanged\n")]).await;
        assert_eq!(result.exit_code, 0);
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait.read_file(Path::new("/test.txt")).await.unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("original"));
    }

    #[tokio::test]
    async fn test_patch_strip_path() {
        assert_eq!(strip_path("a/b/c.txt", 0), "a/b/c.txt");
        assert_eq!(strip_path("a/b/c.txt", 1), "b/c.txt");
        assert_eq!(strip_path("a/b/c.txt", 2), "c.txt");
        assert_eq!(strip_path("a/b/c.txt", 5), "c.txt");
    }

    #[tokio::test]
    async fn test_patch_no_input() {
        let (result, _fs) = run_patch(&[], "", &[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no input"));
    }

    #[tokio::test]
    async fn test_patch_invalid_diff() {
        let (result, _fs) = run_patch(&[], "this is not a diff\n", &[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no valid diff"));
    }

    #[tokio::test]
    async fn test_patch_hunk_mismatch() {
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,2 @@
 line1
-wrong_content
+changed
";
        let (result, _fs) =
            run_patch(&["-p1"], diff, &[("/test.txt", b"line1\nactual_content\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("does not match"));
    }

    #[tokio::test]
    async fn test_patch_new_file() {
        let diff = "\
--- /dev/null
+++ b/newfile.txt
@@ -0,0 +1,2 @@
+hello
+world
";
        let (result, fs) = run_patch(&["-p1"], diff, &[]).await;
        assert_eq!(result.exit_code, 0);
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait.read_file(Path::new("/newfile.txt")).await.unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
    }

    #[tokio::test]
    async fn test_patch_target_file_override() {
        let diff = "\
--- a/original.txt
+++ b/original.txt
@@ -1,2 +1,2 @@
 line1
-old
+new
";
        let (result, fs) = run_patch(
            &["-p1", "target.txt"],
            diff,
            &[("/target.txt", b"line1\nold\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait.read_file(Path::new("/target.txt")).await.unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("new"));
    }

    #[tokio::test]
    async fn test_parse_hunk_header() {
        let hunk = parse_hunk_header("@@ -1,3 +1,4 @@").unwrap();
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.old_count, 3);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.new_count, 4);
    }

    #[tokio::test]
    async fn test_parse_hunk_header_single_line() {
        let hunk = parse_hunk_header("@@ -5 +5,2 @@").unwrap();
        assert_eq!(hunk.old_start, 5);
        assert_eq!(hunk.old_count, 1);
        assert_eq!(hunk.new_start, 5);
        assert_eq!(hunk.new_count, 2);
    }

    #[tokio::test]
    async fn test_patch_delete_file_removes_from_vfs() {
        // Create a file, then apply a delete patch (new_path = /dev/null)
        let diff = "--- a/to_delete.txt\n\
                     +++ /dev/null\n\
                     @@ -1,2 +0,0 @@\n\
                     -hello\n\
                     -world\n";
        let (result, fs) =
            run_patch(&["-p1"], diff, &[("/to_delete.txt", b"hello\nworld\n")]).await;
        assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
        // File should be actually removed, not just emptied
        let fs_dyn = fs as Arc<dyn FileSystem>;
        assert!(
            !fs_dyn.exists(Path::new("/to_delete.txt")).await.unwrap(),
            "deleted file should not exist in VFS"
        );
    }
}
