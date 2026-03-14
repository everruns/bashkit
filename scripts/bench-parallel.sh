#!/usr/bin/env bash
# Run the Criterion parallel_execution benchmark and save results to
# crates/bashkit-bench/results/ alongside the bashkit-bench results.
#
# Usage:
#   ./scripts/bench-parallel.sh          # run + save
#   ./scripts/bench-parallel.sh --dry    # parse last run without re-running
set -euo pipefail

RESULTS_DIR="crates/bashkit-bench/results"
HOSTNAME=$(hostname 2>/dev/null || echo "unknown")
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
CPUS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "?")
TIMESTAMP=$(date +%s)
MONIKER="${HOSTNAME}-${OS}-${ARCH}"

# Run benchmark unless --dry
if [[ "${1:-}" != "--dry" ]]; then
    echo "Running parallel_execution benchmark..."
    cargo bench --bench parallel_execution 2>&1 | tee /tmp/criterion-output.txt
else
    if [[ ! -f /tmp/criterion-output.txt ]]; then
        echo "No previous output found at /tmp/criterion-output.txt"
        exit 1
    fi
    echo "Using cached output from /tmp/criterion-output.txt"
fi

# Extract median time from Criterion output for lines matching a pattern
# Usage: extract_times <grep_pattern> >> output_file
extract_times() {
    local pattern="$1"
    grep -A2 "$pattern" /tmp/criterion-output.txt | \
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
python3 -c "
import re, sys

text = open('/tmp/criterion-output.txt').read()

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
for n in [10, 50, 100, 200]:
    seq = results.get(f'parallel_scaling/medium_seq/{n}')
    par = results.get(f'parallel_scaling/medium_par/{n}')
    sfs = results.get(f'parallel_scaling/shared_fs/{n}')
    if seq and par:
        sfs_str = f'{sfs:.3f} ms' if sfs else 'N/A'
        print(f'| {n} | {seq:.3f} ms | {par:.3f} ms | {sfs_str} | **{seq/par:.2f}x** |')
" >> "$MD_PATH"

echo ""
echo "Saved: ${MD_PATH}"
