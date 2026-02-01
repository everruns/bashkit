//! Wait builtin - wait for background jobs

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The wait builtin - wait for background jobs to complete.
///
/// Usage: wait [JOB_ID...]
///
/// If no JOB_ID is specified, wait for all background jobs.
/// Returns the exit status of the last job waited for.
///
/// Note: In this sandboxed implementation, background jobs run
/// synchronously, so wait is effectively a no-op. However, it
/// is provided for script compatibility.
pub struct Wait;

#[async_trait]
impl Builtin for Wait {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // In our current implementation, background jobs run synchronously,
        // so wait is effectively a no-op that returns success.
        //
        // If specific job IDs are provided, we would wait for those.
        // For now, we just return success.

        if !ctx.args.is_empty() {
            // Parse job IDs
            for arg in ctx.args {
                if let Ok(_job_id) = arg.parse::<usize>() {
                    // Would wait for specific job
                    // Currently a no-op since jobs run synchronously
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_wait(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
        };

        Wait.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_wait_no_args() {
        let result = run_wait(&[]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_wait_with_job_id() {
        let result = run_wait(&["1"]).await;
        assert_eq!(result.exit_code, 0);
    }
}
