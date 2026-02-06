// Runner: orchestrates the full eval pipeline
// Load dataset → run agent per task → score → report

use anyhow::Result;

use crate::agent::run_agent_loop;
use crate::dataset::load_dataset;
use crate::provider::create_provider;
use crate::report::{self, EvalResult};
use crate::scorer;

pub async fn run_eval(
    dataset_path: &str,
    provider_name: &str,
    model: &str,
    max_turns: usize,
    save: bool,
    output_dir: &str,
) -> Result<()> {
    let tasks = load_dataset(dataset_path)?;
    let provider = create_provider(provider_name, model)?;

    println!(
        "Running {} tasks with {}/{}  (max_turns={})",
        tasks.len(),
        provider_name,
        model,
        max_turns
    );
    println!();

    let mut results = Vec::new();

    for (i, task) in tasks.iter().enumerate() {
        println!(
            "[{}/{}] {} - {}",
            i + 1,
            tasks.len(),
            task.id,
            task.description
        );

        match run_agent_loop(&*provider, task, max_turns).await {
            Ok((trace, bash)) => {
                let fs = bash.fs();
                let score = scorer::score_task(&task.id, &trace, &*fs, &task.expectations).await;

                for sr in &score.results {
                    let icon = if sr.passed { "PASS" } else { "FAIL" };
                    println!("  [{}] {} - {}", icon, sr.check, sr.detail);
                }
                println!(
                    "  Score: {:.0}/{:.0} | Tokens: {}in/{}out | Calls: {}",
                    score.score,
                    score.max_score,
                    trace.total_input_tokens,
                    trace.total_output_tokens,
                    trace.tool_call_count,
                );
                println!();

                results.push(EvalResult {
                    task: task.clone(),
                    trace,
                    score,
                });
            }
            Err(e) => {
                println!("  ERROR: {}", e);
                println!();
            }
        }
    }

    let eval_report = report::build_report(provider_name, model, max_turns, &results);
    report::print_terminal_report(&eval_report);

    if save {
        report::save_report(&eval_report, output_dir)?;
    }

    Ok(())
}
