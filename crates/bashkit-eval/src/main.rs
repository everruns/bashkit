// bashkit-eval: LLM evaluation harness for bashkit tool usage
// See specs/012-eval.md for design decisions

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

        /// Model name (e.g., "claude-sonnet-4-20250514", "gpt-4o")
        #[arg(long)]
        model: String,

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
            max_turns,
            save,
            output,
            moniker,
        } => {
            let moniker = moniker.unwrap_or_else(|| {
                let sanitized = model.replace(['/', ':'], "-");
                format!("{}-{}", provider, sanitized)
            });
            bashkit_eval::runner::run_eval(
                &dataset, &provider, &model, max_turns, save, &output, &moniker,
            )
            .await?;
        }
    }

    Ok(())
}
