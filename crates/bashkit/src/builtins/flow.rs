//! Flow control builtins (true, false, exit, break, continue, return)

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::{ControlFlow, ExecResult};

/// The true builtin - always returns 0.
pub struct True;

#[async_trait]
impl Builtin for True {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        Ok(ExecResult::ok(String::new()))
    }
}

/// The false builtin - always returns 1.
pub struct False;

#[async_trait]
impl Builtin for False {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        Ok(ExecResult::err(String::new(), 1))
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
        Ok(ExecResult::err(String::new(), exit_code))
    }
}

/// The break builtin - break out of a loop
pub struct Break;

#[async_trait]
impl Builtin for Break {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let levels = ctx
            .args
            .first()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);

        Ok(ExecResult::with_control_flow(ControlFlow::Break(levels)))
    }
}

/// The continue builtin - continue to next iteration
pub struct Continue;

#[async_trait]
impl Builtin for Continue {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let levels = ctx
            .args
            .first()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);

        Ok(ExecResult::with_control_flow(ControlFlow::Continue(levels)))
    }
}

/// The return builtin - return from a function
pub struct Return;

#[async_trait]
impl Builtin for Return {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let exit_code = ctx
            .args
            .first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        Ok(ExecResult::with_control_flow(ControlFlow::Return(
            exit_code,
        )))
    }
}
