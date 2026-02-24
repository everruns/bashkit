//! Navigation builtins (cd, pwd)

use async_trait::async_trait;
use std::path::PathBuf;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The cd builtin - change directory.
pub struct Cd;

#[async_trait]
impl Builtin for Cd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let target = ctx
            .args
            .first()
            .map(|s| s.as_str())
            .or_else(|| ctx.variables.get("HOME").map(|s| s.as_str()))
            .or_else(|| ctx.env.get("HOME").map(|s| s.as_str()))
            .unwrap_or("/home/user");

        let new_path = if target.starts_with('/') {
            PathBuf::from(target)
        } else if target == "-" {
            // Go to previous directory
            ctx.variables
                .get("OLDPWD")
                .or_else(|| ctx.env.get("OLDPWD"))
                .map(PathBuf::from)
                .unwrap_or_else(|| ctx.cwd.clone())
        } else {
            ctx.cwd.join(target)
        };

        // Normalize the path
        let normalized = normalize_path(&new_path);

        // Check if directory exists
        if ctx.fs.exists(&normalized).await? {
            let metadata = ctx.fs.stat(&normalized).await?;
            if metadata.file_type.is_dir() {
                // Set OLDPWD before changing directory
                let old_cwd = ctx.cwd.to_string_lossy().to_string();
                ctx.variables.insert("OLDPWD".to_string(), old_cwd);
                *ctx.cwd = normalized;
                Ok(ExecResult::ok(""))
            } else {
                Ok(ExecResult::err(
                    format!("cd: {}: Not a directory\n", target),
                    1,
                ))
            }
        } else {
            Ok(ExecResult::err(
                format!("cd: {}: No such file or directory\n", target),
                1,
            ))
        }
    }
}

/// The pwd builtin - print working directory.
pub struct Pwd;

#[async_trait]
impl Builtin for Pwd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let cwd = ctx.cwd.to_string_lossy();
        Ok(ExecResult::ok(format!("{}\n", cwd)))
    }
}

fn normalize_path(path: &std::path::Path) -> PathBuf {
    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::RootDir => {
                result.push("/");
            }
            std::path::Component::Normal(name) => {
                result.push(name);
            }
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            std::path::Component::Prefix(_) => {}
        }
    }

    if result.as_os_str().is_empty() {
        result.push("/");
    }

    result
}
