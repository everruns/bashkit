# Git Support

## Status
Phases 1-3 implemented (Phase 2 remote ops: virtual mode — URL validation only)

## Decision

Virtual git operations behind the `git` feature flag. All git operations work
on the virtual filesystem only. Configure via
`Bash::builder().git(GitConfig::new().author(name, email))`.

### Supported Commands

- Phase 1 (local): `init`, `config`, `add`, `commit -m`, `status`, `log [-n N]`
- Phase 2 (remote, virtual mode): `remote [-v]`, `remote add/remove` (fully
  functional); `clone`/`push`/`pull`/`fetch` validate URL against allowlist
  then return virtual-mode messages (no network in VFS-only mode)
- Phase 3 (advanced): `branch [-d]`, `checkout [-b]`, `diff` (simplified),
  `reset [--soft|--mixed|--hard]`
- Future: `merge`, `rebase`, `stash`

### Security

See `specs/threat-model.md` Section 8: Git Security (TM-GIT-*)

#### Key Mitigations

| Threat | Mitigation |
|--------|------------|
| TM-GIT-002: Host identity leak | Configurable virtual identity |
| TM-GIT-003: Host config access | No host filesystem access |
| TM-GIT-004: Credential theft | No host filesystem access |
| TM-GIT-005: Repository escape | All paths in VFS |
| TM-GIT-007: Many git objects | FS file count limit |
| TM-GIT-008: Deep history | Log limit parameter |
| TM-GIT-009: Large pack files | FS size limits |

#### Remote Operations (Phase 2)

- Remote URLs require explicit allowlist (`GitConfig::allow_remote(pattern)`)
- HTTPS only (no SSH, no git:// protocol)
- Virtual mode: URL validation only, no actual network operations

### API

`GitConfig` (`new()`, `author()`, `allow_remote()`) and
`BashBuilder::git(config)` (feature-gated): see
`crates/bashkit/src/builtins/git/config.rs` / rustdoc.

### Implementation

Phase 1 uses a simplified storage format in the VFS:

- `.git/HEAD` - Current branch reference
- `.git/config` - Repository configuration (INI format)
- `.git/index` - Staged files (newline-separated paths)
- `.git/commits` - Commit history (pipe-separated fields)
- `.git/refs/heads/<branch>` - Branch references

Gives full VFS isolation, correct user-facing behavior, foundation for future
`gitoxide` (gix) integration (real network ops, full git object format,
remote allowlist enforcement).

### Testing

Tests live under `crates/bashkit/tests/integration/`:
`git_integration_tests.rs` (Phase 1), `git_security_tests.rs` (TM-GIT-*),
`git_remote_security_tests.rs` (Phase 2), `git_advanced_tests.rs` (Phase 3),
`git_inspection_tests.rs`.

## See Also

- `specs/threat-model.md` - Security threats and mitigations
- `specs/builtins.md` - Builtin command reference
- `crates/bashkit/src/builtins/git/` - Implementation
