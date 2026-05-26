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

### In-process / persistent-child lineup (vm, 4 CPUs, 2026-05-26)

96 cases, 10 iterations. Apples-to-apples — interpreter cost only, no
per-call process spawn (except `bash`, kept as the cold-start reference).

| Runner | Avg/Case (ms) | Total (ms) | vs bashkit | Errors | Output Match |
|--------|--------------:|-----------:|-----------:|-------:|-------------:|
| bashkit            | 0.457 |    43.85 |     1x | 0 | 100% |
| gbash-server v0.0.38 | 6.286 |   603.49 |  13.8x | 130 (13.5%) | 86.5% |
| just-bash-inproc 3.0.1 | 7.055 |   677.26 |  15.4x | 0 | 100% |
| bash 5.2.21 (subprocess) | 9.277 |   890.56 |  20.3x | 0 | 100% |

Bashkit speedup (geometric mean / median across 96 cases):

| vs | geo-mean | median |
|----|---------:|-------:|
| bash | 24.7x | 31.1x |
| just-bash-inproc | 25.4x | 34.2x |
| gbash-server | 17.6x | 18.8x (N=83 — gbash failed 13 awk/jq cases with exit 127) |

Subprocess-mode lineup (`just-bash` CLI 380 ms/case, `gbash` CLI 12.6 ms/case)
is dominated by per-call Node/Go startup, not interpreter cost — see commit
`2223a72` for the raw numbers if you need the subprocess view. The in-process
runners above are the fair comparison for steady-state workloads.

### Historical: bashkit vs bash (runsc, 16 CPUs, 2026-04-13)

96 cases, 10 iterations, **107.2x faster** overall. 0 errors, 100% output match.
(Higher headline number than the vm run above because runsc + 16 CPUs makes
host `bash`'s per-process spawn much more expensive — bashkit avoids spawn
entirely, so its lead widens.)

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

# Run with default runners (bashkit + bash)
cargo run -p bashkit-bench --release

# Apples-to-apples cross-runtime comparison (warm interpreter, no per-call fork)
# Use the in-process / persistent-child runners — not the *-cli / just-bash / gbash
# subprocess runners, which mostly measure Node/Go process startup.
cargo run -p bashkit-bench --release -- \
  --runners bashkit,bashkit-js,bashkit-py,just-bash-inproc,gbash-server,bash \
  --save --verbose

# Run every available runner (mix of in-process, persistent-child, subprocess)
cargo run -p bashkit-bench --release -- \
  --runners bashkit,bashkit-cli,bashkit-js,bashkit-py,bash,gbash,gbash-server,just-bash,just-bash-inproc \
  --save --verbose

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

Historical results are stored in `results/` as
`bench-<runner>-<moniker>-<timestamp>.{json,md}`.

> **Not for criterion output.** Criterion `.md` files produced by
> `cargo bench --bench <name>` against `crates/bashkit/benches/` live in
> `crates/bashkit/benches/results/` instead. Don't mix the two streams.
