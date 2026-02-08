//! comm builtin command - compare two sorted files line by line

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The comm builtin - compare two sorted files line by line.
///
/// Usage: comm [-123] FILE1 FILE2
///
/// Options:
///   -1   Suppress lines unique to FILE1
///   -2   Suppress lines unique to FILE2
///   -3   Suppress lines that appear in both files
pub struct Comm;

struct CommOptions {
    suppress_1: bool,
    suppress_2: bool,
    suppress_3: bool,
}

fn parse_comm_args(args: &[String]) -> (CommOptions, Vec<String>) {
    let mut opts = CommOptions {
        suppress_1: false,
        suppress_2: false,
        suppress_3: false,
    };
    let mut files = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 && arg[1..].chars().all(|c| "123".contains(c)) {
            for c in arg[1..].chars() {
                match c {
                    '1' => opts.suppress_1 = true,
                    '2' => opts.suppress_2 = true,
                    '3' => opts.suppress_3 = true,
                    _ => {}
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    (opts, files)
}

#[async_trait]
impl Builtin for Comm {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = parse_comm_args(ctx.args);

        if files.len() < 2 {
            return Ok(ExecResult::err("comm: missing operand\n".to_string(), 1));
        }

        // Read both files
        let lines1 = if files[0] == "-" {
            ctx.stdin
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        } else {
            let path = if files[0].starts_with('/') {
                std::path::PathBuf::from(&files[0])
            } else {
                ctx.cwd.join(&files[0])
            };
            match ctx.fs.read_file(&path).await {
                Ok(content) => {
                    let text = String::from_utf8_lossy(&content);
                    text.lines().map(|l| l.to_string()).collect()
                }
                Err(e) => {
                    return Ok(ExecResult::err(format!("comm: {}: {}\n", files[0], e), 1));
                }
            }
        };

        let lines2 = if files[1] == "-" {
            ctx.stdin
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        } else {
            let path = if files[1].starts_with('/') {
                std::path::PathBuf::from(&files[1])
            } else {
                ctx.cwd.join(&files[1])
            };
            match ctx.fs.read_file(&path).await {
                Ok(content) => {
                    let text = String::from_utf8_lossy(&content);
                    text.lines().map(|l| l.to_string()).collect()
                }
                Err(e) => {
                    return Ok(ExecResult::err(format!("comm: {}: {}\n", files[1], e), 1));
                }
            }
        };

        let lines1: Vec<String> = lines1;
        let lines2: Vec<String> = lines2;

        let mut output = String::new();
        let mut i = 0;
        let mut j = 0;

        // Determine column prefixes based on suppressed columns
        let col1_prefix = "";
        let col2_prefix = if opts.suppress_1 { "" } else { "\t" };
        let col3_prefix = match (opts.suppress_1, opts.suppress_2) {
            (false, false) => "\t\t",
            (true, false) | (false, true) => "\t",
            (true, true) => "",
        };

        while i < lines1.len() && j < lines2.len() {
            match lines1[i].cmp(&lines2[j]) {
                std::cmp::Ordering::Less => {
                    // Only in file1
                    if !opts.suppress_1 {
                        output.push_str(col1_prefix);
                        output.push_str(&lines1[i]);
                        output.push('\n');
                    }
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    // Only in file2
                    if !opts.suppress_2 {
                        output.push_str(col2_prefix);
                        output.push_str(&lines2[j]);
                        output.push('\n');
                    }
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    // In both
                    if !opts.suppress_3 {
                        output.push_str(col3_prefix);
                        output.push_str(&lines1[i]);
                        output.push('\n');
                    }
                    i += 1;
                    j += 1;
                }
            }
        }

        // Remaining lines from file1
        while i < lines1.len() {
            if !opts.suppress_1 {
                output.push_str(col1_prefix);
                output.push_str(&lines1[i]);
                output.push('\n');
            }
            i += 1;
        }

        // Remaining lines from file2
        while j < lines2.len() {
            if !opts.suppress_2 {
                output.push_str(col2_prefix);
                output.push_str(&lines2[j]);
                output.push('\n');
            }
            j += 1;
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

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_comm(args: &[&str], stdin: Option<&str>, files: &[(&str, &[u8])]) -> ExecResult {
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
        };

        Comm.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_comm_basic() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\t\tb\n\t\tc\n\td\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_1() {
        let result = run_comm(
            &["-1", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "\tb\n\tc\nd\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_2() {
        let result = run_comm(
            &["-2", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\tb\n\tc\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_3() {
        let result = run_comm(
            &["-3", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\td\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_12() {
        // Show only common lines
        let result = run_comm(
            &["-12", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "b\nc\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_13() {
        // Show only lines unique to file2
        let result = run_comm(
            &["-13", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "d\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_23() {
        // Show only lines unique to file1
        let result = run_comm(
            &["-23", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n");
    }

    #[tokio::test]
    async fn test_comm_identical_files() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"a\nb\nc\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "\t\ta\n\t\tb\n\t\tc\n");
    }

    #[tokio::test]
    async fn test_comm_no_common() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nc\n"), ("/b.txt", b"b\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\tb\nc\n\td\n");
    }

    #[tokio::test]
    async fn test_comm_empty_file() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\n"), ("/b.txt", b"")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_comm_missing_operand() {
        let result = run_comm(&["/a.txt"], None, &[("/a.txt", b"a\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_comm_file_not_found() {
        let result = run_comm(&["/a.txt", "/b.txt"], None, &[("/a.txt", b"a\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("comm:"));
    }
}
