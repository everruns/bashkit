//! Flow control builtins (true, false, exit)

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The true builtin - always returns 0.
pub struct True;

#[async_trait]
impl Builtin for True {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        Ok(ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })
    }
}

/// The false builtin - always returns 1.
pub struct False;

#[async_trait]
impl Builtin for False {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        Ok(ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 1,
        })
    }
}

/// The exit builtin - exit the shell with a status code.
pub struct Exit;

#[async_trait]
impl Builtin for Exit {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let exit_code = ctx
            .args
            .first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        // For now, we just return the exit code
        // In a full implementation, this would terminate the shell
        Ok(ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
        })
    }
}
