//! Navigation builtins (cd, pwd)
//!
//! CDPATH is searched lazily. Borrow its entries and the target; lease one
//! candidate's path workspace before allocating, then release it before the next.

use async_trait::async_trait;
use std::path::PathBuf;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The cd builtin - change directory.
///
/// `cd [-L|-P [-e]] [-@] [dir]`. Paths resolve logically (`..` drops the
/// previous component, L-FS-001) but, as in bash, every component before a
/// `..` must exist. `-P` resolves symlinks. A relative `dir` that does not
/// start with `.` or `..` is looked up in `CDPATH` first; `cd -` and a
/// non-empty `CDPATH` hit print the new directory.
pub struct Cd;

#[async_trait]
impl Builtin for Cd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut physical = false;
        let mut operands: &[String] = ctx.args;
        while let Some(arg) = operands.first() {
            if arg == "--" {
                operands = &operands[1..];
                break;
            }
            if arg.len() < 2 || !arg.starts_with('-') {
                break;
            }
            for ch in arg.chars().skip(1) {
                match ch {
                    'L' => physical = false,
                    'P' => physical = true,
                    'e' | '@' => {}
                    _ => {
                        return Ok(ExecResult::err(
                            format!(
                                "bash: cd: -{ch}: invalid option\n\
                                 cd: usage: cd [-L|[-P [-e]] [-@]] [dir]\n"
                            ),
                            2,
                        ));
                    }
                }
            }
            operands = &operands[1..];
        }
        if operands.len() > 1 {
            return Ok(ExecResult::err("bash: cd: too many arguments\n", 1));
        }

        let var = |name: &str| {
            ctx.variables
                .get(name)
                .or_else(|| ctx.env.get(name))
                .map(String::as_str)
        };
        let mut print_dir = false;
        let target = match operands.first() {
            Some(t) if t == "-" => {
                print_dir = true;
                match var("OLDPWD") {
                    Some(old) => old,
                    None => return Ok(ExecResult::err("bash: cd: OLDPWD not set\n", 1)),
                }
            }
            Some(t) => t.as_str(),
            // Bare `cd` with HOME unset fails, as in bash.
            None => match var("HOME") {
                Some(home) => home,
                None => return Ok(ExecResult::err("bash: cd: HOME not set\n", 1)),
            },
        };
        // `cd ""` stays put (bash 5.2).
        if target.is_empty() {
            return Ok(ExecResult::ok(""));
        }

        let dot_relative = target == "."
            || target == ".."
            || target.starts_with("./")
            || target.starts_with("../");
        let cdpath = if !target.starts_with('/') && !dot_relative {
            var("CDPATH")
        } else {
            None
        };
        let mut found = None;
        // THREAT[TM-DOS-096]: no target copies accumulate across CDPATH entries.
        let mut entries = cdpath.unwrap_or("").split(':');
        let mut direct = false;
        let mut index = 0usize;
        loop {
            let entry = if cdpath.is_some() {
                entries.next()
            } else {
                None
            };
            if entry.is_none() {
                if direct {
                    break;
                }
                direct = true;
            }
            if index.is_multiple_of(64) {
                tokio::task::yield_now().await;
            }
            index += 1;
            let _workspace = lease_path_workspace(&ctx, entry, target)?;
            let path = match entry {
                Some("") | None => vfs_join(ctx.cwd, target),
                Some(entry) => vfs_join(&vfs_join(ctx.cwd, entry), target),
            };
            let Some(mut resolved) = ctx
                .run_budgeted(logical_dir(ctx.fs.as_ref(), &path))
                .await?
            else {
                continue;
            };
            if physical {
                match ctx
                    .run_budgeted(crate::fs::canonicalize(ctx.fs.as_ref(), &resolved))
                    .await?
                {
                    Ok(real) => resolved = real,
                    Err(_) => continue,
                }
            }
            found = Some((
                resolved,
                entry.is_some_and(|entry| !entry.is_empty()),
                _workspace,
            ));
            break;
        }

        if let Some((resolved, from_cdpath, _workspace)) = found {
            let old_cwd = ctx.cwd.to_string_lossy().to_string();
            ctx.variables.insert("OLDPWD".to_string(), old_cwd);
            *ctx.cwd = resolved;
            // `cd` rewrites `$PWD` even after `PWD=x` (same directory too).
            ctx.variables
                .insert("PWD".to_string(), ctx.cwd.to_string_lossy().into_owned());
            let out = if print_dir || from_cdpath {
                format!("{}\n", ctx.cwd.to_string_lossy())
            } else {
                String::new()
            };
            return Ok(ExecResult::ok(out));
        }

        // Report why the direct path failed.
        let _workspace = lease_path_workspace(&ctx, None, target)?;
        let path = if target.starts_with('/') {
            PathBuf::from(target)
        } else {
            vfs_join(ctx.cwd, target)
        };
        let normalized = normalize_path(&path);
        let reason = match ctx.run_budgeted(ctx.fs.stat(&normalized)).await? {
            Ok(m) if !m.file_type.is_dir() => "Not a directory",
            _ => "No such file or directory",
        };
        Ok(ExecResult::err(format!("cd: {target}: {reason}\n"), 1))
    }
}

/// Reserve an upper bound for the simultaneous path buffers and normalization
/// scratch. `normalize_path` can retain up to `len` borrowed components after
/// Vec capacity rounding; joins, logical resolution, and formatting together
/// need at most sixteen path-length copies, including VFS lookup scratch. The
/// minimum covers small-buffer capacities. Charge before any of them allocate.
fn lease_path_workspace(
    ctx: &Context<'_>,
    entry: Option<&str>,
    target: &str,
) -> Result<Option<crate::limits::ExecutionBudgetLease>> {
    let len = ctx
        .cwd
        .as_os_str()
        .len()
        .checked_add(entry.map_or(0, str::len))
        .and_then(|len| len.checked_add(target.len()))
        .and_then(|len| len.checked_add(2))
        .ok_or_else(|| crate::limits::LimitExceeded::Memory("cd path size overflow".into()))?;
    ctx.consume_budget_work(1 + u64::try_from(len.div_ceil(64)).unwrap_or(u64::MAX))?;
    let bytes = len
        .max(8)
        .checked_mul(16 + std::mem::size_of::<&str>())
        .ok_or_else(|| crate::limits::LimitExceeded::Memory("cd path size overflow".into()))?;
    ctx.lease_budget_bytes(bytes)
}

/// Resolve `path` logically: `.` drops, `..` pops the previous component,
/// but only after checking that what it pops is an existing directory
/// (bash: `cd nope/..` fails). Returns the normalized path when it names a
/// directory.
async fn logical_dir(fs: &dyn crate::fs::FileSystem, path: &std::path::Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut out = PathBuf::from("/");
    for comp in path.components() {
        match comp {
            Component::Normal(name) => out.push(name),
            Component::ParentDir => {
                if !fs.stat(&out).await.is_ok_and(|m| m.file_type.is_dir()) {
                    return None;
                }
                out.pop();
            }
            _ => {}
        }
    }
    let out = normalize_path(&out);
    fs.stat(&out)
        .await
        .is_ok_and(|m| m.file_type.is_dir())
        .then_some(out)
}

/// The pwd builtin - print working directory.
///
/// `-P` prints the path with every symlink resolved; `-L` (default) the
/// logical one.
pub struct Pwd;

#[async_trait]
impl Builtin for Pwd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut physical = false;
        for arg in ctx.args {
            match arg.as_str() {
                "-P" => physical = true,
                "-L" => physical = false,
                "-LP" | "-PL" => physical = arg.ends_with('P'),
                _ => {}
            }
        }
        let cwd = if physical {
            crate::fs::canonicalize(ctx.fs.as_ref(), ctx.cwd)
                .await
                .unwrap_or_else(|_| ctx.cwd.clone())
        } else {
            ctx.cwd.clone()
        };
        Ok(ExecResult::ok(format!("{}\n", cwd.to_string_lossy())))
    }
}

use crate::fs::normalize_path;
use crate::fs::vfs_join;
