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

### BashKit Principles

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

<!-- TODO: Add API keys as needed -->

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

### Pre-PR Checklist

1. `just pre-pr` (runs 2-4 automatically)
2. `cargo fmt --check`
3. `cargo clippy --all-targets --all-features -- -D warnings`
4. `cargo test --all-features`
5. Rebase on main: `git fetch origin main && git rebase origin/main`
6. Update specs if behavior changes
7. CI green before merge
8. Resolve all PR comments
9. `cargo bench --bench parallel_execution` if touching Arc/async/Interpreter/builtins (see `specs/007-parallel-execution.md`)
10. `just bench` if changes might impact performance (interpreter, builtins, tools)

### CI

- GitHub Actions. Check via `gh` tool.
- **NEVER merge when CI is red.** No exceptions.

### Commits

[Conventional Commits](https://www.conventionalcommits.org): `type(scope): description`

Types: feat, fix, docs, refactor, test, chore

### PRs

Squash and Merge. Use PR template if exists.

**NEVER add links to Claude sessions in PR body.**

See `CONTRIBUTING.md` for details.
