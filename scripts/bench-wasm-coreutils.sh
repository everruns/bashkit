#!/usr/bin/env bash
# Run the wasm coreutils benchmarks (uutils as WASI guests vs native builtins)
# and save results to
# crates/bashkit/benches/results/criterion-wasm-coreutils-<moniker>-<timestamp>.md.
#
# Two parts:
#   1. Raw start: `wasm_coreutils_startup` example in fresh OS processes
#      (first call in a process, including one-time engine/module load).
#   2. Criterion `wasm_coreutils` bench: per call, throughput on 1 MiB,
#      per session, parallel sessions.
#
# Usage:
#   ./scripts/bench-wasm-coreutils.sh            # run + save
#   RUNS=20 ./scripts/bench-wasm-coreutils.sh
set -euo pipefail

RESULTS_DIR="crates/bashkit/benches/results"
HOSTNAME=$(hostname 2>/dev/null || echo "unknown")
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
CPUS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "?")
TIMESTAMP=$(date +%s)
MONIKER="${MONIKER:-${HOSTNAME}-${OS}-${ARCH}}"
RUNS="${RUNS:-10}"

# Security: private temp file so tee cannot follow a pre-created /tmp symlink.
BENCH_OUT=$(mktemp "${TMPDIR:-/tmp}/criterion-wasm-coreutils-output.XXXXXX")
trap 'rm -f "$BENCH_OUT"' EXIT

echo "Building example..."
cargo build --release -p bashkit --example wasm_coreutils_startup --features wasm-coreutils

summarize() { # LABEL < values  ->  one markdown row (min / median / max)
    sort -n | awk -v k="$1" \
        '{a[NR]=$1} END {printf "| %s | %.3f ms | %.3f ms | %.3f ms |\n", k, a[1], a[int((NR+1)/2)], a[NR]}'
}

echo "Measuring raw start ($RUNS fresh processes)..."
LINES=$(for _ in $(seq "$RUNS"); do ./target/release/examples/wasm_coreutils_startup; done)
STARTUP=$(
    sed -E 's/.*first_call_ms=([0-9.]+).*/\1/' <<<"$LINES" | summarize "wasm, first call (fresh process)"
    sed -E 's/.*second_call_ms=([0-9.]+).*/\1/' <<<"$LINES" | summarize "wasm, second call"
    sed -E 's/.*native_call_ms=([0-9.]+).*/\1/' <<<"$LINES" | summarize "native builtin"
)

echo "Running criterion wasm_coreutils bench..."
cargo bench -p bashkit --bench wasm_coreutils --features wasm-coreutils 2>&1 | tee "$BENCH_OUT"

extract_times() {
    local pattern="$1"
    grep -A3 "$pattern" "$BENCH_OUT" |
        awk -v pat="$pattern" '
            $0 ~ pat {name=$1}
            /time:/ {
                match($0, /\[.*\]/)
                bracket = substr($0, RSTART+1, RLENGTH-2)
                split(bracket, vals, " ")
                t = vals[3] " " vals[4]
            }
            /thrpt:/ {
                match($0, /\[.*\]/)
                bracket = substr($0, RSTART+1, RLENGTH-2)
                split(bracket, vals, " ")
                th = vals[3] " " vals[4]
            }
            t != "" && (/thrpt:/ || !/time:/) {
                printf "| %s | %s | %s |\n", name, t, th
                t = ""; th = ""
            }'
}

section() { # title pattern
    printf '\n## %s\n\n| Benchmark | Time | Throughput |\n|-----------|------|------------|\n' "$1"
    extract_times "$2" | sort -u
}

MD_PATH="${RESULTS_DIR}/criterion-wasm-coreutils-${MONIKER}-${TIMESTAMP}.md"
{
    cat <<HEADER
# Wasm Coreutils Benchmark: uutils (wasm) vs native builtins

Commands through the bashkit interpreter, end to end. \`wasm\` runs uutils
0.12 compiled to wasm32-wasip1 as a WebAssembly guest on wasmtime's Pulley
interpreter (\`coreutils <util>\`); \`native\` is bashkit's own builtin. See
knowledge/runtimes/wasm-coreutils.md.

## System Information

- **Moniker**: \`${MONIKER}\`
- **Hostname**: ${HOSTNAME}
- **OS**: ${OS}
- **Architecture**: ${ARCH}
- **CPUs**: ${CPUS}
- **Timestamp**: ${TIMESTAMP}

## Raw start (fresh OS process, ${RUNS} runs)

First \`coreutils echo 1\` in a new process, timed from \`main\` (includes
building \`Bash\` and the one-time engine and module load).

| Measure | Min | Median | Max |
|---------|-----|--------|-----|
${STARTUP}
HEADER
    section "Per call (warm session, small input)" '^coreutils_call/'
    section "Throughput (1 MiB input)" '^coreutils_throughput/'
    section "Per session (new Bash per call)" '^coreutils_session/'
    section "Parallel sessions (N concurrent, one call each)" '^coreutils_parallel/'
} >"$MD_PATH"

echo ""
echo "Saved: $MD_PATH"
