//! mkfifo builtin - create named pipes in the virtual filesystem.
//!
//! Creates FIFO entries in the VFS. `test -p` returns true for these.
//! In VFS mode, data written/read behaves like a regular file buffer.

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The mkfifo builtin - create named pipes.
///
/// Usage: mkfifo [-m MODE] NAME...
///
/// Creates FIFO entries at the given paths via `FileSystemExt::mkfifo`.
/// The `-m` flag sets the permission mode (octal, default 0o666).
pub struct Mkfifo;

/// Parse an octal mode string (e.g. "0644", "755") to u32.
/// Returns None if the string is not valid octal.
fn parse_mode(s: &str) -> Option<u32> {
    let trimmed = s.trim_start_matches('0');
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    u32::from_str_radix(trimmed, 8).ok()
}

#[async_trait]
impl Builtin for Mkfifo {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: mkfifo [OPTION]... NAME...\nCreate named pipes (FIFOs) with the given NAMEs.\n\n  -m MODE\tset file permission bits to MODE (default: 0666)\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("mkfifo (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        if ctx.args.is_empty() {
            return Ok(ExecResult::err("mkfifo: missing operand\n".to_string(), 1));
        }

        // Parse arguments: extract -m mode, collect paths
        let mut paths = Vec::new();
        let mut mode: u32 = 0o666;
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-m" {
                if i + 1 >= ctx.args.len() {
                    return Ok(ExecResult::err(
                        "mkfifo: option requires an argument -- 'm'\n".to_string(),
                        1,
                    ));
                }
                if let Some(m) = parse_mode(&ctx.args[i + 1]) {
                    mode = m;
                } else {
                    return Ok(ExecResult::err(
                        format!("mkfifo: invalid mode '{}'\n", ctx.args[i + 1]),
                        1,
                    ));
                }
                i += 2;
            } else if let Some(mode_str) = arg.strip_prefix("-m") {
                // -mMODE (combined form)
                if let Some(m) = parse_mode(mode_str) {
                    mode = m;
                } else {
                    return Ok(ExecResult::err(
                        format!("mkfifo: invalid mode '{}'\n", mode_str),
                        1,
                    ));
                }
                i += 1;
            } else if arg.starts_with('-') && arg != "-" {
                return Ok(ExecResult::err(
                    format!("mkfifo: invalid option -- '{}'\n", &arg[1..]),
                    1,
                ));
            } else {
                paths.push(arg.clone());
                i += 1;
            }
        }

        if paths.is_empty() {
            return Ok(ExecResult::err("mkfifo: missing operand\n".to_string(), 1));
        }

        let mut stderr = String::new();
        let mut failed = false;

        for name in &paths {
            let path = resolve_path(ctx.cwd, name);

            if let Err(e) = ctx.fs.mkfifo(&path, mode).await {
                let msg = e.to_string();
                // Map error messages to mkfifo-style output
                if msg.contains("lready") || msg.contains("exists") {
                    stderr.push_str(&format!(
                        "mkfifo: cannot create fifo '{}': File exists\n",
                        name
                    ));
                } else if msg.contains("ot found") || msg.contains("o such") {
                    stderr.push_str(&format!(
                        "mkfifo: cannot create fifo '{}': No such file or directory\n",
                        name
                    ));
                } else {
                    stderr.push_str(&format!("mkfifo: cannot create fifo '{}': {}\n", name, e));
                }
                failed = true;
            }
        }

        if failed {
            Ok(ExecResult::err(stderr, 1))
        } else {
            Ok(ExecResult::ok(String::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_mkfifo(args: &[&str]) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new());

        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Mkfifo.execute(ctx).await.unwrap()
    }

    async fn run_mkfifo_with_fs(args: &[&str], fs: Arc<InMemoryFs>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Mkfifo.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn mkfifo_creates_fifo() {
        let fs = Arc::new(InMemoryFs::new());
        let result = run_mkfifo_with_fs(&["mypipe"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(Path::new("/mypipe")).await.unwrap());
        // Verify it's a FIFO
        let meta = fs.stat(Path::new("/mypipe")).await.unwrap();
        assert!(meta.file_type.is_fifo());
    }

    #[tokio::test]
    async fn mkfifo_multiple_paths() {
        let fs = Arc::new(InMemoryFs::new());
        let result = run_mkfifo_with_fs(&["pipe1", "pipe2", "pipe3"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(Path::new("/pipe1")).await.unwrap());
        assert!(fs.exists(Path::new("/pipe2")).await.unwrap());
        assert!(fs.exists(Path::new("/pipe3")).await.unwrap());
    }

    #[tokio::test]
    async fn mkfifo_existing_file_error() {
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(Path::new("/existing"), b"data")
            .await
            .unwrap();

        let result = run_mkfifo_with_fs(&["existing"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("File exists"));
    }

    #[tokio::test]
    async fn mkfifo_missing_operand() {
        let result = run_mkfifo(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn mkfifo_mode_flag_accepted() {
        let fs = Arc::new(InMemoryFs::new());
        let result = run_mkfifo_with_fs(&["-m", "0644", "mypipe"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        let meta = fs.stat(Path::new("/mypipe")).await.unwrap();
        assert_eq!(meta.mode, 0o644);
    }

    #[tokio::test]
    async fn mkfifo_mode_flag_combined() {
        let fs = Arc::new(InMemoryFs::new());
        let result = run_mkfifo_with_fs(&["-m0755", "mypipe"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        let meta = fs.stat(Path::new("/mypipe")).await.unwrap();
        assert_eq!(meta.mode, 0o755);
    }

    #[tokio::test]
    async fn mkfifo_mode_missing_arg() {
        let result = run_mkfifo(&["-m"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("option requires an argument"));
    }

    #[tokio::test]
    async fn mkfifo_nonexistent_parent() {
        let fs = Arc::new(InMemoryFs::new());
        let result = run_mkfifo_with_fs(&["/no/such/dir/pipe"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn mkfifo_default_mode() {
        let fs = Arc::new(InMemoryFs::new());
        let result = run_mkfifo_with_fs(&["mypipe"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        let meta = fs.stat(Path::new("/mypipe")).await.unwrap();
        assert_eq!(meta.mode, 0o666);
    }
}
