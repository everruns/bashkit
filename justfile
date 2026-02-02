# Development commands
# Install just: ./init-cloud-env.sh (pre-built) or cargo install just
# Usage: just <recipe>   (or: just --list)

# Default: show available commands
default:
    @just --list

# === Build & Test ===

# Build all crates
build:
    cargo build

# Run all tests (including fail-point tests)
test:
    cargo test --features network
    cargo test --features failpoints --test security_failpoint_tests -- --test-threads=1

# Run fail-point tests only (single-threaded, requires failpoints feature)
test-failpoints:
    cargo test --features failpoints --test security_failpoint_tests -- --test-threads=1

# Run formatters and linters (auto-fix)
fmt:
    cargo fmt
    cargo clippy --all-targets --fix --allow-dirty --allow-staged 2>/dev/null || true

# Run format, lint, and test checks
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# Run all pre-PR checks
pre-pr: check vet
    @echo "Pre-PR checks passed"

# Check spec tests against real bash
check-bash-compat:
    ./scripts/update-spec-expected.sh

# Check spec tests against real bash (verbose)
check-bash-compat-verbose:
    ./scripts/update-spec-expected.sh --verbose

# Generate comprehensive compatibility report
compat-report:
    cargo test --test spec_tests -- compatibility_report --ignored --nocapture

# Run differential fuzzing tests (grammar-based proptest)
fuzz-diff:
    cargo test --test proptest_differential -- --nocapture

# Run differential fuzzing with more iterations
fuzz-diff-deep:
    PROPTEST_CASES=500 cargo test --test proptest_differential -- --nocapture

# Clean build artifacts
clean:
    cargo clean

# === Run ===

# Run the CLI
run *args:
    cargo run -p bashkit-cli -- {{args}}

# Run REPL
repl:
    cargo run -p bashkit-cli -- repl

# Run a script file
run-script file:
    cargo run -p bashkit-cli -- run {{file}}

# === Benchmarks ===

# Run benchmarks comparing bashkit to bash
bench:
    cargo run -p bashkit-bench --release

# Run benchmarks and save results to JSON
bench-save file="bench-results.json":
    cargo run -p bashkit-bench --release -- --save {{file}}

# Run benchmarks with verbose output
bench-verbose:
    cargo run -p bashkit-bench --release -- --verbose

# Run specific benchmark category (startup, variables, arithmetic, control, strings, arrays, pipes, tools, complex)
bench-category cat:
    cargo run -p bashkit-bench --release -- --category {{cat}}

# Run benchmarks with more iterations for accuracy
bench-accurate:
    cargo run -p bashkit-bench --release -- --iterations 50 --warmup 5

# List available benchmarks
bench-list:
    cargo run -p bashkit-bench --release -- --list

# Run benchmarks with all runners (including just-bash if available)
bench-all:
    cargo run -p bashkit-bench --release -- --runners bashkit,bash,just-bash

# === Security ===

# Run supply chain audit (cargo-vet)
vet:
    cargo vet

# Suggest crates to audit
vet-suggest:
    cargo vet suggest

# Certify a crate after audit
vet-certify crate version:
    cargo vet certify {{crate}} {{version}}

# === Release ===

# Prepare a release (update version, remind to edit changelog)
release-prepare version:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Preparing release v{{version}}..."

    # Update workspace version
    sed -i 's/^version = ".*"/version = "{{version}}"/' Cargo.toml

    # Verify the change
    echo "Updated Cargo.toml workspace version to {{version}}"
    grep '^version' Cargo.toml | head -1

    # Remind to update changelog
    echo ""
    echo "Next steps:"
    echo "1. Edit CHANGELOG.md to add release notes for {{version}}"
    echo "2. Run: just release-check"
    echo "3. Run: just release-tag {{version}}"

# Verify release is ready
release-check:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Running release checks..."

    # Run pre-PR checks
    just pre-pr

    # Dry-run publish
    echo ""
    echo "Dry-run publish bashkit..."
    cargo publish -p bashkit --dry-run

    echo ""
    echo "Dry-run publish bashkit-cli..."
    cargo publish -p bashkit-cli --dry-run

    echo ""
    echo "All release checks passed!"

# Create and push release tag
release-tag version:
    #!/usr/bin/env bash
    set -euo pipefail

    # Verify version matches Cargo.toml
    CARGO_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
    if [ "{{version}}" != "$CARGO_VERSION" ]; then
        echo "Error: Requested version ({{version}}) does not match Cargo.toml version ($CARGO_VERSION)"
        echo "Run: just release-prepare {{version}}"
        exit 1
    fi

    # Check for uncommitted changes
    if [ -n "$(git status --porcelain)" ]; then
        echo "Error: Uncommitted changes detected. Commit all changes before tagging."
        git status --short
        exit 1
    fi

    # Create tag
    echo "Creating tag v{{version}}..."
    git tag -a "v{{version}}" -m "Release v{{version}}"

    # Push tag
    echo "Pushing tag to origin..."
    git push origin "v{{version}}"

    echo ""
    echo "Release v{{version}} tagged and pushed!"
    echo "CI will now publish to crates.io"
