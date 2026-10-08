//! Navigation builtins (cd, pwd)

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
                .cloned()
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
            Some(t) => t.clone(),
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

        let mut candidates: Vec<(PathBuf, bool)> = Vec::new();
        let dot_relative = target == "."
            || target == ".."
            || target.starts_with("./")
            || target.starts_with("../");
        if !target.starts_with('/')
            && !dot_relative
            && let Some(cdpath) = var("CDPATH")
        {
            for entry in cdpath.split(':') {
                let base = if entry.is_empty() {
                    ctx.cwd.clone()
                } else {
                    vfs_join(ctx.cwd, entry)
                };
                candidates.push((vfs_join(&base, &target), !entry.is_empty()));
            }
        }
        let direct = if target.starts_with('/') {
            PathBuf::from(&target)
        } else {
            vfs_join(ctx.cwd, &target)
        };
        candidates.push((direct, false));

        for (path, from_cdpath) in candidates {
            let Some(mut resolved) = logical_dir(ctx.fs.as_ref(), &path).await else {
                continue;
            };
            if physical {
                match crate::fs::canonicalize(ctx.fs.as_ref(), &resolved).await {
                    Ok(real) => resolved = real,
                    Err(_) => continue,
                }
            }
            let old_cwd = ctx.cwd.to_string_lossy().to_string();
            ctx.variables.insert("OLDPWD".to_string(), old_cwd);
            *ctx.cwd = resolved;
            let out = if print_dir || from_cdpath {
                format!("{}\n", ctx.cwd.to_string_lossy())
            } else {
                String::new()
            };
            return Ok(ExecResult::ok(out));
        }

        // Report why the direct path failed.
        let path = if target.starts_with('/') {
            PathBuf::from(&target)
        } else {
            vfs_join(ctx.cwd, &target)
        };
        let normalized = normalize_path(&path);
        let reason = match ctx.fs.stat(&normalized).await {
            Ok(m) if !m.file_type.is_dir() => "Not a directory",
            _ => "No such file or directory",
        };
        Ok(ExecResult::err(format!("cd: {target}: {reason}\n"), 1))
    }
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
