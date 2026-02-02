# Release Process

## Versioning Strategy

BashKit uses semantic versioning:
- **Major**: Breaking API changes
- **Minor**: New features, new builtins
- **Patch**: Bug fixes, documentation updates

## Package Names and Registries

- `bashkit` on crates.io (core library)
- `bashkit-cli` on crates.io (CLI tool)

## Release Process Steps

1. Update version in `Cargo.toml` workspace
2. Update CHANGELOG.md with release notes
3. Create git tag: `git tag v0.1.0`
4. Push tag: `git push origin v0.1.0`
5. CI publishes to crates.io automatically

## Quick Release Commands

```bash
# Prepare release (updates version, opens changelog for editing)
just release-prepare 0.1.0

# After editing CHANGELOG.md, create and push tag
just release-tag 0.1.0
```

## CI Workflow Configuration

The publish workflow triggers on version tags:

```yaml
on:
  push:
    tags:
      - "v*"
```

## Authentication

**Required Secrets** (GitHub Settings > Secrets > Actions):

- `CARGO_REGISTRY_TOKEN`: crates.io API token
  - Generate at: https://crates.io/settings/tokens
  - Scope: Publish new crates, Publish updates

## Changelog Format

Follow everruns/sdk changelog conventions:

```markdown
## [0.1.0] - 2026-02-02

### What's Changed

* feat(scope): description ([#83](https://github.com/everruns/bashkit/pull/83)) by @author
* feat(scope): description ([#82](https://github.com/everruns/bashkit/pull/82)) by @author
...

**Full Changelog**: https://github.com/everruns/bashkit/commits/v0.1.0
```

**Rules:**
- Use `### What's Changed` section (not separate Added/Changed/Fixed)
- List PRs in **descending order** (newest first, by PR number)
- Format: `* type(scope): description ([#N](URL)) by @author`
- End with `**Full Changelog**: URL`

## Pre-release Checklist

1. [ ] All CI checks pass on main
2. [ ] CHANGELOG.md updated with release notes
3. [ ] Version bumped in workspace Cargo.toml
4. [ ] `cargo publish --dry-run` succeeds for all crates
5. [ ] Examples work with new version
6. [ ] Documentation is up to date

## Publishing Order

Crates must be published in dependency order:

1. `bashkit` (core library, no internal deps)
2. `bashkit-cli` (depends on bashkit)

The CI workflow handles this with a dependency chain.

## Rollback Procedure

Yanking a crate version (use sparingly):

```bash
cargo yank --version 0.1.0 bashkit
cargo yank --version 0.1.0 bashkit-cli
```

Note: Yanked versions can still be used by existing Cargo.lock files but won't be selected for new projects.
