//! Directory stack builtins - pushd, popd, dirs
//!
//! Stack stored in variables: _DIRSTACK_SIZE, _DIRSTACK_0, _DIRSTACK_1, etc.

use async_trait::async_trait;
use std::path::PathBuf;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

fn get_stack_size(ctx: &Context<'_>) -> usize {
    ctx.variables
        .get("_DIRSTACK_SIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn get_stack_entry(ctx: &Context<'_>, idx: usize) -> Option<String> {
    ctx.variables.get(&format!("_DIRSTACK_{}", idx)).cloned()
}

fn set_stack_size(ctx: &mut Context<'_>, size: usize) {
    ctx.variables
        .insert("_DIRSTACK_SIZE".to_string(), size.to_string());
}

fn push_stack(ctx: &mut Context<'_>, dir: &str) {
    let size = get_stack_size(ctx);
    ctx.variables
        .insert(format!("_DIRSTACK_{}", size), dir.to_string());
    set_stack_size(ctx, size + 1);
}

fn pop_stack(ctx: &mut Context<'_>) -> Option<String> {
    let size = get_stack_size(ctx);
    if size == 0 {
        return None;
    }
    let entry = get_stack_entry(ctx, size - 1);
    ctx.variables.remove(&format!("_DIRSTACK_{}", size - 1));
    set_stack_size(ctx, size - 1);
    entry
}

fn normalize_path(base: &std::path::Path, target: &str) -> PathBuf {
    let path = if target.starts_with('/') {
        PathBuf::from(target)
    } else {
        base.join(target)
    };
    super::resolve_path(&PathBuf::from("/"), &path.to_string_lossy())
}

/// The pushd builtin - push directory onto stack and cd.
///
/// Usage: pushd [dir]
///
/// Without args, swaps top two directories.
/// With dir, pushes current dir onto stack and cd to dir.
pub struct Pushd;

#[async_trait]
impl Builtin for Pushd {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            // Swap top two: current dir <-> top of stack
            let top = pop_stack(&mut ctx);
            match top {
                Some(dir) => {
                    let old_cwd = ctx.cwd.to_string_lossy().to_string();
                    let new_path = normalize_path(ctx.cwd, &dir);
                    if ctx.fs.exists(&new_path).await.unwrap_or(false) {
                        push_stack(&mut ctx, &old_cwd);
                        *ctx.cwd = new_path;
                        // Print stack
                        let output = format_stack(&ctx);
                        Ok(ExecResult::ok(format!("{}\n", output)))
                    } else {
                        // Restore stack
                        push_stack(&mut ctx, &dir);
                        Ok(ExecResult::err(
                            format!("pushd: {}: No such file or directory\n", dir),
                            1,
                        ))
                    }
                }
                None => Ok(ExecResult::err(
                    "pushd: no other directory\n".to_string(),
                    1,
                )),
            }
        } else {
            let target = &ctx.args[0].clone();
            let new_path = normalize_path(ctx.cwd, target);

            if ctx.fs.exists(&new_path).await.unwrap_or(false) {
                let meta = ctx.fs.stat(&new_path).await;
                if meta.map(|m| m.file_type.is_dir()).unwrap_or(false) {
                    let old_cwd = ctx.cwd.to_string_lossy().to_string();
                    push_stack(&mut ctx, &old_cwd);
                    *ctx.cwd = new_path;
                    let output = format_stack(&ctx);
                    Ok(ExecResult::ok(format!("{}\n", output)))
                } else {
                    Ok(ExecResult::err(
                        format!("pushd: {}: Not a directory\n", target),
                        1,
                    ))
                }
            } else {
                Ok(ExecResult::err(
                    format!("pushd: {}: No such file or directory\n", target),
                    1,
                ))
            }
        }
    }
}

/// The popd builtin - pop directory from stack and cd.
///
/// Usage: popd
///
/// Removes top directory from stack and cd to it.
pub struct Popd;

#[async_trait]
impl Builtin for Popd {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        match pop_stack(&mut ctx) {
            Some(dir) => {
                let new_path = normalize_path(ctx.cwd, &dir);
                *ctx.cwd = new_path;
                let output = format_stack(&ctx);
                Ok(ExecResult::ok(format!("{}\n", output)))
            }
            None => Ok(ExecResult::err(
                "popd: directory stack empty\n".to_string(),
                1,
            )),
        }
    }
}

/// The dirs builtin - display directory stack.
///
/// Usage: dirs [-c] [-l] [-p] [-v]
///
/// -c: clear the stack
/// -l: long listing (no ~ substitution)
/// -p: one entry per line
/// -v: numbered one entry per line
pub struct Dirs;

#[async_trait]
impl Builtin for Dirs {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let mut clear = false;
        let mut per_line = false;
        let mut verbose = false;

        for arg in ctx.args.iter() {
            match arg.as_str() {
                "-c" => clear = true,
                "-p" => per_line = true,
                "-v" => {
                    verbose = true;
                    per_line = true;
                }
                "-l" => {} // long listing (we don't do ~ substitution anyway)
                _ => {}
            }
        }

        if clear {
            let size = get_stack_size(&ctx);
            for i in 0..size {
                ctx.variables.remove(&format!("_DIRSTACK_{}", i));
            }
            set_stack_size(&mut ctx, 0);
            return Ok(ExecResult::ok(String::new()));
        }

        let cwd = ctx.cwd.to_string_lossy().to_string();
        let size = get_stack_size(&ctx);

        if verbose {
            let mut output = format!(" 0  {}\n", cwd);
            for i in (0..size).rev() {
                if let Some(dir) = get_stack_entry(&ctx, i) {
                    output.push_str(&format!(" {}  {}\n", size - i, dir));
                }
            }
            Ok(ExecResult::ok(output))
        } else if per_line {
            let mut output = format!("{}\n", cwd);
            for i in (0..size).rev() {
                if let Some(dir) = get_stack_entry(&ctx, i) {
                    output.push_str(&format!("{}\n", dir));
                }
            }
            Ok(ExecResult::ok(output))
        } else {
            let output = format_stack(&ctx);
            Ok(ExecResult::ok(format!("{}\n", output)))
        }
    }
}

fn format_stack(ctx: &Context<'_>) -> String {
    let cwd = ctx.cwd.to_string_lossy().to_string();
    let size = get_stack_size(ctx);
    let mut parts = vec![cwd];
    for i in (0..size).rev() {
        if let Some(dir) = get_stack_entry(ctx, i) {
            parts.push(dir);
        }
    }
    parts.join(" ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn setup() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();
        fs.mkdir(&cwd, true).await.unwrap();
        fs.mkdir(Path::new("/tmp"), true).await.unwrap();
        fs.mkdir(Path::new("/var"), true).await.unwrap();
        (fs, cwd, variables)
    }

    // ==================== pushd ====================

    #[tokio::test]
    async fn pushd_to_directory() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Pushd.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(cwd, PathBuf::from("/tmp"));
        // Stack should have old cwd
        assert_eq!(variables.get("_DIRSTACK_0").unwrap(), "/home/user");
    }

    #[tokio::test]
    async fn pushd_nonexistent_dir() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["/nonexistent".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Pushd.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file or directory"));
        // cwd unchanged
        assert_eq!(cwd, PathBuf::from("/home/user"));
    }

    #[tokio::test]
    async fn pushd_file_not_dir() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/file.txt"), b"data")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["file.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Pushd.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("Not a directory"));
    }

    #[tokio::test]
    async fn pushd_no_args_empty_stack() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Pushd.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no other directory"));
    }

    #[tokio::test]
    async fn pushd_no_args_swaps_top() {
        let (fs, mut cwd, mut variables) = setup().await;
        // Push /tmp first so stack has an entry
        let env = HashMap::new();
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();
        assert_eq!(cwd, PathBuf::from("/tmp"));

        // Now pushd with no args should swap
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Pushd.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(cwd, PathBuf::from("/home/user"));
    }

    // ==================== popd ====================

    #[tokio::test]
    async fn popd_empty_stack() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Popd.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("directory stack empty"));
    }

    #[tokio::test]
    async fn popd_after_pushd() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();

        // pushd /tmp
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();
        assert_eq!(cwd, PathBuf::from("/tmp"));

        // popd
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Popd.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(cwd, PathBuf::from("/home/user"));
    }

    #[tokio::test]
    async fn pushd_popd_multiple() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();

        // pushd /tmp
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();

        // pushd /var
        let args = vec!["/var".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();
        assert_eq!(cwd, PathBuf::from("/var"));

        // popd -> /tmp
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Popd.execute(ctx).await.unwrap();
        assert_eq!(cwd, PathBuf::from("/tmp"));

        // popd -> /home/user
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Popd.execute(ctx).await.unwrap();
        assert_eq!(cwd, PathBuf::from("/home/user"));
    }

    // ==================== dirs ====================

    #[tokio::test]
    async fn dirs_empty_stack() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Dirs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/home/user"));
    }

    #[tokio::test]
    async fn dirs_after_pushd() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();

        // pushd /tmp
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();

        // dirs
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Dirs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/tmp"));
        assert!(result.stdout.contains("/home/user"));
    }

    #[tokio::test]
    async fn dirs_clear() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();

        // pushd /tmp
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();

        // dirs -c
        let args = vec!["-c".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Dirs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(get_stack_size_from_vars(&variables), 0);
    }

    #[tokio::test]
    async fn dirs_per_line() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();

        // pushd /tmp
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();

        // dirs -p
        let args = vec!["-p".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Dirs.execute(ctx).await.unwrap();
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn dirs_verbose() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();

        // pushd /tmp
        let args = vec!["/tmp".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Pushd.execute(ctx).await.unwrap();

        // dirs -v
        let args = vec!["-v".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Dirs.execute(ctx).await.unwrap();
        // Verbose format has numbered entries
        assert!(result.stdout.contains(" 0  "));
        assert!(result.stdout.contains(" 1  "));
    }

    fn get_stack_size_from_vars(vars: &HashMap<String, String>) -> usize {
        vars.get("_DIRSTACK_SIZE")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }
}
