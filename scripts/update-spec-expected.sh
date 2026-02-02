#!/usr/bin/env bash
# Check and update expected outputs in spec test files
#
# Usage:
#   ./scripts/update-spec-expected.sh           # Check all tests match real bash
#   ./scripts/update-spec-expected.sh --verbose # Show details for each test
#
# This script runs the bash comparison tests to verify that expected outputs
# match what real bash produces. Tests marked with ### skip or ### bash_diff
# are excluded from comparison.
#
# To update expected outputs:
# 1. Run this script to see which tests differ
# 2. Edit the .test.sh files manually
# 3. Or add ### bash_diff marker if the difference is intentional
#
# This is a wrapper around: cargo test --test spec_tests -- bash_comparison_tests

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${SCRIPT_DIR}/.."

VERBOSE=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Check that spec test expected outputs match real bash."
            echo ""
            echo "Options:"
            echo "  --verbose, -v   Show all test comparisons"
            echo "  --help, -h      Show this help message"
            echo ""
            echo "Exit codes:"
            echo "  0 - All non-excluded tests match real bash"
            echo "  1 - Some tests have mismatches"
            echo ""
            echo "Tests are excluded from comparison if they have:"
            echo "  ### skip: reason      - Not yet implemented"
            echo "  ### bash_diff: reason - Known intentional difference"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

cd "$ROOT_DIR"

echo "Checking spec tests against real bash..."
echo ""

if $VERBOSE; then
    cargo test --test spec_tests -- bash_comparison_tests_verbose --ignored --nocapture 2>&1
else
    cargo test --test spec_tests -- bash_comparison_tests --nocapture 2>&1
fi

exit_code=$?

if [[ $exit_code -eq 0 ]]; then
    echo ""
    echo "All non-excluded tests match real bash."
else
    echo ""
    echo "Some tests have differences from real bash."
    echo ""
    echo "To fix:"
    echo "  1. Update the expected output in .test.sh to match real bash, or"
    echo "  2. Add '### bash_diff: reason' if the difference is intentional"
    echo ""
    echo "Run with --verbose to see all comparisons."
fi

exit $exit_code
