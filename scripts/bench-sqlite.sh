#!/usr/bin/env bash
# Run the Criterion sqlite benchmark and save results to
# crates/bashkit/benches/results/, next to the bench source.
#
# Usage:
#   ./scripts/bench-sqlite.sh                  # run + save
#   ./scripts/bench-sqlite.sh --dry OUTPUT     # parse trusted output without re-running
set -euo pipefail

RESULTS_DIR="crates/bashkit/benches/results"
HOSTNAME=$(hostname 2>/dev/null || echo "unknown")
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
CPUS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "?")
TIMESTAMP=$(date +%s)
MONIKER="${HOSTNAME}-${OS}-${ARCH}"

usage() {
    echo "Usage: $0 [--dry TRUSTED_OUTPUT_FILE]" >&2
}

OUTPUT_FILE=""
CLEAN_OUTPUT_FILE=0

if [[ "${1:-}" == "--dry" ]]; then
    if [[ $# -ne 2 ]]; then
        usage
        exit 2
    fi
    OUTPUT_FILE="$2"
    if [[ -L "$OUTPUT_FILE" || ! -f "$OUTPUT_FILE" || ! -r "$OUTPUT_FILE" ]]; then
        echo "Dry output must be a readable regular file: $OUTPUT_FILE" >&2
        exit 1
    fi
    echo "Using trusted output from $OUTPUT_FILE"
elif [[ $# -eq 0 ]]; then
    # Security: mktemp creates a private file so tee cannot follow a pre-created /tmp symlink.
    OUTPUT_FILE=$(mktemp "${TMPDIR:-/tmp}/criterion-sqlite-output.XXXXXX")
    CLEAN_OUTPUT_FILE=1
    trap 'if [[ "$CLEAN_OUTPUT_FILE" -eq 1 ]]; then rm -f "$OUTPUT_FILE"; fi' EXIT

    echo "Running sqlite benchmark..."
    cargo bench --bench sqlite --features sqlite 2>&1 | tee "$OUTPUT_FILE"
else
    usage
    exit 2
fi

extract_times() {
    local pattern="$1"
    grep -A2 "$pattern" "$OUTPUT_FILE" | \
        awk -v pat="$pattern" '
            $0 ~ pat {name=$1}
            /time:/ {
                match($0, /\[.*\]/)
                bracket = substr($0, RSTART+1, RLENGTH-2)
                split(bracket, vals, " ")
                printf "| %s | %s %s |\n", name, vals[3], vals[4]
            }'
}

BASE="criterion-sqlite-${MONIKER}-${TIMESTAMP}"
MD_PATH="${RESULTS_DIR}/${BASE}.md"

cat > "$MD_PATH" <<EOF
# Criterion SQLite Builtin Benchmark

Measures the \`sqlite\` builtin (Turso embedded engine) end-to-end through
the bashkit interpreter. Per-invocation overhead (interpreter setup, script
parse, engine open, VFS flush) is included in every number — these are
"what a script author observes", not isolated engine micro-benchmarks.

## System Information

- **Moniker**: \`${MONIKER}\`
- **Hostname**: ${HOSTNAME}
- **OS**: ${OS}
- **Architecture**: ${ARCH}
- **CPUs**: ${CPUS}
- **Timestamp**: ${TIMESTAMP}

## CRUD (insert / update, Memory vs Vfs backend, n rows)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^sqlite_crud/' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Indexing (create index, indexed lookup, full scan)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^sqlite_index/' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Query (GROUP BY aggregate)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^sqlite_query/' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Output mode formatters (1k rows)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^sqlite_output_mode/' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Persistence (cost per invocation)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^sqlite_persistence/' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Parallel sessions (N concurrent over shared VFS)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^sqlite_parallel/' >> "$MD_PATH"

echo ""
echo "Saved: ${MD_PATH}"
