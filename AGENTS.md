## Coding-agent guidance

### Style

Telegraph. Drop filler/grammar. Min tokens.

### Critical Thinking

Fix root cause. Unsure: read more code; if stuck, ask w/ short options. Unrecognized changes: assume other agent; keep going. If causes issues, stop + ask.

### Principles

- Always make sure you are working on top of latest main from remote. Especially in worktrees: fetch `origin/main`, then rebase or recreate the worktree on top of it before editing.
- Important decisions as comments on top of file
- Code testable, smoke testable, runnable locally
- Small, incremental PR-sized changes
- No backward compat needed (internal code)
- Write failing test before fixing bug

### Specs

`specs/` contains feature specifications. New code should comply with these or propose changes.

| Spec | Description |
|------|-------------|
| architecture | Core interpreter architecture, module structure |
| parser | Bash syntax parser design |
| vfs | Virtual filesystem abstraction |
| testing | Testing strategy and patterns |
| builtins | Builtin command design (trait, ShellRef, ExecutionPlan) |
| security-testing | Fail-point injection for security testing |
| threat-model | Security threats and mitigations |
| parallel-execution | Threading model, Arc usage |
| documentation | Rustdoc guides, embedded markdown |
| release-process | Version tagging, crates.io + PyPI + npm publishing |
| limitations | Negative spec: intentional gaps (L-* IDs), partial features, POSIX stance |
| tool-contract | Public LLM Tool trait contract |
| git-support | Sandboxed git operations on VFS |
| python-builtin | Embedded Python via Monty, security, resource limits |
| eval | LLM eval study on the mira framework, dataset format, scoring |
| maintenance | Pre-release maintenance requirements |
| python-package | Python package, PyPI wheels, platform matrix |
| scripted-tool-orchestration | Compose ToolDef+callback pairs into OrchestratorTool via bash scripts |
| ssh-support | Sandboxed SSH/SCP/SFTP operations |
| zapcode-runtime | Embedded TypeScript via ZapCode, VFS bridging, resource limits |
| request-signing | Transparent Ed25519 request signing (bot-auth) per RFC 9421 |
| interactive-shell | Interactive REPL mode with rustyline line editing |
| sqlite-builtin | Embedded SQLite via Turso (MemoryIO + VfsIO backends, dot-commands) |
| coreutils-args-port | Codegen port of uutils clap definitions + uucore modules |
| credential-injection | Per-host HTTP credential injection without exposing secrets |
| http-transport | Pluggable HTTP transport: route curl/wget via host egress boundary |
| performance-results | Benchmark/eval result locations and `/benches` site aggregation contract |
| emscripten-wheels | Reduced-feature Pyodide/Emscripten Python wheel |

### Documentation

- **Public docs** live in `docs/` — user-facing articles (security, guides, etc.)
- **Rustdoc guides** live in `crates/bashkit/docs/` as markdown files
- Rustdoc guides embedded via `include_str!` (see `specs/documentation.md`)
- Edit `crates/bashkit/docs/*.md`, not the doc modules in `lib.rs`
- Add "See also" cross-links when creating new guides
- Run `cargo doc --open` to preview rustdoc changes

### Bashkit Principles

- All design decisions in `specs/` - no undocumented choices
- Everything runnable and testable - no theoretical code
- Don't stop until e2e works - verify before declaring done
- Examples tested in CI - must pass
- No silent deferral - `TODO` or `WTF` comment with explanation
- Verify crate assumptions before planning to use them

### Cloud Agent Setup

```bash
./scripts/init-cloud-env.sh   # Install just + gh
just build                    # Build project
```

#### Secrets via Doppler

`DOPPLER_TOKEN` is pre-configured. Fetch secrets with:

```bash
curl -s "https://api.doppler.com/v3/configs/config/secret?name=SECRET_NAME" \
  -u "$DOPPLER_TOKEN:" | python3 -c "import sys,json; print(json.load(sys.stdin)['value']['raw'])"
```

Common secrets: `GITHUB_TOKEN`, `ANTHROPIC_API_KEY`

For `gh` CLI, set `GH_TOKEN`:

```bash
export GH_TOKEN=$(curl -s "https://api.doppler.com/v3/configs/config/secret?name=GITHUB_TOKEN" \
  -u "$DOPPLER_TOKEN:" | python3 -c "import sys,json; print(json.load(sys.stdin)['value']['raw'])")
```

### Local Dev

```bash
just --list       # All commands
just build        # Build
just test         # Run tests
just check        # fmt + clippy + test
just pre-pr       # Pre-PR checks
```

**Do not run `cargo test --all-features` as a single invocation.** Not
exercised in CI; statically links every embedded interpreter and enables
`failpoints` (global state, needs `--test-threads=1`); the parallel link
step exceeds sandbox/cloud-runner memory and the supervisor kills the
shell. Use `just check` / `just pre-pr`, or slice by feature the way CI
does (see `.github/workflows/ci.yml`):

```bash
cargo test --workspace --lib --bins --tests --features http_client,ssh,sqlite
cargo test --workspace --doc --features http_client,ssh,sqlite
cargo test --features realfs     -p bashkit --test realfs_tests -p bashkit-cli
cargo test --features failpoints --test security_failpoint_tests -- --test-threads=1
cargo test --test proptest_security -- --test-threads=1
cargo test --features ssh        -p bashkit --test ssh_builtin_tests
```

Python is tested via `pytest` (`.github/workflows/python.yml`), TypeScript
via `cargo run --example typescript_external_functions --features typescript`.

Bashkit's integration tests live under `crates/bashkit/tests/integration/`
and are aggregated by `crates/bashkit/tests/integration/main.rs` into a single binary.
New behavioral tests go there. A small number of files stay as top-level
`tests/*.rs` because they need their own binary (process-global env
mutation, `--test-threads=1`, ssh-only feature isolation) — the list and
criteria live in `specs/testing.md`.

### Rust

- Stable Rust, version pinned in `rust-toolchain.toml` (bump deliberately;
  match the new version in `dtolnay/rust-toolchain@<version>` refs across
  `.github/workflows/*` so CI can't be broken by a same-day rustc release)
- `cargo fmt` and `cargo clippy -- -D warnings`
- License checks: `cargo deny check` (see `deny.toml`)

### Stderr from builtins must not leak internal Debug shapes

**TM-INF-022** in `specs/threat-model.md`. No `{:?}`/`{:#?}` in
`crates/bashkit/src/builtins/`; use `Display` or a domain formatter
(reference: `format_compile_errors` in `builtins/jq/errors.rs`); cap
diagnostics ≤ 1 KB; test-only Debug needs `// debug-ok: <reason>`.
Enforced by `cargo test` (static scan, per-tool `assert_no_leak`, fuzz
invariants) — see `bashkit::testing` rustdoc for the layers.
New library-wrapping builtin: add a `no_leak_*` test (see `jq/tests.rs`);
fuzz targets must use `bashkit::testing::fuzz_exec(...)`, not bare
`bash.exec(...)`.

### Benches

Two distinct harnesses, two distinct result locations — keep them separate.

- **Criterion benches** for the `bashkit` crate live in `crates/bashkit/benches/`.
  Run via `cargo bench --bench <name>` or `just bench-parallel` / `just bench-sqlite`.
  Historical results go in **`crates/bashkit/benches/results/`**, named
  `criterion-<bench>-<moniker>-<timestamp>.md`. The `bench-parallel.sh` and
  `bench-sqlite.sh` scripts write there.
- **`bashkit-bench` harness** (bashkit vs real bash) lives in `crates/bashkit-bench/`.
  Run via `just bench`. Historical results go in **`crates/bashkit-bench/results/`**,
  named `bench-<runner>-<moniker>-<timestamp>.{json,md}`.

When adding a new criterion bench, save its first run under
`crates/bashkit/benches/results/` so future runs have a baseline to diff against.
Do not mix criterion `.md` files into `crates/bashkit-bench/results/`.

### Python

- Python package in `crates/bashkit-python/`
- Linter/formatter: `ruff` (config in `pyproject.toml`)
- `ruff check crates/bashkit-python` and `ruff format --check crates/bashkit-python`
- Tests: `pytest crates/bashkit-python/tests/ -v` (requires `maturin develop` first)
- CI: `.github/workflows/python.yml` (lint, test on 3.9/3.12/3.13, build wheel)

### Pre-PR Checklist

1. `just pre-pr` (runs 2-4 automatically)
2. `cargo fmt --check`
3. `cargo clippy --all-targets --all-features -- -D warnings`
4. `just test` (feature-sliced; never `cargo test --all-features` in one invocation — see Local Dev)
5. Unit tests cover both positive (expected behavior) and negative (error handling, edge cases) scenarios
6. Security tests if change touches user input, parsing, sandboxing, or permissions (see `specs/security-testing.md`)
7. Compatibility/differential tests if change affects Bash behavior parity (compare against real Bash)
8. Rebase on main: `git fetch origin main && git rebase origin/main` (for worktrees: verify the worktree `HEAD` is on latest `origin/main` before editing)
9. Update specs if behavior changes
10. CI green before merge
11. Resolve all PR comments
12. `cargo bench --bench parallel_execution` if touching Arc/async/Interpreter/builtins (see `specs/parallel-execution.md`)
13. `just bench-sqlite` if touching the sqlite builtin or its VFS/IO bridge (see `specs/sqlite-builtin.md`)
14. `just bench` if changes might impact performance (interpreter, builtins, tools)
15. `ruff check crates/bashkit-python && ruff format --check crates/bashkit-python` if touching Python code

### CI

- GitHub Actions. Check via `gh` tool.
- **NEVER merge when CI is red.** No exceptions.

### Commits

[Conventional Commits](https://www.conventionalcommits.org): `type(scope): description`

Types: feat, fix, docs, refactor, test, chore

- Updates to `specs/` and `AGENTS.md`: use `chore` type
- NEVER add links to Claude sessions in PR body or commits

### Commit Attribution

All commits (incl. merge commits) attributed to the real human user —
never a bot/agent identity, no AI `Co-authored-by` trailers, no
"generated by" text. Verify `git config user.name`/`user.email` are
human before committing; if missing or bot-like, set from
`$GIT_USER_NAME`/`$GIT_USER_EMAIL`. If those are also missing, stop and
ask — never commit with a default/bot identity. Pre-push script warns on
bot-like author names.

### PRs

Squash and Merge. Use `.github/pull_request_template.md` for the description.

Center the description on functional change and impact, not a code-location
walkthrough (the diff shows that). Add a Before / After with proof — CLI output,
logs, differential-test results, or screenshots for UI — whenever behavior changes.

**NEVER add links to Claude sessions in PR body or commits. Never attribute commit or merge commit to coding agents, always use real user.**

- Prefer small, shippable PRs. Split large changes into independent, reviewable units.
- When asked to create separate PRs, follow that instruction—do not bundle unrelated changes.

See `CONTRIBUTING.md` for details.
