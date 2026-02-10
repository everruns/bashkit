# Bashkit Benchmark

Benchmark tool for comparing Bashkit against native bash and just-bash interpreters.

## Overview

This tool measures and compares:
- **Performance**: Execution time for various shell operations
- **Start time**: Interpreter startup overhead
- **Error rates**: Correctness compared to bash output

## Usage

```bash
# Run benchmarks (bashkit vs bash)
cargo run -p bashkit-bench --release

# Auto-generate results with system identifier
cargo run -p bashkit-bench --release -- --save

# Use custom moniker for CI environments
cargo run -p bashkit-bench --release -- --save --moniker ci-4cpu-8gb

# Save to specific file
cargo run -p bashkit-bench --release -- --save my-results

# Run with all available interpreters
cargo run -p bashkit-bench --release -- --runners bashkit,bash,just-bash

# Run specific category
cargo run -p bashkit-bench --release -- --category tools

# Filter by benchmark name
cargo run -p bashkit-bench --release -- --filter grep

# High accuracy run
cargo run -p bashkit-bench --release -- --iterations 50 --warmup 5

# Skip prewarming phase
cargo run -p bashkit-bench --release -- --no-prewarm

# List available benchmarks
cargo run -p bashkit-bench --release -- --list
```

## Options

| Option | Description |
|--------|-------------|
| `--save [file]` | Save results to JSON and Markdown. Auto-generates filename with moniker if not provided |
| `--moniker <id>` | Custom system identifier (e.g., `ci-4cpu-8gb`, `macbook-m1`) |
| `--runners <list>` | Comma-separated runners: `bashkit`, `bash`, `just-bash` (default: `bashkit,bash`) |
| `--filter <name>` | Run only benchmarks matching substring |
| `--category <cat>` | Run only specific category |
| `--iterations <n>` | Iterations per benchmark (default: 10) |
| `--warmup <n>` | Per-benchmark warmup iterations (default: 2) |
| `--no-prewarm` | Skip prewarming phase |
| `--verbose` | Show per-benchmark timing details |
| `--list` | List available benchmarks |

## Benchmark Categories

| Category | Description | Cases |
|----------|-------------|-------|
| `startup` | Interpreter startup overhead | 4 |
| `variables` | Variable assignment, expansion, defaults | 8 |
| `arithmetic` | Math operations, loops | 6 |
| `control` | if/else, for, while, case, functions | 9 |
| `strings` | Printf, concatenation, case conversion | 8 |
| `arrays` | Array creation, iteration, slicing | 6 |
| `pipes` | Pipelines, heredocs, redirects | 6 |
| `tools` | grep, sed, awk, jq operations | 21 |
| `complex` | Fibonacci, JSON transforms, pipelines | 7 |

## Output Files

When using `--save`, two files are generated:

1. **JSON** (`bench-{system}-{timestamp}.json`): Machine-readable results with all timing data
2. **Markdown** (`bench-{system}-{timestamp}.md`): Human-readable report with tables and summary

The system moniker includes hostname, OS, and architecture (e.g., `myhost-linux-x86_64`).

## Assumptions & Methodology

### Timing
- Times are measured in nanoseconds using `std::time::Instant`
- Each benchmark runs `warmup` iterations (not timed) followed by `iterations` timed runs
- Statistics include mean, stddev, min, and max

### Output Matching
- When bash is available, its output is used as the reference
- Bashkit output is compared after normalizing whitespace
- Mismatches are flagged but don't affect timing

### Error Handling
- Execution failures count as errors with 1000ms penalty time
- Exit code mismatches count as errors
- Up to 3 error messages are captured per benchmark

### Performance Context
- **Bashkit runs in-process**: No fork/exec overhead, shared memory
- **Bash spawns subprocess**: ~8-10ms startup per invocation
- This explains why Bashkit appears 100-1000x faster for simple operations

### What This Measures
- Bashkit: Pure interpreter performance (parsing + execution)
- Bash: Full process lifecycle (fork + exec + parsing + execution + exit)

### What This Doesn't Measure
- Real-world script performance with I/O
- Memory usage
- Concurrent execution
- Long-running scripts (resource limits apply)

## Known Limitations

Some bash features are not implemented in Bashkit:
- `:` (null command) - use `true` instead
- `set -e` (errexit)
- `trap` signal handling
- Brace expansion `{a,b,c}`
- Process substitution `<(cmd)`

Benchmarks are designed to use compatible subset of features.

## Interpreters

### Bashkit
The Rust-based virtual interpreter being benchmarked. Runs in-process without subprocess overhead.

### Bash
System bash (`/bin/bash` or similar). Spawns a new process for each benchmark, which includes fork/exec overhead.

### just-bash
[Vercel's just-bash](https://github.com/vercel-labs/just-bash) virtual interpreter. Optional - will be skipped if not installed.

Install via: `npm install -g just-bash`

## Example Output

```
Running 75 benchmarks with 2 runner(s): bashkit, bash
  System: myhost (linux-x86_64, 8 CPUs)
  Iterations: 10, Warmup: 2

  ▶ [startup] startup_empty
  ▶ [startup] startup_echo
  ...

Results:
+----------+---------------+---------+-----------+--------+-------+
| Category | Benchmark     | Runner  | Mean (ms) | StdDev | Match |
+----------+---------------+---------+-----------+--------+-------+
| startup  | startup_empty | bashkit | 0.004     | ±0.001 | ✓     |
| startup  | startup_empty | bash    | 9.123     | ±0.456 | ✓     |
+----------+---------------+---------+-----------+--------+-------+

Summary:
  bashkit:
    Total time:      1.23 ms
    Avg per case:    0.016 ms
    Error rate:      0.0%
    Output match:    100.0%

  bash:
    Total time:      891.45 ms
    Avg per case:    11.886 ms
    Error rate:      0.0%
    Output match:    100.0%
```

## Development

```bash
# Build
cargo build -p bashkit-bench --release

# Run tests (if any)
cargo test -p bashkit-bench

# Format
cargo fmt -p bashkit-bench
```
