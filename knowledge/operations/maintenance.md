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

Use the [maintain skill](../../.agents/skills/maintain/SKILL.md) to run the pass
and the [ship skill](../../.agents/skills/ship/SKILL.md) to land the fixes.
Complete fixes, audits, and builds within the pass. Preserve security contracts
and audit criteria, and validate platform-specific behavior in a suitable
environment. If upstream breaks a required contract, validate and document a
known-good pin. Use parallel agents for independent sections when useful and
consistent with the requested order of work.

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
- Monty forks current with upstream: `just check-monty-upstream` (crates.io
  `monty`/`monty-types` vs the fork base, OSV advisories for `monty`,
  `monty-types`, PyPI `pydantic-monty`). cargo audit/deny and Dependabot miss
  both because the fork crates are renamed. A newer release or an open
  advisory means re-sync the forks per [Dependencies](dependencies.md#monty-forks),
  or record a triaged advisory in the script's `ACKNOWLEDGED`. The release
  half is why this is a maintenance item: the advisory half already runs
  daily in `monty-upstream.yml`, so by the time a pass starts an open
  advisory has normally surfaced on its own
- Supply chain audit passes
- Dependency tree analysis (`cargo tree --duplicates`, usage grep):
  - No unused/dead dependencies in workspace or crate Cargo.toml files
  - Single-builtin deps behind feature flags (not always-on)
  - No full crates where a sub-crate suffices (e.g. `futures-util` vs `futures`)
  - Duplicate transitive versions reviewed, fix or document why unfixable

#### Automated coverage

[Dependabot configuration](../../.github/dependabot.yml) lists the workspaces
covered by weekly grouped *version* updates. It is separate from
Dependabot **alerts** and **security updates**, which run off the repository
dependency graph. [Coverage tests](../../scripts/tests/test_dependabot_coverage.py)
check that dependency-resolving workspaces have entries. See the
[dependency policy](dependencies.md#dependabot-rules-that-keep-its-jobs-runnable)
for resolver constraints.

Deliberately not covered, and why:

- `@everruns/*` is ignored in every npm entry. Those are this repo's own
  published packages, pinned to the workspace version by the release process
  ([Release Process](release-process.md)); dependabot moving them independently
  races that process.
- `crates/bashkit-wasm` has a `package.json` with no dependencies and no
  lockfile, so there is nothing to update.
- `.deepsec/` is updated and audited by hand during the security pass below.
  The committed lockfile pins the exact scanner and dependencies used by the
  pass. Refresh it immediately before scanning so the pass uses the latest
  scanner, including releases younger than the npm cooldown.
- Python: `crates/bashkit-python/pyproject.toml` declares only optional extras
  with open lower bounds (`>=`) and ships no lockfile, so a pip entry would
  have nothing to pin.

##### pnpm release cooldown

Every Dependabot-managed pnpm project carries `minimum-release-age=4320` in its `.npmrc`. The
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
  - Run `pnpm audit` against the refreshed lockfile and resolve advisories.
  - Run `pnpm deepsec scan --project-id bashkit`
  - Run `pnpm deepsec process --project-id bashkit --agent codex`
  - Review `pnpm deepsec report --project-id bashkit`, fix findings, and verify
    regressions before shipping; use the external-blocker policy below only when needed
- Security tests exist for every MITIGATED threat
- Failpoint tests pass
- Unsafe usage reviewed (`cargo geiger`). Nightly scans the Bashkit crate
  and its dependencies with all features using the crate's package manifest.
  Nightly fails if the scan fails, package metrics are missing, or Rust inputs
  outside Cargo's generated build directory go unscanned. The audit also
  rejects Geiger parser errors in the checkout. The uploaded report and
  diagnostics list generated inputs and dependency parser errors for review.
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
- Run `just check-doc-links` and verify [documentation requirements](documentation.md#requirements).
- Verify the [agent-facing site surfaces](documentation.md#agent-facing-site-surfaces)
  build and pass their CI checks. Refresh the published skill archive and its
  digest when the public skill changes.
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
    consuming builtin or explicitly rejected with a "not yet
    implemented" error (no silent no-ops).
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

### Vendored jaq-json Drift

See [Vendored jaq-json](../runtimes/jaq-json-vendor.md).

- Run `scripts/sync-jaq-json.sh --check`. If upstream jaq-json has a newer
  release, sync it with `scripts/sync-jaq-json.sh <version>`, resolve any
  `.rej` files, and re-check every `BASHKIT PATCH` hunk: new growth paths
  (natives, container mutations) need a meter check or `resync()`
  (TM-DOS-110).
- Keep `jaq-core`/`jaq-std` at the versions the vendored release requires,
  and match its dependency ranges in `crates/bashkit/Cargo.toml`.
- Check whether upstream now offers a size hook; if so, drop the vendored
  copy and go back to the crate.

### Code Quality

- Run the formatting and Clippy gates in [ship](../../.agents/skills/ship/SKILL.md#validate).
- Review the repository for dead code, resolved TODO/WTF comments, unused
  dependencies, and opportunities to simplify existing code. Use the
  [ship review guidance](../../.agents/skills/ship/SKILL.md#review-test-and-smoke-test).

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

- Verify the agent entry points link to the current playbooks and knowledge
  index. Check that the documented build, test, and shipping commands work.

### CI Health

- **CI on main is green**: the latest CI run on `main` must pass. Any failure
  (audit, test, lint, examples) is a blocker that must be fixed before
  proceeding with the rest of the maintenance pass.
- Nightly and fuzz workflows green for past week
- Fuzz targets compile
- Git-sourced dependencies still resolve
- No orphaned CI quarantines - `grep -rn continue-on-error .github/workflows/`; each hit must cite its tracking issue, and the issue must still be open

#### Workflow Credential Audit

Run-level green is not the same as a safe workflow definition. Audit
`.github/workflows/` itself each pass; the enforced invariants live in
`scripts/tests/test_ci_supply_chain.py` and `scripts/tests/test_maintenance_security.py`
(TM-INF-026, TM-INF-027, TM-INF-034), so a new workflow that violates one fails
`just test` rather than silently shipping.

- **Checkout credentials**: every `actions/checkout` sets
  `persist-credentials: false` unless the job authenticates a `git`/`gh` call.
  Without it the job's `GITHUB_TOKEN` sits in `.git/config` where any
  `build.rs`, npm lifecycle script, or test in the same job can read it. No job
  in a `pull_request`-triggered workflow may keep it.
- **Default permissions**: every workflow declares a top-level `permissions`
  block; write scopes are opt-in per job, never inherited.
- **Secret transport**: a secret fetched at runtime is invisible to GitHub's log
  masker until `::add-mask::` announces it. Fetch, mask, then pass it on through
  `$GITHUB_OUTPUT` and step `env`. Never `$GITHUB_ENV` - that republishes the
  value in the environment block of every later step. Only a
  `secrets.X != ''` presence boolean may reach `$GITHUB_ENV`.
- **Credential lifetime**: the step holding a broad token (`DOPPLER_TOKEN`)
  fetches one key and exits before any repository code runs. No `cargo`, `node`,
  `bun`, `docker`, or `examples/` invocation shares a step with it.
- **Triggers**: no `pull_request_target`, `workflow_run`, or `issue_comment`
  trigger; these run with a writable token in an attacker-influenced context.
- **Untrusted interpolation**: no `${{ github.event.* }}` or `${{ github.head_ref }}`
  expanded directly into a `run:` block. Pass it through `env:` instead.

#### Escalation Policy

Failures persisting **>2 consecutive days** on any workflow (CI, nightly, fuzz)
are blocking. Inspect failing runs, fix the root cause, and verify recovery in
this pass. Preserve evidence in the PR and apply the
[external-blocker policy](#findings-and-external-blockers).

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

`just release-check` calls `just check-nightly`. The gate requires the three
newest runs of each Nightly and Fuzz Testing workflow on `main` to have
completed successfully. Missing or unfinished runs and GitHub CLI errors
block the gate. Review CI on `main` (audit, test, lint) separately.

## References

- [Agent Pull Request Rules](agent-pr-rules.md), when agents may merge and reply to comments
- [Release Process](release-process.md), release workflow
- [Known Limitations](limitations.md), negative spec (intentional gaps, partial features)
- [Threat Model](../security/threat-model.md), threat model
