// Mira integration: turns the bashkit eval into a mira `Study`.
//
// Three pieces wire bashkit into mira (github.com/everruns/mira):
//
//   1. Samples  — each JSONL `EvalTask` / `ScriptingEvalTask` becomes a mira
//      `Sample`. The full task rides in `sample.metadata["task"]` (the subject's
//      source of truth) and its `expectations` in `sample.metadata["expectations"]`
//      (the scorer's source of truth).
//   2. Subject  — `bash_subject` / `scripting_subject` run bashkit's existing
//      agent loop against the case's target model, then pack the result into a
//      `Transcript` (VFS files + a `Snapshot` of tool outputs / directories).
//   3. Scorer   — `expectations_scorer` replays the deterministic bashkit checks
//      against that Transcript. A case passes iff every check passes (mirrors the
//      old `TaskScore::all_passed`); the score value is the weighted pass rate.
//
// The model matrix, scheduling, retries, and reporting are owned by the `mira`
// host CLI. See specs/eval.md.

use mira::subject::{Subject, subject_fn};
use mira::{Eval, Sample, Score, Target, Transcript};

use crate::agent::run_agent_loop;
use crate::checks::evaluate;
use crate::dataset::EvalTask;
use crate::provider::{
    AnthropicProvider, ContentBlock, Message, OpenAiProvider, OpenAiResponsesProvider, Provider,
    Role, ensure_rustls_crypto_provider,
};
use crate::scripting_agent::{ScriptingTrace, run_baseline_agent, run_scripted_agent};
use crate::scripting_dataset::ScriptingEvalTask;
use crate::snapshot::{Snapshot, ToolOutput, snapshot_fs};

/// Default agent-turn budget per task (matches the original harness).
pub const MAX_TURNS: usize = 10;

/// Default model matrix. Each target is gated on its provider's API-key env var,
/// so an offline run skips them all (CI stays green) and a keyed run lights up
/// the subset whose credentials are present. Select subsets with
/// `mira run --targets anthropic/claude-opus-4-8` (exact labels, comma-separated).
pub fn default_targets() -> Vec<Target> {
    vec![
        Target::anthropic("claude-opus-4-8"),
        Target::anthropic("claude-haiku-4-5"),
        Target::anthropic("claude-sonnet-4-6"),
        Target::openai("gpt-5.5"),
        // Codex models require the OpenAI Responses API; route on a custom
        // provider id our subject understands, gated on the OpenAI key.
        Target::cloud("openresponses", "gpt-5.3-codex", "OPENAI_API_KEY"),
    ]
}

/// Map a mira `Target` to a bashkit `Provider`. Errors surface as infra errors
/// (the case scores N/A rather than failing the model).
fn provider_for(target: &Target) -> Result<Box<dyn Provider>, String> {
    let model = target.model.as_str();
    let provider: Box<dyn Provider> = match target.provider.as_str() {
        "anthropic" => Box::new(AnthropicProvider::new(model).map_err(|e| e.to_string())?),
        "openai" => Box::new(OpenAiProvider::new(model).map_err(|e| e.to_string())?),
        "openresponses" => {
            Box::new(OpenAiResponsesProvider::new(model).map_err(|e| e.to_string())?)
        }
        other => return Err(format!("unsupported provider for bashkit eval: {other}")),
    };
    Ok(provider)
}

/// Pull the last assistant text out of a conversation, for `final_response`.
fn last_assistant_text(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)
        .map(|m| {
            m.content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Read the per-sample expectation list the scorer evaluates.
fn expectations_from_sample(sample: &Sample) -> Vec<(String, f64)> {
    sample
        .metadata
        .get("expectations")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let check = e.get("check")?.as_str()?.to_string();
                    let weight = e.get("weight").and_then(|w| w.as_f64()).unwrap_or(1.0);
                    Some((check, weight))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Serialize a task's `expectations` array for sample metadata.
fn expectations_value(expectations: &[crate::dataset::Expectation]) -> serde_json::Value {
    serde_json::json!(
        expectations
            .iter()
            .map(|e| serde_json::json!({"check": e.check, "weight": e.weight}))
            .collect::<Vec<_>>()
    )
}

// ---------------------------------------------------------------------------
// Bash eval (the original 58-task agent eval)
// ---------------------------------------------------------------------------

/// Build the samples for a bash eval dataset (parsed JSONL text).
fn bash_samples(jsonl: &str) -> Vec<Sample> {
    parse_jsonl::<EvalTask>(jsonl)
        .into_iter()
        .map(|task| {
            let mut sample = Sample::new(&task.id, &task.prompt)
                .tag(&task.category)
                .meta("category", task.category.clone())
                .meta("description", task.description.clone())
                .meta("expectations", expectations_value(&task.expectations))
                .meta("task", serde_json::to_value(&task).unwrap_or_default());
            for (path, content) in &task.files {
                sample = sample.file(path, content);
            }
            sample
        })
        .collect()
}

/// Subject for the bash eval: run bashkit's agent loop, snapshot the VFS.
fn bash_subject() -> impl Subject {
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

        let (trace, bash) = match run_agent_loop(&*provider, &task, cx.max_turns).await {
            Ok(x) => x,
            Err(e) => return Transcript::infra_error(format!("agent loop failed: {e:#}")),
        };

        let fs = bash.fs();
        let (files, dirs) = snapshot_fs(fs.as_ref()).await;

        let tool_outputs: Vec<ToolOutput> = trace
            .tool_calls
            .iter()
            .map(|t| ToolOutput {
                commands: t.commands.clone(),
                stdout: t.stdout.clone(),
                stderr: t.stderr.clone(),
                exit_code: t.exit_code,
            })
            .collect();
        let snapshot = Snapshot {
            tool_outputs,
            last_exit_code: trace.last_tool_response.as_ref().map(|r| r.exit_code),
            dirs,
        };

        let ok = trace.tool_calls.iter().filter(|t| t.exit_code == 0).count();
        let err = trace.tool_call_count.saturating_sub(ok);

        let mut t = Transcript::response(last_assistant_text(&trace.messages));
        t.iterations = trace.turns;
        t.tool_calls = trace
            .tool_calls
            .iter()
            .map(|c| c.commands.clone())
            .collect();
        t.tool_calls_count = trace.tool_call_count;
        t.usage.input_tokens = trace.total_input_tokens as u64;
        t.usage.output_tokens = trace.total_output_tokens as u64;
        t.timing.duration_ms = trace.duration_ms;
        t.files = files;
        t.metadata.insert(
            crate::snapshot::SNAPSHOT_KEY.to_string(),
            snapshot.to_value(),
        );
        t.record_metric("turns", trace.turns as f64);
        t.record_metric("tool_calls", trace.tool_call_count as f64);
        t.record_metric("tool_calls_ok", ok as f64);
        t.record_metric("tool_calls_err", err as f64);
        t.record_metric("natural_stop", if trace.natural_stop { 1.0 } else { 0.0 });
        t
    })
}

// ---------------------------------------------------------------------------
// Scripting-tool eval (ScriptedTool orchestration vs. baseline tools)
// ---------------------------------------------------------------------------

/// Build the samples for the scripting-tool eval from several dataset texts.
/// Each `(label, jsonl)` pair tags its samples with the dataset label.
fn scripting_samples(datasets: &[(&str, &str)]) -> Vec<Sample> {
    let mut samples = Vec::new();
    for (label, jsonl) in datasets {
        for task in parse_jsonl::<ScriptingEvalTask>(jsonl) {
            let sample = Sample::new(&task.id, &task.prompt)
                .tag(&task.category)
                .tag(*label)
                .meta("category", task.category.clone())
                .meta("dataset", (*label).to_string())
                .meta("description", task.description.clone())
                .meta("expectations", expectations_value(&task.expectations))
                .meta(
                    "scripting_task",
                    serde_json::to_value(&task).unwrap_or_default(),
                );
            samples.push(sample);
        }
    }
    samples
}

/// Subject for the scripting-tool eval. The `mode` axis (`scripted`/`baseline`)
/// selects whether mock tools are composed into one `ScriptedTool` or exposed
/// individually.
fn scripting_subject() -> impl Subject {
    subject_fn(|sample: Sample, cx| async move {
        let _ = ensure_rustls_crypto_provider();

        let task: ScriptingEvalTask = match sample.metadata.get("scripting_task").cloned() {
            Some(v) => match serde_json::from_value(v) {
                Ok(t) => t,
                Err(e) => return Transcript::infra_error(format!("bad task metadata: {e}")),
            },
            None => return Transcript::infra_error("sample missing scripting_task metadata"),
        };

        let baseline = cx.param("mode") == Some("baseline");

        let provider = match provider_for(&cx.target) {
            Ok(p) => p,
            Err(e) => return Transcript::infra_error(e),
        };

        let result = if baseline {
            run_baseline_agent(&*provider, &task, cx.max_turns).await
        } else {
            run_scripted_agent(&*provider, &task, cx.max_turns).await
        };
        let trace: ScriptingTrace = match result {
            Ok(t) => t,
            Err(e) => return Transcript::infra_error(format!("agent loop failed: {e:#}")),
        };

        scripting_transcript(trace)
    })
}

fn scripting_transcript(trace: ScriptingTrace) -> Transcript {
    use bashkit::ScriptedCommandKind;

    let tool_outputs: Vec<ToolOutput> = trace
        .tool_calls
        .iter()
        .map(|tc| ToolOutput {
            commands: serde_json::to_string(&tc.input).unwrap_or_default(),
            stdout: tc.output.clone(),
            stderr: String::new(),
            exit_code: tc.exit_code,
        })
        .collect();
    let last_exit_code = trace.tool_calls.last().map(|tc| tc.exit_code);
    let snapshot = Snapshot {
        tool_outputs,
        last_exit_code,
        dirs: Vec::new(),
    };

    let ok = trace
        .tool_calls
        .iter()
        .filter(|tc| tc.exit_code == 0)
        .count();
    let err = trace.tool_call_count.saturating_sub(ok);
    let inner_total = trace.inner_command_count();
    let inner_tool = trace.inner_command_count_by_kind(ScriptedCommandKind::Tool);
    let inner_help = trace.inner_command_count_by_kind(ScriptedCommandKind::Help);
    let inner_discover = trace.inner_command_count_by_kind(ScriptedCommandKind::Discover);

    let mut t = Transcript::response(last_assistant_text(&trace.messages));
    t.iterations = trace.turns;
    t.tool_calls = trace
        .tool_calls
        .iter()
        .map(|tc| tc.tool_name.clone())
        .collect();
    t.tool_calls_count = trace.tool_call_count;
    t.usage.input_tokens = trace.total_input_tokens as u64;
    t.usage.output_tokens = trace.total_output_tokens as u64;
    t.timing.duration_ms = trace.duration_ms;
    t.metadata.insert(
        crate::snapshot::SNAPSHOT_KEY.to_string(),
        snapshot.to_value(),
    );
    t.record_metric("turns", trace.turns as f64);
    t.record_metric("tool_calls", trace.tool_call_count as f64);
    t.record_metric("tool_calls_ok", ok as f64);
    t.record_metric("tool_calls_err", err as f64);
    t.record_metric("baseline", if trace.baseline { 1.0 } else { 0.0 });
    t.record_metric("inner_commands", inner_total as f64);
    t.record_metric("inner_tool", inner_tool as f64);
    t.record_metric("inner_help", inner_help as f64);
    t.record_metric("inner_discover", inner_discover as f64);
    t.record_metric("raw_tool_output_bytes", trace.raw_tool_output_bytes as f64);
    t.record_metric(
        "tool_output_sent_bytes",
        trace.tool_output_sent_bytes as f64,
    );
    t
}

// ---------------------------------------------------------------------------
// Shared scorer
// ---------------------------------------------------------------------------

/// The bashkit expectations scorer: replays the deterministic checks for a
/// sample against its run Transcript. Pass = every check passes; value = the
/// weighted pass rate (partial credit surfaces in aggregate reporting).
pub fn expectations_scorer() -> Box<dyn mira::scorer::Scorer> {
    mira::scorer::scorer("bashkit_expectations", |sample: &Sample, t: &Transcript| {
        // Subject bailed out (infra) — nothing to score.
        if t.error.is_some() {
            return Score::na("bashkit_expectations", "subject error");
        }
        let snapshot = Snapshot::from_metadata(&t.metadata).unwrap_or_default();
        let expectations = expectations_from_sample(sample);
        if expectations.is_empty() {
            return Score::na("bashkit_expectations", "no expectations");
        }
        let summary = evaluate(&expectations, &snapshot, &t.files);
        let passed = summary.results.iter().filter(|r| r.passed).count();
        let reason = format!(
            "{}/{} checks passed (weighted rate {:.0}%)",
            passed,
            summary.results.len(),
            summary.rate() * 100.0
        );
        Score {
            scorer: "bashkit_expectations".to_string(),
            value: summary.rate(),
            pass: summary.all_passed(),
            na: false,
            reason,
        }
    })
}

// ---------------------------------------------------------------------------
// JSONL parsing (lenient: skips blanks and `#` / `//` comment lines)
// ---------------------------------------------------------------------------

fn parse_jsonl<T: serde::de::DeserializeOwned>(text: &str) -> Vec<T> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .filter_map(|l| serde_json::from_str::<T>(l).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// Eval definitions — discovered by the `mira` host via `#[eval]`.
// ---------------------------------------------------------------------------

/// Embedded datasets (no runtime path dependence — robust under any cwd).
const EVAL_TASKS: &str = include_str!("../data/eval-tasks.jsonl");
const SMOKE_TASKS: &str = include_str!("../data/smoke-test.jsonl");
const SCRIPTING_MANY_TOOLS: &str = include_str!("../data/scripting-tool/many-tools.jsonl");
const SCRIPTING_DISCOVERY: &str = include_str!("../data/scripting-tool/discovery.jsonl");
const SCRIPTING_PAGINATED: &str = include_str!("../data/scripting-tool/paginated.jsonl");
const SCRIPTING_LARGE_OUTPUT: &str = include_str!("../data/scripting-tool/large-output.jsonl");

/// The main bash agent eval: 58 hand-curated tasks across 15 categories.
/// Select a category with `mira run --tag json_processing`. The `#[eval]`
/// registration wrapper lives in `main.rs` (the bin crate) so the inventory
/// submission is guaranteed to link into the host binary.
pub fn bash_eval() -> Eval {
    let mut b = Eval::new("bashkit_bash")
        .describe("LLM bash-tool usage across 15 task categories (bashkit VFS)")
        .subject(bash_subject())
        .scorer(expectations_scorer())
        .max_turns(MAX_TURNS)
        .targets(default_targets());
    for sample in bash_samples(EVAL_TASKS) {
        b = b.add_sample(sample);
    }
    b.build()
}

/// A 3-task smoke eval for quick verification.
pub fn smoke_eval() -> Eval {
    let mut b = Eval::new("bashkit_smoke")
        .describe("3-task bashkit smoke eval")
        .subject(bash_subject())
        .scorer(expectations_scorer())
        .max_turns(MAX_TURNS)
        .targets(default_targets());
    for sample in bash_samples(SMOKE_TASKS) {
        b = b.add_sample(sample);
    }
    b.build()
}

/// The scripting-tool orchestration eval. The `mode` axis compares composing
/// mock tools into one `ScriptedTool` (`scripted`) against exposing each tool
/// individually (`baseline`). Select with `mira run --axis mode=scripted`.
pub fn scripting_eval() -> Eval {
    let datasets = [
        ("many-tools", SCRIPTING_MANY_TOOLS),
        ("discovery", SCRIPTING_DISCOVERY),
        ("paginated", SCRIPTING_PAGINATED),
        ("large-output", SCRIPTING_LARGE_OUTPUT),
    ];
    let mut b = Eval::new("bashkit_scripting")
        .describe("ScriptedTool orchestration vs. baseline individual tools")
        .subject(scripting_subject())
        .scorer(expectations_scorer())
        .max_turns(MAX_TURNS)
        .axis("mode", ["scripted", "baseline"])
        .targets(default_targets());
    for sample in scripting_samples(&datasets) {
        b = b.add_sample(sample);
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_samples_load_all_tasks_with_expectations() {
        let samples = bash_samples(EVAL_TASKS);
        assert_eq!(samples.len(), 58);
        for s in &samples {
            assert!(
                !expectations_from_sample(s).is_empty(),
                "task {} has no expectations",
                s.id
            );
            assert!(
                s.metadata.contains_key("task"),
                "task {} missing task meta",
                s.id
            );
        }
    }

    #[test]
    fn smoke_samples_load() {
        assert_eq!(bash_samples(SMOKE_TASKS).len(), 3);
    }

    #[test]
    fn scripting_samples_load_all_datasets() {
        let datasets = [
            ("many-tools", SCRIPTING_MANY_TOOLS),
            ("discovery", SCRIPTING_DISCOVERY),
            ("paginated", SCRIPTING_PAGINATED),
            ("large-output", SCRIPTING_LARGE_OUTPUT),
        ];
        let samples = scripting_samples(&datasets);
        assert!(!samples.is_empty());
        for s in &samples {
            assert!(s.metadata.contains_key("scripting_task"));
            assert!(!expectations_from_sample(s).is_empty());
        }
    }

    #[test]
    fn evals_build() {
        // Smoke: the eval builders produce valid Evals without panicking.
        let _ = bash_eval();
        let _ = smoke_eval();
        let _ = scripting_eval();
    }
}
