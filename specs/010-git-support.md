# 010: Git Support

## Status
Phase 1: Implemented
Phase 2: Implemented (sandbox mode - URL validation only)
Phase 3: Implemented (branch, checkout, diff, reset)

## Decision

BashKit provides sandboxed git operations via the `git` feature flag.
All git operations work on the virtual filesystem only.

### Feature Flag

Enable with:
```toml
[dependencies]
bashkit = { version = "0.1", features = ["git"] }
```

### Configuration

```rust
use bashkit::{Bash, GitConfig};

let bash = Bash::builder()
    .git(GitConfig::new()
        .author("Deploy Bot", "deploy@example.com"))
    .build();
```

### Supported Commands

#### Phase 1 (Local) - Implemented

| Command | Description |
|---------|-------------|
| `git init [path]` | Create empty repository |
| `git config [key] [value]` | Get/set repository config |
| `git add <pathspec>...` | Stage files |
| `git commit -m <message>` | Record changes |
| `git status` | Show working tree status |
| `git log [-n N]` | Show commit history |

#### Phase 2 (Remote) - Implemented (Sandbox Mode)

Remote operations validate URLs against the allowlist but return sandbox
mode messages (actual network operations not supported in VFS-only mode).

| Command | Description |
|---------|-------------|
| `git remote` | List remotes (names only) |
| `git remote -v` | List remotes with URLs |
| `git remote add <name> <url>` | Add remote (validates URL) |
| `git remote remove <name>` | Remove remote |
| `git clone <url> [path]` | Validates URL, returns sandbox message |
| `git push [remote]` | Validates URL, returns sandbox message |
| `git pull [remote]` | Validates URL, returns sandbox message |
| `git fetch [remote]` | Validates URL, returns sandbox message |

#### Phase 3 (Advanced) - Implemented

| Command | Description |
|---------|-------------|
| `git branch [-d] [name]` | List, create, or delete branches |
| `git checkout [-b] <branch\|commit>` | Switch branches or create and switch |
| `git diff [from] [to]` | Show changes (simplified in sandbox mode) |
| `git reset [--soft\|--mixed\|--hard]` | Reset HEAD and clear staging |

#### Future (Not Yet Implemented)

| Command | Description |
|---------|-------------|
| `git merge <branch>` | Merge branches |
| `git rebase <branch>` | Rebase commits |
| `git stash [push\|pop\|list]` | Stash changes |

### Security

See `specs/006-threat-model.md` Section 8: Git Security (TM-GIT-*)

#### Key Mitigations

| Threat | Mitigation |
|--------|------------|
| TM-GIT-002: Host identity leak | Configurable sandbox identity |
| TM-GIT-003: Host config access | No host filesystem access |
| TM-GIT-004: Credential theft | No host filesystem access |
| TM-GIT-005: Repository escape | All paths in VFS |
| TM-GIT-007: Many git objects | FS file count limit |
| TM-GIT-008: Deep history | Log limit parameter |
| TM-GIT-009: Large pack files | FS size limits |

#### Remote Operations (Phase 2)

- Remote URLs require explicit allowlist
- HTTPS only (no SSH, no git:// protocol)
- Sandbox mode: URL validation only, actual network operations not supported
- `git remote add/remove` fully functional for managing remote references
- `git clone/push/pull/fetch` validate URLs then return helpful messages

### API Design

#### GitConfig

```rust
/// Git configuration for BashKit.
pub struct GitConfig { ... }

impl GitConfig {
    /// Create new config with default sandbox identity.
    pub fn new() -> Self;

    /// Set author name and email for commits.
    pub fn author(self, name: &str, email: &str) -> Self;

    /// Allow a remote URL pattern (Phase 2).
    pub fn allow_remote(self, pattern: &str) -> Self;
}
```

#### BashBuilder Extension

```rust
impl BashBuilder {
    /// Configure git support.
    #[cfg(feature = "git")]
    pub fn git(self, config: GitConfig) -> Self;
}
```

### Implementation

#### Phase 1 Architecture

For Phase 1, git operations are implemented using a simplified storage
format in the VFS:

- `.git/HEAD` - Current branch reference
- `.git/config` - Repository configuration (INI format)
- `.git/index` - Staged files (newline-separated paths)
- `.git/commits` - Commit history (pipe-separated fields)
- `.git/refs/heads/<branch>` - Branch references

This approach provides:
- Full VFS sandboxing
- Security-focused implementation
- Correct user-facing behavior
- Foundation for Phase 2 gitoxide integration

#### Future Phases

Phase 2 will integrate `gitoxide` (gix) crate for:
- Network operations (clone, push, pull, fetch)
- Full git object format compatibility
- Remote URL allowlist enforcement

### Testing

Tests are organized by category:

| Test File | Purpose |
|-----------|---------|
| `tests/git_integration_tests.rs` | Phase 1 functional tests |
| `tests/git_security_tests.rs` | TM-GIT-* threat tests |
| `tests/git_remote_security_tests.rs` | Phase 2 remote security tests |
| `tests/git_advanced_tests.rs` | Phase 3 advanced operation tests |

Run tests:
```bash
# Run git tests
cargo test --features git git_

# Run security tests
cargo test --features git,failpoints git_security
```

### Verification

```bash
# Build with git feature
cargo build --features git

# Run all git tests
cargo test --features git

# Pre-PR checks
just pre-pr
```

## See Also

- `specs/006-threat-model.md` - Security threats and mitigations
- `specs/005-builtins.md` - Builtin command reference
- `crates/bashkit/src/git/` - Implementation
