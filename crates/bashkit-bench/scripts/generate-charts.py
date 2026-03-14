#!/usr/bin/env python3
"""Generate benchmark comparison charts from results JSON."""

import json
import sys
import os
from collections import defaultdict

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np


def load_results(path):
    with open(path) as f:
        return json.load(f)


def make_summary_chart(data, out_dir, skip_runners=None):
    """Bar chart: average ms per case for each runner."""
    skip = set(skip_runners or [])
    stats = data["summary"]["runner_stats"]
    runners = [(r, s) for r, s in stats.items() if r not in skip]
    runners.sort(key=lambda x: x[1]["avg_time_ms"])

    names = [r for r, _ in runners]
    avgs = [s["avg_time_ms"] for _, s in runners]

    colors = {
        "bashkit": "#2563eb",
        "bashkit-py": "#7c3aed",
        "bashkit-js": "#059669",
        "bashkit-cli": "#6366f1",
        "bash": "#d97706",
        "just-bash-inproc": "#dc2626",
        "just-bash": "#991b1b",
    }

    fig, ax = plt.subplots(figsize=(10, 5))
    bars = ax.barh(names, avgs, color=[colors.get(n, "#666") for n in names])
    ax.set_xlabel("Average time per case (ms)", fontsize=12)
    ax.set_title("Benchmark: Average Execution Time per Case", fontsize=14, fontweight="bold")
    ax.xaxis.set_major_formatter(ticker.FormatStrFormatter("%.1f"))

    for bar, val in zip(bars, avgs):
        ax.text(bar.get_width() + max(avgs) * 0.01, bar.get_y() + bar.get_height() / 2,
                f"{val:.3f} ms", va="center", fontsize=10)

    ax.set_xlim(0, max(avgs) * 1.25)
    plt.tight_layout()
    path = os.path.join(out_dir, "chart-summary.png")
    fig.savefig(path, dpi=150)
    plt.close()
    print(f"Saved: {path}")


def make_category_chart(data, out_dir, skip_runners=None):
    """Grouped bar chart: average ms by category for each runner."""
    skip = set(skip_runners or [])
    results = [r for r in data["results"] if r["runner"] not in skip]

    # Group by category and runner
    cat_runner = defaultdict(lambda: defaultdict(list))
    for r in results:
        cat_runner[r["category"]][r["runner"]].append(r["mean_ns"] / 1_000_000)

    categories = sorted(cat_runner.keys())
    runners = sorted({r["runner"] for r in results})

    cat_avgs = {}
    for cat in categories:
        cat_avgs[cat] = {}
        for runner in runners:
            vals = cat_runner[cat].get(runner, [])
            cat_avgs[cat][runner] = np.mean(vals) if vals else 0

    colors = {
        "bashkit": "#2563eb",
        "bashkit-py": "#7c3aed",
        "bashkit-js": "#059669",
        "bashkit-cli": "#6366f1",
        "bash": "#d97706",
        "just-bash-inproc": "#dc2626",
    }

    x = np.arange(len(categories))
    width = 0.13
    n = len(runners)

    fig, ax = plt.subplots(figsize=(14, 6))
    for i, runner in enumerate(runners):
        vals = [cat_avgs[cat].get(runner, 0) for cat in categories]
        offset = (i - n / 2 + 0.5) * width
        ax.bar(x + offset, vals, width, label=runner, color=colors.get(runner, "#666"))

    ax.set_xlabel("Category", fontsize=12)
    ax.set_ylabel("Average time per case (ms)", fontsize=12)
    ax.set_title("Benchmark: Average Time by Category", fontsize=14, fontweight="bold")
    ax.set_xticks(x)
    ax.set_xticklabels(categories, rotation=45, ha="right")
    ax.legend(loc="upper left", fontsize=9)
    ax.set_yscale("log")
    ax.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.1f"))
    plt.tight_layout()
    path = os.path.join(out_dir, "chart-by-category.png")
    fig.savefig(path, dpi=150)
    plt.close()
    print(f"Saved: {path}")


def make_large_benchmark_chart(data, out_dir, skip_runners=None):
    """Horizontal bar chart for 'large' category — shows compute-heavy workloads."""
    skip = set(skip_runners or [])
    results = [r for r in data["results"]
               if r["category"] == "large" and r["runner"] not in skip]

    benchmarks = sorted({r["case_name"] for r in results})
    runners = sorted({r["runner"] for r in results})

    colors = {
        "bashkit": "#2563eb",
        "bashkit-py": "#7c3aed",
        "bashkit-js": "#059669",
        "bashkit-cli": "#6366f1",
        "bash": "#d97706",
        "just-bash-inproc": "#dc2626",
    }

    y = np.arange(len(benchmarks))
    height = 0.12
    n = len(runners)

    fig, ax = plt.subplots(figsize=(12, 7))
    for i, runner in enumerate(runners):
        vals = []
        for bench in benchmarks:
            match = [r for r in results if r["case_name"] == bench and r["runner"] == runner]
            vals.append(match[0]["mean_ns"] / 1_000_000 if match else 0)
        offset = (i - n / 2 + 0.5) * height
        ax.barh(y + offset, vals, height, label=runner, color=colors.get(runner, "#666"))

    ax.set_ylabel("Benchmark", fontsize=12)
    ax.set_xlabel("Time (ms)", fontsize=12)
    ax.set_title("Large Benchmarks: Execution Time", fontsize=14, fontweight="bold")
    ax.set_yticks(y)
    ax.set_yticklabels(benchmarks)
    ax.legend(loc="lower right", fontsize=9)
    ax.set_xscale("log")
    ax.xaxis.set_major_formatter(ticker.FormatStrFormatter("%.1f"))
    plt.tight_layout()
    path = os.path.join(out_dir, "chart-large-benchmarks.png")
    fig.savefig(path, dpi=150)
    plt.close()
    print(f"Saved: {path}")


def make_speedup_chart(data, out_dir, skip_runners=None):
    """Speedup vs bash for in-process runners on key benchmarks."""
    skip = set(skip_runners or [])
    key_benchmarks = [
        "startup_echo", "arith_loop_sum", "ctrl_nested_loops",
        "tool_grep_simple", "tool_sed_replace", "tool_jq_filter",
        "complex_fibonacci", "large_fibonacci_12", "large_function_calls_500",
        "large_loop_1000", "complex_pipeline_text", "large_multiline_script",
    ]
    compare_runners = ["bashkit", "bashkit-js", "bashkit-py", "just-bash-inproc"]
    compare_runners = [r for r in compare_runners if r not in skip]

    # Get bash baseline
    bash_times = {}
    for r in data["results"]:
        if r["runner"] == "bash" and r["case_name"] in key_benchmarks:
            bash_times[r["case_name"]] = r["mean_ns"] / 1_000_000

    colors = {
        "bashkit": "#2563eb",
        "bashkit-py": "#7c3aed",
        "bashkit-js": "#059669",
        "just-bash-inproc": "#dc2626",
    }

    benchmarks = [b for b in key_benchmarks if b in bash_times]
    x = np.arange(len(benchmarks))
    width = 0.18
    n = len(compare_runners)

    fig, ax = plt.subplots(figsize=(14, 6))
    for i, runner in enumerate(compare_runners):
        speedups = []
        for bench in benchmarks:
            match = [r for r in data["results"]
                     if r["case_name"] == bench and r["runner"] == runner]
            if match and match[0]["mean_ns"] > 0:
                runner_ms = match[0]["mean_ns"] / 1_000_000
                speedups.append(bash_times[bench] / runner_ms)
            else:
                speedups.append(0)
        offset = (i - n / 2 + 0.5) * width
        ax.bar(x + offset, speedups, width, label=runner, color=colors.get(runner, "#666"))

    ax.set_xlabel("Benchmark", fontsize=12)
    ax.set_ylabel("Speedup vs bash (×)", fontsize=12)
    ax.set_title("Speedup Over Bash (higher = faster)", fontsize=14, fontweight="bold")
    ax.set_xticks(x)
    ax.set_xticklabels([b.replace("_", "\n", 1) for b in benchmarks], rotation=45, ha="right", fontsize=8)
    ax.axhline(y=1, color="gray", linestyle="--", alpha=0.5, label="bash baseline")
    ax.legend(loc="upper right", fontsize=9)
    ax.set_yscale("log")
    ax.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.0f"))
    plt.tight_layout()
    path = os.path.join(out_dir, "chart-speedup-vs-bash.png")
    fig.savefig(path, dpi=150)
    plt.close()
    print(f"Saved: {path}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: generate-charts.py <results.json> [output_dir]")
        sys.exit(1)

    results_path = sys.argv[1]
    out_dir = sys.argv[2] if len(sys.argv) > 2 else os.path.dirname(results_path)
    skip = {"just-bash"}

    data = load_results(results_path)
    os.makedirs(out_dir, exist_ok=True)

    make_summary_chart(data, out_dir, skip)
    make_category_chart(data, out_dir, skip)
    make_large_benchmark_chart(data, out_dir, skip)
    make_speedup_chart(data, out_dir, skip)
    print("Done!")
