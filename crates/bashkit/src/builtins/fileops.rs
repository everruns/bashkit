//! File operation builtins - mkdir, rm, cp, mv, touch, chmod

// Uses unwrap() after length checks (e.g., files.last() after files.len() >= 2)
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::path::Path;

use super::{resolve_path, Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The mkdir builtin - create directories.
///
/// Usage: mkdir [-p] DIRECTORY...
///
/// Options:
///   -p   Create parent directories as needed, no error if existing
pub struct Mkdir;

#[async_trait]
impl Builtin for Mkdir {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("mkdir: missing operand\n".to_string(), 1));
        }

        let recursive = ctx.args.iter().any(|a| a == "-p");
        let dirs: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if dirs.is_empty() {
            return Ok(ExecResult::err("mkdir: missing operand\n".to_string(), 1));
        }

        for dir in dirs {
            let path = resolve_path(ctx.cwd, dir);

            // Check if already exists
            if ctx.fs.exists(&path).await.unwrap_or(false) {
                if !recursive {
                    return Ok(ExecResult::err(
                        format!("mkdir: cannot create directory '{}': File exists\n", dir),
                        1,
                    ));
                }
                // With -p, existing directory is not an error
                continue;
            }

            if let Err(e) = ctx.fs.mkdir(&path, recursive).await {
                return Ok(ExecResult::err(
                    format!("mkdir: cannot create directory '{}': {}\n", dir, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The rm builtin - remove files or directories.
///
/// Usage: rm [-rf] FILE...
///
/// Options:
///   -r, -R   Remove directories and their contents recursively
///   -f       Ignore nonexistent files, never prompt
pub struct Rm;

#[async_trait]
impl Builtin for Rm {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("rm: missing operand\n".to_string(), 1));
        }

        let recursive = ctx.args.iter().any(|a| {
            a == "-r"
                || a == "-R"
                || a == "-rf"
                || a == "-fr"
                || a.contains('r') && a.starts_with('-')
        });
        let force = ctx.args.iter().any(|a| {
            a == "-f" || a == "-rf" || a == "-fr" || a.contains('f') && a.starts_with('-')
        });

        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.is_empty() {
            return Ok(ExecResult::err("rm: missing operand\n".to_string(), 1));
        }

        for file in files {
            let path = resolve_path(ctx.cwd, file);

            // Check if exists
            let exists = ctx.fs.exists(&path).await.unwrap_or(false);
            if !exists {
                if !force {
                    return Ok(ExecResult::err(
                        format!("rm: cannot remove '{}': No such file or directory\n", file),
                        1,
                    ));
                }
                continue;
            }

            // Check if it's a directory
            let metadata = ctx.fs.stat(&path).await;
            if let Ok(meta) = metadata {
                if meta.file_type.is_dir() && !recursive {
                    return Ok(ExecResult::err(
                        format!("rm: cannot remove '{}': Is a directory\n", file),
                        1,
                    ));
                }
            }

            if let Err(e) = ctx.fs.remove(&path, recursive).await {
                if !force {
                    return Ok(ExecResult::err(
                        format!("rm: cannot remove '{}': {}\n", file, e),
                        1,
                    ));
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The cp builtin - copy files and directories.
///
/// Usage: cp [-r] SOURCE... DEST
///
/// Options:
///   -r, -R   Copy directories recursively
pub struct Cp;

#[async_trait]
impl Builtin for Cp {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.len() < 2 {
            return Ok(ExecResult::err("cp: missing file operand\n".to_string(), 1));
        }

        let _recursive = ctx.args.iter().any(|a| a == "-r" || a == "-R");
        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.len() < 2 {
            return Ok(ExecResult::err(
                "cp: missing destination file operand\n".to_string(),
                1,
            ));
        }

        let dest = files.last().unwrap();
        let sources = &files[..files.len() - 1];
        let dest_path = resolve_path(ctx.cwd, dest);

        // Check if destination is a directory
        let dest_is_dir = if let Ok(meta) = ctx.fs.stat(&dest_path).await {
            meta.file_type.is_dir()
        } else {
            false
        };

        if sources.len() > 1 && !dest_is_dir {
            return Ok(ExecResult::err(
                format!("cp: target '{}' is not a directory\n", dest),
                1,
            ));
        }

        for source in sources {
            let src_path = resolve_path(ctx.cwd, source);

            let final_dest = if dest_is_dir {
                // Copy into directory
                let filename = Path::new(source)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.to_string());
                dest_path.join(&filename)
            } else {
                dest_path.clone()
            };

            if let Err(e) = ctx.fs.copy(&src_path, &final_dest).await {
                return Ok(ExecResult::err(
                    format!("cp: cannot copy '{}': {}\n", source, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The mv builtin - move (rename) files.
///
/// Usage: mv SOURCE... DEST
pub struct Mv;

#[async_trait]
impl Builtin for Mv {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.len() < 2 {
            return Ok(ExecResult::err("mv: missing file operand\n".to_string(), 1));
        }

        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.len() < 2 {
            return Ok(ExecResult::err(
                "mv: missing destination file operand\n".to_string(),
                1,
            ));
        }

        let dest = files.last().unwrap();
        let sources = &files[..files.len() - 1];
        let dest_path = resolve_path(ctx.cwd, dest);

        // Check if destination is a directory
        let dest_is_dir = if let Ok(meta) = ctx.fs.stat(&dest_path).await {
            meta.file_type.is_dir()
        } else {
            false
        };

        if sources.len() > 1 && !dest_is_dir {
            return Ok(ExecResult::err(
                format!("mv: target '{}' is not a directory\n", dest),
                1,
            ));
        }

        for source in sources {
            let src_path = resolve_path(ctx.cwd, source);

            let final_dest = if dest_is_dir {
                // Move into directory
                let filename = Path::new(source)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.to_string());
                dest_path.join(&filename)
            } else {
                dest_path.clone()
            };

            if let Err(e) = ctx.fs.rename(&src_path, &final_dest).await {
                return Ok(ExecResult::err(
                    format!("mv: cannot move '{}': {}\n", source, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The touch builtin - change file timestamps or create empty files.
///
/// Usage: touch FILE...
pub struct Touch;

#[async_trait]
impl Builtin for Touch {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "touch: missing file operand\n".to_string(),
                1,
            ));
        }

        for file in ctx.args.iter().filter(|a| !a.starts_with('-')) {
            let path = resolve_path(ctx.cwd, file);

            // Check if file exists
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                // Create empty file
                if let Err(e) = ctx.fs.write_file(&path, &[]).await {
                    return Ok(ExecResult::err(
                        format!("touch: cannot touch '{}': {}\n", file, e),
                        1,
                    ));
                }
            }
            // For existing files, we would update mtime but VFS doesn't track it in a modifiable way
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The chmod builtin - change file mode bits.
///
/// Usage: chmod MODE FILE...
///
/// MODE can be octal (e.g., 755) or symbolic (e.g., u+x) - only octal supported currently
pub struct Chmod;

#[async_trait]
impl Builtin for Chmod {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.len() < 2 {
            return Ok(ExecResult::err("chmod: missing operand\n".to_string(), 1));
        }

        let mode_str = &ctx.args[0];
        let files = &ctx.args[1..];

        // Parse octal mode
        let mode = match u32::from_str_radix(mode_str, 8) {
            Ok(m) => m,
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("chmod: invalid mode: '{}'\n", mode_str),
                    1,
                ));
            }
        };

        for file in files.iter().filter(|a| !a.starts_with('-')) {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "chmod: cannot access '{}': No such file or directory\n",
                        file
                    ),
                    1,
                ));
            }

            if let Err(e) = ctx.fs.chmod(&path, mode).await {
                return Ok(ExecResult::err(
                    format!("chmod: changing permissions of '{}': {}\n", file, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
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

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        // Create the cwd
        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    #[tokio::test]
    async fn test_mkdir_simple() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["testdir".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Mkdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("testdir")).await.unwrap());
    }

    #[tokio::test]
    async fn test_mkdir_recursive() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-p".to_string(), "a/b/c".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Mkdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("a/b/c")).await.unwrap());
    }

    #[tokio::test]
    async fn test_touch_create() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["newfile.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Touch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("newfile.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_rm_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a file first
        fs.write_file(&cwd.join("testfile.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["testfile.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Rm.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("testfile.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_rm_force_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-f".to_string(), "nonexistent".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Rm.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0); // No error with -f
    }

    #[tokio::test]
    async fn test_cp_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create source file
        fs.write_file(&cwd.join("source.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["source.txt".to_string(), "dest.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Cp.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("dest.txt")).await.unwrap());

        let content = fs.read_file(&cwd.join("dest.txt")).await.unwrap();
        assert_eq!(content, b"content");
    }

    #[tokio::test]
    async fn test_mv_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create source file
        fs.write_file(&cwd.join("source.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["source.txt".to_string(), "dest.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Mv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("source.txt")).await.unwrap());
        assert!(fs.exists(&cwd.join("dest.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_chmod_octal() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a file
        fs.write_file(&cwd.join("script.sh"), b"#!/bin/bash")
            .await
            .unwrap();

        let args = vec!["755".to_string(), "script.sh".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = Chmod.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let meta = fs.stat(&cwd.join("script.sh")).await.unwrap();
        assert_eq!(meta.mode, 0o755);
    }
}
