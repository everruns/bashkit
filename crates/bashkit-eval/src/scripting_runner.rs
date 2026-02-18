// Runner for scripting tool evals.
// Loads scripting dataset → runs scripted or baseline agent per task → scores → reports.

use anyhow::Result;

use crate::provider::create_provider;
use crate::scorer;
use crate::scripting_agent::{run_baseline_agent, run_scripted_agent, ScriptingTrace};
use crate::scripting_dataset::load_scripting_dataset;
use crate::scripting_report::{
    build_scripting_report, print_scripting_terminal_report, save_scripting_report,
    ScriptingEvalResult,
};

#[allow(clippy::too_many_arguments)]
pub async fn run_scripting_eval(
    dataset_path: &str,
    provider_name: &str,
    model: &str,
    max_turns: usize,
    baseline: bool,
    save: bool,
    output_dir: &str,
    moniker: &str,
) -> Result<()> {
    let tasks = load_scripting_dataset(dataset_path)?;
    let provider = create_provider(provider_name, model)?;

    let mode = if baseline { "baseline" } else { "scripted" };
    println!(
        "Running {} scripting-tool tasks ({} mode) with {}/{}  (max_turns={})",
        tasks.len(),
        mode,
        provider_name,
        model,
        max_turns
    );
    println!();

    let mut results = Vec::new();

    for (i, task) in tasks.iter().enumerate() {
        println!(
            "[{}/{}] {} - {} (tools: {})",
            i + 1,
            tasks.len(),
            task.id,
            task.description,
            task.tools.len()
        );

        let run_result = if baseline {
            run_baseline_agent(&*provider, task, max_turns).await
        } else {
            run_scripted_agent(&*provider, task, max_turns).await
        };

        match run_result {
            Ok(trace) => {
                // Score using shared scorer via a compatibility shim
                let compat_trace = trace_to_agent_trace(&trace);
                let fs = bashkit::InMemoryFs::new();
                let score =
                    scorer::score_task(&task.id, &compat_trace, &fs, &task.expectations).await;

                for sr in &score.results {
                    let icon = if sr.passed { "PASS" } else { "FAIL" };
                    println!("  [{}] {} - {}", icon, sr.check, sr.detail);
                }
                let calls_ok = trace
                    .tool_calls
                    .iter()
                    .filter(|tc| tc.exit_code == 0)
                    .count();
                let calls_err = trace.tool_call_count - calls_ok;
                println!(
                    "  Score: {:.0}/{:.0} | Turns: {} | Calls: {} ({} ok, {} err) | Tokens: {}in/{}out | Raw output: {} bytes | {:.1}s",
                    score.score,
                    score.max_score,
                    trace.turns,
                    trace.tool_call_count,
                    calls_ok,
                    calls_err,
                    trace.total_input_tokens,
                    trace.total_output_tokens,
                    trace.raw_tool_output_bytes,
                    trace.duration_ms as f64 / 1000.0,
                );
                println!();

                results.push(ScriptingEvalResult {
                    task: task.clone(),
                    trace,
                    score,
                });
            }
            Err(e) => {
                println!("  ERROR: {:#}", e);
                println!();
            }
        }
    }

    let report = build_scripting_report(provider_name, model, max_turns, baseline, &results);
    print_scripting_terminal_report(&report);

    if save {
        save_scripting_report(&report, output_dir, moniker)?;
    }

    Ok(())
}

/// Convert ScriptingTrace → AgentTrace for reusing the scorer.
fn trace_to_agent_trace(trace: &ScriptingTrace) -> crate::agent::AgentTrace {
    use crate::agent::ToolCallResult;

    let tool_calls: Vec<ToolCallResult> = trace
        .tool_calls
        .iter()
        .map(|tc| ToolCallResult {
            commands: serde_json::to_string(&tc.input).unwrap_or_default(),
            stdout: tc.output.clone(),
            stderr: String::new(),
            exit_code: tc.exit_code,
        })
        .collect();

    let last = tool_calls.last().cloned();
    let count = tool_calls.len();

    crate::agent::AgentTrace {
        messages: trace.messages.clone(),
        tool_calls,
        tool_call_count: count,
        turns: trace.turns,
        last_tool_response: last,
        natural_stop: trace.natural_stop,
        total_input_tokens: trace.total_input_tokens,
        total_output_tokens: trace.total_output_tokens,
        duration_ms: trace.duration_ms,
    }
}
