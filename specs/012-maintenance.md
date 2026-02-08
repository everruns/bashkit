# 012: Pre-Release Maintenance Checklist

## Status
Implemented

## Abstract

Pre-release maintenance procedure. Run before every minor/major release to catch
regressions, stale docs, dependency rot, and security gaps.

## When to Run

- Before every minor or major release
- Quarterly for patch-only periods
- After large feature merges

## Checklist

### 1. Dependencies

- [ ] `cargo update` — pull latest compatible versions
- [ ] `cargo outdated --root-deps-only` — check for major version bumps
- [ ] `cargo outdated --depth 1` — check all direct deps including major
- [ ] Upgrade major versions where safe; test after each bump
- [ ] `cargo audit` — no known CVEs
- [ ] `cargo deny check` — licenses and advisories clean
- [ ] `just vet` — supply chain audit passes

### 2. Security

- [ ] Threat model (`specs/006-threat-model.md`) covers all current features
- [ ] Public threat model (`crates/bashkit/docs/threat-model.md`) in sync with spec
- [ ] New builtins/features have corresponding TM-XXX entries
- [ ] Security tests exist for every MITIGATED threat
- [ ] `cargo test --features failpoints --test security_failpoint_tests -- --test-threads=1` passes
- [ ] `cargo geiger --all-features` — review unsafe usage
- [ ] Code review: no OWASP-style issues (injection, path traversal, etc.)

### 3. Tests

- [ ] `just test` — all tests pass
- [ ] No test gaps for recently added features
- [ ] Spec test counts in `009-implementation-status.md` match reality
- [ ] `just check-bash-compat` — no new regressions against real bash
- [ ] Coverage report reviewed (no major uncovered paths)

### 4. Documentation

- [ ] `README.md` feature list matches implemented builtins
- [ ] `crates/bashkit/docs/compatibility.md` scorecard up to date
- [ ] `crates/bashkit/docs/threat-model.md` matches `specs/006-threat-model.md`
- [ ] `crates/bashkit/docs/python.md` matches current python feature status
- [ ] Rustdoc builds clean: `cargo doc --all-features` (no warnings)
- [ ] `CONTRIBUTING.md` instructions still accurate
- [ ] `CHANGELOG.md` has entries for all changes since last release

### 5. Examples

- [ ] All Rust examples compile and run (`cargo run --example <name>`)
- [ ] Feature-gated examples run: `cargo run --features python --example python_scripts`
- [ ] Feature-gated examples run: `cargo run --features git --example git_workflow`
- [ ] Python agent examples run end-to-end (requires `ANTHROPIC_API_KEY`):
  - `cd crates/bashkit-python && maturin develop`
  - `pip install "langchain>=1.0" "langchain-anthropic>=0.3"`
  - `python3 examples/treasure_hunt_agent.py`
  - `pip install "deepagents>=0.3.11"`
  - `python3 examples/deepagent_coding_agent.py`
- [ ] Code examples in docs/rustdoc still accurate

### 6. Specs

- [ ] Each spec's status reflects reality (Implemented / Living doc / Planned)
- [ ] `009-implementation-status.md` feature tables match code
- [ ] No orphaned TODOs in specs that are now resolved
- [ ] New features have spec entries or are covered by existing specs

### 7. Code Quality

- [ ] `cargo fmt --check` — formatted
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` — no warnings
- [ ] No stale `TODO`/`WTF` comments that are now resolved
- [ ] No dead code or unused dependencies

### 8. Agent Configuration

- [ ] `AGENTS.md` / `CLAUDE.md` instructions still accurate
- [ ] Spec table in `AGENTS.md` lists all current specs
- [ ] Build/test commands in `AGENTS.md` still work
- [ ] Pre-PR checklist in `AGENTS.md` covers current tooling

## Running the Checklist

Ask a coding agent: "Run the maintenance checklist from `specs/012-maintenance.md`"

The agent should:
1. Execute each section
2. Fix issues found
3. Update docs/specs as needed
4. Commit fixes
5. Report findings

## Automation

Sections 1, 3, 5, 7 are fully automatable. Sections 2, 4, 6, 8 require human
or agent review.

## References

- `specs/008-release-process.md` — release workflow
- `specs/009-implementation-status.md` — feature status
- `specs/006-threat-model.md` — threat model
