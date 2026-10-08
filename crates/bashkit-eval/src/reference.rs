// Reference solutions: prove dataset tasks are solvable without an LLM.
//
// Decision: a reference solution is a list of `steps`, each one bash tool call,
// replayed in order on the exact starting `Bash` the model gets
// (`agent::build_task_bash`: files + git + setup). Each step becomes a
// `ToolOutput`, so the checks score it exactly like a model run (multi-turn
// shape included: `exit_code` reads the last step, `stdout_contains` any step).
// CI runs every reference (see tests below), so a dataset edit or a bashkit
// regression that makes a task unsolvable fails `cargo test -p bashkit-eval`.
// The setup-only state must NOT pass, so a task cannot be trivially green.
// See knowledge/operations/eval.md ("repo_workflow").

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::agent::build_task_bash;
use crate::checks::{CheckSummary, evaluate};
use crate::dataset::EvalTask;
use crate::snapshot::{Snapshot, SnapshotTargets, ToolOutput, snapshot_fs, snapshot_links};

/// One task's reference solution (`data/*-solutions.jsonl`).
#[derive(Debug, Clone, Deserialize)]
pub struct ReferenceSolution {
    pub id: String,
    /// Bash tool calls, in order (one per agent turn).
    pub steps: Vec<String>,
}

/// Run `steps` against the task's starting state and score the result.
pub async fn run_reference(task: &EvalTask, steps: &[String]) -> Result<(CheckSummary, Snapshot)> {
    let mut bash = build_task_bash(task).await?;
    let mut tool_outputs = Vec::with_capacity(steps.len());
    for step in steps {
        let (stdout, stderr, exit_code) = match bash.exec(step).await {
            Ok(r) => (r.stdout.to_string(), r.stderr.to_string(), r.exit_code),
            Err(e) => (String::new(), e.to_string(), 1),
        };
        tool_outputs.push(ToolOutput {
            commands: step.clone(),
            stdout,
            stderr,
            exit_code,
        });
    }

    let expectations: Vec<(String, f64)> = task
        .expectations
        .iter()
        .map(|e| (e.check.clone(), e.weight))
        .collect();
    let targets = SnapshotTargets::from_expectations(&expectations);
    let fs = bash.fs();
    let (files, dirs) = snapshot_fs(fs.as_ref(), &targets).await;
    let links = snapshot_links(fs.as_ref(), &targets).await;
    let snap = Snapshot {
        last_exit_code: tool_outputs.last().map(|t| t.exit_code),
        tool_outputs,
        dirs,
        links,
    };
    Ok((evaluate(&expectations, &snap, &files), snap))
}

/// Parse a solutions JSONL (blank and `#`/`//` lines skipped).
pub fn parse_solutions(jsonl: &str) -> Result<Vec<ReferenceSolution>> {
    jsonl
        .lines()
        .enumerate()
        .map(|(i, l)| (i, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .map(|(i, l)| serde_json::from_str(l).with_context(|| format!("solutions line {}", i + 1)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const REPO_TASKS: &str = include_str!("../data/repo-workflow.jsonl");
    const REPO_SOLUTIONS: &str = include_str!("../data/repo-workflow-solutions.jsonl");

    fn tasks() -> Vec<EvalTask> {
        REPO_TASKS
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("task parses"))
            .collect()
    }

    fn report(summary: &CheckSummary, snap: &Snapshot) -> String {
        let mut out = String::new();
        for r in &summary.results {
            out.push_str(&format!(
                "  [{}] {} -- {}\n",
                if r.passed { "ok" } else { "FAIL" },
                r.check,
                r.detail
            ));
        }
        for t in &snap.tool_outputs {
            out.push_str(&format!(
                "--- step (exit {}) ---\n$ {}\nstdout:\n{}stderr:\n{}\n",
                t.exit_code, t.commands, t.stdout, t.stderr
            ));
        }
        out
    }

    #[test]
    fn every_task_has_exactly_one_solution() {
        let ids: BTreeSet<String> = tasks().into_iter().map(|t| t.id).collect();
        let sols = parse_solutions(REPO_SOLUTIONS).unwrap();
        let sol_ids: BTreeSet<String> = sols.iter().map(|s| s.id.clone()).collect();
        assert_eq!(sols.len(), sol_ids.len(), "duplicate solution ids");
        assert_eq!(ids, sol_ids, "tasks and reference solutions differ");
        assert!(ids.len() >= 6, "repo_workflow needs at least 6 tasks");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reference_solutions_pass_all_checks() {
        let sols = parse_solutions(REPO_SOLUTIONS).unwrap();
        let mut failures = Vec::new();
        for task in tasks() {
            let sol = sols.iter().find(|s| s.id == task.id).unwrap();
            let (summary, snap) = run_reference(&task, &sol.steps).await.unwrap();
            if !summary.all_passed() {
                failures.push(format!("== {} ==\n{}", task.id, report(&summary, &snap)));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn untouched_fixture_does_not_pass() {
        // Only the agent's natural first look (`make test`), no fix: the task
        // must still fail, so a do-nothing model cannot score.
        for task in tasks() {
            let dir = task
                .prompt
                .split_whitespace()
                .find(|w| w.starts_with("/home/eval/"))
                .map(|w| w.trim_end_matches(['.', ',', ':', ')']))
                .unwrap_or("/home/eval");
            let steps = vec![format!("cd {dir} 2>/dev/null; make test; git status")];
            let (summary, snap) = run_reference(&task, &steps).await.unwrap();
            assert!(
                !summary.all_passed(),
                "{} passes without a fix:\n{}",
                task.id,
                report(&summary, &snap)
            );
        }
    }
}
