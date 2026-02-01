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

# Run all tests
test:
    cargo test --all-features

# Run bash comparison tests (compare with real bash)
compare:
    cargo test --test spec_tests -- bash_comparison_tests --nocapture

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
pre-pr: check
    @echo "Pre-PR checks passed"

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
