//! Path manipulation builtins - basename, dirname

// Uses unwrap() after is_empty() check (e.g., args.next() after !args.is_empty())
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::path::Path;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The basename builtin - strip directory and suffix from filenames.
///
/// Usage: basename NAME [SUFFIX]
///        basename OPTION... NAME...
///
/// Print NAME with any leading directory components removed.
/// If SUFFIX is specified, also remove a trailing SUFFIX.
pub struct Basename;

#[async_trait]
impl Builtin for Basename {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "basename: missing operand\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        let mut args_iter = ctx.args.iter();

        // Get the path argument
        let path_arg = args_iter.next().unwrap();
        let path = Path::new(path_arg);

        // Get the basename
        let basename = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                // Handle special cases like "/" or empty
                if path_arg == "/" {
                    "/".to_string()
                } else if path_arg.is_empty() {
                    String::new()
                } else {
                    path_arg.clone()
                }
            });

        // Check for suffix argument
        let result = if let Some(suffix) = args_iter.next() {
            if let Some(stripped) = basename.strip_suffix(suffix.as_str()) {
                stripped.to_string()
            } else {
                basename
            }
        } else {
            basename
        };

        output.push_str(&result);
        output.push('\n');

        Ok(ExecResult::ok(output))
    }
}

/// The dirname builtin - strip last component from file name.
///
/// Usage: dirname NAME...
///
/// Output each NAME with its last non-slash component and trailing slashes removed.
/// If NAME contains no slashes, output "." (current directory).
pub struct Dirname;

#[async_trait]
impl Builtin for Dirname {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("dirname: missing operand\n".to_string(), 1));
        }

        let mut output = String::new();

        for (i, arg) in ctx.args.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }

            let path = Path::new(arg);
            let dirname = path
                .parent()
                .map(|p| {
                    let s = p.to_string_lossy();
                    if s.is_empty() {
                        ".".to_string()
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_else(|| {
                    // Handle special cases
                    if arg == "/" {
                        "/".to_string()
                    } else {
                        ".".to_string()
                    }
                });

            output.push_str(&dirname);
        }

        output.push('\n');
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

    async fn run_basename(args: &[&str]) -> ExecResult {
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
            stdin: None,
        };

        Basename.execute(ctx).await.unwrap()
    }

    async fn run_dirname(args: &[&str]) -> ExecResult {
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
            stdin: None,
        };

        Dirname.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_basename_simple() {
        let result = run_basename(&["/usr/bin/sort"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "sort\n");
    }

    #[tokio::test]
    async fn test_basename_with_suffix() {
        let result = run_basename(&["file.txt", ".txt"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "file\n");
    }

    #[tokio::test]
    async fn test_basename_no_suffix_match() {
        let result = run_basename(&["file.txt", ".doc"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "file.txt\n");
    }

    #[tokio::test]
    async fn test_basename_no_dir() {
        let result = run_basename(&["filename"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "filename\n");
    }

    #[tokio::test]
    async fn test_basename_trailing_slash() {
        let result = run_basename(&["/usr/bin/"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "bin\n");
    }

    #[tokio::test]
    async fn test_basename_missing_operand() {
        let result = run_basename(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_dirname_simple() {
        let result = run_dirname(&["/usr/bin/sort"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/usr/bin\n");
    }

    #[tokio::test]
    async fn test_dirname_no_dir() {
        let result = run_dirname(&["filename"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, ".\n");
    }

    #[tokio::test]
    async fn test_dirname_root() {
        let result = run_dirname(&["/"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/\n");
    }

    #[tokio::test]
    async fn test_dirname_trailing_slash() {
        let result = run_dirname(&["/usr/bin/"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/usr\n");
    }

    #[tokio::test]
    async fn test_dirname_missing_operand() {
        let result = run_dirname(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }
}
