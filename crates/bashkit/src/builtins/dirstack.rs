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
