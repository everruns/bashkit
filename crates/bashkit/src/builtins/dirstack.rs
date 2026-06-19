//! Directory stack builtins - pushd, popd, dirs
//!
//! The stack is typed interpreter state (`ShellRef::dir_stack`), reached via
//! `ctx.shell`. It is bottom-to-top: index 0 is the oldest entry, the last
//! element is the most recently pushed. The current directory (`cwd`) is not
//! part of the vec. (It used to live in the user variable namespace as
//! `_DIRSTACK_SIZE` / `_DIRSTACK_N`, which let scripts forge it.)

use async_trait::async_trait;
use std::path::PathBuf;

use super::limits::DIRSTACK_MAX_SIZE as MAX_DIRSTACK_SIZE;
use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

fn normalize_path(base: &std::path::Path, target: &str) -> PathBuf {
    let path = if target.starts_with('/') {
        PathBuf::from(target)
    } else {
        base.join(target)
    };
    super::resolve_path(&PathBuf::from("/"), &path.to_string_lossy())
}

/// Format the stack as bash's `dirs` default: current dir followed by the
/// stack from top (most recent) to bottom.
fn format_stack(ctx: &Context<'_>) -> String {
    let cwd = ctx.cwd.to_string_lossy().to_string();
    let mut parts = vec![cwd];
    if let Some(shell) = ctx.shell.as_ref() {
        parts.extend(shell.dir_stack.iter().rev().cloned());
    }
    parts.join(" ")
}

/// The pushd builtin - push directory onto stack and cd.
///
/// Usage: pushd [dir]
///
/// Without args, swaps current dir with the top of the stack.
/// With dir, pushes current dir onto the stack and cd to dir.
pub struct Pushd;

#[async_trait]
impl Builtin for Pushd {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            // Swap current dir with the top of the stack.
            let Some(top) = ctx.shell.as_ref().and_then(|s| s.dir_stack.last()).cloned() else {
                return Ok(ExecResult::err(
                    "pushd: no other directory\n".to_string(),
                    1,
                ));
            };
            let new_path = normalize_path(ctx.cwd, &top);
            if !ctx.fs.exists(&new_path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!("pushd: {}: No such file or directory\n", top),
                    1,
                ));
            }
            let old_cwd = ctx.cwd.to_string_lossy().to_string();
            if let Some(shell) = ctx.shell.as_mut() {
                shell.dir_stack.pop();
                shell.dir_stack.push(old_cwd);
            }
            *ctx.cwd = new_path;
            Ok(ExecResult::ok(format!("{}\n", format_stack(&ctx))))
        } else {
            let target = ctx.args[0].clone();
            let new_path = normalize_path(ctx.cwd, &target);

            // Single stat: distinguish "not found" from "not a directory" without
            // a redundant exists() + stat() pair (which would misreport IO/TOCTOU
            // errors as "Not a directory").
            match ctx.fs.stat(&new_path).await {
                Ok(meta) if meta.file_type.is_dir() => {}
                Ok(_) => {
                    return Ok(ExecResult::err(
                        format!("pushd: {}: Not a directory\n", target),
                        1,
                    ));
                }
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("pushd: {}: No such file or directory\n", target),
                        1,
                    ));
                }
            }

            let old_cwd = ctx.cwd.to_string_lossy().to_string();
            if let Some(shell) = ctx.shell.as_mut() {
                // DoS guard: cap stack growth (the user can no longer forge size).
                if shell.dir_stack.len() >= MAX_DIRSTACK_SIZE {
                    return Ok(ExecResult::err(
                        "pushd: directory stack full\n".to_string(),
                        1,
                    ));
                }
                shell.dir_stack.push(old_cwd);
            }
            *ctx.cwd = new_path;
            Ok(ExecResult::ok(format!("{}\n", format_stack(&ctx))))
        }
    }
}

/// The popd builtin - pop directory from stack and cd.
///
/// Usage: popd
///
/// Removes the top directory from the stack and cd to it.
pub struct Popd;

#[async_trait]
impl Builtin for Popd {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let Some(dir) = ctx.shell.as_mut().and_then(|s| s.dir_stack.pop()) else {
            return Ok(ExecResult::err(
                "popd: directory stack empty\n".to_string(),
                1,
            ));
        };
        *ctx.cwd = normalize_path(ctx.cwd, &dir);
        Ok(ExecResult::ok(format!("{}\n", format_stack(&ctx))))
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
            if let Some(shell) = ctx.shell.as_mut() {
                shell.dir_stack.clear();
            }
            return Ok(ExecResult::ok(String::new()));
        }

        if !verbose && !per_line {
            // Default output doesn't need to walk the stack separately.
            return Ok(ExecResult::ok(format!("{}\n", format_stack(&ctx))));
        }

        let cwd = ctx.cwd.to_string_lossy().to_string();
        // Stack from top (most recent) to bottom, borrowed (no clone).
        let empty: Vec<String> = Vec::new();
        let stack = ctx.shell.as_ref().map(|s| &*s.dir_stack).unwrap_or(&empty);

        if verbose {
            let mut output = format!(" 0  {}\n", cwd);
            for (n, dir) in stack.iter().rev().enumerate() {
                output.push_str(&format!(" {}  {}\n", n + 1, dir));
            }
            Ok(ExecResult::ok(output))
        } else {
            let mut output = format!("{}\n", cwd);
            for dir in stack.iter().rev() {
                output.push_str(&format!("{}\n", dir));
            }
            Ok(ExecResult::ok(output))
        }
    }
}
