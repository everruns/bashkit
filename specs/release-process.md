# Release Process

## Status

Implemented

## Abstract

This document describes the release process for Bashkit. Releases are initiated by asking a coding agent to prepare the release, with CI automation handling the rest.

## Versioning

Bashkit follows [Semantic Versioning](https://semver.org/):

- **MAJOR** (X.0.0): Breaking API changes
- **MINOR** (0.X.0): New features, new builtins
- **PATCH** (0.0.X): Bug fixes, documentation updates

## Release Workflow

### Overview

```
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│ Human    │   │ Agent    │   │ Agent    │   │ Human    │   │ CI       │   │ Agent    │
│ asks     │──>│ prepares │──>│ verifies │──>│ merges   │──>│ tags +   │──>│ monitors │
│ release  │   │ PR       │   │ publish  │   │ PR       │   │ publishes│   │ registries│
└──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘
```

The flow is: **prepare → verify-can-publish → merge → monitor-published**. Skipping the verify step risks tagging a release that fails to publish to crates.io / PyPI / npm (as v0.4.0 did, requiring the v0.4.1 hotfix); skipping the monitor step risks declaring "shipped" while one or more registries silently failed.

### Human Steps

1. **Ask the agent** to create a release:
   - "Create release v0.2.0"
   - "Prepare a patch release"
   - "Release the current changes as v0.2.0"

2. **Review the PR** created by the agent — including the agent's publish-readiness report (see Agent step 5)

3. **Merge to main** - CI handles GitHub Release and crates.io publish

4. **Ask the agent to monitor publishing** (or let it auto-monitor if subscribed to PR activity) — the agent watches the publish workflows and registries, and reports back when all targets show the new version.

### Agent Steps (automated)

When asked to create a release, the agent:

0. **Ensure full git history** — cloud agent sandboxes are commonly shallow-cloned (depth ≈ 50), which silently hides half the commits and yields a wrong commit count / changelog. Before counting or listing commits, run `git fetch --unshallow origin main 2>/dev/null || git fetch origin main` and cross-check with the GitHub compare API (`/repos/everruns/bashkit/compare/v<prev>...main` → `total_commits`); if local `git log v<prev>..HEAD | wc -l` disagrees, the local clone is still shallow.

1. **Determine version**
   - Use version specified by human, OR
   - Suggest next version based on changes (patch/minor/major)

2. **Update CHANGELOG.md**
   - Add release date: `## [X.Y.Z] - YYYY-MM-DD`
   - Add breaking changes section if applicable (see format below)
   - List PRs in descending order with GitHub-style links and contributors
   - End with `**Full Changelog**: URL`

3. **Update version across all manifests** (must all match the workspace version):
   - `Cargo.toml` workspace `version`
   - `crates/bashkit-cli/Cargo.toml` path-dep version pin on `bashkit`
   - `crates/bashkit-js/package.json` `version`
   - `Cargo.lock` (regenerate via `cargo update -p bashkit -p bashkit-cli ...`)

4. **Run local verification**
   - `cargo fmt --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test`

5. **Verify publish-readiness** (catches what local tests don't — the `cargo publish` packaging step, missing files, version drift across registries):
   - **crates.io**: `cargo publish --dry-run -p bashkit` and `cargo publish --dry-run -p bashkit-cli` — must succeed. This is what caught the v0.4.0 → v0.4.1 incident: the rustdoc guide lived outside the crate dir, so `cargo publish` couldn't find it. Local `cargo build` did not catch it.
   - **PyPI / npm**: confirm the build pipelines that feed those registries still build (e.g. `maturin build --release` smoke for Python, `napi build` smoke for JS) when the changes touch packaging.
   - **Version sync check**: confirm all manifests from step 3 read the same `X.Y.Z`, and that the new version is greater than the latest published version on every registry (`cargo search bashkit`, `pip index versions bashkit`, `npm view @everruns/bashkit version`).
   - If any check fails, fix the root cause and re-run before opening the PR — do **not** merge a release PR with a known-broken publish path.

6. **Commit and push**
   - Commit message: `chore(release): prepare vX.Y.Z`
   - Push to feature branch

7. **Create PR**
   - Title: `chore(release): prepare vX.Y.Z`
   - Include changelog excerpt in description
   - Include a **publish-readiness report** summarizing step 5 results (which dry-runs ran, registry version checks)

8. **Monitor post-merge publishing** (after the human merges the PR):
   - Watch `release.yml` complete and confirm the GitHub Release + tag `vX.Y.Z` were created.
   - Watch `publish.yml`, `publish-python.yml`, `publish-js.yml`, and `cli-binaries.yml` to completion; surface any failure immediately.
   - Run the post-release verification commands (see "Post-Release Verification" below) and report which registries show the new version.
   - Only declare the release "shipped" when **all four** targets (crates.io, PyPI, npm, Homebrew) report the new version. If one fails, open a hotfix PR rather than leaving the release half-shipped.

### CI Automation

**On merge to main** (release.yml):
- Detects commit message `chore(release): prepare vX.Y.Z`
- Extracts release notes from CHANGELOG.md
- Creates GitHub Release with tag `vX.Y.Z`

**On GitHub Release published** (publish.yml, publish-js.yml, publish-python.yml):
- Publishes to crates.io, npm, and PyPI
- Each publish workflow includes a verification step that checks the published version matches expectations

## Pre-Release Checklist

The agent verifies before creating a release PR:

- [ ] All CI checks pass on main
- [ ] `cargo fmt` - code is formatted
- [ ] `cargo clippy` - no warnings
- [ ] `cargo test` - all tests pass
- [ ] CHANGELOG.md has entries for changes since last release
- [ ] Version is consistent across `Cargo.toml`, `crates/bashkit-cli/Cargo.toml`, `crates/bashkit-js/package.json`, and `Cargo.lock`
- [ ] `cargo publish --dry-run -p bashkit` succeeds
- [ ] `cargo publish --dry-run -p bashkit-cli` succeeds
- [ ] New version > latest published version on each registry (crates.io, PyPI, npm)

## Post-Merge Monitoring

After the release PR merges, the agent watches each publish target until it reports the new version (or fails):

| Target      | Workflow                | How to confirm                                            |
|-------------|-------------------------|-----------------------------------------------------------|
| GitHub Release | `release.yml`        | `gh release view vX.Y.Z`                                  |
| crates.io   | `publish.yml`           | `cargo search bashkit` shows `X.Y.Z`                      |
| PyPI        | `publish-python.yml`    | `pip index versions bashkit` includes `X.Y.Z`             |
| npm         | `publish-js.yml`        | `npm view @everruns/bashkit version` returns `X.Y.Z`      |
| Homebrew    | `cli-binaries.yml`      | `everruns/homebrew-tap` formula bumped to `X.Y.Z`         |

If a workflow fails, the agent inspects logs (`gh run view <run-id> --log-failed`), identifies root cause, and either re-runs the workflow (transient) or opens a hotfix PR (code/packaging bug — see v0.4.0 → v0.4.1 for a worked example).

## Changelog Format

Follow everruns/sdk changelog conventions with GitHub-style commit listings.

### Structure

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Highlights

- 2-5 bullet points summarizing the most impactful changes
- Focus on user-facing features and improvements
- Keep it concise and scannable

### Breaking Changes

- **Short description**: Detailed explanation of what changed and migration steps.
  - Before: `old_api()`
  - After: `new_api()`

### What's Changed

* feat(scope): description ([#83](https://github.com/everruns/bashkit/pull/83)) by @contributor
* fix(scope): description ([#82](https://github.com/everruns/bashkit/pull/82)) by @contributor

**Full Changelog**: https://github.com/everruns/bashkit/commits/vX.Y.Z
```

### Rules

- Add `### Highlights` section with 2-5 most impactful changes (user-facing summary)
- Use `### What's Changed` section (not separate Added/Changed/Fixed)
- List PRs in **descending order** (newest first, by PR number)
- Format: `* type(scope): description ([#N](URL)) by @author`
- End with `**Full Changelog**: URL`
- Add `### Breaking Changes` section for MINOR/MAJOR versions with migration guides

### Breaking Changes Section

Include when the release has breaking changes:

1. **Bold summary** of the breaking change
2. **Migration guide** showing before/after
3. **Code examples** if helpful

Example:
```markdown
### Breaking Changes

- **BashBuilder API changed**: The `with_fs` method now takes ownership.
  - Before: `builder.with_fs(&fs)`
  - After: `builder.with_fs(fs)`
```

## Package Names and Registries

- `bashkit` on crates.io (core library)
- `bashkit-cli` on crates.io (CLI tool)
- `bashkit` on PyPI (Python package, pre-built wheels)
- `@everruns/bashkit` on npm (JavaScript/TypeScript package, native NAPI-RS bindings)

## Publishing Order

Crates must be published in dependency order:

1. `bashkit` (core library, no internal deps)
2. `bashkit-cli` (depends on bashkit)

Python wheels are published independently (no crates.io dependency). This
includes the native platform matrix plus a reduced-feature Pyodide/Emscripten
wheel (`wasm32-unknown-emscripten`); see `specs/emscripten-wheels.md`.

npm packages are published independently (no crates.io dependency).

The CI workflows handle this automatically on GitHub Release.

## Workflows

### release.yml

- **Trigger**: Push to `main` with commit message starting with `chore(release): prepare v`
- **Actions**: Creates GitHub Release with tag and release notes from CHANGELOG, verifies manual runs are from `refs/heads/main`, verifies the release source is reachable from `origin/main`, verifies the tag points at the release commit, then dispatches publish and binary build workflows
- **File**: `.github/workflows/release.yml`

### cli-binaries.yml

- **Trigger**: Dispatched by release.yml after GitHub Release is created and its tag is verified
- **Validation**: Accepts only stable `vX.Y.Z` tags, checks the tag matches `Cargo.toml`, checks the workflow ref resolves to the tag, and verifies the tag commit is reachable from `origin/main` before building artifacts
- **Actions**: Builds prebuilt CLI binaries for macOS (ARM64, x86_64) and Linux (x86_64), uploads to GitHub Release, updates Homebrew formula
- **File**: `.github/workflows/cli-binaries.yml`
- **Secret required**: `DOPPLER_TOKEN` (for Homebrew tap push via Doppler-managed GitHub PAT)

#### CLI binary matrix

| OS | Target | Runner |
|----|--------|--------|
| macOS | aarch64-apple-darwin | macos-latest |
| macOS | x86_64-apple-darwin | macos-latest |
| Linux | x86_64-unknown-linux-gnu | ubuntu-latest |

#### Homebrew

After binaries are built, the workflow generates a Homebrew formula and pushes it to `everruns/homebrew-tap`. Users install via:

```bash
brew install everruns/tap/bashkit
```

### publish.yml

- **Trigger**: GitHub Release published
- **Actions**: Publishes to crates.io in dependency order, then verifies published versions
- **File**: `.github/workflows/publish.yml`
- **Secret required**: `CARGO_REGISTRY_TOKEN`

### publish-python.yml

- **Trigger**: GitHub Release published (runs in parallel with publish.yml)
- **Actions**: Builds pre-compiled wheels for all platforms, smoke-tests, publishes to PyPI
- **File**: `.github/workflows/publish-python.yml`
- **Auth**: PyPI trusted publishing (OIDC, no secrets needed)
- **Environment**: `release-python` (must exist in GitHub repo settings)

### publish-js.yml

- **Trigger**: GitHub Release published (runs in parallel with publish.yml and publish-python.yml)
- **Actions**: Builds native NAPI-RS bindings for all platforms, tests on Node 20/22, publishes to npm
- **File**: `.github/workflows/publish-js.yml`
- **Secret required**: `NPM_TOKEN` (npm access token)
- **Auth**: `id-token: write` for npm provenance (OIDC attestation), same pattern as everruns/sdk

#### JS native binding matrix

| OS | Target | Runner |
|----|--------|--------|
| macOS | x86_64-apple-darwin | macos-latest |
| macOS | aarch64-apple-darwin | macos-latest |
| Linux | x86_64-unknown-linux-gnu | ubuntu-latest |
| Linux | aarch64-unknown-linux-gnu | ubuntu-24.04-arm |
| Windows | x86_64-pc-windows-msvc | windows-latest |
| WASM | wasm32-wasip1-threads | ubuntu-latest |

Node.js versions tested: 20, 22, 24

#### JS version sync

JS package version is synced from `Cargo.toml` workspace version via `build.rs`.
The build script updates `package.json` automatically when the Cargo version changes.

#### Wheel matrix

| OS | Architecture | Variant |
|----|-------------|---------|
| Linux | x86_64 | manylinux (glibc) |
| Linux | aarch64 | manylinux (glibc) |
| Linux | x86_64 | musllinux |
| Linux | aarch64 | musllinux |
| macOS | x86_64 | universal |
| macOS | aarch64 (Apple Silicon) | universal |
| Windows | x86_64 | MSVC |

Python versions: 3.9, 3.10, 3.11, 3.12, 3.13, 3.14

#### Version sync

Python package version is read dynamically from `Cargo.toml` via maturin
(`dynamic = ["version"]` in pyproject.toml). No manual version sync needed.

## Authentication

**Required Secrets** (GitHub Settings > Secrets > Actions):

- `CARGO_REGISTRY_TOKEN`: crates.io API token
  - Generate at: https://crates.io/settings/tokens
  - Scope: Publish new crates, Publish updates

**PyPI Trusted Publishing** (no secret needed):

- Configure at: https://pypi.org/manage/project/bashkit/settings/publishing/
- Add publisher: GitHub, repo `everruns/bashkit`, workflow `publish-python.yml`, environment `release-python`

**npm Publishing** (same pattern as everruns/sdk):

- `NPM_TOKEN`: npm access token (GitHub Settings > Secrets > Actions)
  - Generate at: https://www.npmjs.com/settings/~/tokens
  - Type: Automation
- Provenance enabled via `id-token: write` OIDC permission + `--provenance` flag
- No separate GitHub environment required

## Example Conversation

```
Human: Create release v0.2.0

Agent: I'll prepare the v0.2.0 release. Let me:
1. Update CHANGELOG.md with the v0.2.0 section
2. Update Cargo.toml version to 0.2.0
3. Run verification checks
4. Create the release PR

[Agent performs steps...]

Done. PR created: https://github.com/everruns/bashkit/pull/XX
Please review and merge to trigger the release.
```

## Post-Release Verification

Each publish workflow includes automated verification. After a release, the agent (or human) should also verify manually:

```bash
# crates.io
cargo search bashkit           # Should show latest version
cargo search bashkit-cli       # Should show latest version

# npm
npm view @everruns/bashkit version          # Should show latest version
npm dist-tags ls @everruns/bashkit          # "latest" should point to new version

# PyPI
pip index versions bashkit     # Should show latest version

# GitHub
gh release view --repo everruns/bashkit     # Should show latest tag
```

If any registry is missing the new version, check the corresponding publish workflow run for errors.

## Hotfix Releases

For urgent fixes:

1. Ask agent: "Create patch release v0.1.1 for the security fix"
2. Agent prepares release with patch version
3. Review and merge

## Rollback Procedure

Yanking a crate version (use sparingly):

```bash
cargo yank --version 0.1.0 bashkit
cargo yank --version 0.1.0 bashkit-cli
```

Note: Yanked versions can still be used by existing Cargo.lock files but won't be selected for new projects.

## Release Artifacts

Each release includes:

- **GitHub Release**: Tag, release notes, source archives, prebuilt CLI binaries (macOS ARM64/x86_64, Linux x86_64)
- **crates.io**: Published crates for `cargo add bashkit`
- **PyPI**: Pre-built wheels for `pip install bashkit`
- **npm**: Native NAPI-RS bindings for `npm install @everruns/bashkit`
- **Homebrew**: Formula at `everruns/homebrew-tap` for `brew install everruns/tap/bashkit`
