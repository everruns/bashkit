//! Flow control builtins (true, false, exit, break, continue, return, colon)

use async_trait::async_trait;

use super::{Builtin, Context};
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
pub struct Exit;

#[async_trait]
impl Builtin for Exit {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let exit_code = ctx
            .args
            .first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0)
            & 0xFF;

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

/// The return builtin - return from a function.
/// Bash truncates return codes to 8-bit unsigned range (0-255) via `& 0xFF`.
pub struct Return;

#[async_trait]
impl Builtin for Return {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let exit_code = ctx
            .args
            .first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0)
            & 0xFF;

        Ok(ExecResult::with_control_flow(ControlFlow::Return(
            exit_code,
        )))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
    async fn exit_non_numeric_defaults_zero() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Exit.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
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
    async fn break_non_numeric_defaults_one() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Break.execute(ctx).await.unwrap();
        assert!(matches!(result.control_flow, ControlFlow::Break(1)));
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
