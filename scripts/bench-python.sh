#!/usr/bin/env bash
# Run the Python runtime benchmarks (CPython wasm vs Monty) and save results to
# crates/bashkit/benches/results/criterion-python-<moniker>-<timestamp>.md.
#
# Three parts:
#   1. Raw start: `python_startup` example in fresh OS processes (first call in
#      a process, including one-time runtime load), RUNS times per runtime.
#   2. Criterion `python` bench: per call, per session, compute,
#      parallel sessions.
#   3. Load: `cpython_load` example, many concurrent tenants in one process.
#
# Usage:
#   ./scripts/bench-python.sh            # run + save
#   RUNS=20 LOAD_LEVELS="1 64 1024" ./scripts/bench-python.sh
set -euo pipefail

RESULTS_DIR="crates/bashkit/benches/results"
HOSTNAME=$(hostname 2>/dev/null || echo "unknown")
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
CPUS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "?")
TIMESTAMP=$(date +%s)
MONIKER="${HOSTNAME}-${OS}-${ARCH}"
RUNS="${RUNS:-10}"
LOAD_LEVELS="${LOAD_LEVELS:-1 16 64 256 1024}"

# Security: private temp file so tee cannot follow a pre-created /tmp symlink.
BENCH_OUT=$(mktemp "${TMPDIR:-/tmp}/criterion-python-output.XXXXXX")
trap 'rm -f "$BENCH_OUT"' EXIT

echo "Building examples..."
cargo build --release -p bashkit --example python_startup --example cpython_load \
    --features python,cpython

# summarize RUNTIME LABEL < values  ->  one markdown row (min / median / max)
summarize() {
    sort -n | awk -v r="$1" -v k="$2" \
        '{a[NR]=$1} END {printf "| %s | %s | %.3f ms | %.3f ms | %.3f ms |\n", r, k, a[1], a[int((NR+1)/2)], a[NR]}'
}

startup_rows() {
    local runtime="$1" lines
    lines=$(for _ in $(seq "$RUNS"); do ./target/release/examples/python_startup "$runtime"; done)
    sed -E 's/.*first_call_ms=([0-9.]+).*/\1/' <<<"$lines" | summarize "$runtime" "first call (fresh process)"
    sed -E 's/.*second_call_ms=([0-9.]+).*/\1/' <<<"$lines" | summarize "$runtime" "second call"
}

echo "Measuring raw start ($RUNS fresh processes per runtime)..."
STARTUP=$(startup_rows monty; startup_rows cpython)

echo "Running criterion python bench..."
cargo bench -p bashkit --bench python --features python,cpython 2>&1 | tee "$BENCH_OUT"

echo "Running load test ($LOAD_LEVELS)..."
# shellcheck disable=SC2086 # word-split levels on purpose
LOAD=$(./target/release/examples/cpython_load $LOAD_LEVELS)

extract_times() {
    local pattern="$1"
    grep -A2 "$pattern" "$BENCH_OUT" |
        awk -v pat="$pattern" '
            $0 ~ pat {name=$1}
            /time:/ {
                match($0, /\[.*\]/)
                bracket = substr($0, RSTART+1, RLENGTH-2)
                split(bracket, vals, " ")
                printf "| %s | %s %s |\n", name, vals[3], vals[4]
            }'
}

section() { # title pattern
    printf '\n## %s\n\n| Benchmark | Time |\n|-----------|------|\n' "$1"
    extract_times "$2"
}

MD_PATH="${RESULTS_DIR}/criterion-python-${MONIKER}-${TIMESTAMP}.md"
{
    cat <<HEADER
# Python Runtimes Benchmark: CPython (wasm) vs Monty

\`python3\` through the bashkit interpreter, end to end. CPython 3.14 runs as a
WebAssembly guest on wasmtime's Pulley interpreter, from a pre-initialized
snapshot; Monty runs natively in-process. See
knowledge/runtimes/cpython-wasm.md.

## System Information

- **Moniker**: \`${MONIKER}\`
- **Hostname**: ${HOSTNAME}
- **OS**: ${OS}
- **Architecture**: ${ARCH}
- **CPUs**: ${CPUS}
- **Timestamp**: ${TIMESTAMP}

## Raw start (fresh OS process, ${RUNS} runs)

First \`python3 -c 'print(1)'\` in a new process, timed from \`main\`
(includes building \`Bash\` and any one-time runtime load).

| Runtime | Measure | Min | Median | Max |
|---------|---------|-----|--------|-----|
${STARTUP}
HEADER
    section "Per call (warm session)" '^python_call/'
    section "Per session (new Bash per call)" '^python_session/'
    section "Compute (CPU-bound Python)" '^python_compute/'
    section "Parallel sessions (N concurrent, one call each)" '^python_parallel/'
    printf '\n## Load (CPython, concurrent tenants, 4 calls each, one process)\n\n%s\n' "$LOAD"
} >"$MD_PATH"

echo ""
echo "Saved: $MD_PATH"
