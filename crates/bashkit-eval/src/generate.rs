// One-shot script generation eval (`bashkit_generate`).
//
// Why: `bashkit_bash` is an agent loop; the model probes, sees errors and
// retries, which hides how often its FIRST script fails on bashkit. Here the
// model gets the task plus a fixed description of the sandbox, answers with ONE
// bash script (no tools, no feedback), and bashkit runs it exactly once on a
// fresh VFS built like the agent eval's (`agent::build_task_bash`). The
// unchanged `expectations_scorer` then scores files/stdout/exit code.
//
// Decisions (see knowledge/operations/eval.md, "Generate Eval"):
//   - One provider call with NO tools offered (providers omit `tools` when the
//     list is empty), so the reply is plain text.
//   - Extraction rule (`extract_script`): the first fenced block tagged
//     `bash`/`sh`/`shell` (case-insensitive); else the first untagged fenced
//     block; else the whole reply, trimmed. An unclosed fence (truncated reply)
//     runs to the end of the text. Fenced blocks tagged with another language
//     are never run.
//   - Failure split: a provider/HTTP error or a broken starting state is an
//     infra error (case scores N/A); an empty/absent script is a model failure
//     scored 0 (the subject records an empty Snapshot and no files, so every
//     check fails without touching the scorer); a script that errors, exits
//     non-zero or times out is an ordinary scored run.
//   - The script runs once via `bash.exec` under `SCRIPT_TIMEOUT`; a timeout
//     records exit code 124 (coreutils `timeout` convention).
//   - The task's `system` field is not used: the system prompt is the fixed,
//     documented `GENERATE_SYSTEM_PROMPT` so every task sees the same sandbox
//     description.

use std::time::{Duration, Instant};

use anyhow::Result;
use bashkit::Bash;
use mira::subject::{Subject, subject_fn};
use mira::{Sample, Transcript};

use crate::agent::build_task_bash;
use crate::dataset::EvalTask;
use crate::mira_study::{expectations_from_sample, provider_for};
use crate::provider::{ContentBlock, Message, Provider, Role, ensure_rustls_crypto_provider};
use crate::snapshot::{
    SNAPSHOT_KEY, Snapshot, SnapshotTargets, ToolOutput, snapshot_fs, snapshot_links,
};

/// Wall-clock bound for the single script run.
pub const SCRIPT_TIMEOUT: Duration = Duration::from_secs(60);

/// Exit code recorded when the script hits `SCRIPT_TIMEOUT`.
pub const TIMEOUT_EXIT_CODE: i32 = 124;

/// Fixed system prompt for every generate task. Describes the sandbox the
/// script runs in and the required answer format.
pub const GENERATE_SYSTEM_PROMPT: &str = "\
You write bash scripts that are executed exactly once, unattended, with no chance to inspect \
output or retry. You will not see the result.

Execution environment:
- The script runs in bashkit, a sandboxed bash-compatible interpreter (bash 5 syntax: arrays, \
associative arrays, [[ ]], $(( )), parameter expansion, here-docs, functions, traps, getopts, \
background jobs with & and wait) over a virtual filesystem.
- It runs as user `eval` with HOME=/home/eval. Use the absolute paths given in the task.
- Available commands are builtins, including the usual coreutils (ls, cat, cp, mv, rm, mkdir, \
touch, ln, chmod, find, xargs, sort, uniq, cut, tr, wc, head, tail, tee, paste, join, comm, \
seq, date, basename, dirname, mktemp, sleep, stat, du), sed, awk, grep, jq, diff, tar, gzip, \
envsubst, bc and printf.
- There is no network access, no package manager, and no python, node or perl unless the task \
says otherwise.
- Only the files described in the task exist; everything else you need, create it.

Answer format: reply with exactly one fenced code block tagged bash (```bash ... ```) that \
contains the complete script. Nothing outside the code block is executed.";

/// How the script was found in the reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extraction {
    /// A fenced code block (tagged bash/sh/shell, or untagged).
    Fenced,
    /// No usable fence: the whole reply, trimmed.
    Raw,
}

impl Extraction {
    pub fn as_str(self) -> &'static str {
        match self {
            Extraction::Fenced => "fenced",
            Extraction::Raw => "raw",
        }
    }
}

/// A fenced block: its info-string language (lowercased, may be empty) and body.
struct Fence<'a> {
    lang: String,
    body: &'a str,
}

/// Split `text` into its ``` fenced blocks, in order. An unclosed final fence
/// runs to the end of the text.
fn fences(text: &str) -> Vec<Fence<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("```") {
        let after = &rest[open + 3..];
        let (info, body_start) = match after.find('\n') {
            Some(nl) => (&after[..nl], nl + 1),
            None => (after, after.len()),
        };
        let lang = info
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let body_all = &after[body_start..];
        // The closing fence must start a line.
        let close = if body_all.starts_with("```") {
            Some(0)
        } else {
            body_all.find("\n```").map(|i| i + 1)
        };
        match close {
            Some(c) => {
                out.push(Fence {
                    lang,
                    body: &body_all[..c],
                });
                let after_close = &body_all[c + 3..];
                // Skip the rest of the closing fence line.
                rest = match after_close.find('\n') {
                    Some(nl) => &after_close[nl + 1..],
                    None => "",
                };
            }
            None => {
                out.push(Fence {
                    lang,
                    body: body_all,
                });
                break;
            }
        }
    }
    out
}

/// Extract the script from a model reply (rule documented at the top of this
/// file). Returns `None` when nothing runnable remains (empty reply/block).
pub fn extract_script(reply: &str) -> Option<(String, Extraction)> {
    let blocks = fences(reply);
    let shell = blocks
        .iter()
        .find(|f| matches!(f.lang.as_str(), "bash" | "sh" | "shell"));
    let chosen = shell.or_else(|| blocks.iter().find(|f| f.lang.is_empty()));
    let (script, how) = match chosen {
        Some(f) => (f.body.to_string(), Extraction::Fenced),
        None if blocks.is_empty() => (reply.trim().to_string(), Extraction::Raw),
        // Only fences in other languages: nothing to run.
        None => return None,
    };
    if script.trim().is_empty() {
        None
    } else {
        Some((script, how))
    }
}

/// Outcome of one generate run.
#[derive(Debug, Clone)]
pub struct GenerateRun {
    /// The model's full text reply.
    pub reply: String,
    /// Extracted script and how it was found; `None` = no script (model failure).
    pub script: Option<(String, Extraction)>,
    /// The single run's output (`None` when no script was produced).
    pub output: Option<ToolOutput>,
    pub timed_out: bool,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Provider round-trip time.
    pub generate_ms: u64,
    /// Script execution time.
    pub exec_ms: u64,
}

/// Concatenate the text blocks of an assistant message.
fn reply_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run `script` once on `bash` under `SCRIPT_TIMEOUT`.
pub async fn run_script_once(bash: &mut Bash, script: &str) -> (ToolOutput, bool) {
    match tokio::time::timeout(SCRIPT_TIMEOUT, bash.exec(script)).await {
        Ok(Ok(r)) => (
            ToolOutput {
                commands: script.to_string(),
                stdout: r.stdout.to_string(),
                stderr: r.stderr.to_string(),
                exit_code: r.exit_code,
            },
            false,
        ),
        Ok(Err(e)) => (
            ToolOutput {
                commands: script.to_string(),
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
            },
            false,
        ),
        Err(_) => (
            ToolOutput {
                commands: script.to_string(),
                stdout: String::new(),
                stderr: format!("script timed out after {}s", SCRIPT_TIMEOUT.as_secs()),
                exit_code: TIMEOUT_EXIT_CODE,
            },
            true,
        ),
    }
}

/// Generate one script for `task` and run it once. `Err` = infra error
/// (provider failure or a broken starting state); everything the model
/// controls is returned as a scored `GenerateRun`.
pub async fn run_generate(provider: &dyn Provider, task: &EvalTask) -> Result<(GenerateRun, Bash)> {
    // Build the starting state first: a broken fixture is infra, and failing
    // here avoids spending a provider call on it.
    let mut bash = build_task_bash(task).await?;

    let messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: task.prompt.clone(),
        }],
    }];
    let start = Instant::now();
    let response = provider
        .chat(&messages, &[], GENERATE_SYSTEM_PROMPT)
        .await?;
    let generate_ms = start.elapsed().as_millis() as u64;

    let reply = reply_text(&response.message);
    let script = extract_script(&reply);

    let (output, timed_out, exec_ms) = match &script {
        Some((s, _)) => {
            let start = Instant::now();
            let (out, timed_out) = run_script_once(&mut bash, s).await;
            (Some(out), timed_out, start.elapsed().as_millis() as u64)
        }
        None => (None, false, 0),
    };

    Ok((
        GenerateRun {
            reply,
            script,
            output,
            timed_out,
            input_tokens: response.input_tokens,
            output_tokens: response.output_tokens,
            generate_ms,
            exec_ms,
        },
        bash,
    ))
}

/// Score-ready state for a run: the Snapshot and the expectation-relevant VFS
/// files. No script → empty snapshot and no files, so every check fails
/// (score 0) through the unchanged scorer.
pub async fn snapshot_run(
    run: &GenerateRun,
    bash: &Bash,
    expectations: &[(String, f64)],
) -> (Snapshot, std::collections::BTreeMap<String, String>) {
    let Some(output) = run.output.clone() else {
        return (Snapshot::default(), Default::default());
    };
    let targets = SnapshotTargets::from_expectations(expectations);
    let fs = bash.fs();
    let (files, dirs) = snapshot_fs(fs.as_ref(), &targets).await;
    let links = snapshot_links(fs.as_ref(), &targets).await;
    let snapshot = Snapshot {
        last_exit_code: Some(output.exit_code),
        tool_outputs: vec![output],
        dirs,
        links,
    };
    (snapshot, files)
}

/// Subject for `bashkit_generate`: one provider call, one script run, then the
/// same Transcript shape as `bash_subject` (VFS files + a single-tool-output
/// `Snapshot`), so `expectations_scorer` scores it unchanged.
pub fn generate_subject() -> impl Subject {
    subject_fn(|sample: Sample, cx| async move {
        let _ = ensure_rustls_crypto_provider();

        let task: EvalTask = match sample.metadata.get("task").cloned() {
            Some(v) => match serde_json::from_value(v) {
                Ok(t) => t,
                Err(e) => return Transcript::infra_error(format!("bad task metadata: {e}")),
            },
            None => return Transcript::infra_error("sample missing task metadata"),
        };
        let provider = match provider_for(&cx.target) {
            Ok(p) => p,
            Err(e) => return Transcript::infra_error(e),
        };
        let (run, bash) = match run_generate(&*provider, &task).await {
            Ok(x) => x,
            Err(e) => return Transcript::infra_error(format!("generate failed: {e:#}")),
        };

        let expectations = expectations_from_sample(&sample);
        let (snapshot, files) = snapshot_run(&run, &bash, &expectations).await;
        generate_transcript(&run, snapshot, files)
    })
}

/// Pack a run into a Transcript (split out so tests can check the shape).
pub fn generate_transcript(
    run: &GenerateRun,
    snapshot: Snapshot,
    files: std::collections::BTreeMap<String, String>,
) -> Transcript {
    let mut t = Transcript::response(run.reply.clone());
    t.iterations = 1;
    if let Some((script, _)) = &run.script {
        t.tool_calls = vec![script.clone()];
        t.tool_calls_count = 1;
    }
    t.usage.input_tokens = run.input_tokens as u64;
    t.usage.output_tokens = run.output_tokens as u64;
    t.timing.duration_ms = run.generate_ms + run.exec_ms;
    t.files = files;
    t.metadata
        .insert(SNAPSHOT_KEY.to_string(), snapshot.to_value());
    t.metadata.insert(
        "generate".to_string(),
        serde_json::json!({
            "extraction": run.script.as_ref().map(|(_, how)| how.as_str()).unwrap_or("none"),
            "timed_out": run.timed_out,
        }),
    );

    let exit_code = run.output.as_ref().map(|o| o.exit_code);
    t.record_metric("script_found", if run.script.is_some() { 1.0 } else { 0.0 });
    // 1 = fenced block, 0 = raw reply; absent when no script was produced.
    if let Some((script, how)) = &run.script {
        t.record_metric(
            "extracted",
            if *how == Extraction::Fenced { 1.0 } else { 0.0 },
        );
        t.record_metric("script_bytes", script.len() as f64);
    }
    if let Some(code) = exit_code {
        t.record_metric("exit_code", code as f64);
    }
    t.record_metric("timed_out", if run.timed_out { 1.0 } else { 0.0 });
    t.record_metric("generate_ms", run.generate_ms as f64);
    t.record_metric("exec_ms", run.exec_ms as f64);
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_bash_fence() {
        let reply = "Here:\n```python\nprint(1)\n```\n```bash\necho hi\n```\n```sh\necho no\n```";
        let (s, how) = extract_script(reply).unwrap();
        assert_eq!(s, "echo hi\n");
        assert_eq!(how, Extraction::Fenced);
    }

    #[test]
    fn tag_is_case_insensitive_and_sh_shell_count() {
        assert_eq!(extract_script("```Bash\nA\n```").unwrap().0, "A\n");
        assert_eq!(extract_script("```sh\nB\n```").unwrap().0, "B\n");
        assert_eq!(extract_script("```shell\nC\n```").unwrap().0, "C\n");
    }

    #[test]
    fn falls_back_to_untagged_fence() {
        let (s, how) = extract_script("text\n```\nls /\n```\nmore").unwrap();
        assert_eq!(s, "ls /\n");
        assert_eq!(how, Extraction::Fenced);
    }

    #[test]
    fn raw_when_no_fence() {
        let (s, how) = extract_script("\n  echo raw\n").unwrap();
        assert_eq!(s, "echo raw");
        assert_eq!(how, Extraction::Raw);
    }

    #[test]
    fn unclosed_fence_runs_to_end() {
        let (s, _) = extract_script("```bash\necho a\necho b").unwrap();
        assert_eq!(s, "echo a\necho b");
    }

    #[test]
    fn inner_backticks_not_at_line_start_do_not_close() {
        let (s, _) = extract_script("```bash\necho '```x'\necho y\n```").unwrap();
        assert_eq!(s, "echo '```x'\necho y\n");
    }

    #[test]
    fn no_script_cases() {
        assert!(extract_script("").is_none());
        assert!(extract_script("   \n ").is_none());
        assert!(extract_script("```bash\n\n```").is_none());
        // Only another language: never run it.
        assert!(extract_script("```python\nprint(1)\n```").is_none());
    }

    // --- dataset + reference solutions --------------------------------------

    use crate::checks::{CheckSummary, evaluate};
    use crate::reference::parse_solutions;
    use std::collections::BTreeSet;

    const TASKS: &str = include_str!("../data/generate-tasks.jsonl");
    const SOLUTIONS: &str = include_str!("../data/generate-solutions.jsonl");

    fn all_tasks() -> Vec<EvalTask> {
        TASKS
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("generate task parses"))
            .collect()
    }

    fn expectations(task: &EvalTask) -> Vec<(String, f64)> {
        task.expectations
            .iter()
            .map(|e| (e.check.clone(), e.weight))
            .collect()
    }

    /// Run `script` once on the task's starting state, exactly like the subject.
    async fn score_script(task: &EvalTask, script: &str) -> (CheckSummary, Snapshot) {
        let mut bash = build_task_bash(task).await.expect("starting state builds");
        let (out, timed_out) = run_script_once(&mut bash, script).await;
        let run = GenerateRun {
            reply: String::new(),
            script: Some((script.to_string(), Extraction::Fenced)),
            output: Some(out),
            timed_out,
            input_tokens: 0,
            output_tokens: 0,
            generate_ms: 0,
            exec_ms: 0,
        };
        let exps = expectations(task);
        let (snap, files) = snapshot_run(&run, &bash, &exps).await;
        (evaluate(&exps, &snap, &files), snap)
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
                "exit {}\nstdout:\n{}stderr:\n{}\n",
                t.exit_code, t.stdout, t.stderr
            ));
        }
        out
    }

    #[test]
    fn dataset_is_valid() {
        let tasks = all_tasks();
        assert!(tasks.len() >= 15, "{} tasks", tasks.len());
        let ids: BTreeSet<_> = tasks.iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids.len(), tasks.len(), "duplicate task ids");
        for t in &tasks {
            assert_eq!(t.mode, "generate", "{}", t.id);
            assert!(
                ["basic", "hard"].contains(&t.difficulty.as_str()),
                "{}",
                t.id
            );
            assert!(t.id.starts_with("gen_"), "{}", t.id);
            // The fixed system prompt is the contract; per-task overrides and
            // turn budgets mean nothing in a one-shot eval.
            assert!(t.system.is_none() && t.max_turns.is_none(), "{}", t.id);
            assert!(!t.expectations.is_empty(), "{}", t.id);
            // The model cannot look, so the prompt must name every file.
            for path in t.files.keys() {
                assert!(t.prompt.contains(path.as_str()), "{}: {path}", t.id);
            }
        }
        let hard = tasks.iter().filter(|t| t.difficulty == "hard").count();
        assert!(hard >= 5 && tasks.len() - hard >= 5, "{hard} hard");
    }

    #[test]
    fn every_task_has_exactly_one_single_step_solution() {
        let sols = parse_solutions(SOLUTIONS).unwrap();
        let sol_ids: BTreeSet<_> = sols.iter().map(|s| s.id.clone()).collect();
        assert_eq!(sol_ids.len(), sols.len(), "duplicate solution ids");
        let task_ids: BTreeSet<_> = all_tasks().into_iter().map(|t| t.id).collect();
        assert_eq!(sol_ids, task_ids);
        for s in &sols {
            assert_eq!(
                s.steps.len(),
                1,
                "{}: generate solutions are one script",
                s.id
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reference_scripts_pass_all_checks() {
        let sols = parse_solutions(SOLUTIONS).unwrap();
        let mut failures = Vec::new();
        for task in all_tasks() {
            let sol = sols.iter().find(|s| s.id == task.id).expect("solution");
            let (summary, snap) = score_script(&task, &sol.steps[0]).await;
            if !summary.all_passed() {
                failures.push(format!("== {} ==\n{}", task.id, report(&summary, &snap)));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn do_nothing_script_fails_every_task() {
        for task in all_tasks() {
            let (summary, snap) = score_script(&task, "true").await;
            assert!(
                !summary.all_passed(),
                "{} passes with `true`:\n{}",
                task.id,
                report(&summary, &snap)
            );
        }
    }

    #[tokio::test]
    async fn no_script_scores_zero_through_the_shared_scorer() {
        let task = all_tasks().remove(0);
        let run = GenerateRun {
            reply: "I cannot help with that.".into(),
            script: None,
            output: None,
            timed_out: false,
            input_tokens: 10,
            output_tokens: 5,
            generate_ms: 1,
            exec_ms: 0,
        };
        let bash = build_task_bash(&task).await.unwrap();
        let (snap, files) = snapshot_run(&run, &bash, &expectations(&task)).await;
        let t = generate_transcript(&run, snap, files);
        assert!(t.error.is_none(), "model failure, not infra");
        let sample = crate::mira_study::bash_samples(TASKS).remove(0);
        let score = crate::mira_study::expectations_scorer()
            .score(&sample, &t)
            .await;
        assert!(!score.pass && !score.na);
        assert_eq!(score.value, 0.0);
    }

    #[tokio::test]
    async fn timeout_or_error_is_recorded_not_raised() {
        let mut bash = Bash::builder().build();
        let (out, timed_out) = run_script_once(&mut bash, "echo ok; exit 3").await;
        assert_eq!(out.stdout, "ok\n");
        assert_eq!(out.exit_code, 3);
        assert!(!timed_out);
    }
}
