#!/usr/bin/env bash
# Run the Criterion parallel_execution benchmark and save results to
# crates/bashkit/benches/results/, next to the bench source.
#
# Usage:
#   ./scripts/bench-parallel.sh          # run + save
#   ./scripts/bench-parallel.sh --dry    # parse last run without re-running
set -euo pipefail

# Cache Criterion output in the caller's private cache directory. Avoid shared /tmp
# paths so local users cannot pre-create symlinks or poison --dry parsing.
RESULTS_DIR="crates/bashkit/benches/results"
HOSTNAME=$(hostname 2>/dev/null || echo "unknown")
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
CPUS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "?")
TIMESTAMP=$(date +%s)
MONIKER="${HOSTNAME}-${OS}-${ARCH}"
CACHE_ROOT="${XDG_CACHE_HOME:-${HOME}/.cache}"
CACHE_DIR="${CACHE_ROOT}/bashkit"
OUTPUT_FILE="${CACHE_DIR}/criterion-parallel-output.txt"
TEMP_OUTPUT=""
cleanup_temp_output() {
    if [[ -n "${TEMP_OUTPUT}" && -e "${TEMP_OUTPUT}" ]]; then
        rm -f "${TEMP_OUTPUT}"
    fi
}
trap cleanup_temp_output EXIT

mkdir -p "$RESULTS_DIR" "$CACHE_DIR"
if [[ -L "$CACHE_DIR" ]]; then
    echo "Refusing symlinked cache directory: $CACHE_DIR"
    exit 1
fi
chmod 700 "$CACHE_DIR"

# Run benchmark unless --dry
if [[ "${1:-}" != "--dry" ]]; then
    echo "Running parallel_execution benchmark..."
    TEMP_OUTPUT=$(mktemp "${CACHE_DIR}/criterion-parallel-output.XXXXXX")
    chmod 600 "$TEMP_OUTPUT"
    cargo bench --bench parallel_execution 2>&1 | tee "$TEMP_OUTPUT"
    mv -f "$TEMP_OUTPUT" "$OUTPUT_FILE"
    TEMP_OUTPUT=""
    chmod 600 "$OUTPUT_FILE"
else
    if [[ ! -f "$OUTPUT_FILE" || -L "$OUTPUT_FILE" ]]; then
        echo "No previous output found at $OUTPUT_FILE"
        exit 1
    fi
    echo "Using cached output from $OUTPUT_FILE"
fi

# Extract median time from Criterion output for lines matching a pattern
# Usage: extract_times <grep_pattern> >> output_file
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

BASE="criterion-parallel-${MONIKER}-${TIMESTAMP}"
MD_PATH="${RESULTS_DIR}/${BASE}.md"

cat > "$MD_PATH" <<EOF
# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: \`${MONIKER}\`
- **Hostname**: ${HOSTNAME}
- **OS**: ${OS}
- **Architecture**: ${ARCH}
- **CPUs**: ${CPUS}
- **Timestamp**: ${TIMESTAMP}

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^workload_types/' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^parallel_scaling/' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Single Operations

| Benchmark | Time |
|-----------|------|
EOF

extract_times '^single_' >> "$MD_PATH"

cat >> "$MD_PATH" <<EOF

## Speedup Summary

EOF

# Calculate speedups from the parsed output
OUTPUT_FILE="$OUTPUT_FILE" python3 -c "
import os, re, sys

text = open(os.environ['OUTPUT_FILE']).read()

# Parse all timing results: name -> median_ms
results = {}
for m in re.finditer(r'^(\S+)\s*\n\s+time:\s+\[[\d.]+ \S+ ([\d.]+) (\S+)', text, re.MULTILINE):
    name = m.group(1)
    val = float(m.group(2))
    unit = m.group(3)
    # Normalize to ms
    if unit == 'µs':
        val /= 1000
    elif unit == 's':
        val *= 1000
    results[name] = val

# Workload speedups
print('| Workload | Sequential | Parallel | Speedup |')
print('|----------|-----------|----------|---------|')
for w in ['light', 'medium', 'heavy']:
    seq = results.get(f'workload_types/{w}_sequential')
    par = results.get(f'workload_types/{w}_parallel')
    if seq and par:
        print(f'| {w} | {seq:.3f} ms | {par:.3f} ms | **{seq/par:.2f}x** |')

print()
print('| Sessions | Sequential | Parallel | Shared FS | Par Speedup |')
print('|----------|-----------|----------|-----------|-------------|')
scaling_counts = sorted({
    int(k.rsplit('/', 1)[1])
    for k in results
    if k.startswith('parallel_scaling/medium_seq/')
})
for n in scaling_counts:
    seq = results.get(f'parallel_scaling/medium_seq/{n}')
    par = results.get(f'parallel_scaling/medium_par/{n}')
    sfs = results.get(f'parallel_scaling/shared_fs/{n}')
    if seq and par:
        sfs_str = f'{sfs:.3f} ms' if sfs else 'N/A'
        print(f'| {n} | {seq:.3f} ms | {par:.3f} ms | {sfs_str} | **{seq/par:.2f}x** |')
" >> "$MD_PATH"

echo ""
echo "Saved: ${MD_PATH}"
