# Release Process

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
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Human asks     │     │  Agent creates  │     │  GitHub         │     │  crates.io      │
│  "release v0.2" │────>│  release PR     │────>│  Release        │────>│  Publish        │
│                 │     │                 │     │  (automatic)    │     │  (automatic)    │
└─────────────────┘     └─────────────────┘     └─────────────────┘     └─────────────────┘
```

### Human Steps

1. **Ask the agent** to create a release:
   - "Create release v0.2.0"
   - "Prepare a patch release"
   - "Release the current changes as v0.2.0"

2. **Review the PR** created by the agent

3. **Merge to main** - CI handles GitHub Release and crates.io publish

### Agent Steps (automated)

When asked to create a release, the agent:

1. **Determine version**
   - Use version specified by human, OR
   - Suggest next version based on changes (patch/minor/major)

2. **Update CHANGELOG.md**
   - Add release date: `## [X.Y.Z] - YYYY-MM-DD`
   - Add breaking changes section if applicable (see format below)
   - List PRs in descending order with GitHub-style links and contributors
   - End with `**Full Changelog**: URL`

3. **Update Cargo.toml**
   - Set `version = "X.Y.Z"` in workspace

4. **Run verification**
   - `cargo fmt --check`
   - `cargo clippy`
   - `cargo test`

5. **Commit and push**
   - Commit message: `chore(release): prepare vX.Y.Z`
   - Push to feature branch

6. **Create PR**
   - Title: `chore(release): prepare vX.Y.Z`
   - Include changelog excerpt in description

### CI Automation

**On merge to main** (release.yml):
- Detects commit message `chore(release): prepare vX.Y.Z`
- Extracts release notes from CHANGELOG.md
- Creates GitHub Release with tag `vX.Y.Z`

**On GitHub Release published** (publish.yml):
- Publishes to crates.io in dependency order
- Note: No verification step - CI already ran when PR merged to main

## Pre-Release Checklist

The agent verifies before creating a release PR:

- [ ] All CI checks pass on main
- [ ] `cargo fmt` - code is formatted
- [ ] `cargo clippy` - no warnings
- [ ] `cargo test` - all tests pass
- [ ] CHANGELOG.md has entries for changes since last release

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

## Publishing Order

Crates must be published in dependency order:

1. `bashkit` (core library, no internal deps)
2. `bashkit-cli` (depends on bashkit)

The CI workflow handles this with a dependency chain and wait for index update.

## Workflows

### release.yml

- **Trigger**: Push to `main` with commit message starting with `chore(release): prepare v`
- **Actions**: Creates GitHub Release with tag and release notes from CHANGELOG
- **File**: `.github/workflows/release.yml`

### publish.yml

- **Trigger**: GitHub Release published
- **Actions**: Publishes to crates.io (no verification - CI ran on merge)
- **File**: `.github/workflows/publish.yml`
- **Secret required**: `CARGO_REGISTRY_TOKEN`

## Authentication

**Required Secrets** (GitHub Settings > Secrets > Actions):

- `CARGO_REGISTRY_TOKEN`: crates.io API token
  - Generate at: https://crates.io/settings/tokens
  - Scope: Publish new crates, Publish updates

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

- **GitHub Release**: Tag, release notes, source archives
- **crates.io**: Published crates for `cargo add bashkit`
