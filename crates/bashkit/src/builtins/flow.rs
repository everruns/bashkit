//! Flow control builtins (true, false, exit, break, continue, return, colon)

use async_trait::async_trait;

use super::{Builtin, BuiltinSideEffect, Context};
use crate::error::Result;
use crate::interpreter::{ControlFlow, ExecResult};

/// The colon builtin (`:`) - POSIX null utility.
///
/// Does nothing and returns success. Required by POSIX as a special built-in.
/// Common uses:
/// - Infinite loops: `while :; do ...; done`
/// - No-op in conditionals: `if cond; then :; else ...; fi`
/// - Variable expansion side effects: `: ${VAR:=default}`
pub struct Colon;

#[async_trait]
impl Builtin for Colon {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        Ok(ExecResult::ok(String::new()))
    }
}

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
/// Bash truncates exit codes to 8-bit unsigned range (0-255) via `& 0xFF`.
/// Without an argument it exits with `$?`; a non-numeric argument is
/// reported and exits 2; more than one argument is an error (status 1) that
/// abandons the rest of the line instead of exiting.
pub struct Exit;

#[async_trait]
impl Builtin for Exit {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let exit_code = match ctx.args.first() {
            None => ctx.shell.as_ref().map_or(0, |s| s.last_exit_code),
            Some(arg) => match arg.trim().parse::<i64>() {
                Ok(n) => (n & 0xFF) as i32,
                Err(_) => {
                    let mut result = ExecResult::with_control_flow(ControlFlow::Exit(2));
                    result.exit_code = 2;
                    result.stderr =
                        format!("bash: exit: {arg}: numeric argument required\n").into();
                    return Ok(result);
                }
            },
        };
        if ctx.args.len() > 1 {
            // bash discards the rest of the line (`jump_to_top_level(DISCARD)`):
            // the script resumes at the next line, a subshell ends.
            let mut result = ExecResult::err("bash: exit: too many arguments\n", 1);
            result.control_flow = ControlFlow::Abort;
            result
                .side_effects
                .push(BuiltinSideEffect::DiscardCommandString);
            return Ok(result);
        }

        Ok(ExecResult {
            exit_code,
            control_flow: ControlFlow::Exit(exit_code),
            ..Default::default()
        })
    }
}

/// Resolve the level count of `break`/`continue` against the enclosing loops.
///
/// Bash semantics: outside a loop the command only warns (status 0); a count
/// below 1 is an error (status 1); a count above the loop depth leaves every
/// enclosing loop of the current function, never a caller's loop (a function
/// call starts outside any loop, so the count is clamped to its depth).
fn loop_control(name: &str, ctx: &Context<'_>, make: fn(u32) -> ControlFlow) -> ExecResult {
    // Builtins run without shell state (e.g. direct unit calls) keep the
    // plain unwinding behaviour.
    let depth = ctx.shell.as_ref().map_or(u32::MAX, |s| {
        u32::try_from(s.loop_depth).unwrap_or(u32::MAX)
    });
    if depth == 0 {
        return ExecResult::err(
            format!("bash: {name}: only meaningful in a `for', `while', or `until' loop\n"),
            0,
        );
    }
    if let Some(arg) = ctx.args.first()
        && arg.trim().parse::<i64>().is_ok()
        && ctx.args.len() > 1
    {
        // Like `exit`: bash discards the rest of the line.
        let mut result = ExecResult::err(format!("bash: {name}: too many arguments\n"), 1);
        result.control_flow = ControlFlow::Abort;
        result
            .side_effects
            .push(BuiltinSideEffect::DiscardCommandString);
        return result;
    }
    let levels = match ctx.args.first() {
        None => 1,
        Some(arg) => match arg.trim().parse::<i64>() {
            Ok(n) if n >= 1 => n,
            Ok(_) => {
                // Bash still leaves every enclosing loop.
                let mut result = ExecResult::with_control_flow(ControlFlow::Break(depth));
                result.exit_code = 1;
                result.stderr = format!("bash: {name}: {arg}: loop count out of range\n").into();
                return result;
            }
            Err(_) => {
                // A special-builtin usage error ends a non-interactive shell.
                let mut result = ExecResult::with_control_flow(ControlFlow::Exit(128));
                result.exit_code = 128;
                result.stderr = format!("bash: {name}: {arg}: numeric argument required\n").into();
                return result;
            }
        },
    };
    let levels = u32::try_from(levels).unwrap_or(u32::MAX).min(depth);
    ExecResult::with_control_flow(make(levels))
}

/// The break builtin - break out of a loop
pub struct Break;

#[async_trait]
impl Builtin for Break {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        Ok(loop_control("break", &ctx, ControlFlow::Break))
    }
}

/// The continue builtin - continue to next iteration
pub struct Continue;

#[async_trait]
impl Builtin for Continue {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        Ok(loop_control("continue", &ctx, ControlFlow::Continue))
    }
}

/// The return builtin - return from a function or sourced file.
/// Bash truncates return codes to 8-bit unsigned range (0-255) via `& 0xFF`.
/// Without an argument it returns `$?`; outside a function or sourced file
/// it fails with status 2 and the script goes on.
pub struct Return;

#[async_trait]
impl Builtin for Return {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(shell) = ctx.shell.as_ref()
            && shell.return_depth == 0
        {
            return Ok(ExecResult::err(
                "bash: return: can only `return' from a function or sourced script\n",
                2,
            ));
        }
        let exit_code = match ctx.args.first() {
            None => ctx.shell.as_ref().map_or(0, |s| s.last_exit_code),
            Some(arg) => match arg.trim().parse::<i64>() {
                Ok(n) => (n & 0xFF) as i32,
                Err(_) => {
                    let mut result = ExecResult::with_control_flow(ControlFlow::Return(2));
                    result.exit_code = 2;
                    result.stderr =
                        format!("bash: return: {arg}: numeric argument required\n").into();
                    return Ok(result);
                }
            },
        };
        if ctx.args.len() > 1 {
            return Ok(ExecResult::err("bash: return: too many arguments\n", 1));
        }

        let mut result = ExecResult::with_control_flow(ControlFlow::Return(exit_code));
        result.exit_code = exit_code;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn setup() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();
        fs.mkdir(&cwd, true).await.unwrap();
        (fs, cwd, variables)
    }

    // ==================== colon ====================

    #[tokio::test]
    async fn colon_returns_success() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Colon.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn colon_ignores_args() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["ignored".to_string(), "stuff".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Colon.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== true ====================

    #[tokio::test]
    async fn true_returns_zero() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = True.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    // ==================== false ====================

    #[tokio::test]
    async fn false_returns_one() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = False.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    // ==================== exit ====================

    #[tokio::test]
    async fn exit_default_zero() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn exit_with_code() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["42".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn exit_truncates_to_8bit() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["256".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0); // 256 & 0xFF = 0
    }

    #[tokio::test]
    async fn exit_truncates_large_code() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["300".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 44); // 300 & 0xFF = 44
    }

    #[tokio::test]
    async fn exit_negative_code() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-1".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 255); // -1 & 0xFF = 255
    }

    #[tokio::test]
    async fn exit_non_numeric_is_usage_error() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.control_flow, ControlFlow::Exit(2));
        assert!(
            result
                .stderr
                .contains("exit: abc: numeric argument required")
        );
    }

    #[tokio::test]
    async fn exit_too_many_args_aborts_the_line() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["1".to_string(), "2".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.control_flow, ControlFlow::Abort);
    }

    // ==================== break ====================

    #[tokio::test]
    async fn break_default_one_level() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Break.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(matches!(result.control_flow, ControlFlow::Break(1)));
    }

    #[tokio::test]
    async fn break_multiple_levels() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["3".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Break.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Break(3)));
    }

    #[tokio::test]
    async fn break_non_numeric_exits_the_shell() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Break.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Exit(128)));
        assert!(result.stderr.contains("numeric argument required"));
    }

    // ==================== continue ====================

    #[tokio::test]
    async fn continue_default_one_level() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Continue.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(matches!(result.control_flow, ControlFlow::Continue(1)));
    }

    #[tokio::test]
    async fn continue_multiple_levels() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["2".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Continue.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Continue(2)));
    }

    // ==================== return ====================

    #[tokio::test]
    async fn return_default_zero() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Return.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Return(0)));
    }

    #[tokio::test]
    async fn return_with_code() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["42".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Return.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Return(42)));
    }

    #[tokio::test]
    async fn return_truncates_to_8bit() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["256".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Return.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Return(0)));
    }

    #[tokio::test]
    async fn return_negative_wraps() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-1".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Return.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Return(255)));
    }
}
