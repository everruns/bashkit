#!/usr/bin/env bash
# Example: calling bashkit from real bash with host directory mounting
#
# Demonstrates the --mount-ro and --mount-rw CLI flags to expose host
# directories inside a sandboxed virtual bash session.
#
# Usage:
#   bash examples/realfs_mount.sh
#
# Prereqs:
#   cargo build -p bashkit-cli --features realfs

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BASHKIT="${BASHKIT:-$PROJECT_ROOT/target/debug/bashkit}"

# Build if binary doesn't exist
if [[ ! -x "$BASHKIT" ]]; then
    echo "Building bashkit CLI with realfs support..."
    cargo build -p bashkit-cli --features realfs --quiet
fi

# --- Setup test data ---
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "hello world" > "$TMPDIR/greeting.txt"
printf "1,alice,100\n2,bob,200\n3,charlie,300\n" > "$TMPDIR/data.csv"
mkdir -p "$TMPDIR/output"

# --- 1. Readonly mount at a specific VFS path ---
echo "=== 1. Readonly mount at /mnt/data ==="
$BASHKIT --mount-ro "$TMPDIR:/mnt/data" -c '
echo "Files visible in VFS:"
ls /mnt/data

echo ""
echo "Greeting:"
cat /mnt/data/greeting.txt

echo ""
echo "CSV processing (grep):"
cat /mnt/data/data.csv | grep alice
'

# --- 2. Read-write mount for output ---
echo ""
echo "=== 2. Read-write mount for processing ==="
$BASHKIT \
    --mount-ro "$TMPDIR:/mnt/input" \
    --mount-rw "$TMPDIR/output:/mnt/output" \
    -c '
# Read from readonly mount, write to readwrite mount
cat /mnt/input/data.csv | grep alice > /mnt/output/filtered.txt
echo "Wrote filtered.txt"

# Count lines
wc -l < /mnt/input/data.csv
'

# --- 3. Verify host-side results ---
echo ""
echo "=== 3. Verifying host files ==="

echo -n "filtered.txt: "
cat "$TMPDIR/output/filtered.txt"

echo -n "original data.csv unchanged: "
wc -l < "$TMPDIR/data.csv"

echo ""
echo "Success!"
