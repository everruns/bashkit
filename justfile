# Development commands
# Install just: ./init-cloud-env.sh (pre-built) or cargo install just
# Usage: just <recipe>   (or: just --list)

# Upstream OKF spec linter, pinned. Keep in lockstep with .github/workflows/ci.yml.
okf_lint_version := "0.1.1"

# Knowledge docs wrap prose at author discretion, so the line-length rule is off.
okf_lint_max_line_length := "10000"

# Default: show available commands
default:
    @just --list

# === Build & Test ===

# Build all crates
build:
    cargo build

# Build the browser wasm package (@everruns/bashkit-wasm) and run its tests.
# Requires: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli
build-wasm:
    bash crates/bashkit-wasm/scripts/build.sh release
    node --test crates/bashkit-wasm/__test__/bashkit-wasm.test.mjs

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
    python3 -m unittest discover -s scripts/tests -p 'test_*.py'
    just check-capability-parity
    just check-okf
    just check-doc-links

# Validate the canonical public-surface capability matrix and generated inventory.
check-capability-parity:
    python3 scripts/capability_parity.py --check

# Regenerate the public-surface capability inventory from its canonical manifest.
regen-capability-parity:
    python3 scripts/capability_parity.py

# The site rewrites doc links by basename, so a cross-tree link written as a
# bare `jq.md` renders fine on bashkit.sh while 404-ing on GitHub. This checks
# the source-relative form the site rewriter also accepts.
# Validate relative markdown links in docs/ and crates/bashkit/docs/
check-doc-links:
    python3 scripts/check_doc_links.py

# okf-lint covers the spec rules; check_okf.py covers the bundle-local
# conventions it does not enforce (see knowledge/knowledge-contract.md).
# okf-lint is pinned in CI and skipped here when absent: just install-okf-lint
# Validate the knowledge/ OKF v0.2 bundle
check-okf:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 scripts/check_okf.py knowledge
    if command -v okf-lint >/dev/null; then
        okf-lint knowledge --max-line-length {{ okf_lint_max_line_length }}
        echo "okf-lint {{ okf_lint_version }}: knowledge conforms to OKF v0.2"
    else
        echo "okf-lint not installed — skipping (just install-okf-lint)"
    fi

# Install the pinned upstream OKF spec linter
install-okf-lint:
    cargo install okf-lint --version {{ okf_lint_version }} --locked

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

# Regenerate the self-hosted Python API reference (docs.rs analog for PyPI).
# Output committed at site/src/content/apidocs/python.md; refresh on release.
# Needs griffe: pip install griffe. See knowledge/operations/documentation.md.
apidocs-python:
    python3 scripts/gen_python_apidocs.py

# Regenerate the self-hosted TypeScript API reference (docs.rs analog for npm).
# Output committed at site/src/content/apidocs/typescript.md; refresh on release.
# Requires `napi build` first (generates index.d.ts the .ts wrappers import).
# Needs network for `npx typedoc`. See knowledge/operations/documentation.md.
apidocs-ts:
    cd crates/bashkit-js && pnpm exec napi build --platform
    cd crates/bashkit-js && pnpm run build:cjs
    node scripts/gen_ts_apidocs.mjs

# Regenerate all self-hosted package API references.
apidocs: apidocs-python apidocs-ts

# Regenerate the canonical builtin inventory consumed by the site's
# builtins page. Committed output; the builtins-drift workflow fails on diff.
regen-builtins:
    cargo run -q --example dump_builtins \
        --features jq,git,ssh,http_client,python,typescript,sqlite \
        > knowledge/status/builtins.json

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

# === Eval (mira study) ===
# Evals run on the mira framework (github.com/everruns/mira). The crate is a
# study binary the `mira` host CLI spawns over stdio; mira owns the model
# matrix, scheduling, and reporting. Install the host once:
#   cargo install mira-cli      # provides the `mira` binary
# Targets are gated on ANTHROPIC_API_KEY / OPENAI_API_KEY — set the keys for
# the models you want to run; unkeyed targets are skipped. See knowledge/operations/eval.md.

# List advertised evals, samples, scorers, and targets
eval-list:
    mira --bin bashkit-eval list

# Run the bash agent eval. Pass extra mira flags through, e.g. (--targets takes
# exact labels, comma-separated — globs are not supported):
#   just eval --targets anthropic/claude-opus-4-8 --tag json_processing --format html --out report.html
eval *ARGS:
    mira --bin bashkit-eval run bashkit_bash {{ARGS}}

# Quick 3-task smoke eval
eval-smoke *ARGS:
    mira --bin bashkit-eval run bashkit_smoke {{ARGS}}

# Scripting-tool eval. The `mode` axis compares scripted vs baseline; select one
# with `--axis mode=scripted` (omit to run both).
eval-scripting *ARGS:
    mira --bin bashkit-eval run bashkit_scripting {{ARGS}}

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

    # Monty is a registry dependency, so verify the exact package that CI
    # publishes. Rewriting manifests here can hide real packaging failures.
    echo ""
    echo "Dry-run publish bashkit..."
    cargo publish -p bashkit --dry-run --allow-dirty

    echo ""
    echo "Dry-run publish bashkit-cli..."
    # bashkit-cli verifies against the registry package of bashkit.
    # Before the matching bashkit release is published, local dry-run cannot
    # resolve that package. Package a disposable workspace against the latest
    # published core as a structural proxy. The real workflow publishes and
    # verifies the matching core before publishing the CLI.
    CLI_VERIFY_ROOT=$(mktemp -d)
    trap 'rm -rf "$CLI_VERIFY_ROOT"' EXIT
    rsync -a \
        --exclude .git \
        --exclude node_modules \
        --exclude target \
        ./ "$CLI_VERIFY_ROOT/workspace/"
    LATEST_CORE=$(cargo search bashkit --limit 1 | sed -n 's/^bashkit = "\([^"]*\)".*/\1/p')
    if [ -z "$LATEST_CORE" ]; then
        echo "Error: could not determine latest published bashkit version"
        exit 1
    fi
    WORKSPACE_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
    CLI_TOML="$CLI_VERIFY_ROOT/workspace/crates/bashkit-cli/Cargo.toml"
    sed -i.bak \
        "s/version = \"$WORKSPACE_VERSION\"/version = \"$LATEST_CORE\"/" \
        "$CLI_TOML"
    rm "$CLI_TOML.bak"
    # The latest published core predates Monty's crates.io publication and
    # therefore lacks the Python feature. The exact new core package dry-run
    # above validates that feature. Resolve the proxy from crates.io rather
    # than the copied path crate, and omit Python only in this disposable copy.
    perl -0pi.bak -e \
        's/path = "\.\.\/bashkit", //; s/^python = \["bashkit\/python"\]\n//m; s/"python", //g; s/, "python"//g' \
        "$CLI_TOML"
    rm "$CLI_TOML.bak"
    cargo publish \
        --manifest-path "$CLI_VERIFY_ROOT/workspace/Cargo.toml" \
        -p bashkit-cli \
        --dry-run \
        --allow-dirty \
        --no-verify

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
