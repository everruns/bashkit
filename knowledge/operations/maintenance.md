---
type: Playbook
title: Maintenance
description: Pre-release dependency, security, compatibility, and artifact maintenance requirements.
tags:
  - bashkit
  - maintenance
  - operations
---

# Pre-Release Maintenance

## Status
Implemented

## Abstract

Requirements for pre-release maintenance. Ensures no regressions, stale docs,
dependency rot, or security gaps ship in a release.

## Invocation contract

"Run maintenance", "maintain", and common misspellings such as "maintainace"
and "maintaiance" mean **analyze, fix, and ship**. The request includes local
validation, pushing, PR creation, fixing CI/review findings, and squash-merging
with every required check green. A local commit or a set of deferred issues is
not a completed pass. Only an explicit analysis-only request narrows this outcome.

The maintain skill and command implement this contract; the ship skill completes
it. Do not stop because a fix is large, audits remain, or a build is slow. Preserve
security contracts and audit criteria. Validate platform-specific behavior in a
suitable environment. Proven upstream incompatibilities may require a tested,
documented safe-version pin; they must not justify weaker execution limits.

## When to Run

- Before every minor or major release
- Quarterly for patch-only periods
- After large feature merges

## Requirements

### Dependencies

- All direct dependencies at latest versions, including major/breaking upgrades
- Upgrade procedure for each outdated dependency:
  1. Bump version constraint in `Cargo.toml` (workspace or crate-level)
  2. Run `cargo build`, fix any compilation errors from API changes
  3. Run `cargo test`, fix any test failures
  4. Resolve API changes and validate the upgrade. If upstream cannot preserve a
     required security contract, document and test the newest safe-version pin;
     diff size alone is not grounds for deferral.
- `cargo update` run after all version bumps to lock latest patch versions
- No known CVEs in dependency tree
- License and advisory checks pass (`deny.toml`)
- Supply chain audit passes
- Dependency tree analysis (`cargo tree --duplicates`, usage grep):
  - No unused/dead dependencies in workspace or crate Cargo.toml files
  - Single-builtin deps behind feature flags (not always-on)
  - No full crates where a sub-crate suffices (e.g. `futures-util` vs `futures`)
  - Duplicate transitive versions reviewed, fix or document why unfixable

#### Automated coverage

`.github/dependabot.yml` drives weekly *version* updates. It is separate from
Dependabot **alerts** and **security updates**, which run off the repository
dependency graph and need no config — an ecosystem missing from
`dependabot.yml` still gets alerts, it just never gets routine freshness bumps,
so it drifts until the eventual security fix is a multi-major jump.

Covered, one grouped PR per entry per week:

| Ecosystem | Directory |
|---|---|
| cargo | `/` (workspace) |
| cargo | `/crates/bashkit/fuzz` (separate workspace + lockfile) |
| npm | `/site` |
| npm | `/crates/bashkit-js` |
| npm | `/examples`, `/examples/browser`, `/examples/bashkit-pi` |
| github-actions | `/` |

Deliberately not covered, and why:

- `@everruns/*` is ignored in every npm entry. Those are this repo's own
  published packages, pinned to the workspace version by the release process
  ([Release Process](release-process.md)); dependabot moving them independently
  races that process.
- `crates/bashkit-wasm` has a `package.json` with no dependencies and no
  lockfile, so there is nothing to update.
- `.deepsec/` is updated by hand during the security pass below
  (`pnpm update deepsec@latest`), because the scan is run against whatever
  version the pass pulls, not on dependabot's schedule.
- Python: `crates/bashkit-python/pyproject.toml` declares only optional extras
  with open lower bounds (`>=`) and ships no lockfile, so a pip entry would
  have nothing to pin.

##### pnpm release cooldown

Every npm directory carries `minimum-release-age=4320` in its `.npmrc`. The
value mirrors the `--config.minimumReleaseAge=4320` (3 days) that Dependabot
injects into the `pnpm update` it runs for npm entries.

They have to agree. Dependabot re-resolves the whole tree for each update, and
on a version younger than the cooldown pnpm **errors**
(`ERR_PNPM_NO_MATURE_MATCHING_VERSION`) instead of falling back to an older
mature release — so one freshly-published *transitive* dependency fails that
directory's entire weekly run. The 2026-08-19 `/crates/bashkit-js` run died
exactly this way: `es-toolkit@1.51.0` was 2 days old and reached the tree via
`@napi-rs/cli`, so `ava` (6.4.1, two majors behind) never got its bump and the
run reported `ava | unknown_error`.

Resolving locally under the same cooldown makes locked versions mature by
construction, so Dependabot's check can never reject one. `--frozen-lockfile`
does not resolve, so CI is unaffected. If Dependabot's cooldown changes, change
these files with it.

### Security

- Threat model ([Threat Model](../security/threat-model.md)) covers all current features
- Public threat model doc (`crates/bashkit/docs/threat-model.md`) in sync with spec
- Every new builtin/feature has a corresponding TM-XXX entry
- DeepSec is updated to the latest published version before scanning:
  - Run `cd .deepsec && pnpm update deepsec@latest`
  - Run `pnpm deepsec scan --project-id bashkit`
  - Run `pnpm deepsec process --project-id bashkit --agent codex`
  - Review `pnpm deepsec report --project-id bashkit`, fix findings, and verify
    regressions before shipping; use the external-blocker policy below only when needed
- Security tests exist for every MITIGATED threat
- Failpoint tests pass
- Unsafe usage reviewed (`cargo geiger`)
- No OWASP-style issues (injection, path traversal, etc.)

### Tests

- All tests pass
- No test gaps for recently added features
- `builtins-drift` workflow green (generated [`builtins.json`](../status/builtins.json) in sync)
- Bash compatibility, no new regressions against real bash
- Coverage reviewed, no major uncovered paths

### Documentation

- Rust crate docs (`lib.rs`) match reality: command count, categories, guide list, features, examples
- Guide docs (`crates/bashkit/docs/`) up to date: compatibility, threat-model, custom_builtins, logging, python
- Rustdoc builds clean (no warnings)
- Python docs (`crates/bashkit-python/README.md`) match current bindings and exports
- Python docstrings match behavior
- `README.md` feature list matches implemented builtins
- Public docs (`docs/`) match current code: CLI flags, security boundaries, feature descriptions, test counts, and examples all reflect reality
- Relative markdown links resolve as source: `just check-doc-links` green. The
  site rewrites link targets by *basename* (`DOC_LINKS` in
  `site/astro.config.mjs`), so a cross-tree link written as a bare `jq.md`
  renders correctly on bashkit.sh while 404-ing for anyone reading the markdown
  on GitHub, `site/scripts/verify-doc-*.mjs` check the built routes and cannot
  see it. Write cross-tree links source-relative
  (`../crates/bashkit/docs/jq.md`); the site rewriter accepts that form too.
  Do **not** add YAML frontmatter to `docs/` or `crates/bashkit/docs/` to carry
  titles or descriptions: page metadata lives in `site/src/pages/docs/_meta.ts`,
  and frontmatter in `crates/bashkit/docs/` would leak into rustdoc via
  `include_str!`
- Agent surfaces (`/llms.txt`, `/llms-full.txt`, Markdown routes) regenerate and pass `verify-llms` (auto-enforced in CI); refresh the agent-skills tarball/`index.json` digest if `skills/bashkit/` changed (see [Documentation Architecture](documentation.md) § Agent-facing site surfaces)
- `CONTRIBUTING.md` instructions accurate
- `CHANGELOG.md` has entries for all changes since last release

### Examples

- All Rust examples compile and run
- Feature-gated examples work (python, git)
- Python agent examples run end-to-end
- Code examples in docs/rustdoc still accurate
- `examples/browser` uses exact dependency versions and a committed lockfile:
  run `pnpm install --frozen-lockfile && pnpm start`, load
  the page, and confirm it runs a plain command, a pipe through `jq`, **and a
  subshell** (`bash /home/user/demo.sh`). The subshell path exercises the wasm
  child-shell parse that a top-level command does not, the smoke suite in
  `crates/bashkit-wasm/__test__/` covers the same ground headlessly. If the
  published package lags a fix the example depends on, hold the example until
  the next `bashkit-wasm` release, then review and update both the dependency
  pin and lockfile.

  The exact-version half of that is now enforced by CI rather than left to this
  checklist: `examples/browser/dependency-security.test.js` asserts every
  dependency matches `MAJOR.MINOR.PATCH` with a committed lockfile, and the
  `Examples` job in `ci.yml` runs the suite. Important decision: it runs there,
  not in `js.yml`, because the suite is plain `node --test` with no install
  step, while `js.yml` builds the napi binding and filters on paths that
  exclude `examples/browser` — the guard would not fire on the `package.json`
  edits it exists to catch. The test existed before but nothing ran it, so
  `vite` sat on a caret range (`^6.4.3`) through a two-major dependabot bump
  (\#2313) without anyone noticing. Dependabot preserves whatever range style
  it finds, so an exact pin keeps future bumps exact.

### Specs

- Each spec status reflects reality
- `limitations.md` rows still true (lifted limitations removed)
- No orphaned TODOs in specs that are now resolved
- New features have spec entries

### Coreutils Argument-Surface and Module-Vendor Drift

See [Coreutils Argument Port](../runtimes/coreutils-args-port.md).

- Review any open `chore: sync uutils/coreutils argument surfaces and
  vendored modules` PR produced by the `coreutils-args-drift` workflow
  (weekly cron). The PR covers **both** ported argument surfaces (args
  mode) and vendored uucore modules (module mode):
  - **Args mode review**: confirm new flags are wired into the
    consuming builtin or explicitly rejected (matching the existing
    `tac -b/-r/-s` "not yet implemented" pattern, no silent no-ops).
    Confirm removed/renamed flags don't break downstream scripts;
    migrate or document.
  - **Module mode review**: for every entry in
    `crates/bashkit-coreutils-port/vendored.toml`, scan the diff for
    body changes in the vendored sources, this is verbatim copy, so
    upstream behaviour changes land directly. Validate that
    `vendored.toml` substitutions still cover every internal `use`
    (the port aborts loudly if not, but check that the rationale of
    any `error` actions still reflects intent).
  - Squash-merge as a human (PR's intermediate commits are bot-authored).
  - Confirm the `coreutils_differential_tests` step in the auto-PR is
    green, body drift (semantic divergence vs GNU/uutils) surfaces here
    even when args parity holds.
- Run `just regen-coreutils-args` locally if no drift PR exists; commit any
  diff yourself rather than letting it accumulate. Module-mode regen is
  driven by the same workflow; trigger `workflow_dispatch` if you need
  to refresh vendored modules out-of-band.
- Bump the pinned uutils revision recorded in the generated file headers
  if it has fallen >3 months behind upstream `main`.

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
- No premature generalizations, code serves current needs, not hypothetical future ones

### Binding Parity

- Python and Node bindings expose the same public API surface
- Feature gaps tracked and resolved before release
- Parity checklist:
  - Core classes: `Bash`, `BashTool`, `ExecResult`, `ScriptedTool`, `BashError`
  - Execution methods: `execute`, `execute_sync`, `executeOrThrow`/`execute_or_throw`
  - Configuration: `username`, `hostname`, `max_commands`, `max_loop_iterations`, `python`, `external_functions`/`external_handler`
  - Mount API: `files` dict, `mounts` list (read-only default), runtime `mount`/`unmount` (see [Virtual Filesystem](../foundations/vfs.md) § Binding API Parity)
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

- **CI on main is green**: the latest CI run on `main` must pass. Any failure
  (audit, test, lint, examples) is a blocker that must be fixed before
  proceeding with the rest of the maintenance pass.
- Nightly and fuzz workflows green for past week
- Fuzz targets compile
- Git-sourced dependencies still resolve

#### Escalation Policy

Failures persisting **>2 consecutive days** on any workflow (CI, nightly, fuzz)
are blocking. Inspect failing runs, fix the root cause, and verify recovery in
this pass. Preserve evidence in the PR. If upstream breaks a required contract,
validate and document a known-good pin. Only a genuine external blocker or an
explicitly requested scope split warrants a follow-up issue; include the failing
runs and concrete reason work cannot proceed.

**This section is a hard gate.** Never mark maintenance complete or merge while
required checks are red. Resolve ordinary failures rather than deferring them.

## September 2026 pass

The 2026-09-05 pass starts at `ab04bca2` with main CI and seven daily nightly/fuzz
runs green. It refreshes Rust/npm dependencies and updates DeepSec to 2.3.9.
Monty 0.0.19 and get-size2 0.10.1 are exact compatibility constraints, not an
unimplemented upgrade: newer Monty releases remove the required per-VM tracker,
and newer get-size2 releases conflict with the pinned Ruff AST. Isolated
compatibility builds and 284 Python-feature tests verify the safe versions;
see [Dependency Policy](dependencies.md) and [Python Builtin](../runtimes/python-builtin.md).

DeepSec analyzed 44 files after 54 matcher hits and reported six findings.
Fixes cover CI credentials, the aggregate WASM gate, Anthropic escaped-output
expansion, and Deep Agents truncation metadata/grep glob filters. Credential-fetch
steps now exit before repository execution; only scoped API keys cross the step
boundary. Tests execute the actual workflow scripts and check process environments.
Deep Agents now implements the current structured protocol through native VFS
operations, with a real `create_deep_agent` integration using a deterministic model.
A matcher scan is not a complete Rust security audit; DeepSec reported low Rust coverage.

Local verification includes 5,564 workspace unit/integration tests, 167 rustdoc
tests, 17 failpoint tests, 29 security property tests, 77 repository-script tests,
831 Python tests (four Linux-only skips on macOS), 573 JS tests, and 63 browser
WASM tests. The JS/Python native security suites use release builds, matching
published artifacts and CI; debug native stack-stress builds are not interchangeable
with that validation profile. Strict Linux/GNU Bash parity passes all 1,859 cases.
The macOS strict comparison exposes BSD utility/path differences and is not the
GNU compatibility baseline.

The WASM bindgen installer derives its exact schema version from Cargo.lock,
replaces stale cached CLIs, and verifies the executable on PATH in CI and
publication. Regression tests cover cache mismatch and ambiguous lock versions.

All-feature clippy/rustdoc, site build and generated-page checks, locked fuzz
compilation/advisory checks, and standalone-workspace dependency/advisory checks
pass. Coreutils regeneration produces no drift. CLI sort/jq/SQLite smoke tests
pass, as do embedded Python/TypeScript external callbacks. The published 0.17.1
browser example executes `jq` and the bundled child-shell script successfully.

Unsafe-code review covers the changed dependency deltas and existing native
FFI/callback ownership boundaries; this pass adds no Rust unsafe code. Geiger
reports 449 packages but cannot parse two upstream files (signal-hook-registry
and an aws-lc benchmark helper), and generated includes require source review.
Its successful exit is not proof of complete unsafe coverage.

The earlier August DeepSec tracking findings are also reproduced and fixed:
browser persistence retains the previous complete snapshot after traversal errors;
BashTool snapshot constructors register supplied custom builtins; SQLite setup
installs only when absent; binary release jobs validate and build one immutable
commit; Cargo publication verifies without registry credentials; CI grants no
write permission or persisted checkout credential; release examples install the
reviewed lockfile rather than resolving fresh dependencies. Regression tests cover
failed/successful browser saves, keyed/unkeyed callbacks, SQLite present/missing
branches, moved release tags, compiler environments, and release package links.

Supply-chain validation passes with 57 individually reviewed dependency deltas
and a non-importable jiter Rust-only baseline audit. The patched Git revision is
explicitly audited, and `deny.toml` bans jiter's unused Python FFI feature after
review found an unchecked allocation failure there. A negative fixture proves
the feature ban rejects that configuration. Existing exemptions and publisher
trust were not broadened.

Performance baselines are saved in the respective Criterion and comparison
result directories, and the site timeline is regenerated. The comparison harness
now selects Bash from `PATH`, requires Bash >=4, and records the executable/version;
it previously forced macOS Bash 3.2 and produced invalid oracle errors. The GNU
Bash 5.3.15 run matches all 96 cases with zero errors. Parallel and SQLite
Criterion runs complete successfully. These local measurements are observational,
not a controlled before/after dependency performance claim.

The aggregate CI gate must depend on every validation job, including `wasm`,
`wasm-component`, and `wasm-web`. The maintenance security tests execute its
condition with each dependency failing, preventing a green gate from hiding
an omitted platform failure.

## Findings and external blockers

Keep findings in the active maintenance pass and resolve them before shipping.
Do not create follow-up issues as a substitute for authorized fixes, audits, or
checks. A genuine external blocker or a user-requested scope split may be tracked
explicitly; report the evidence and continue independent work. Never mark a pass
complete or merge while required CI is red.

### Deferred items

Standing transitive limitation:

- **RustCrypto 0.10/0.11 split** (was tracked as #1634, now closed). Our
  directly-declared hashes (`md-5`/`sha1`/`sha2`) are on the 0.11 line, but the
  dependency tree still pulls in the 0.10 line transitively, `aes-gcm 0.10.3`
  (and its `digest 0.10` / `sha2 0.10` / `aead 0.5` / `cipher 0.4` chain) via
  `turso_core`, plus the `russh` / `argon2` crypto stack. Resolution remains
  blocked on those upstreams releasing on the 0.11 line; no in-tree action
  available.

Previously tracked items resolved: #880 (ArgParser migration), #881 (errexit
propagation helper).

## Automation

Sections dependencies, tests, examples, code quality, and nightly CI are fully
automatable. Security, documentation, specs, simplification, and agent config
require human or agent review.

CI health check enforced by `just check-nightly` (nightly + fuzz) and manual
inspection of CI on `main` (audit, test, lint). Called by `just release-check`.

## Invocation

Use `/maintain` skill to execute this checklist interactively.

## References

- [Release Process](release-process.md), release workflow
- [Known Limitations](limitations.md), negative spec (intentional gaps, partial features)
- [Threat Model](../security/threat-model.md), threat model
