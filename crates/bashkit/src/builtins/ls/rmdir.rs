//! rmdir builtin - remove empty directories.

use async_trait::async_trait;

use crate::builtins::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The rmdir builtin - remove empty directories.
///
/// Usage: rmdir [-p] DIRECTORY...
///
/// Options:
///   -p   Remove parent directories as well if they become empty
pub struct Rmdir;

#[async_trait]
impl Builtin for Rmdir {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = crate::builtins::check_help_version(
            ctx.args,
            "Usage: rmdir [OPTION]... DIRECTORY...\nRemove empty directories.\n\n  -p\t\tremove DIRECTORY and its ancestors\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("rmdir (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        if ctx.args.is_empty() {
            return Ok(ExecResult::err("rmdir: missing operand\n".to_string(), 1));
        }

        let parents = ctx.args.iter().any(|a| a == "-p");
        let dirs: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if dirs.is_empty() {
            return Ok(ExecResult::err("rmdir: missing operand\n".to_string(), 1));
        }

        for dir in dirs {
            let path = resolve_path(ctx.cwd, dir);

            // Check if exists
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "rmdir: failed to remove '{}': No such file or directory\n",
                        dir
                    ),
                    1,
                ));
            }

            // Check if it's a directory
            let metadata = ctx.fs.stat(&path).await?;
            if !metadata.file_type.is_dir() {
                return Ok(ExecResult::err(
                    format!("rmdir: failed to remove '{}': Not a directory\n", dir),
                    1,
                ));
            }

            // Check if directory is empty
            let entries = ctx.fs.read_dir(&path).await?;
            if !entries.is_empty() {
                return Ok(ExecResult::err(
                    format!("rmdir: failed to remove '{}': Directory not empty\n", dir),
                    1,
                ));
            }

            // Remove the directory
            if let Err(e) = ctx.fs.remove(&path, false).await {
                return Ok(ExecResult::err(
                    format!("rmdir: failed to remove '{}': {}\n", dir, e),
                    1,
                ));
            }

            // If -p, try to remove parent directories
            if parents {
                let mut current = path.parent();
                while let Some(parent) = current {
                    // Don't remove root or cwd
                    if parent.as_os_str().is_empty() || parent == ctx.cwd.as_path() {
                        break;
                    }

                    // Check if parent is empty
                    if let Ok(entries) = ctx.fs.read_dir(parent).await {
                        if entries.is_empty() {
                            if ctx.fs.remove(parent, false).await.is_err() {
                                break;
                            }
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }

                    current = parent.parent();
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}
