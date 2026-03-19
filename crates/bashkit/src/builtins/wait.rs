//! Wait builtin — wait for background jobs to complete.
//!
//! Accesses the shared job table via [`ShellRef`](super::ShellRef).

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::{BuiltinSideEffect, ExecResult};

/// `wait` builtin — wait for background jobs to complete.
///
/// Usage: wait [JOB_ID...]
///
/// If no JOB_ID is specified, wait for all background jobs.
/// Returns the exit status of the last job waited for.
/// Merges background job stdout/stderr into the result.
pub struct Wait;

#[async_trait]
impl Builtin for Wait {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            // No shell state — no-op (no jobs to wait for)
            return Ok(ExecResult::ok(String::new()));
        };

        let jobs = shell.jobs();
        let mut last_exit_code = 0i32;
        let mut stdout = String::new();
        let mut stderr = String::new();

        if ctx.args.is_empty() {
            // Wait for all background jobs, collecting their output
            let results = jobs.lock().await.wait_all_results().await;
            for r in results {
                stdout.push_str(&r.stdout);
                stderr.push_str(&r.stderr);
                last_exit_code = r.exit_code;
            }
        } else {
            // Wait for specific job IDs
            for arg in ctx.args {
                if let Ok(job_id) = arg.parse::<usize>()
                    && let Some(r) = jobs.lock().await.wait_for(job_id).await
                {
                    stdout.push_str(&r.stdout);
                    stderr.push_str(&r.stderr);
                    last_exit_code = r.exit_code;
                }
            }
        }

        let mut result = ExecResult {
            stdout,
            stderr,
            exit_code: last_exit_code,
            ..Default::default()
        };
        result
            .side_effects
            .push(BuiltinSideEffect::SetLastExitCode(last_exit_code));
        Ok(result)
    }
}

// Integration tests for wait are in tests/interpreter_tests.rs
// (wait needs the full interpreter to have meaningful background jobs)
