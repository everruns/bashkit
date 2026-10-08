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
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

fn normalize_path(base: &std::path::Path, target: &str) -> PathBuf {
    let path = if target.starts_with('/') {
        PathBuf::from(target)
    } else {
        vfs_join(base, target)
    };
    super::resolve_path(&PathBuf::from("/"), &path.to_string_lossy())
}

/// bash `polite_directory_format`: a directory under `$HOME` is shown as
/// `~` plus the rest (`$HOME` of `/` or unset disables it).
fn polite(dir: &str, home: Option<&str>) -> String {
    if let Some(home) = home.filter(|h| h.len() > 1)
        && let Some(rest) = dir.strip_prefix(home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    dir.to_string()
}

/// Current dir followed by the stack from top (most recent) to bottom,
/// `~`-abbreviated unless `long`.
fn stack_entries(ctx: &Context<'_>, long: bool) -> Vec<String> {
    let home = if long {
        None
    } else {
        ctx.variables.get("HOME").map(String::as_str)
    };
    let cwd = ctx.cwd.to_string_lossy();
    let mut parts = vec![polite(&cwd, home)];
    if let Some(shell) = ctx.shell.as_ref() {
        parts.extend(shell.dir_stack.iter().rev().map(|d| polite(d, home)));
    }
    parts
}

/// Format the stack as bash's `dirs` default: one line, `~`-abbreviated.
fn format_stack(ctx: &Context<'_>) -> String {
    stack_entries(ctx, false).join(" ")
}

/// Operands of `pushd`/`popd` after `--`; a `-x`/`+x` that is no number is
/// a usage error (status 2), as in bash. `+N`/`-N` stack rotation is not
/// supported and is treated as an operand.
fn stack_operands<'a>(
    args: &'a [String],
    name: &str,
    usage: &str,
) -> std::result::Result<Vec<&'a String>, Box<ExecResult>> {
    let mut out = Vec::new();
    let mut opts = true;
    for arg in args {
        if opts && arg == "--" {
            opts = false;
            continue;
        }
        if opts && arg.len() > 1 && arg.starts_with(['-', '+']) {
            if arg == "-n" {
                continue;
            }
            if !arg[1..].bytes().all(|b| b.is_ascii_digit()) {
                return Err(Box::new(ExecResult::err(
                    format!("{name}: {arg}: invalid number\n{name}: usage: {usage}\n"),
                    2,
                )));
            }
        }
        opts = false;
        out.push(arg);
    }
    Ok(out)
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
        let operands = match stack_operands(ctx.args, "pushd", "pushd [-n] [+N | -N | dir]") {
            Ok(o) => o,
            Err(e) => return Ok(*e),
        };
        if operands.len() > 1 {
            return Ok(ExecResult::err(
                "pushd: too many arguments\n".to_string(),
                1,
            ));
        }
        if operands.is_empty() {
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
            let target = operands[0].clone();
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
        let operands = match stack_operands(ctx.args, "popd", "popd [-n] [+N | -N]") {
            Ok(o) => o,
            Err(e) => return Ok(*e),
        };
        if let Some(arg) = operands.first() {
            return Ok(ExecResult::err(
                format!("popd: {arg}: invalid argument\npopd: usage: popd [-n] [+N | -N]\n"),
                2,
            ));
        }
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
        let mut long = false;

        for arg in ctx.args.iter() {
            match arg.as_str() {
                "-c" => clear = true,
                "-p" => per_line = true,
                "-v" => {
                    verbose = true;
                    per_line = true;
                }
                "-l" => long = true,
                a if !a.starts_with(['-', '+']) => {
                    return Ok(ExecResult::err(
                        format!("dirs: {a}: invalid option\ndirs: usage: dirs [-clpv] [+N] [-N]\n"),
                        2,
                    ));
                }
                _ => {}
            }
        }

        if clear {
            if let Some(shell) = ctx.shell.as_mut() {
                shell.dir_stack.clear();
            }
            return Ok(ExecResult::ok(String::new()));
        }

        let entries = stack_entries(&ctx, long);
        let output = if verbose {
            entries
                .iter()
                .enumerate()
                .map(|(n, dir)| format!(" {n}  {dir}\n"))
                .collect()
        } else if per_line {
            entries.iter().map(|dir| format!("{dir}\n")).collect()
        } else {
            format!("{}\n", entries.join(" "))
        };
        Ok(ExecResult::ok(output))
    }
}
