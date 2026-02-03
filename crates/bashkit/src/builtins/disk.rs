//! Disk usage builtins - du and df

use async_trait::async_trait;
use std::path::Path;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The du builtin - estimate file space usage.
///
/// Usage: du [-s] [-h] [FILE...]
///
/// Options:
///   -s   Display only a total for each argument
///   -h   Print sizes in human readable format
///
/// If no FILE is specified, shows usage for current directory.
pub struct Du;

#[async_trait]
impl Builtin for Du {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut summary_only = false;
        let mut human_readable = false;
        let mut paths: Vec<String> = Vec::new();

        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        's' => summary_only = true,
                        'h' => human_readable = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("du: invalid option -- '{}'\n", c),
                                1,
                            ));
                        }
                    }
                }
            } else {
                paths.push(arg.clone());
            }
        }

        // Default to current directory
        if paths.is_empty() {
            paths.push(".".to_string());
        }

        let mut output = String::new();

        for path_str in &paths {
            let path = if path_str.starts_with('/') {
                std::path::PathBuf::from(path_str)
            } else if path_str == "." {
                ctx.cwd.clone()
            } else {
                ctx.cwd.join(path_str)
            };

            match calculate_size(&ctx, &path, summary_only, human_readable).await {
                Ok(result) => output.push_str(&result),
                Err(e) => {
                    return Ok(ExecResult::err(format!("du: {}: {}\n", path_str, e), 1));
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// Calculate size of a path recursively.
async fn calculate_size(
    ctx: &Context<'_>,
    path: &Path,
    summary_only: bool,
    human_readable: bool,
) -> Result<String> {
    let mut output = String::new();
    let total =
        calculate_size_recursive(ctx, path, summary_only, human_readable, &mut output).await?;

    if summary_only {
        let size_str = format_size(total, human_readable);
        output.push_str(&format!("{}\t{}\n", size_str, path.display()));
    }

    Ok(output)
}

/// Recursively calculate size.
fn calculate_size_recursive<'a>(
    ctx: &'a Context<'_>,
    path: &'a Path,
    summary_only: bool,
    human_readable: bool,
    output: &'a mut String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + 'a>> {
    Box::pin(async move {
        let metadata = ctx.fs.stat(path).await?;

        if metadata.file_type.is_file() {
            if !summary_only {
                let size_str = format_size(metadata.size, human_readable);
                output.push_str(&format!("{}\t{}\n", size_str, path.display()));
            }
            return Ok(metadata.size);
        }

        if metadata.file_type.is_dir() {
            let mut total = 0u64;
            let entries = ctx.fs.read_dir(path).await?;

            for entry in entries {
                let child_path = path.join(&entry.name);
                total += calculate_size_recursive(
                    ctx,
                    &child_path,
                    summary_only,
                    human_readable,
                    output,
                )
                .await?;
            }

            if !summary_only {
                let size_str = format_size(total, human_readable);
                output.push_str(&format!("{}\t{}\n", size_str, path.display()));
            }

            return Ok(total);
        }

        Ok(0)
    })
}

/// Format size in bytes or human-readable format.
fn format_size(bytes: u64, human_readable: bool) -> String {
    if !human_readable {
        // Return size in 1K blocks (like real du)
        return format!("{}", bytes.div_ceil(1024));
    }

    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// The df builtin - report file system disk space usage.
///
/// Usage: df [-h]
///
/// Options:
///   -h   Print sizes in human readable format
///
/// Shows total, used, and available space for the virtual filesystem.
pub struct Df;

#[async_trait]
impl Builtin for Df {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut human_readable = false;

        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'h' => human_readable = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("df: invalid option -- '{}'\n", c),
                                1,
                            ));
                        }
                    }
                }
            }
        }

        let usage = ctx.fs.usage();
        let limits = ctx.fs.limits();

        let total = limits.max_total_bytes;
        let used = usage.total_bytes;
        let available = total.saturating_sub(used);
        let use_percent = if total > 0 {
            ((used as f64 / total as f64) * 100.0) as u64
        } else {
            0
        };

        let mut output = String::new();

        // Header
        if human_readable {
            output.push_str("Filesystem      Size  Used Avail Use% Mounted on\n");
        } else {
            output.push_str("Filesystem     1K-blocks      Used Available Use% Mounted on\n");
        }

        // Data row
        let (total_str, used_str, avail_str) = if human_readable {
            (
                format_size(total, true),
                format_size(used, true),
                format_size(available, true),
            )
        } else {
            (
                format!("{}", total / 1024),
                format!("{}", used / 1024),
                format!("{}", available / 1024),
            )
        };

        if human_readable {
            output.push_str(&format!(
                "{:<15} {:>5} {:>5} {:>5} {:>3}% {}\n",
                "bashkit-vfs", total_str, used_str, avail_str, use_percent, "/"
            ));
        } else {
            output.push_str(&format!(
                "{:<14} {:>10} {:>9} {:>9} {:>3}% {}\n",
                "bashkit-vfs", total_str, used_str, avail_str, use_percent, "/"
            ));
        }

        // Additional info about limits
        output.push_str(&format!(
            "# Files: {}/{}, Dirs: {}\n",
            usage.file_count, limits.max_file_count, usage.dir_count
        ));

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, FsLimits, InMemoryFs};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    // ==================== du tests ====================

    #[tokio::test]
    async fn test_du_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create test file with known size
        fs.write_file(&cwd.join("test.txt"), b"hello world")
            .await
            .unwrap();

        let args = vec!["test.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_du_summary() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("subdir"), false).await.unwrap();
        fs.write_file(&cwd.join("subdir/file1.txt"), b"content1")
            .await
            .unwrap();
        fs.write_file(&cwd.join("subdir/file2.txt"), b"content2")
            .await
            .unwrap();

        let args = vec!["-s".to_string(), "subdir".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Should only have one line for summary
        let lines: Vec<_> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[tokio::test]
    async fn test_du_human_readable() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a larger file
        let large_content = vec![b'x'; 2048];
        fs.write_file(&cwd.join("large.txt"), &large_content)
            .await
            .unwrap();

        let args = vec!["-h".to_string(), "large.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("K"));
    }

    #[tokio::test]
    async fn test_du_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["nonexistent".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    // ==================== df tests ====================

    #[tokio::test]
    async fn test_df_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Df.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("bashkit-vfs"));
        assert!(result.stdout.contains("Filesystem"));
    }

    #[tokio::test]
    async fn test_df_human_readable() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-h".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Df.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Human readable should have M or G for 100MB limit
        assert!(result.stdout.contains("M") || result.stdout.contains("G"));
    }

    #[tokio::test]
    async fn test_df_shows_usage() {
        let limits = FsLimits::new().max_total_bytes(1_000_000); // 1MB
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut cwd = PathBuf::from("/tmp");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // Write some data
        fs.write_file(&cwd.join("data.txt"), &vec![b'x'; 100_000])
            .await
            .unwrap();

        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Df.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Should show some usage percentage
        assert!(result.stdout.contains("%"));
    }

    // ==================== format_size tests ====================

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(500, true), "500B");
        assert_eq!(format_size(0, true), "0B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024, true), "1.0K");
        assert_eq!(format_size(2048, true), "2.0K");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1024 * 1024, true), "1.0M");
        assert_eq!(format_size(5 * 1024 * 1024, true), "5.0M");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1024 * 1024 * 1024, true), "1.0G");
    }

    #[test]
    fn test_format_size_blocks() {
        // Non-human-readable returns 1K blocks
        assert_eq!(format_size(512, false), "1");
        assert_eq!(format_size(1024, false), "1");
        assert_eq!(format_size(2048, false), "2");
    }
}
