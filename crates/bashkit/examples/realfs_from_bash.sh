#!/usr/bin/env bash
# Example: calling bashkit from real bash with host directory mounting
#
# This script demonstrates how to use bashkit CLI to process files
# from a real directory in a sandboxed virtual bash environment.
#
# Usage: bash examples/realfs_from_bash.sh
# Requires: cargo build -p bashkit-cli --features realfs

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Build bashkit CLI with realfs support
echo "=== Building bashkit CLI with realfs support ==="
cargo build -p bashkit-cli --features realfs --quiet 2>/dev/null || {
    echo "Building bashkit-cli..."
    cargo build -p bashkit-cli --features realfs
}

BASHKIT="$PROJECT_ROOT/target/debug/bashkit"

# Create a temp directory with test data
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "hello world" > "$TMPDIR/greeting.txt"
echo "1,alice,100" > "$TMPDIR/data.csv"
echo "2,bob,200" >> "$TMPDIR/data.csv"
echo "3,charlie,300" >> "$TMPDIR/data.csv"
mkdir -p "$TMPDIR/output"

echo ""
echo "=== Readonly mount at /mnt/data ==="
echo "Host dir: $TMPDIR"
echo ""

# Mount the temp dir as readonly at /mnt/data inside bashkit
$BASHKIT --mount-ro "$TMPDIR:/mnt/data" -c '
echo "Files visible in VFS:"
ls /mnt/data

echo ""
echo "Greeting:"
cat /mnt/data/greeting.txt

echo ""
echo "CSV data:"
cat /mnt/data/data.csv

echo ""
echo "Processing CSV (count lines):"
wc -l /mnt/data/data.csv
'

echo ""
echo "=== Read-write mount for output ==="
echo ""

# Mount read-write for output processing
$BASHKIT --mount-ro "$TMPDIR:/mnt/input" --mount-rw "$TMPDIR/output:/mnt/output" -c '
echo "Reading input..."
cat /mnt/input/data.csv

echo ""
echo "Writing processed output..."
cat /mnt/input/data.csv | grep "alice" > /mnt/output/filtered.txt
echo "Filtered output written to /mnt/output/filtered.txt"
'

echo ""
echo "=== Verifying host files ==="
echo "Output file contents:"
cat "$TMPDIR/output/filtered.txt"
echo "Original data unchanged:"
cat "$TMPDIR/data.csv"

echo ""
echo "Success! Bashkit processed host files in a sandboxed environment."
