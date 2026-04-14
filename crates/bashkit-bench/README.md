# bashkit-bench

Benchmark tool for comparing bashkit against bash and just-bash across multiple execution models.

## Runners

| Runner | Type | Description |
|--------|------|-------------|
| `bashkit` | in-process | Rust library call, no fork/exec |
| `bashkit-cli` | subprocess | bashkit binary, new process per run |
| `bashkit-js` | persistent child | Node.js + @everruns/bashkit, warm interpreter |
| `bashkit-py` | persistent child | Python + bashkit package, warm interpreter |
| `bash` | subprocess | /bin/bash, new process per run |
| `gbash` | subprocess | gbash binary (Go), new process per run |
| `gbash-server` | persistent child | gbash JSON-RPC server, warm interpreter |
| `just-bash` | subprocess | just-bash CLI, new process per run |
| `just-bash-inproc` | persistent child | Node.js + just-bash library, warm interpreter |

**In-process**: interpreter runs inside the benchmark process (fastest, no IPC overhead).
**Persistent child**: long-lived child process communicates via JSON lines over stdin/stdout; interpreter startup paid once.
**Subprocess**: new process spawned per benchmark run; measures full startup + execution.

## Latest Results

96 benchmarks across 12 categories. All runners: **0 errors, 100% output match**.

| Runner | Avg/Case (ms) | Total (ms) | vs bashkit |
|--------|--------------|-----------|------------|
| bashkit | 0.345 | 33.11 | 1x |
| bashkit-py | 0.513 | 49.28 | 1.5x |
| bashkit-js | 0.646 | 61.97 | 1.9x |
| just-bash-inproc | 4.458 | 428.01 | 12.9x |
| bashkit-cli | 8.186 | 785.83 | 23.7x |
| bash | 8.204 | 787.61 | 23.8x |
| just-bash | 367.538 | 35,283.69 | 1,065x |

### Apples-to-apples comparisons

**In-process (warm interpreter):**
bashkit-js (0.65ms) vs just-bash-inproc (4.46ms) — bashkit is **6.9x faster** in the same execution model (Node.js persistent child).

**Subprocess (cold start):**
bashkit-cli (8.19ms) vs bash (8.20ms) — **roughly equivalent**; both dominated by process spawn overhead.
bashkit-cli (8.19ms) vs just-bash (367.5ms) — bashkit is **44.9x faster**; just-bash pays ~360ms Node.js boot per invocation.

### Latest bashkit vs bash (runsc, 16 CPUs, 2026-04-13)

96 cases, 10 iterations, **107.2x faster** overall. 0 errors, 100% output match.

| Benchmark | bashkit | bash | Speedup | Description |
|-----------|---------|------|---------|-------------|
| startup_echo | 0.07ms | 8.4ms | 120x | Minimal overhead |
| large_fibonacci_12 | 10.6ms | 1,416ms | 133x | Recursive computation |
| large_loop_1000 | 4.3ms | 11.1ms | 2.6x | Sustained iteration |
| large_function_calls_500 | 5.0ms | 1,232ms | 246x | Function call overhead |
| complex_pipeline_text | 0.33ms | 24.0ms | 73x | grep + sed pipeline |
| tool_jq_filter | 0.64ms | 28.5ms | 44x | jq JSON processing |

## Benchmark Categories

| Category | Cases | Description |
|----------|-------|-------------|
| `startup` | 4 | Interpreter startup overhead |
| `variables` | 8 | Variable assignment and expansion |
| `arithmetic` | 6 | Math operations |
| `control` | 9 | Loops, conditionals, functions |
| `strings` | 8 | String manipulation |
| `arrays` | 6 | Array operations |
| `pipes` | 6 | Pipelines and redirections |
| `tools` | 21 | grep, sed, awk, jq |
| `complex` | 7 | Real-world scripts |
| `large` | 9 | Sustained execution, large scripts |
| `subshell` | 6 | Subshell isolation and nesting |
| `io` | 6 | File I/O and redirections |

## Usage

```bash
# Build
cargo build -p bashkit-bench --release

# Run with all runners
cargo run -p bashkit-bench --release -- \
  --runners bashkit,bashkit-cli,bashkit-js,bashkit-py,bash,just-bash,just-bash-inproc \
  --save --verbose

# Run with default runners (bashkit + bash)
cargo run -p bashkit-bench --release

# Filter by category or name
cargo run -p bashkit-bench --release -- --category large --verbose
cargo run -p bashkit-bench --release -- --filter fibonacci --verbose

# High accuracy run
cargo run -p bashkit-bench --release -- --iterations 50 --warmup 5

# List available benchmarks
cargo run -p bashkit-bench --release -- --list
```

## Options

| Option | Description |
|--------|-------------|
| `--save [file]` | Save results to JSON and Markdown (auto-generates filename if not provided) |
| `--moniker <id>` | Custom system identifier (e.g., `ci-4cpu-8gb`) |
| `--runners <list>` | Comma-separated runners (default: `bashkit,bash`) |
| `--filter <name>` | Run only benchmarks matching substring |
| `--category <cat>` | Run only specific category |
| `--iterations <n>` | Iterations per benchmark (default: 10) |
| `--warmup <n>` | Per-benchmark warmup iterations (default: 2) |
| `--no-prewarm` | Skip prewarming phase |
| `--verbose` | Show per-benchmark timing details |
| `--list` | List available benchmarks |

## Prerequisites

| Runner | Setup |
|--------|-------|
| `bashkit` | Built automatically (in-process) |
| `bashkit-cli` | `cargo build -p bashkit-cli --release` |
| `bashkit-js` | `cd crates/bashkit-js && npm install && npm run build` |
| `bashkit-py` | `maturin build --release && pip install target/wheels/bashkit-*.whl` |
| `bash` | Pre-installed on most systems |
| `gbash` | `go install github.com/ewhauser/gbash/cmd/gbash@latest` |
| `gbash-server` | Same as gbash (uses JSON-RPC server mode) |
| `just-bash` | `npm install -g just-bash` |
| `just-bash-inproc` | Same as just-bash (uses library API) |

## Methodology

- Times measured in nanoseconds using `std::time::Instant`, displayed in milliseconds
- Each benchmark: warmup iterations (not timed) → timed iterations → statistics (mean, stddev, min, max)
- Prewarm phase runs first 3 cases to warm up JIT/compilation before actual benchmarks
- Output compared against bash reference output; mismatches flagged but don't affect timing
- Benchmarks run sequentially — no parallel execution competing for resources
- Execution failures count as errors with 1000ms penalty time

## Output Files

When using `--save`, two files are generated in the working directory:

1. **JSON** (`bench-{moniker}-{timestamp}.json`): Machine-readable results
2. **Markdown** (`bench-{moniker}-{timestamp}.md`): Human-readable report

Historical results are stored in `results/`.
