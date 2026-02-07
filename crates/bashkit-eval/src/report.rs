// Report generation: terminal output and optional disk persistence
// Terminal: per-task PASS/FAIL, summary table
// Disk: JSON results + markdown report

use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::agent::AgentTrace;
use crate::dataset::EvalTask;
use crate::scorer::TaskScore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub task: EvalTask,
    pub trace: AgentTrace,
    pub score: TaskScore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub provider: String,
    pub model: String,
    pub timestamp: String,
    pub max_turns: usize,
    pub results: Vec<EvalResult>,
    pub summary: EvalSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSummary {
    pub total_tasks: usize,
    pub total_passed: usize,
    pub total_score: f64,
    pub total_max_score: f64,
    pub overall_rate: f64,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_turns: usize,
    pub total_tool_calls: usize,
    pub total_duration_ms: u64,
    pub avg_turns_per_task: f64,
    pub avg_tool_calls_per_task: f64,
    pub avg_duration_ms: f64,
    pub by_category: HashMap<String, CategorySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySummary {
    pub tasks: usize,
    pub passed: usize,
    pub score: f64,
    pub max_score: f64,
    pub rate: f64,
}

pub fn build_report(
    provider: &str,
    model: &str,
    max_turns: usize,
    results: &[EvalResult],
) -> EvalReport {
    let total_tasks = results.len();
    let total_passed = results.iter().filter(|r| r.score.all_passed()).count();
    let total_score: f64 = results.iter().map(|r| r.score.score).sum();
    let total_max_score: f64 = results.iter().map(|r| r.score.max_score).sum();
    let overall_rate = if total_max_score > 0.0 {
        total_score / total_max_score
    } else {
        1.0
    };

    let total_input_tokens: u32 = results.iter().map(|r| r.trace.total_input_tokens).sum();
    let total_output_tokens: u32 = results.iter().map(|r| r.trace.total_output_tokens).sum();
    let total_turns: usize = results.iter().map(|r| r.trace.turns).sum();
    let total_tool_calls: usize = results.iter().map(|r| r.trace.tool_call_count).sum();
    let total_duration_ms: u64 = results.iter().map(|r| r.trace.duration_ms).sum();
    let n = total_tasks.max(1) as f64;
    let avg_turns_per_task = total_turns as f64 / n;
    let avg_tool_calls_per_task = total_tool_calls as f64 / n;
    let avg_duration_ms = total_duration_ms as f64 / n;

    let mut by_category: HashMap<String, CategorySummary> = HashMap::new();
    for r in results {
        let entry = by_category
            .entry(r.task.category.clone())
            .or_insert(CategorySummary {
                tasks: 0,
                passed: 0,
                score: 0.0,
                max_score: 0.0,
                rate: 0.0,
            });
        entry.tasks += 1;
        if r.score.all_passed() {
            entry.passed += 1;
        }
        entry.score += r.score.score;
        entry.max_score += r.score.max_score;
    }
    for cat in by_category.values_mut() {
        cat.rate = if cat.max_score > 0.0 {
            cat.score / cat.max_score
        } else {
            1.0
        };
    }

    EvalReport {
        provider: provider.to_string(),
        model: model.to_string(),
        timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        max_turns,
        results: results.to_vec(),
        summary: EvalSummary {
            total_tasks,
            total_passed,
            total_score,
            total_max_score,
            overall_rate,
            total_input_tokens,
            total_output_tokens,
            total_turns,
            total_tool_calls,
            total_duration_ms,
            avg_turns_per_task,
            avg_tool_calls_per_task,
            avg_duration_ms,
            by_category,
        },
    }
}

/// Print summary to terminal
pub fn print_terminal_report(report: &EvalReport) {
    println!();
    println!("=== Eval Report: {}/{} ===", report.provider, report.model);
    println!();

    // Per-task results
    for r in &report.results {
        let status = if r.score.all_passed() { "PASS" } else { "FAIL" };
        println!(
            "  [{}] {} ({}) - {:.0}/{:.0}",
            status, r.task.id, r.task.category, r.score.score, r.score.max_score
        );
    }

    println!();
    println!("--- Summary ---");
    println!(
        "  Tasks: {}/{} passed",
        report.summary.total_passed, report.summary.total_tasks
    );
    println!(
        "  Score: {:.1}/{:.1} ({:.0}%)",
        report.summary.total_score,
        report.summary.total_max_score,
        report.summary.overall_rate * 100.0
    );
    println!(
        "  Turns: {} total, {:.1} avg/task",
        report.summary.total_turns, report.summary.avg_turns_per_task
    );
    println!(
        "  Tool calls: {} total, {:.1} avg/task",
        report.summary.total_tool_calls, report.summary.avg_tool_calls_per_task
    );
    println!(
        "  Tokens: {} input, {} output",
        report.summary.total_input_tokens, report.summary.total_output_tokens
    );
    println!(
        "  Duration: {:.1}s total, {:.1}s avg/task",
        report.summary.total_duration_ms as f64 / 1000.0,
        report.summary.avg_duration_ms / 1000.0
    );

    println!();
    println!("--- By Category ---");
    let mut cats: Vec<_> = report.summary.by_category.iter().collect();
    cats.sort_by_key(|(k, _)| (*k).clone());
    for (cat, summary) in &cats {
        println!(
            "  {:<25} {}/{} tasks  {:.0}%",
            cat,
            summary.passed,
            summary.tasks,
            summary.rate * 100.0
        );
    }
    println!();
}

/// Save JSON + Markdown to disk
/// Filename: eval-{moniker}-{YYYY-MM-DD-HHmmss}.{json,md}
pub fn save_report(report: &EvalReport, output_dir: &str, moniker: &str) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;

    let date = chrono::Utc::now().format("%Y-%m-%d-%H%M%S");
    let base = format!("{}/eval-{}-{}", output_dir, moniker, date);

    // JSON
    let json_path = format!("{}.json", base);
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(&json_path, json)?;
    println!("Saved JSON: {}", json_path);

    // Markdown
    let md_path = format!("{}.md", base);
    let md = generate_markdown(report);
    std::fs::write(&md_path, md)?;
    println!("Saved Markdown: {}", md_path);

    Ok(())
}

fn generate_markdown(report: &EvalReport) -> String {
    let mut md = String::new();

    md.push_str(&format!(
        "# Eval Report: {}/{}\n\n",
        report.provider, report.model
    ));
    md.push_str(&format!("- **Date**: {}\n", report.timestamp));
    md.push_str(&format!("- **Max turns**: {}\n", report.max_turns));
    md.push_str(&format!(
        "- **Turns**: {} total ({:.1} avg/task)\n",
        report.summary.total_turns, report.summary.avg_turns_per_task
    ));
    md.push_str(&format!(
        "- **Tool calls**: {} total ({:.1} avg/task)\n",
        report.summary.total_tool_calls, report.summary.avg_tool_calls_per_task
    ));
    md.push_str(&format!(
        "- **Tokens**: {} input, {} output\n",
        report.summary.total_input_tokens, report.summary.total_output_tokens
    ));
    md.push_str(&format!(
        "- **Duration**: {:.1}s total ({:.1}s avg/task)\n\n",
        report.summary.total_duration_ms as f64 / 1000.0,
        report.summary.avg_duration_ms / 1000.0
    ));

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "**{}/{} tasks passed ({:.0}%)**\n\n",
        report.summary.total_passed,
        report.summary.total_tasks,
        report.summary.overall_rate * 100.0
    ));

    // By category
    md.push_str("## By Category\n\n");
    md.push_str("| Category | Passed | Total | Rate |\n");
    md.push_str("|----------|--------|-------|------|\n");
    let mut cats: Vec<_> = report.summary.by_category.iter().collect();
    cats.sort_by_key(|(k, _)| (*k).clone());
    for (cat, summary) in &cats {
        md.push_str(&format!(
            "| {} | {} | {} | {:.0}% |\n",
            cat,
            summary.passed,
            summary.tasks,
            summary.rate * 100.0
        ));
    }
    md.push('\n');

    // Per-task detail
    md.push_str("## Task Details\n\n");
    for r in &report.results {
        let status = if r.score.all_passed() { "PASS" } else { "FAIL" };
        md.push_str(&format!(
            "### [{}] {} ({})\n\n",
            status, r.task.id, r.task.category
        ));
        md.push_str(&format!("{}\n\n", r.task.description));
        md.push_str(&format!(
            "- Turns: {} | Tool calls: {} | Duration: {:.1}s\n",
            r.trace.turns,
            r.trace.tool_call_count,
            r.trace.duration_ms as f64 / 1000.0
        ));
        md.push_str(&format!(
            "- Tokens: {} input, {} output\n",
            r.trace.total_input_tokens, r.trace.total_output_tokens
        ));
        md.push_str(&format!(
            "- Score: {:.0}/{:.0}\n\n",
            r.score.score, r.score.max_score
        ));

        md.push_str("| Check | Result | Detail |\n");
        md.push_str("|-------|--------|--------|\n");
        for sr in &r.score.results {
            let icon = if sr.passed { "PASS" } else { "FAIL" };
            md.push_str(&format!("| {} | {} | {} |\n", sr.check, icon, sr.detail));
        }
        md.push('\n');
    }

    md
}
