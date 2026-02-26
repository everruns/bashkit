//! test builtin command ([ and test)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

/// The test builtin command.
pub struct Test;

#[async_trait]
impl Builtin for Test {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Handle empty args - returns false
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(String::new(), 1));
        }

        let cwd = ctx.cwd.clone();
        // Parse and evaluate the expression
        let result = evaluate_expression(ctx.args, &ctx.fs, &cwd).await;

        if result {
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::err(String::new(), 1))
        }
    }
}

/// The [ builtin (alias for test, but expects ] as last arg)
pub struct Bracket;

#[async_trait]
impl Builtin for Bracket {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Check for closing ]
        if ctx.args.is_empty() || ctx.args.last() != Some(&"]".to_string()) {
            return Ok(ExecResult::err("missing ]\n".to_string(), 2));
        }

        // Remove the trailing ]
        let args: Vec<String> = ctx.args[..ctx.args.len() - 1].to_vec();

        // Handle empty args - returns false
        if args.is_empty() {
            return Ok(ExecResult::err(String::new(), 1));
        }

        let cwd = ctx.cwd.clone();
        // Parse and evaluate the expression
        let result = evaluate_expression(&args, &ctx.fs, &cwd).await;

        if result {
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::err(String::new(), 1))
        }
    }
}

/// Resolve a file path against cwd (relative paths become absolute)
fn resolve_file_path(cwd: &Path, arg: &str) -> PathBuf {
    let p = Path::new(arg);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

/// Evaluate a test expression
fn evaluate_expression<'a>(
    args: &'a [String],
    fs: &'a Arc<dyn FileSystem>,
    cwd: &'a Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
    Box::pin(async move {
        if args.is_empty() {
            return false;
        }

        // Handle negation
        if args[0] == "!" {
            return !evaluate_expression(&args[1..], fs, cwd).await;
        }

        // Handle parentheses (basic support)
        if args[0] == "(" && args.last().map(|s| s.as_str()) == Some(")") {
            return evaluate_expression(&args[1..args.len() - 1], fs, cwd).await;
        }

        // Look for binary operators
        for (i, arg) in args.iter().enumerate() {
            match arg.as_str() {
                // Logical operators (lowest precedence)
                "-a" if i > 0 => {
                    return evaluate_expression(&args[..i], fs, cwd).await
                        && evaluate_expression(&args[i + 1..], fs, cwd).await;
                }
                "-o" if i > 0 => {
                    return evaluate_expression(&args[..i], fs, cwd).await
                        || evaluate_expression(&args[i + 1..], fs, cwd).await;
                }
                _ => {}
            }
        }

        // Now handle binary comparisons and unary tests
        match args.len() {
            1 => {
                // Single arg: true if non-empty string
                !args[0].is_empty()
            }
            2 => {
                // Unary operators
                evaluate_unary(&args[0], &args[1], fs, cwd).await
            }
            3 => {
                // Binary operators
                evaluate_binary(&args[0], &args[1], &args[2], fs, cwd).await
            }
            _ => false,
        }
    })
}

/// Evaluate a unary test expression
async fn evaluate_unary(op: &str, arg: &str, fs: &Arc<dyn FileSystem>, cwd: &Path) -> bool {
    match op {
        // String tests
        "-z" => arg.is_empty(),
        "-n" => !arg.is_empty(),

        // File tests using the virtual filesystem
        "-e" | "-a" => {
            // file exists
            let path = resolve_file_path(cwd, arg);
            fs.exists(&path).await.unwrap_or(false)
        }
        "-f" => {
            // regular file
            let path = resolve_file_path(cwd, arg);
            if let Ok(meta) = fs.stat(&path).await {
                meta.file_type.is_file()
            } else {
                false
            }
        }
        "-d" => {
            // directory
            let path = resolve_file_path(cwd, arg);
            if let Ok(meta) = fs.stat(&path).await {
                meta.file_type.is_dir()
            } else {
                false
            }
        }
        "-r" => {
            // readable - in virtual fs, check if file exists
            // (permissions are stored but not enforced)
            let path = resolve_file_path(cwd, arg);
            fs.exists(&path).await.unwrap_or(false)
        }
        "-w" => {
            // writable - in virtual fs, check if file exists
            let path = resolve_file_path(cwd, arg);
            fs.exists(&path).await.unwrap_or(false)
        }
        "-x" => {
            // executable - in virtual fs, check if file exists and has executable permission
            let path = resolve_file_path(cwd, arg);
            if let Ok(meta) = fs.stat(&path).await {
                // Check if any execute bit is set (u+x, g+x, o+x)
                (meta.mode & 0o111) != 0
            } else {
                false
            }
        }
        "-s" => {
            // file exists and has size > 0
            let path = resolve_file_path(cwd, arg);
            if let Ok(meta) = fs.stat(&path).await {
                meta.size > 0
            } else {
                false
            }
        }
        "-L" | "-h" => {
            // symbolic link
            let path = resolve_file_path(cwd, arg);
            if let Ok(meta) = fs.stat(&path).await {
                meta.file_type.is_symlink()
            } else {
                false
            }
        }
        "-p" => false, // named pipe (not supported)
        "-S" => false, // socket (not supported)
        "-b" => false, // block device (not supported)
        "-c" => false, // character device (not supported)
        "-t" => false, // file descriptor is open and refers to a terminal (not supported)

        _ => false,
    }
}

/// Evaluate a binary test expression
async fn evaluate_binary(
    left: &str,
    op: &str,
    right: &str,
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
) -> bool {
    match op {
        // String comparisons
        "=" | "==" => left == right,
        "!=" => left != right,
        "<" => left < right,
        ">" => left > right,

        // Numeric comparisons
        "-eq" => parse_int(left) == parse_int(right),
        "-ne" => parse_int(left) != parse_int(right),
        "-lt" => parse_int(left) < parse_int(right),
        "-le" => parse_int(left) <= parse_int(right),
        "-gt" => parse_int(left) > parse_int(right),
        "-ge" => parse_int(left) >= parse_int(right),

        // File comparisons
        "-nt" => {
            // file1 is newer than file2
            let left_meta = fs.stat(&resolve_file_path(cwd, left)).await;
            let right_meta = fs.stat(&resolve_file_path(cwd, right)).await;
            match (left_meta, right_meta) {
                (Ok(lm), Ok(rm)) => lm.modified > rm.modified,
                (Ok(_), Err(_)) => true, // left exists, right doesn't → left is newer
                _ => false,
            }
        }
        "-ot" => {
            // file1 is older than file2
            let left_meta = fs.stat(&resolve_file_path(cwd, left)).await;
            let right_meta = fs.stat(&resolve_file_path(cwd, right)).await;
            match (left_meta, right_meta) {
                (Ok(lm), Ok(rm)) => lm.modified < rm.modified,
                (Err(_), Ok(_)) => true, // left doesn't exist, right does → left is older
                _ => false,
            }
        }
        "-ef" => {
            // file1 and file2 refer to the same file (same path after resolution)
            // In VFS without inodes, compare canonical paths
            let left_path = super::resolve_path(cwd, left);
            let right_path = super::resolve_path(cwd, right);
            left_path == right_path
        }

        _ => false,
    }
}

fn parse_int(s: &str) -> i64 {
    s.parse().unwrap_or(0)
}
