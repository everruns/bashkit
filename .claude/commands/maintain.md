Run the pre-release maintenance checklist from `specs/012-maintenance.md`. Find and fix all issues before reporting.

## Arguments

- `$ARGUMENTS` - Optional: scope to a specific section (e.g. "dependencies", "docs", "simplification"). If omitted, run all sections.

## Goals

Each section below is an outcome to achieve, not a script to follow. Use whatever tools and approaches make sense to verify and fix.

### 1. Dependencies are current and clean

Ensure all direct dependencies are at their latest versions — including major/breaking upgrades. Don't just run `cargo update`; also check `cargo outdated` for major version bumps and update the version constraints in `Cargo.toml` accordingly.

For each outdated dependency (including major bumps):
1. Bump the version constraint in workspace `Cargo.toml` (or crate-level if not in workspace)
2. Run `cargo build` — fix compilation errors from API changes
3. Run `cargo test` — fix test failures
4. If an upgrade requires non-trivial refactoring (>50 lines changed), defer it to a tracked GitHub issue

After all bumps, run `cargo update` to lock latest patch versions.

Ensure no CVEs exist and license/advisory/supply-chain checks pass.

Key tools: `cargo update`, `cargo outdated`, `cargo audit`, `cargo deny check`, `just vet`

### 2. Security posture is solid

Ensure the threat model covers all features, security tests exist for all mitigated threats, and no OWASP-style issues exist in the codebase.

Key references: `specs/006-threat-model.md`, `specs/005-security-testing.md`, `crates/bashkit/docs/threat-model.md`

### 3. Tests are comprehensive and green

All tests pass, no gaps for recent features, bash compatibility holds, coverage has no major holes.

Key tools: `just test`, `just check-bash-compat`

### 4. Documentation matches reality

All docs (rustdoc, guides, public docs in `docs/`, Python, README, CONTRIBUTING, CHANGELOG) accurately reflect current code. Command counts, feature lists, CLI flags, security boundaries, API signatures, and examples are correct.

Fix any drift — update the docs, not the code (unless the code is wrong).

### 5. Examples work end-to-end

All Rust examples compile and run. Feature-gated examples (python, git) work. Python agent examples run successfully.

Key tools: `cargo run --example <name>`, feature-gated variants

### 6. Specs reflect reality

Every spec status is accurate. Implementation status tables match code. No orphaned TODOs. New features have spec entries.

### 7. Code is clean

Formatted, no clippy warnings, no stale TODOs, no dead code or unused deps.

Key tools: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`

### 8. Code is as simple as possible

Review the codebase (focus on recently changed areas) for simplification opportunities:

- **Duplication** — repeated patterns that should share a helper
- **Over-engineering** — abstractions, indirection, or configurability that doesn't serve current needs
- **Complexity** — deeply nested logic, long match arms, convoluted control flow
- **Dead code** — unused functions, unreachable branches, commented-out code
- **Naming** — unclear or misleading names

Make the simplifications. Run tests after each change. The goal is less code that does the same thing.

### 9. Agent configuration is accurate

`AGENTS.md` and `CLAUDE.md` reflect current specs, commands, tooling, and workflows.

### 10. Nightly CI is healthy

Nightly and fuzz workflows green for past week. Fuzz targets compile. Git-sourced deps resolve.

Key tools: `gh run list --workflow=nightly.yml --limit 7`, `gh run list --workflow=fuzz.yml --limit 7`

If failures persist >2 days, escalate per the policy in `specs/012-maintenance.md`.

## Execution

- Run all sections (or scoped subset from `$ARGUMENTS`)
- Fix issues as you find them — don't just report
- Commit fixes incrementally with conventional commit messages
- After all sections complete, report a summary of findings and fixes
- If any section has unfixable issues, report them clearly with recommended next steps

## Notes

- This is a goal-based checklist. The spec (`specs/012-maintenance.md`) defines *what* must be true. This skill defines *how* to verify and fix.
- Use parallel agents for independent sections when possible.
- For scoped runs, still verify that fixes don't break other areas (`just test` at minimum).
