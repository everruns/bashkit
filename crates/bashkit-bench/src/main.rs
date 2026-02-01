// BashKit Benchmark Tool
// Compares bashkit, bash, and just-bash on:
// - Performance (execution time)
// - Start time (interpreter startup overhead)
// - Error rates (correctness)
//
// Usage: bashkit-bench [OPTIONS]
//   --save <file>     Save results to JSON file
//   --runners <list>  Comma-separated: bashkit,bash,just-bash (default: all available)
//   --filter <name>   Run only benchmarks matching name
//   --iterations <n>  Iterations per benchmark (default: 10)
//   --warmup <n>      Warmup iterations (default: 2)

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tabled::{Table, Tabled};

mod cases;
mod runners;

use cases::BenchCase;
use runners::{BashRunner, BashkitRunner, JustBashRunner, Runner};

#[derive(Parser, Debug)]
#[command(name = "bashkit-bench")]
#[command(about = "Benchmark bashkit against bash and just-bash")]
struct Args {
    /// Save results to JSON file
    #[arg(long)]
    save: Option<PathBuf>,

    /// Runners to use (comma-separated: bashkit,bash,just-bash)
    #[arg(long, default_value = "bashkit,bash")]
    runners: String,

    /// Filter benchmarks by name (substring match)
    #[arg(long)]
    filter: Option<String>,

    /// Number of iterations per benchmark
    #[arg(long, default_value = "10")]
    iterations: usize,

    /// Number of warmup iterations
    #[arg(long, default_value = "2")]
    warmup: usize,

    /// List available benchmarks without running
    #[arg(long)]
    list: bool,

    /// Run only specific category
    #[arg(long)]
    category: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub runner: String,
    pub case_name: String,
    pub category: String,
    pub iterations: usize,
    pub times_ns: Vec<u128>,
    pub mean_ns: f64,
    pub stddev_ns: f64,
    pub min_ns: u128,
    pub max_ns: u128,
    pub errors: usize,
    pub error_messages: Vec<String>,
    pub output_match: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    pub timestamp: String,
    pub iterations: usize,
    pub warmup: usize,
    pub runners: Vec<String>,
    pub results: Vec<BenchResult>,
    pub summary: BenchSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchSummary {
    pub total_cases: usize,
    pub runner_stats: HashMap<String, RunnerStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerStats {
    pub total_time_ms: f64,
    pub avg_time_ms: f64,
    pub error_count: usize,
    pub error_rate: f64,
    pub output_match_rate: f64,
}

#[derive(Tabled)]
struct ResultRow {
    #[tabled(rename = "Category")]
    category: String,
    #[tabled(rename = "Benchmark")]
    name: String,
    #[tabled(rename = "Runner")]
    runner: String,
    #[tabled(rename = "Mean (ms)")]
    mean_ms: String,
    #[tabled(rename = "StdDev")]
    stddev: String,
    #[tabled(rename = "Min")]
    min_ms: String,
    #[tabled(rename = "Max")]
    max_ms: String,
    #[tabled(rename = "Errors")]
    errors: String,
    #[tabled(rename = "Match")]
    output_match: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Get all benchmark cases
    let all_cases = cases::all_cases();

    // Filter cases
    let cases: Vec<_> = all_cases
        .into_iter()
        .filter(|c| {
            if let Some(ref filter) = args.filter {
                c.name.contains(filter)
            } else {
                true
            }
        })
        .filter(|c| {
            if let Some(ref cat) = args.category {
                c.category.as_str().eq_ignore_ascii_case(cat)
            } else {
                true
            }
        })
        .collect();

    if args.list {
        println!("{}", "Available benchmarks:".bold());
        let mut current_cat = String::new();
        for case in &cases {
            if case.category.as_str() != current_cat {
                current_cat = case.category.as_str().to_string();
                println!("\n  {}:", current_cat.cyan());
            }
            println!("    {} - {}", case.name.green(), case.description);
        }
        return Ok(());
    }

    // Initialize runners
    let runner_names: Vec<&str> = args.runners.split(',').map(|s| s.trim()).collect();
    let mut runners: Vec<Runner> = Vec::new();

    for name in &runner_names {
        match *name {
            "bashkit" => {
                runners.push(BashkitRunner::new().await?);
            }
            "bash" => {
                if let Ok(r) = BashRunner::new().await {
                    runners.push(r);
                } else {
                    eprintln!("{}: bash not available", "Warning".yellow());
                }
            }
            "just-bash" => {
                if let Ok(r) = JustBashRunner::new().await {
                    runners.push(r);
                } else {
                    eprintln!("{}: just-bash not available", "Warning".yellow());
                }
            }
            _ => eprintln!("{}: unknown runner '{}'", "Warning".yellow(), name),
        }
    }

    if runners.is_empty() {
        anyhow::bail!("No runners available");
    }

    println!(
        "\n{} {} benchmarks with {} runner(s): {}",
        "Running".bold().green(),
        cases.len(),
        runners.len(),
        runners
            .iter()
            .map(|r| r.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  Iterations: {}, Warmup: {}\n",
        args.iterations, args.warmup
    );

    // Get reference output from bash if available
    let bash_runner = runners.iter().find(|r| r.name() == "bash");

    // Run benchmarks
    let mut results: Vec<BenchResult> = Vec::new();

    for case in &cases {
        println!(
            "  {} [{}] {}",
            "▶".blue(),
            case.category.as_str().cyan(),
            case.name.bold()
        );

        // Get expected output from bash (if available) or use case.expected
        let expected_output = if let Some(bash) = bash_runner {
            match bash.run(&case.script).await {
                Ok((out, _, _)) => Some(out),
                Err(_) => case.expected.clone(),
            }
        } else {
            case.expected.clone()
        };

        for runner in &runners {
            let result = run_benchmark(runner, case, &expected_output, &args).await;

            let status = if result.errors > 0 {
                format!("{} errors", result.errors).red().to_string()
            } else if !result.output_match {
                "mismatch".yellow().to_string()
            } else {
                "ok".green().to_string()
            };

            if args.verbose {
                println!(
                    "    {}: {:.3}ms ± {:.3}ms [{}]",
                    runner.name(),
                    result.mean_ns as f64 / 1_000_000.0,
                    result.stddev_ns / 1_000_000.0,
                    status
                );
            }

            results.push(result);
        }
    }

    // Generate report
    let report = generate_report(&results, &args, &runner_names);

    // Print results table
    println!("\n{}", "Results:".bold());
    print_results_table(&results);

    // Print summary
    println!("\n{}", "Summary:".bold());
    print_summary(&report.summary);

    // Save if requested
    if let Some(ref path) = args.save {
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(path, json).context("Failed to write results")?;
        println!("\n{} results to {}", "Saved".green(), path.display());
    }

    Ok(())
}

async fn run_benchmark(
    runner: &Runner,
    case: &BenchCase,
    expected: &Option<String>,
    args: &Args,
) -> BenchResult {
    let mut times_ns: Vec<u128> = Vec::new();
    let mut errors = 0;
    let mut error_messages: Vec<String> = Vec::new();
    let mut last_output = String::new();

    // Warmup
    for _ in 0..args.warmup {
        let _ = runner.run(&case.script).await;
    }

    // Timed runs
    for _ in 0..args.iterations {
        let start = Instant::now();
        let result = runner.run(&case.script).await;
        let elapsed = start.elapsed();

        match result {
            Ok((stdout, _stderr, exit_code)) => {
                times_ns.push(elapsed.as_nanos());
                last_output = stdout;

                // Check for expected exit code (default 0)
                if exit_code != case.expected_exit.unwrap_or(0) {
                    errors += 1;
                    if error_messages.len() < 3 {
                        error_messages.push(format!(
                            "exit code {} (expected {})",
                            exit_code,
                            case.expected_exit.unwrap_or(0)
                        ));
                    }
                }
            }
            Err(e) => {
                errors += 1;
                // Use a penalty time for errors
                times_ns.push(Duration::from_millis(1000).as_nanos());
                if error_messages.len() < 3 {
                    error_messages.push(e.to_string());
                }
            }
        }
    }

    // Check output match
    let output_match = match expected {
        Some(exp) => normalize_output(&last_output) == normalize_output(exp),
        None => true,
    };

    // Calculate statistics
    let mean_ns = times_ns.iter().sum::<u128>() as f64 / times_ns.len() as f64;
    let variance = times_ns
        .iter()
        .map(|&t| (t as f64 - mean_ns).powi(2))
        .sum::<f64>()
        / times_ns.len() as f64;
    let stddev_ns = variance.sqrt();
    let min_ns = *times_ns.iter().min().unwrap_or(&0);
    let max_ns = *times_ns.iter().max().unwrap_or(&0);

    BenchResult {
        runner: runner.name().to_string(),
        case_name: case.name.clone(),
        category: case.category.as_str().to_string(),
        iterations: args.iterations,
        times_ns,
        mean_ns,
        stddev_ns,
        min_ns,
        max_ns,
        errors,
        error_messages,
        output_match,
    }
}

fn normalize_output(s: &str) -> String {
    s.trim().replace("\r\n", "\n")
}

fn generate_report(results: &[BenchResult], args: &Args, runner_names: &[&str]) -> BenchReport {
    let mut runner_stats: HashMap<String, RunnerStats> = HashMap::new();

    for name in runner_names {
        let runner_results: Vec<_> = results.iter().filter(|r| r.runner == *name).collect();

        let total_time_ms: f64 = runner_results.iter().map(|r| r.mean_ns / 1_000_000.0).sum();
        let avg_time_ms = if !runner_results.is_empty() {
            total_time_ms / runner_results.len() as f64
        } else {
            0.0
        };
        let error_count: usize = runner_results.iter().map(|r| r.errors).sum();
        let total_runs: usize = runner_results.iter().map(|r| r.iterations).sum();
        let error_rate = if total_runs > 0 {
            error_count as f64 / total_runs as f64
        } else {
            0.0
        };
        let match_count = runner_results.iter().filter(|r| r.output_match).count();
        let output_match_rate = if !runner_results.is_empty() {
            match_count as f64 / runner_results.len() as f64
        } else {
            0.0
        };

        runner_stats.insert(
            name.to_string(),
            RunnerStats {
                total_time_ms,
                avg_time_ms,
                error_count,
                error_rate,
                output_match_rate,
            },
        );
    }

    let unique_cases: std::collections::HashSet<_> = results.iter().map(|r| &r.case_name).collect();

    BenchReport {
        timestamp: chrono_lite_now(),
        iterations: args.iterations,
        warmup: args.warmup,
        runners: runner_names.iter().map(|s| s.to_string()).collect(),
        results: results.to_vec(),
        summary: BenchSummary {
            total_cases: unique_cases.len(),
            runner_stats,
        },
    }
}

fn chrono_lite_now() -> String {
    // Simple timestamp without chrono dependency
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

fn print_results_table(results: &[BenchResult]) {
    let rows: Vec<ResultRow> = results
        .iter()
        .map(|r| ResultRow {
            category: r.category.clone(),
            name: r.case_name.clone(),
            runner: r.runner.clone(),
            mean_ms: format!("{:.3}", r.mean_ns as f64 / 1_000_000.0),
            stddev: format!("±{:.3}", r.stddev_ns / 1_000_000.0),
            min_ms: format!("{:.3}", r.min_ns as f64 / 1_000_000.0),
            max_ms: format!("{:.3}", r.max_ns as f64 / 1_000_000.0),
            errors: if r.errors > 0 {
                format!("{}", r.errors)
            } else {
                "-".to_string()
            },
            output_match: if r.output_match { "✓" } else { "✗" }.to_string(),
        })
        .collect();

    let table = Table::new(rows).to_string();
    println!("{}", table);
}

fn print_summary(summary: &BenchSummary) {
    println!("  Total benchmark cases: {}", summary.total_cases);
    println!();

    for (runner, stats) in &summary.runner_stats {
        println!("  {}:", runner.bold());
        println!("    Total time:      {:.2} ms", stats.total_time_ms);
        println!("    Avg per case:    {:.3} ms", stats.avg_time_ms);
        println!("    Error count:     {}", stats.error_count);
        println!("    Error rate:      {:.1}%", stats.error_rate * 100.0);
        println!(
            "    Output match:    {:.1}%",
            stats.output_match_rate * 100.0
        );
        println!();
    }
}
