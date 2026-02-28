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
    s.trim().parse().unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn setup() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();
        fs.mkdir(&cwd, true).await.unwrap();
        (fs, cwd, variables)
    }

    // ==================== test builtin ====================

    #[tokio::test]
    async fn test_empty_args_returns_false() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_nonempty_string_is_true() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["hello".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_z_empty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_z_nonempty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_n_nonempty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_n_empty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_string_equality() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "=".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_inequality() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "!=".to_string(), "def".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_equality_fails() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "=".to_string(), "xyz".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_eq_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["42".to_string(), "-eq".to_string(), "42".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_eq_numeric_fails() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["42".to_string(), "-eq".to_string(), "99".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_lt_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-lt".to_string(), "10".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_lt_numeric_fails() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["10".to_string(), "-lt".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_gt_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["10".to_string(), "-gt".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_e_file_exists() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/file.txt"), b"hello")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-e".to_string(), "file.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_e_file_not_exists() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-e".to_string(), "nope.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_f_regular_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/file.txt"), b"data")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-f".to_string(), "file.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_f_directory_is_not_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.mkdir(Path::new("/home/user/subdir"), true)
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-f".to_string(), "subdir".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_d_directory() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.mkdir(Path::new("/home/user/subdir"), true)
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-d".to_string(), "subdir".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_d_file_is_not_dir() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/file.txt"), b"data")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-d".to_string(), "file.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_negation() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["!".to_string(), "-z".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0); // ! -z "abc" => ! false => true
    }

    #[tokio::test]
    async fn test_negation_true_becomes_false() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["!".to_string(), "-n".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1); // ! -n "abc" => ! true => false
    }

    // ==================== bracket builtin ====================

    #[tokio::test]
    async fn bracket_missing_closing() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("missing ]"));
    }

    #[tokio::test]
    async fn bracket_with_closing() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "".to_string(), "]".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn bracket_empty_expression() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["]".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1); // empty expression => false
    }

    #[tokio::test]
    async fn bracket_empty_args() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
    }

    // ==================== additional numeric operators ====================

    #[tokio::test]
    async fn test_ne_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["1".to_string(), "-ne".to_string(), "2".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_ne_numeric_equal() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-ne".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_le_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-le".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_le_numeric_greater() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["10".to_string(), "-le".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_ge_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-ge".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_ge_numeric_less() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["3".to_string(), "-ge".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    // ==================== string comparison operators ====================

    #[tokio::test]
    async fn test_double_eq_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["foo".to_string(), "==".to_string(), "foo".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_less_than() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "<".to_string(), "def".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_greater_than() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["xyz".to_string(), ">".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== file tests: -s, -r, -w, -x ====================

    #[tokio::test]
    async fn test_s_file_has_size() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/nonempty.txt"), b"data")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-s".to_string(), "nonempty.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_s_empty_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/empty.txt"), b"")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-s".to_string(), "empty.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_r_readable_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/readable.txt"), b"x")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "readable.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_w_writable_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/writable.txt"), b"x")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-w".to_string(), "writable.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_r_nonexistent() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "nope".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    // ==================== absolute path handling ====================

    #[tokio::test]
    async fn test_e_absolute_path() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/tmp/abs.txt"), b"hi")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-e".to_string(), "/tmp/abs.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== numeric parse edge cases ====================

    #[tokio::test]
    async fn test_eq_non_numeric_treated_as_zero() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        // Non-numeric values parse as 0
        let args = vec!["abc".to_string(), "-eq".to_string(), "0".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_negative_numbers() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-5".to_string(), "-lt".to_string(), "0".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== bracket with binary ops ====================

    #[tokio::test]
    async fn bracket_numeric_eq() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec![
            "3".to_string(),
            "-eq".to_string(),
            "3".to_string(),
            "]".to_string(),
        ];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn bracket_string_neq() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec![
            "a".to_string(),
            "!=".to_string(),
            "b".to_string(),
            "]".to_string(),
        ];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }
}
