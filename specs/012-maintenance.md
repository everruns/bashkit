# 012: Pre-Release Maintenance

## Status
Implemented

## Abstract

Requirements for pre-release maintenance. Ensures no regressions, stale docs,
dependency rot, or security gaps ship in a release.

## When to Run

- Before every minor or major release
- Quarterly for patch-only periods
- After large feature merges

## Requirements

### Dependencies

- All direct dependencies at latest versions, including major/breaking upgrades
- Upgrade procedure for each outdated dependency:
  1. Bump version constraint in `Cargo.toml` (workspace or crate-level)
  2. Run `cargo build` — fix any compilation errors from API changes
  3. Run `cargo test` — fix any test failures
  4. If upgrade requires non-trivial refactoring (>50 lines changed), defer to a
     tracked GitHub issue instead of blocking the maintenance pass
- `cargo update` run after all version bumps to lock latest patch versions
- No known CVEs in dependency tree
- License and advisory checks pass (`deny.toml`)
- Supply chain audit passes

### Security

- Threat model (`specs/006-threat-model.md`) covers all current features
- Public threat model doc (`crates/bashkit/docs/threat-model.md`) in sync with spec
- Every new builtin/feature has a corresponding TM-XXX entry
- Security tests exist for every MITIGATED threat
- Failpoint tests pass
- Unsafe usage reviewed (`cargo geiger`)
- No OWASP-style issues (injection, path traversal, etc.)

### Tests

- All tests pass
- No test gaps for recently added features
- Test counts in `009-implementation-status.md` match reality
- Bash compatibility — no new regressions against real bash
- Coverage reviewed — no major uncovered paths

### Documentation

- Rust crate docs (`lib.rs`) match reality: command count, categories, guide list, features, examples
- Guide docs (`crates/bashkit/docs/`) up to date: compatibility, threat-model, custom_builtins, logging, python
- Rustdoc builds clean (no warnings)
- Python docs (`crates/bashkit-python/README.md`) match current bindings and exports
- Python docstrings match behavior
- `README.md` feature list matches implemented builtins
- Public docs (`docs/`) match current code: CLI flags, security boundaries, feature descriptions, test counts, and examples all reflect reality
- `CONTRIBUTING.md` instructions accurate
- `CHANGELOG.md` has entries for all changes since last release

### Examples

- All Rust examples compile and run
- Feature-gated examples work (python, git)
- Python agent examples run end-to-end
- Code examples in docs/rustdoc still accurate

### Specs

- Each spec status reflects reality
- `009-implementation-status.md` feature tables match code
- No orphaned TODOs in specs that are now resolved
- New features have spec entries

### Code Quality

- Formatted (`cargo fmt`)
- No clippy warnings
- No stale TODO/WTF comments that are now resolved
- No dead code or unused dependencies

### Code Simplification

- Duplicated patterns consolidated into shared helpers where it reduces total code
- Unnecessary abstractions, indirection, or over-engineering removed
- Complex nested logic simplified (deep nesting, long match arms)
- Dead code removed (unused functions, unreachable branches, commented-out code)
- Names are clear and descriptive (functions, variables, types)
- No premature generalizations — code serves current needs, not hypothetical future ones

### Binding Parity

- Python and Node bindings expose the same public API surface
- Feature gaps tracked and resolved before release
- Parity checklist:
  - Core classes: `Bash`, `BashTool`, `ExecResult`, `ScriptedTool`, `BashError`
  - Execution methods: `execute`, `execute_sync`, `executeOrThrow`/`execute_or_throw`
  - Configuration: `username`, `hostname`, `max_commands`, `max_loop_iterations`, `python`, `external_functions`/`external_handler`
  - Mount API: `files` dict, `mounts` list (read-only default), runtime `mount`/`unmount` (see `specs/003-vfs.md` § Binding API Parity)
  - Tool metadata: `name`, `description`, `help`, `system_prompt`, `input_schema`, `output_schema`, `version`
  - Module functions: `getVersion`/`get_version`
  - Framework integrations: LangChain available in both bindings
  - ExecResult fields: `stdout`, `stderr`, `exit_code`, `error`, `success`, truncation flags, `final_env`
- New features added to one binding must have a tracking issue for the other

### Agent Configuration

- `AGENTS.md` / `CLAUDE.md` instructions accurate
- Spec table in `AGENTS.md` lists all current specs
- Build/test commands work
- Pre-PR checklist covers current tooling

### CI Health

- **CI on main is green** — the latest CI run on `main` must pass. Any failure
  (audit, test, lint, examples) is a blocker that must be fixed before
  proceeding with the rest of the maintenance pass.
- Nightly and fuzz workflows green for past week
- Fuzz targets compile
- Git-sourced dependencies still resolve

#### Escalation Policy

Failures persisting **>2 consecutive days** on any workflow (CI, nightly, fuzz)
are blocking:
1. Open GitHub issue with label `ci:nightly`
2. Link failing run(s)
3. Assign to most recent contributor in failing area
4. If upstream dep change: pin to known-good rev, open follow-up issue

**This section is a hard gate.** The maintenance pass MUST NOT be marked
complete or merged while any of the above checks are red. If the agent cannot
fix a failure, it must open a GitHub issue and report the pass as blocked.

## Deferred Items

When a maintenance pass identifies issues too large to fix inline (e.g.
multi-file refactors, cross-cutting changes), the pass must:

1. Create a GitHub issue for each deferred item with clear scope and reproduction steps
2. Record the issue numbers in the summary below so they are tracked

Deferred items are **not** failures — they are expected for large-scope
improvements. The requirement is that they are **tracked**, not silently skipped.

### Deferred from 2026-03-27 run

| Issue | Section | Description |
|-------|---------|-------------|
| #880  | Simplification | Migrate 27 builtins from manual arg parsing to ArgParser |
| #881  | Simplification | Extract errexit suppression propagation helper |

## Automation

Sections dependencies, tests, examples, code quality, and nightly CI are fully
automatable. Security, documentation, specs, simplification, and agent config
require human or agent review.

CI health check enforced by `just check-nightly` (nightly + fuzz) and manual
inspection of CI on `main` (audit, test, lint). Called by `just release-check`.

## Invocation

Use `/maintain` skill to execute this checklist interactively.

## References

- `specs/008-release-process.md` — release workflow
- `specs/009-implementation-status.md` — feature status
- `specs/006-threat-model.md` — threat model
