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
