// BashKit Benchmark Tool
// Compares bashkit, bash, and just-bash on:
// - Performance (execution time)
// - Start time (interpreter startup overhead)
// - Error rates (correctness)
//
// Usage: bashkit-bench [OPTIONS]
//   --save [file]     Save results (auto-generates name if not provided)
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
    /// Save results to file (auto-generates name with system info if no path given)
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    save: Option<String>,

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
pub struct SystemInfo {
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub cpus: usize,
}

impl SystemInfo {
    fn collect() -> Self {
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("HOST"))
            .unwrap_or_else(|_| gethostname());

        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1);

        Self {
            hostname,
            os,
            arch,
            cpus,
        }
    }

    fn moniker(&self) -> String {
        // Create short identifier: hostname-os-arch
        let host = self
            .hostname
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(12)
            .collect::<String>()
            .to_lowercase();
        format!("{}-{}-{}", host, self.os, self.arch)
    }
}

fn gethostname() -> String {
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(not(unix))]
    {
        "unknown".to_string()
    }
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
    pub system: SystemInfo,
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

    // Collect system info
    let system_info = SystemInfo::collect();

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
        "  System: {} ({}-{}, {} CPUs)",
        system_info.hostname, system_info.os, system_info.arch, system_info.cpus
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
    let report = generate_report(&results, &args, &runner_names, &system_info);

    // Print results table
    println!("\n{}", "Results:".bold());
    print_results_table(&results);

    // Print summary
    println!("\n{}", "Summary:".bold());
    print_summary(&report.summary);

    // Save if requested
    if let Some(ref save_arg) = args.save {
        let base_name = if save_arg.is_empty() {
            // Auto-generate filename with system moniker and timestamp
            let timestamp = chrono_lite_now();
            format!("bench-{}-{}", system_info.moniker(), timestamp)
        } else {
            // Use provided name, strip extension if present
            let path = PathBuf::from(save_arg);
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("bench-results")
                .to_string()
        };

        let json_path = format!("{}.json", base_name);
        let md_path = format!("{}.md", base_name);

        // Save JSON
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&json_path, json).context("Failed to write JSON results")?;

        // Save Markdown report
        let markdown = generate_markdown_report(&report);
        std::fs::write(&md_path, markdown).context("Failed to write Markdown report")?;

        println!(
            "\n{} results to:\n  - {}\n  - {}",
            "Saved".green(),
            json_path,
            md_path
        );
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

fn generate_report(
    results: &[BenchResult],
    args: &Args,
    runner_names: &[&str],
    system_info: &SystemInfo,
) -> BenchReport {
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
        system: system_info.clone(),
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

fn generate_markdown_report(report: &BenchReport) -> String {
    let mut md = String::new();

    // Header
    md.push_str("# BashKit Benchmark Report\n\n");

    // System info
    md.push_str("## System Information\n\n");
    md.push_str(&format!("- **Hostname**: {}\n", report.system.hostname));
    md.push_str(&format!("- **OS**: {}\n", report.system.os));
    md.push_str(&format!("- **Architecture**: {}\n", report.system.arch));
    md.push_str(&format!("- **CPUs**: {}\n", report.system.cpus));
    md.push_str(&format!("- **Timestamp**: {}\n", report.timestamp));
    md.push_str(&format!("- **Iterations**: {}\n", report.iterations));
    md.push_str(&format!("- **Warmup**: {}\n", report.warmup));
    md.push_str("\n");

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "Benchmarked {} cases across {} runners.\n\n",
        report.summary.total_cases,
        report.runners.len()
    ));

    md.push_str(
        "| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |\n",
    );
    md.push_str(
        "|--------|-----------------|---------------|--------|------------|-------------|\n",
    );

    for runner in &report.runners {
        if let Some(stats) = report.summary.runner_stats.get(runner) {
            md.push_str(&format!(
                "| {} | {:.2} | {:.3} | {} | {:.1}% | {:.1}% |\n",
                runner,
                stats.total_time_ms,
                stats.avg_time_ms,
                stats.error_count,
                stats.error_rate * 100.0,
                stats.output_match_rate * 100.0
            ));
        }
    }
    md.push_str("\n");

    // Performance comparison (if multiple runners)
    if report.runners.len() >= 2 {
        md.push_str("## Performance Comparison\n\n");

        let bashkit_stats = report.summary.runner_stats.get("bashkit");
        let bash_stats = report.summary.runner_stats.get("bash");

        if let (Some(bk), Some(b)) = (bashkit_stats, bash_stats) {
            if bk.avg_time_ms > 0.0 && b.avg_time_ms > 0.0 {
                let speedup = b.avg_time_ms / bk.avg_time_ms;
                if speedup > 1.0 {
                    md.push_str(&format!(
                        "**BashKit is {:.1}x faster** than bash on average.\n\n",
                        speedup
                    ));
                } else {
                    md.push_str(&format!(
                        "**Bash is {:.1}x faster** than bashkit on average.\n\n",
                        1.0 / speedup
                    ));
                }
            }
        }
    }

    // Results by category
    md.push_str("## Results by Category\n\n");

    let mut categories: Vec<_> = report
        .results
        .iter()
        .map(|r| r.category.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    categories.sort();

    for category in categories {
        md.push_str(&format!("### {}\n\n", capitalize(category)));
        md.push_str("| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |\n");
        md.push_str("|-----------|--------|-----------|--------|--------|-------|\n");

        let cat_results: Vec<_> = report
            .results
            .iter()
            .filter(|r| r.category == category)
            .collect();

        for result in cat_results {
            md.push_str(&format!(
                "| {} | {} | {:.3} | ±{:.3} | {} | {} |\n",
                result.case_name,
                result.runner,
                result.mean_ns / 1_000_000.0,
                result.stddev_ns / 1_000_000.0,
                if result.errors > 0 {
                    result.errors.to_string()
                } else {
                    "-".to_string()
                },
                if result.output_match { "✓" } else { "✗" }
            ));
        }
        md.push_str("\n");
    }

    // Assumptions
    md.push_str("## Assumptions & Notes\n\n");
    md.push_str("- Times measured in nanoseconds, displayed in milliseconds\n");
    md.push_str("- Warmup iterations excluded from timing\n");
    md.push_str("- Output match compares against bash output when available\n");
    md.push_str("- Errors include execution failures and exit code mismatches\n");
    md.push_str("- BashKit runs in-process (no fork), bash spawns subprocess\n");
    md.push_str("\n");

    md
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn chrono_lite_now() -> String {
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
