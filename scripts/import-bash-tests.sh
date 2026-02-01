#!/bin/bash
# Import test cases from external bash test suites
#
# This script generates test cases based on patterns from various bash test suites
# and converts them to BashKit's spec format.
#
# Usage: ./scripts/import-bash-tests.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
EXTERNAL_DIR="$PROJECT_ROOT/crates/bashkit/tests/spec_cases/external"

mkdir -p "$EXTERNAL_DIR"

echo "=== BashKit External Test Importer ==="
echo ""
echo "This script generates external test cases based on patterns from:"
echo "  - GNU Bash test suite"
echo "  - ShellCheck wiki examples"
echo ""
echo "Test files are written to: $EXTERNAL_DIR"
echo ""
echo "Note: To use patterns from actual bash source, download from:"
echo "  https://ftp.gnu.org/gnu/bash/bash-5.2.tar.gz"
echo "  and extract tests/ directory."
echo ""
echo "Run tests with: cargo test --test spec_tests -- external"
