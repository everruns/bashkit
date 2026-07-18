# Release Process

## Status

Implemented

## Abstract

Releases are initiated by asking a coding agent to prepare the release; CI automation handles the rest.

## Versioning

[Semantic Versioning](https://semver.org/): MAJOR = breaking API changes, MINOR = new features/builtins, PATCH = bug fixes/docs.

## Release Workflow

Flow: **prepare → verify-can-publish → merge → monitor-published**.
Skipping the verify step risks tagging a release that fails to publish to
crates.io / PyPI / npm (as v0.4.0 did, requiring the v0.4.1 hotfix);
skipping the monitor step risks declaring "shipped" while a registry
silently failed.

### Human Steps

1. Ask the agent to create a release ("Create release v0.2.0").
2. Review the PR — including the agent's publish-readiness report.
3. Merge to main — CI creates the GitHub Release and publishes.
4. Ask the agent to monitor publishing until all registries show the new version.

### Agent Steps (automated)

0. **Ensure full git history** — cloud sandboxes are commonly shallow-cloned
   (depth ≈ 50), silently hiding commits and yielding a wrong changelog. Run
   `git fetch --unshallow origin main 2>/dev/null || git fetch origin main`
   and cross-check with the GitHub compare API
   (`/repos/everruns/bashkit/compare/v<prev>...main` → `total_commits`); if
   local `git log v<prev>..HEAD | wc -l` disagrees, the clone is still shallow.
1. **Determine version** — human-specified, or suggest from changes.
2. **Update CHANGELOG.md** (format below).
3. **Update version across all manifests** (must match the workspace version):
   workspace `Cargo.toml`, `crates/bashkit-cli/Cargo.toml` path-dep pin on
   `bashkit`, `crates/bashkit-js/package.json`,
   `crates/bashkit-wasm/package.json`, and `Cargo.lock`
   (`cargo update -p bashkit -p bashkit-cli ...`).
   - Refresh the self-hosted API references: `just apidocs` and commit any
     changes. The `apidocs-drift` workflow only checks TypeScript weekly (its
     regen needs a Rust build), so a release is the reliable point to catch TS
     drift. See `specs/documentation.md` ("API reference hosting").
4. **Run local verification** — `cargo fmt --check`, `cargo clippy
   --all-targets --all-features -- -D warnings`, `cargo test`.
5. **Verify publish-readiness** (catches what local tests don't — the
   `cargo publish` packaging step, missing files, version drift):
   - `cargo publish --dry-run -p bashkit` must succeed in a disposable copy
     after applying the same git-only Monty/Python manifest transform as
     `publish.yml`. Package `bashkit-cli` there against the latest published
     core version as a structural proxy; Cargo cannot resolve the CLI's new
     registry dependency until the core crate is live. Normal workspace checks
     still compile the CLI against the new local core, and `publish.yml` waits
     for the core registry version before publishing the CLI. Packaging caught
     the v0.4.0 → v0.4.1 incident: the rustdoc guide lived outside the crate
     dir, so `cargo publish` couldn't find it; local `cargo build` did not catch
     it.
   - PyPI / npm: confirm the feeding pipelines still build (`maturin build
     --release`, `napi build`, browser WASM build smokes) when changes touch
     packaging.
   - Version sync: all manifests read the same `X.Y.Z`, and it is greater
     than the latest published version on every registry (`cargo search
     bashkit`, `pip index versions bashkit`, `npm view @everruns/bashkit
     version`, `npm view @everruns/bashkit-web version`). A missing registry
     entry is valid only for a package's first release.
   - On any failure, fix root cause and re-run before opening the PR — do
     **not** merge a release PR with a known-broken publish path.
6. **Commit and push** — `chore(release): prepare vX.Y.Z` on a feature branch.
7. **Create PR** — same title, changelog excerpt + publish-readiness report in description.
8. **Monitor post-merge publishing** — watch `release.yml` create the
   Release + tag, watch `publish.yml`, `publish-python.yml`, `publish-js.yml`,
   `publish-web.yml`, and `cli-binaries.yml` to completion, run the
   post-release verification commands, and only declare "shipped" when all
   six published artifacts (`bashkit` + `bashkit-cli` on crates.io, `bashkit`
   on PyPI, both npm packages, and Homebrew) report the new version. If one
   fails, open a hotfix PR rather than leaving the release half-shipped.

### CI Automation

- On merge to main, `release.yml` detects the `chore(release): prepare vX.Y.Z` commit, extracts notes from CHANGELOG.md, creates the GitHub Release + tag.
- On Release published, `publish.yml` / `publish-js.yml` /
  `publish-web.yml` / `publish-python.yml` publish to crates.io, npm, PyPI;
  each includes a published-version verification step.

## Pre-Release Checklist

`AGENTS.md` Pre-PR Checklist applies, plus: CI green on main, CHANGELOG.md
has entries since last release, version consistent across all manifests
(step 3), the core publish dry-run and CLI package check succeed, and new
version > latest published on each registry.

## Post-Merge Monitoring

Confirm each target (workflow → check):

- GitHub Release (`release.yml`): `gh release view vX.Y.Z`
- crates.io (`publish.yml`): `cargo search bashkit` / `cargo search bashkit-cli`
- PyPI (`publish-python.yml`): `pip index versions bashkit`
- npm Node (`publish-js.yml`): `npm view @everruns/bashkit version`;
  `npm dist-tags ls @everruns/bashkit` ("latest" points at it)
- npm browser (`publish-web.yml`): `npm view @everruns/bashkit-web version`;
  `npm dist-tags ls @everruns/bashkit-web` ("latest" points at it)
- Homebrew (`cli-binaries.yml`): `everruns/homebrew-tap` formula bumped

If a workflow fails: `gh run view <run-id> --log-failed`, identify root
cause, re-run (transient) or open a hotfix PR (code/packaging bug — see
v0.4.0 → v0.4.1 for a worked example).

## Changelog Format

Use the latest entries in `CHANGELOG.md` as the template. Rules:

- `## [X.Y.Z] - YYYY-MM-DD` header.
- `### Highlights` — 2-5 most impactful, user-facing bullets.
- `### Breaking Changes` for MINOR/MAJOR with bold summary + before/after migration guide.
- `### What's Changed` (not separate Added/Changed/Fixed) — PRs in descending PR-number order, format `* type(scope): description ([#N](URL)) by @author`.
- End with `**Full Changelog**: URL`.

## Package Names and Registries

- `bashkit` on crates.io (core library)
- `bashkit-cli` on crates.io (CLI tool)
- `bashkit` on PyPI (pre-built wheels)
- `@everruns/bashkit` on npm (native NAPI-RS bindings)
- `@everruns/bashkit-web` on npm (single-threaded browser WebAssembly)

## Publishing Order

Crates publish in dependency order: `bashkit` (no internal deps) then
`bashkit-cli` (depends on bashkit). Python wheels (native matrix + the
reduced-feature Pyodide/Emscripten wheel — see `specs/emscripten-wheels.md`)
and both npm packages publish independently (no crates.io dependency). CI
workflows handle ordering automatically on GitHub Release.

## Workflows

### release.yml

Trigger: push to `main` with commit message starting `chore(release): prepare v`.
Creates the GitHub Release with tag + notes from CHANGELOG, verifies manual
runs are from `refs/heads/main`, the release source is reachable from
`origin/main`, and the tag points at the release commit, then dispatches
publish and binary-build workflows.

### cli-binaries.yml

Dispatched by release.yml after tag verification. Accepts only stable
`vX.Y.Z` tags, checks tag matches `Cargo.toml`, workflow ref resolves to the
tag, and the tag commit is reachable from `origin/main` before building.
Builds prebuilt CLI binaries (macOS aarch64/x86_64, Linux x86_64), uploads
to the Release, and pushes a Homebrew formula to `everruns/homebrew-tap`
(`brew install everruns/tap/bashkit`). Secret: `DOPPLER_TOKEN`
(Doppler-managed GitHub PAT for the tap push).

### publish.yml

Trigger: Release published. Publishes to crates.io in dependency order, then
verifies published versions. Secret: `CARGO_REGISTRY_TOKEN`.

### publish-python.yml

Trigger: Release published (parallel with publish.yml). Builds wheels for
all platforms (matrix in `specs/python-package.md`), smoke-tests, publishes
to PyPI via trusted publishing (OIDC, no secrets; environment
`release-python` must exist in repo settings). Python version is read
dynamically from `Cargo.toml` via maturin (`dynamic = ["version"]`).

### publish-js.yml

Trigger: Release published (parallel). Builds native NAPI-RS bindings
(macOS x86_64/aarch64, Linux x86_64/aarch64, Windows x86_64,
wasm32-wasip1-threads), tests on Node 20/22/24, publishes to npm. Secret:
`NPM_TOKEN` (Automation token); provenance via `id-token: write` OIDC +
`--provenance`, same pattern as everruns/sdk. JS package version is synced
from the workspace `Cargo.toml` by `build.rs` updating `package.json`.

### publish-web.yml

Dispatched by `release.yml` from the verified release tag. Builds the
single-threaded `wasm32-unknown-unknown` browser bundle, runs the headless Node
integration suite, and publishes `@everruns/bashkit-web` to npm with
provenance. Uses the same `NPM_TOKEN` as `publish-js.yml`; the package version
is synced manually from the workspace version during release preparation.

## Authentication

- `CARGO_REGISTRY_TOKEN` (crates.io token, publish scopes) in GitHub Actions secrets.
- PyPI trusted publishing: configure publisher at pypi.org for repo `everruns/bashkit`, workflow `publish-python.yml`, environment `release-python`.
- `NPM_TOKEN` (Automation token) in GitHub Actions secrets; no separate GitHub environment required.

## Post-Release Verification

```bash
cargo search bashkit; cargo search bashkit-cli
npm view @everruns/bashkit version; npm dist-tags ls @everruns/bashkit
npm view @everruns/bashkit-web version; npm dist-tags ls @everruns/bashkit-web
pip index versions bashkit
gh release view --repo everruns/bashkit
```

If a registry is missing the version, check the corresponding publish workflow run.

## Hotfix Releases

Ask the agent for a patch release ("Create patch release v0.1.1 for the security fix"); same prepare/verify/merge/monitor flow.

## Rollback Procedure

`cargo yank --version X.Y.Z bashkit` (and `bashkit-cli`) — use sparingly;
yanked versions still resolve for existing `Cargo.lock` files but aren't
selected for new projects.

## Release Artifacts

GitHub Release (tag, notes, source archives, prebuilt CLI binaries),
crates.io crates, PyPI wheels, Node and browser npm bindings, Homebrew formula.
