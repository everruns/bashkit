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
    cargo test --features http_client
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

# Lint and format-check Python bindings
python-lint:
    ruff check crates/bashkit-python
    ruff format --check crates/bashkit-python

# Run all pre-PR checks
pre-pr: check vet
    @echo "Pre-PR checks passed"

# Run all pre-PR checks plus strict host-bash parity
pre-pr-strict: pre-pr check-bash-compat
    @echo "Strict pre-PR checks passed"

# Check spec tests against real bash
check-bash-compat:
    ./scripts/update-spec-expected.sh

# Check spec tests against real bash (verbose)
check-bash-compat-verbose:
    ./scripts/update-spec-expected.sh --verbose

# Generate comprehensive compatibility report
compat-report:
    cargo test --test integration -- spec_tests::bash_comparison_tests --ignored --nocapture

# Run differential fuzzing tests (grammar-based proptest)
fuzz-diff:
    cargo test --test integration -- proptest_differential:: --nocapture

# Run differential fuzzing with more iterations
fuzz-diff-deep:
    PROPTEST_CASES=500 cargo test --test integration -- proptest_differential:: --nocapture

# Clean build artifacts
clean:
    cargo clean

# Regenerate the canonical builtin inventory consumed by the site's
# builtins page. Committed output; the builtins-drift workflow fails on diff.
regen-builtins:
    cargo run -q --example dump_builtins \
        --features jq,git,ssh,http_client,python,typescript,sqlite \
        > specs/status/builtins.json

# === uutils argument-surface port (POC) ===

# Regenerate the clap `Command` builders for utilities ported from
# uutils/coreutils. Output is committed under
# `crates/bashkit/src/builtins/generated/`. Pass UUTILS=/path/to/uutils to
# point at a checkout (defaults to /tmp/uutils, cloned if missing).
#
# Add new utilities by extending the for-loop below and wiring the resulting
# `<util>_command()` into the matching builtin module.
regen-coreutils-args UUTILS="/tmp/uutils":
    #!/usr/bin/env bash
    set -euo pipefail
    pinned="$(grep -oE 'UUTILS_REVISION: &str = "[^"]+"' \
        crates/bashkit/src/builtins/generated/mod.rs \
        | sed -E 's/.*"([^"]+)"/\1/')"
    if [[ -z "$pinned" ]]; then
        echo "could not parse UUTILS_REVISION pin" >&2
        exit 1
    fi
    if [[ ! -d "{{UUTILS}}/.git" ]]; then
        echo "Cloning uutils into {{UUTILS}}..."
        git clone https://github.com/uutils/coreutils.git "{{UUTILS}}"
    fi
    git -C "{{UUTILS}}" fetch --quiet
    git -C "{{UUTILS}}" checkout --quiet "$pinned"
    rev="$(git -C "{{UUTILS}}" rev-parse --short HEAD)"
    out="crates/bashkit/src/builtins/generated"
    mkdir -p "$out"
    # Discover utils from the manifest (every `pub mod <util>_args;` line)
    # so adding a new util is one edit in mod.rs, not two.
    mapfile -t utils < <(grep -oE 'pub mod [a-z0-9_]+_args' "$out/mod.rs" \
        | sed -E 's/pub mod ([a-z0-9_]+)_args/\1/')
    for util in "${utils[@]}"; do
        cargo run -q -p bashkit-coreutils-port -- "{{UUTILS}}" "$util" "$rev" \
            > "$out/${util}_args.rs"
        echo "regenerated $out/${util}_args.rs (uutils@$rev)"
    done
    cargo fmt -- "$out"/*.rs

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

# Run benchmarks comparing bashkit to bash and save site-indexed JSON/Markdown results
bench:
    cargo run -p bashkit-bench --release -- --save
    pnpm --dir site run data:performance

# Run benchmarks and save results to JSON/Markdown
bench-save file="":
    cargo run -p bashkit-bench --release -- --save {{file}}
    pnpm --dir site run data:performance

# Run benchmarks with verbose output and save site-indexed JSON/Markdown results
bench-verbose:
    cargo run -p bashkit-bench --release -- --verbose --save
    pnpm --dir site run data:performance

# Exploratory: run specific benchmark category without updating site results (startup, variables, arithmetic, control, strings, arrays, pipes, tools, complex)
bench-category cat:
    cargo run -p bashkit-bench --release -- --category {{cat}}

# Run benchmarks with more iterations for accuracy and save site-indexed JSON/Markdown results
bench-accurate:
    cargo run -p bashkit-bench --release -- --iterations 50 --warmup 5 --save
    pnpm --dir site run data:performance

# List available benchmarks
bench-list:
    cargo run -p bashkit-bench --release -- --list

# Run benchmarks with all runners and save site-indexed JSON/Markdown results (including just-bash if available)
bench-all:
    cargo run -p bashkit-bench --release -- --runners bashkit,bash,just-bash --save
    pnpm --dir site run data:performance

# Run Criterion parallel_execution benchmark and save results
bench-parallel:
    ./scripts/bench-parallel.sh
    pnpm --dir site run data:performance

# Run Criterion sqlite builtin benchmark and save results
bench-sqlite:
    ./scripts/bench-sqlite.sh
    pnpm --dir site run data:performance

# === Eval ===

# Run LLM eval and save site-indexed JSON/Markdown results (requires ANTHROPIC_API_KEY or OPENAI_API_KEY)
eval dataset="crates/bashkit-eval/data/eval-tasks.jsonl" provider="anthropic" model="claude-sonnet-4-20250514":
    cargo run -p bashkit-eval --release -- run --dataset {{dataset}} --provider {{provider}} --model {{model}} --save
    pnpm --dir site run data:performance

# Run eval and save results
eval-save dataset="crates/bashkit-eval/data/eval-tasks.jsonl" provider="anthropic" model="claude-sonnet-4-20250514":
    cargo run -p bashkit-eval --release -- run --dataset {{dataset}} --provider {{provider}} --model {{model}} --save
    pnpm --dir site run data:performance

# Run scripting-tool eval (scripted mode) and save site-indexed JSON/Markdown results
eval-scripting dataset="crates/bashkit-eval/data/scripting-tool/many-tools.jsonl" provider="openai" model="gpt-5.4":
    cargo run -p bashkit-eval --release -- run --eval-type scripting-tool --dataset {{dataset}} --provider {{provider}} --model {{model}} --save
    pnpm --dir site run data:performance

# Run scripting-tool eval (baseline mode — individual tools, no ScriptedTool) and save site-indexed JSON/Markdown results
eval-scripting-baseline dataset="crates/bashkit-eval/data/scripting-tool/many-tools.jsonl" provider="openai" model="gpt-5.4":
    cargo run -p bashkit-eval --release -- run --eval-type scripting-tool --baseline --dataset {{dataset}} --provider {{provider}} --model {{model}} --save
    pnpm --dir site run data:performance

# Run scripting-tool eval and save results
eval-scripting-save dataset="crates/bashkit-eval/data/scripting-tool/many-tools.jsonl" provider="openai" model="gpt-5.4":
    cargo run -p bashkit-eval --release -- run --eval-type scripting-tool --dataset {{dataset}} --provider {{provider}} --model {{model}} --save
    pnpm --dir site run data:performance

# === Security ===

# Auto-install cargo-vet if missing (idempotent, matches CI's
# taiki-e/install-action step). Internal helper for vet recipes.
_ensure-vet:
    @command -v cargo-vet >/dev/null 2>&1 || cargo install cargo-vet --locked

# Run supply chain audit (cargo-vet)
vet: _ensure-vet
    cargo vet --locked

# Suggest crates to audit
vet-suggest: _ensure-vet
    cargo vet suggest

# Certify a crate after audit
vet-certify crate version: _ensure-vet
    cargo vet certify {{crate}} {{version}}

# === Nightly CI ===

# Check that recent nightly and fuzz CI runs are green (requires gh CLI)
check-nightly:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Checking nightly CI status..."
    failed=0
    for workflow in nightly.yml fuzz.yml; do
        name=$(echo "$workflow" | sed 's/\.yml//')
        echo ""
        echo "=== $name ==="
        conclusions=$(gh run list --workflow="$workflow" --limit 3 --json conclusion --jq '.[].conclusion')
        i=0
        for c in $conclusions; do
            i=$((i + 1))
            if [ "$c" = "success" ]; then
                echo "  Run $i: ok"
            else
                echo "  Run $i: FAILED ($c)"
                failed=$((failed + 1))
            fi
        done
        if [ "$i" -eq 0 ]; then
            echo "  WARNING: no runs found (is gh authenticated?)"
        fi
    done
    echo ""
    if [ "$failed" -gt 0 ]; then
        echo "ERROR: $failed nightly run(s) failed in last 3 runs."
        echo "Inspect with: gh run list --workflow=<workflow>.yml --limit 5"
        echo "Do NOT release with red nightly jobs."
        exit 1
    fi
    echo "Nightly CI: all recent runs green."

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

    # Check nightly CI jobs (last 3 runs must be green)
    just check-nightly

    # Match publish.yml locally: strip git-only publish blockers, dry-run, restore.
    TMPDIR=$(mktemp -d)
    LOCKFILE=Cargo.lock
    BASHKIT_TOML=crates/bashkit/Cargo.toml
    CLI_TOML=crates/bashkit-cli/Cargo.toml
    JS_TOML=crates/bashkit-js/Cargo.toml
    PY_TOML=crates/bashkit-python/Cargo.toml
    cp "$LOCKFILE" "$TMPDIR/Cargo.lock"
    cp "$BASHKIT_TOML" "$TMPDIR/bashkit.Cargo.toml"
    cp "$CLI_TOML" "$TMPDIR/bashkit-cli.Cargo.toml"
    cp "$JS_TOML" "$TMPDIR/bashkit-js.Cargo.toml"
    cp "$PY_TOML" "$TMPDIR/bashkit-python.Cargo.toml"
    trap 'cp "$TMPDIR/Cargo.lock" "$LOCKFILE"; \
          cp "$TMPDIR/bashkit.Cargo.toml" "$BASHKIT_TOML"; \
          cp "$TMPDIR/bashkit-cli.Cargo.toml" "$CLI_TOML"; \
          cp "$TMPDIR/bashkit-js.Cargo.toml" "$JS_TOML"; \
          cp "$TMPDIR/bashkit-python.Cargo.toml" "$PY_TOML"; \
          rm -rf "$TMPDIR"' EXIT
    perli() {
        perl -0pi.bak -e "$1" "$2"
        rm -f "$2.bak"
    }

    # --- bashkit core: remove monty dep and python feature ---
    perli 's/^monty = .*?\n//m' "$BASHKIT_TOML"
    perli 's/^python = \["dep:monty"\]\n//m' "$BASHKIT_TOML"
    perli 's/\n\[\[example\]\]\nname = "python_scripts"\nrequired-features = \["python"\]\n//g' "$BASHKIT_TOML"
    perli 's/\n\[\[example\]\]\nname = "python_external_functions"\nrequired-features = \["python"\]\n//g' "$BASHKIT_TOML"

    # --- bashkit-cli: remove python feature ---
    perli 's/^python = \["bashkit\/python"\]\n//m' "$CLI_TOML"
    perli 's/, "python"//g; s/"python", //g; s/\["python"\]/[]/g' "$CLI_TOML"

    # --- bashkit-js/bashkit-python: remove python from features list ---
    perli 's/, "python"//g; s/"python", //g' "$JS_TOML"
    perli 's/, "python"//g; s/"python", //g' "$PY_TOML"

    # Dry-run publish
    echo ""
    echo "Dry-run publish bashkit..."
    cargo publish -p bashkit --dry-run --allow-dirty

    echo ""
    echo "Dry-run publish bashkit-cli..."
    # bashkit-cli verifies against the registry package of bashkit.
    # Before the matching bashkit release is published, local dry-run cannot
    # compile that packaged dependency graph. The real publish workflow keeps
    # verification enabled after bashkit is live on crates.io.
    cargo publish -p bashkit-cli --dry-run --allow-dirty --no-verify

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
