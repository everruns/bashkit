## Coding-agent guidance

### Style

Telegraph. Drop filler/grammar. Min tokens.

### Critical Thinking

Fix root cause. Unsure: read more code; if stuck, ask w/ short options. Unrecognized changes: assume other agent; keep going. If causes issues, stop + ask.

### Principles

- Important decisions as comments on top of file
- Code testable, smoke testable, runnable locally
- Small, incremental PR-sized changes
- No backward compat needed (internal code)
- Write failing test before fixing bug

### Specs

`specs/` contains feature specifications. New code should comply with these or propose changes.

| Spec | Description |
|------|-------------|
| 001-architecture | Core interpreter architecture, module structure |
| 002-parser | Bash syntax parser design |
| 003-vfs | Virtual filesystem abstraction |
| 004-testing | Testing strategy and patterns |
| 005-builtins | Builtin command implementations |
| 005-security-testing | Fail-point injection for security testing |
| 006-threat-model | Security threats and mitigations |
| 007-parallel-execution | Threading model, Arc usage |
| 008-documentation | Rustdoc guides, embedded markdown |
| 008-posix-compliance | POSIX design rationale, security exclusions |
| 008-release-process | Version tagging, crates.io + PyPI publishing |
| 009-implementation-status | Feature status, test coverage, limitations |
| 009-tool-contract | Public LLM Tool trait contract |
| 010-git-support | Sandboxed git operations on VFS |
| 011-python-builtin | Embedded Python via Monty, security, resource limits |
| 012-eval | LLM evaluation harness, dataset format, scoring |
| 012-maintenance | Pre-release maintenance checklist |
| 013-python-package | Python package, PyPI wheels, platform matrix |
| 014-scripted-tool-orchestration | Compose ToolDef+callback pairs into OrchestratorTool via bash scripts |

### Documentation

- Guides live in `crates/bashkit/docs/` as markdown files
- Embedded into rustdoc via `include_str!` (see `specs/008-documentation.md`)
- Edit `crates/bashkit/docs/*.md`, not the doc modules in `lib.rs`
- Add "See also" cross-links when creating new guides
- Run `cargo doc --open` to preview changes

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

Pre-configured: `GITHUB_TOKEN`

<!-- API keys: ANTHROPIC_API_KEY for eval/examples -->

### Local Dev

```bash
just --list       # All commands
just build        # Build
just test         # Run tests
just check        # fmt + clippy + test
just pre-pr       # Pre-PR checks
```

### Rust

- Stable Rust, toolchain in `rust-toolchain.toml`
- `cargo fmt` and `cargo clippy -- -D warnings`
- License checks: `cargo deny check` (see `deny.toml`)

### Python

- Python package in `crates/bashkit-python/`
- Linter/formatter: `ruff` (config in `pyproject.toml`)
- `ruff check crates/bashkit-python` and `ruff format --check crates/bashkit-python`
- Tests: `pytest crates/bashkit-python/tests/ -v` (requires `maturin develop` first)
- CI: `.github/workflows/python.yml` (lint, test on 3.9/3.12/3.13, build wheel)

### Shipping

"Ship" means: implement with extensive test coverage (positive and negative paths), then complete the full Pre-PR Checklist (especially smoke testing impacted functionality), create PR, and merge when CI is green.

Use the `/ship` command (`.claude/commands/ship.md`) to execute the complete shipping workflow. For "fix and ship" requests: implement the fix first, then run `/ship`.

### Pre-PR Checklist

1. `just pre-pr` (runs 2-4 automatically)
2. `cargo fmt --check`
3. `cargo clippy --all-targets --all-features -- -D warnings`
4. `cargo test --all-features`
5. Unit tests cover both positive (expected behavior) and negative (error handling, edge cases) scenarios
6. Security tests if change touches user input, parsing, sandboxing, or permissions (see `specs/005-security-testing.md`)
7. Compatibility/differential tests if change affects Bash behavior parity (compare against real Bash)
8. Rebase on main: `git fetch origin main && git rebase origin/main`
9. Update specs if behavior changes
10. CI green before merge
11. Resolve all PR comments
12. `cargo bench --bench parallel_execution` if touching Arc/async/Interpreter/builtins (see `specs/007-parallel-execution.md`)
13. `just bench` if changes might impact performance (interpreter, builtins, tools)
14. `ruff check crates/bashkit-python && ruff format --check crates/bashkit-python` if touching Python code

### CI

- GitHub Actions. Check via `gh` tool.
- **NEVER merge when CI is red.** No exceptions.

### Commits

[Conventional Commits](https://www.conventionalcommits.org): `type(scope): description`

Types: feat, fix, docs, refactor, test, chore

- Updates to `specs/` and `AGENTS.md`: use `chore` type
- NEVER add links to Claude sessions in PR body or commits

### Commit Attribution

All commits MUST be attributed to the real human user, never to a coding agent or bot.

Before committing, configure git user from environment variables:

```bash
git config user.name "$GIT_USER_NAME"
git config user.email "$GIT_USER_EMAIL"
```

Environment variables `GIT_USER_NAME` and `GIT_USER_EMAIL` must be set in the session. If they are missing, stop and ask the user — do not commit with default/bot identity.

- Do NOT set `GIT_AUTHOR_NAME`, `GIT_COMMITTER_NAME`, or `user.name` to any AI/bot identity (e.g. "Claude", "Cursor", "Copilot", "github-actions[bot]")
- Do NOT use `Co-authored-by` trailers referencing AI tools
- Do NOT add "generated by", "authored by AI", or similar attribution in commit messages
- The pre-push script checks for bot-like author names and will warn if detected
- Merge commits must also use the real user as author — never use agent identities

### PRs

Squash and Merge. Use PR template if exists.

**NEVER add links to Claude sessions in PR body or commits. Never attribute commit or merge commit to coding agents, always use real user.**

- Prefer small, shippable PRs. Split large changes into independent, reviewable units.
- When asked to create separate PRs, follow that instruction—do not bundle unrelated changes.

See `CONTRIBUTING.md` for details.
