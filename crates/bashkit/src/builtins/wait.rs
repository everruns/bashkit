//! Wait builtin — wait for background jobs to complete.
//!
//! Accesses the shared job table via [`ShellRef`](super::ShellRef). Jobs are
//! driven by the interpreter alongside the foreground script, so awaiting
//! here lets them run (see `interpreter/jobs.rs`).

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::{BuiltinSideEffect, ExecResult};

/// `wait` builtin — wait for background jobs to complete.
///
/// Usage: `wait [-n] [-p VAR] [ID...]`; an ID is a PID (`$!`) or a jobspec
/// (`%N`, `%%`, `%-`).
///
/// - No ID: wait for every job; status 0.
/// - IDs: wait for each; status of the last; 127 for an unknown ID.
/// - `-n`: wait for the next job (of the IDs, if given) to finish; its
///   status, or 127 when there is none. `-p VAR` stores its PID.
///
/// Output of the jobs waited for that was not reported yet is part of this
/// command's output.
pub struct Wait;

#[async_trait]
impl Builtin for Wait {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            // No shell state — no-op (no jobs to wait for)
            return Ok(ExecResult::ok(String::new()));
        };
        let jobs = shell.jobs();

        let mut next = false;
        let mut pid_var: Option<String> = None;
        let mut ids: Vec<&str> = Vec::new();
        let mut args = ctx.args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-n" => next = true,
                "-f" => {}
                "-p" => match args.next() {
                    Some(v) => pid_var = Some(v.clone()),
                    None => {
                        return Ok(ExecResult::err(
                            "bash: wait: -p: option requires an argument\n",
                            2,
                        ));
                    }
                },
                "--" => ids.extend(args.by_ref().map(String::as_str)),
                _ => ids.push(arg),
            }
        }

        let mut stdout = crate::StreamData::new();
        let mut stderr = crate::StreamData::new();
        let mut exit_code = 0i32;
        let mut reaped_pid: Option<u32> = None;

        if next {
            // Wait for any (listed) job to finish; 127 when none is left.
            let wanted: Option<Vec<usize>> = (!ids.is_empty()).then(|| {
                let table = jobs.lock();
                ids.iter().filter_map(|s| table.resolve(s)).collect()
            });
            let picked = jobs
                .wait_until(|t| {
                    let candidates: Vec<usize> = match &wanted {
                        Some(w) => w
                            .iter()
                            .copied()
                            .filter(|id| t.ids().contains(id))
                            .collect(),
                        None => t.ids(),
                    };
                    if candidates.is_empty() {
                        return Some(None);
                    }
                    let done = t.finished_ids();
                    candidates
                        .iter()
                        .find(|id| done.contains(id))
                        .map(|id| Some(*id))
                })
                .await;
            match picked {
                Some(id) => {
                    let mut table = jobs.lock();
                    reaped_pid = u32::try_from(id).ok();
                    if let Some(r) = table.reap(id) {
                        stdout.append(&r.stdout);
                        stderr.append(&r.stderr);
                        exit_code = r.exit_code;
                    }
                }
                None => exit_code = 127,
            }
        } else if ids.is_empty() {
            jobs.wait_until(|t| (t.running_count() == 0).then_some(()))
                .await;
            let mut table = jobs.lock();
            for id in table.ids() {
                if let Some(r) = table.reap(id) {
                    stdout.append(&r.stdout);
                    stderr.append(&r.stderr);
                }
            }
        } else {
            for spec in ids {
                let Some(id) = jobs.lock().resolve(spec) else {
                    let msg = if spec.starts_with('%') {
                        format!("bash: wait: {spec}: no such job\n")
                    } else if spec.parse::<u32>().is_ok() {
                        format!("bash: wait: pid {spec} is not a child of this shell\n")
                    } else {
                        format!("bash: wait: `{spec}': not a pid or valid job spec\n")
                    };
                    stderr.append(&crate::StreamData::from(msg));
                    exit_code = 127;
                    continue;
                };
                let r = jobs
                    .wait_until(|t| {
                        t.reap(id).map(Some).or_else(|| {
                            // Reaped by someone else meanwhile.
                            (!t.ids().contains(&id)).then_some(None)
                        })
                    })
                    .await;
                match r {
                    Some(r) => {
                        stdout.append(&r.stdout);
                        stderr.append(&r.stderr);
                        exit_code = r.exit_code;
                    }
                    None => exit_code = 127,
                }
            }
        }

        let mut result = ExecResult {
            stdout,
            stderr,
            exit_code,
            ..Default::default()
        };
        if let Some(var) = pid_var {
            result.side_effects.push(BuiltinSideEffect::SetVariable {
                name: var,
                value: reaped_pid.map(|p| p.to_string()).unwrap_or_default(),
            });
        }
        result
            .side_effects
            .push(BuiltinSideEffect::SetLastExitCode(exit_code));
        Ok(result)
    }
}

// Integration tests for wait live in tests/spec_cases/bash/{wait,jobs}.test.sh
// (wait needs the full interpreter to have background jobs).
