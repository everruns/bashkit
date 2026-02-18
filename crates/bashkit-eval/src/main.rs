// bashkit-eval: LLM evaluation harness for bashkit tool usage
// See specs/012-eval.md for design decisions
// Supports multiple eval types: "bash" (original) and "scripting-tool"

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bashkit-eval")]
#[command(about = "Evaluate LLM models using bashkit as a tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run evaluation on a dataset
    Run {
        /// Path to JSONL dataset file
        #[arg(long)]
        dataset: String,

        /// Provider: "anthropic" or "openai"
        #[arg(long)]
        provider: String,

        /// Model name (e.g., "claude-sonnet-4-20250514", "gpt-5.2")
        #[arg(long)]
        model: String,

        /// Eval type: "bash" (default) or "scripting-tool"
        #[arg(long, default_value = "bash")]
        eval_type: String,

        /// Run in baseline mode (scripting-tool only): expose each tool
        /// individually instead of composing them into a ScriptedTool
        #[arg(long)]
        baseline: bool,

        /// Max agent turns per task
        #[arg(long, default_value = "10")]
        max_turns: usize,

        /// Save results to disk (JSON + Markdown)
        #[arg(long)]
        save: bool,

        /// Output directory for saved results
        #[arg(long, default_value = "crates/bashkit-eval/results")]
        output: String,

        /// Custom moniker for identifying this run (default: auto from provider+model)
        #[arg(long)]
        moniker: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            dataset,
            provider,
            model,
            eval_type,
            baseline,
            max_turns,
            save,
            output,
            moniker,
        } => {
            let moniker = moniker.unwrap_or_else(|| {
                let sanitized = model.replace(['/', ':'], "-");
                format!("{}-{}", provider, sanitized)
            });

            match eval_type.as_str() {
                "bash" => {
                    if baseline {
                        anyhow::bail!("--baseline is only valid with --eval-type scripting-tool");
                    }
                    bashkit_eval::runner::run_eval(
                        &dataset, &provider, &model, max_turns, save, &output, &moniker,
                    )
                    .await?;
                }
                "scripting-tool" => {
                    bashkit_eval::scripting_runner::run_scripting_eval(
                        &dataset, &provider, &model, max_turns, baseline, save, &output, &moniker,
                    )
                    .await?;
                }
                other => {
                    anyhow::bail!(
                        "unknown eval type: '{}'. Use 'bash' or 'scripting-tool'",
                        other
                    );
                }
            }
        }
    }

    Ok(())
}
