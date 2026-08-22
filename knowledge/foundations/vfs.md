---
type: Subsystem Design
title: Virtual Filesystem
description: Filesystem abstraction, path safety, implementations, and sandbox invariants.
tags:
  - bashkit
  - filesystem
  - sandbox
---

# Virtual Filesystem Design

## Status
Implemented

## Decision

Two-layer filesystem abstraction:

| Layer | Trait/Type | Responsibility |
|-------|------------|----------------|
| Backend | `FsBackend` | Raw storage operations (minimal contract) |
| POSIX | `FileSystem` / `PosixFs` | POSIX-like semantics enforcement |

`FsBackend` handles raw storage without enforcing POSIX semantics; wrap with
`PosixFs` for type-safe behavior. See `crates/bashkit/src/fs/` for trait
definitions and implementations.

### Which Trait Should I Implement?

```
Do you need a custom filesystem?
    │
    ├─ NO → Use InMemoryFs (default with Bash::new())
    │
    └─ YES → Is your storage simple (key-value, database, cloud)?
              │
              ├─ YES → Implement FsBackend + wrap with PosixFs
              │        (POSIX checks are automatic, less code)
              │
              └─ NO → Implement FileSystem directly
                      (full control, you handle all checks)
```

| Approach | Implement | POSIX Checks | Best For |
|----------|-----------|--------------|----------|
| `FsBackend` + `PosixFs` | Raw storage only | Automatic | Databases, cloud, key-value stores |
| `FileSystem` directly | Everything | Manual | Complex caching, custom semantics |

### Implementations

#### InMemoryFs
- `HashMap<PathBuf, FsEntry>`, thread-safe via `RwLock`; no persistence
- Initial directories: `/`, `/tmp`, `/home`, `/home/user`, `/dev`
- Special handling for `/dev/null`, `/dev/urandom`, `/dev/random`
- Mount files at build time via `BashBuilder::mount_text()` /
  `mount_readonly_text()`

#### OverlayFs
- Copy-on-write layer over another FileSystem, whiteout tracking for deletes
- Useful for: temp modifications, testing, isolation
- Uses the same POSIX-rooted path normalization as `InMemoryFs` on every host.
  On Windows, `\` is accepted as an alternate separator, host drive/UNC/device
  prefixes are discarded, and VFS lookup remains case-sensitive.

#### MountableFs
- Mount multiple filesystems at different paths
- Longest-prefix matching for nested mounts
- Always used as outermost FS layer for live mount/unmount support
- File and symlink copies and moves may cross mounts; failed moves restore the
  previous destination before returning the source-side error

#### NamespaceFs
- Static visible tree composed from arbitrary `FileSystem` instances
- Builder supports absolute targets, source-root rebasing, and read-only or
  read-write access per mount
- Longest target-prefix wins deterministically for nested mounts
- Missing ancestors and mount points are visible as synthetic directories;
  `stat()` and `read_dir()` metadata agree through rebasing
- File and symlink copies may cross mounts when the destination is writable
- Cross-mount rename returns `ErrorKind::CrossesDevices` instead of non-atomic
  copy-delete; cross-mount directory and FIFO copy is unsupported
- Visible paths are normalized before mount selection and source-root joining,
  preventing traversal, source-root escape, nested-mount escape, and read-only bypass
- Object ownership defines lifetime; there is no command/session lifetime mode

#### ReadOnlyFs
- Wraps another `FileSystem`, delegates read/stat/list, denies all mutations
  with `PermissionDenied`
- Useful for inspection-only tool sessions where even in-memory writes to
  `/tmp`, redirections, `cp`, `mv`, `mkdir`, `rm`, and `chmod` must fail

#### RealFs (Optional, `realfs` feature)
- Direct access to a host directory as an `FsBackend`
- Two modes: `ReadOnly` (safe) and `ReadWrite` (dangerous)
- Async backend operations use Tokio filesystem APIs and never perform
  synchronous host filesystem I/O on the runtime worker
- `RealFs::open` is the async-safe constructor and preserves root validation
  and canonicalization semantics without blocking a current-thread runtime
- The synchronous `RealFs::new` constructor is deprecated and retained only as
  a migration shim
- Path traversal prevented via canonicalization + root prefix check
- New-path writes canonicalize the nearest existing ancestor before attaching a
  missing suffix, blocking symlink escapes through non-existent subpaths
- Windows drive-relative, drive-absolute, UNC, and device-style virtual paths
  are normalized into the POSIX VFS root before host joining. Existing-path and
  missing-descendant checks follow symlinks, junctions, and other reparse points
  before applying component-aware root containment; host lookup itself retains
  Windows case-insensitive behavior.
- Windows `RealFs::symlink()` does not create a host reparse point; it creates an
  empty file after applying target-containment validation. Pre-existing host
  symlinks and junctions are supported and containment-checked.
- Replacement write, append, and copy stage a sibling file and rename it only
  after a complete flush, so partial host I/O retains the prior destination;
  append streams existing bytes into staging instead of buffering the host file
- Copy and rename act on a symlink entry itself instead of dereferencing it
- Builder: `mount_real_readonly[_at]()`, `mount_real_readwrite()`; CLI:
  `--mount-ro` / `--mount-rw` (`host:vfs` syntax for mount point)

#### Live Mount/Unmount

Every `Bash` instance wraps its filesystem stack in a `MountableFs`, enabling
post-build `bash.mount(path, fs)` / `bash.unmount(path)` without rebuilding
the interpreter.

### FS Layering Stack

```text
┌──────────────────────────────────┐
│  MountableFs (live mounts)       │  ← Bash::mount() / unmount()
├──────────────────────────────────┤
│  ReadOnlyFs (optional)           │  ← BashBuilder::readonly_filesystem()
├──────────────────────────────────┤
│  OverlayFs (text mounts)         │  ← BashBuilder::mount_text()
├──────────────────────────────────┤
│  MountableFs (real mounts)       │  ← BashBuilder::mount_real_*_at()
├──────────────────────────────────┤
│  Base filesystem                 │  ← InMemoryFs or custom
└──────────────────────────────────┘
```

`NamespaceFs` can be supplied as the base when callers need a bounded,
pre-composed tree. The usual outer `MountableFs` still enables later live mounts.

### Special Device Files

#### /dev/null
Handled at the **interpreter level**, not filesystem. Security-critical: custom
filesystem implementations cannot intercept `/dev/null` behavior. Path
normalization handles bypass attempts.

#### /dev/urandom and /dev/random
Handled at filesystem level: return 8192 bytes of random data per read
(bounded to prevent memory growth).

### File Size Reporting

`Metadata.size` must be correct for `ls -l`, `stat`, `test -s`:
- Regular files: actual content length
- Empty files: 0
- Directories: always 0
- Both `stat()` and `read_dir()` must return consistent sizes

### POSIX Semantics Contract

All `FileSystem` implementations MUST enforce:
1. No duplicate names (file and dir can't share path)
2. Type-safe operations (`write_file` on dir → error)
3. Parent directory requirement (exception: `mkdir -p`)
4. Failed write/copy/rename leaves contents, entry types, and usage unchanged
5. Cross-backend rename restores the destination on failure, or is rejected
   before mutation when that rollback contract cannot be supported

### Security Conformance Certification

`tests/support/filesystem_security_conformance.rs` is the shared, private
certification helper. The consolidated integration suite runs it against
`InMemoryFs`, `OverlayFs`, `MountableFs`, and read-write `NamespaceFs`; the
feature-isolated RealFs job runs the same helper through `PosixFs<RealFs>`.
Wrapper-specific cases additionally certify `ReadOnlyFs`, mount boundaries,
symlink identity, quota rollback, normalized errors, type-conflict rollback,
archive preflight, and injected partial-write failures. This remains test-only
instead of expanding the public API; external `FileSystem` authors follow the
trait's atomicity contract.

### Pathname Expansion Against the VFS

`Interpreter::expand_glob` (`interpreter/glob.rs`) globs **every** path
component, not just the trailing one, so mounted trees are fully addressable:
`cat /skills/*/SKILL.md` works against a read-only skills mount the same way it
does in bash.

Rules that fall out of the per-component walk:
- Non-final components only match directories, a regular file sharing the
  prefix is never descended into.
- The dotfile rule is applied per component: a component matches names starting
  with `.` only when `dotglob` is set or that component literally starts with `.`.
- Lookup uses the normalized absolute path while the emitted word is rebuilt
  from the caller's spelling, so `./` and `../` prefixes survive expansion.
- No match anywhere in the walk falls back to the literal pattern, or to nothing
  under `nullglob`, same as a trailing-component miss.
- `**` with `globstar` is handled separately by `expand_glob_recursive`.
- THREAT[TM-DOS-095]: the candidate set multiplies per component, so patterns
  deeper than `FsLimits::max_path_depth` are rejected and the live candidate set
  is capped at `FsLimits::max_file_count`.

Known gap: backslash-escaped metacharacters (`echo /skills/\*`) still expand
instead of staying literal, the backslash is dropped before pathname expansion
runs. Pre-dates per-component expansion and affects trailing components too.

### Symlink Handling

Symlinks are stored but intentionally not followed for security:
- Prevents symlink escape attacks (TM-ESC-002)
- Prevents symlink loop DoS (TM-DOS-011)

## Host Mount Table (`HostMounts`)

`realfs` mounts are recorded as a `HostMounts` table on the `Bash` instance:
`Bash::host_mounts()` lists them, `Bash::host_path_for(vfs)` maps a VFS path
back to the host path backing it. Mount points and lookup paths use the shared
POSIX VFS normalizer before longest-prefix selection and host joining. Thus
`.`/`..` cannot select a mount under their unnormalized spelling or survive in
the suffix passed to the host OS (TM-ESC-034).

Decision: published because embedders that bridge commands to host processes
must map a VFS cwd to a host directory to spawn in, and hand-rolling it is a
trap, a naive string prefix match puts `/workspace2` inside `/workspace`.
`HostMounts::resolve` matches whole path components and prefers the longest
match, so a specific mount beats a root overlay.

Rules:

- Only mounts that actually applied are recorded. A path skipped by the
  allowlist or a failed `canonicalize` is absent, so the table never reports a
  host path for something that was never mounted.
- `host_path` is the canonicalized host directory, which may differ from the
  path passed to the builder (symlinks, `/tmp` → `/private/tmp` on macOS).
- A root overlay mount (`mount_real_readonly` with no VFS path) is recorded at
  `/`, so it acts as the catch-all. Without one, an uncovered path resolves to
  `None`.
- `None` means "no mount covers this". Callers must treat it as an error rather
  than falling back to a default directory: spawning a host process in the
  wrong directory is worse than refusing to spawn it.
- `HostMounts::new` lets an embedder build the table up front. A
  `CommandResolver` is passed *into* the builder, so builtins it produces
  cannot call `Bash::host_mounts()` on an instance that does not exist yet;
  sharing one `Arc` between the resolver and the `mount_real_*` calls makes
  both agree by construction.

Tests: `crates/bashkit/tests/integration/host_mounts_tests.rs`.

## Binding API Parity

All language bindings must expose the same filesystem concepts:

```
files:  { "/path": "content" }                # text files (writable, in-memory)
mounts: [{ host_path, vfs_path?, writable? }] # real FS (read-only by default)
readonly_filesystem: bool                     # deny all VFS mutations after setup
FileSystem()                                  # standalone in-memory filesystem
FileSystem.real(host_path, writable=false)    # standalone real filesystem
                                              # JS requires allowed_mount_paths
```

Runtime methods:
- host-path mount: `mount(host_path, vfs_path, writable=false)`
- filesystem mount: `mount(vfs_path, filesystem)`
- `unmount(vfs_path)`

Native-extension interop is binding-specific but must preserve bashkit-owned
filesystem objects when crossing the language runtime boundary:
- Python: `FileSystem.from_capsule(capsule)`, `FileSystem.to_capsule()`
- Node.js: `FileSystem.fromExternal(external)`, `FileSystem.toExternal()`

Interop contract:
- The native Rust contract lives at `bashkit::interop::fs` behind the
  `interop` cargo feature
- The cross-addon payload must be a versioned `repr(C)` handle + vtable
- Do not expose `Arc<dyn FileSystem>` or any addon-private Rust layout
- Python capsules carry the stable owned handle directly
- Node interop values carry stable handle bytes plus an owner token
- On import, bashkit reconstructs a binding-owned `FileSystem` wrapper from the
  stable handle payload

Safety: real mounts are **read-only by default**. Text files are writable
(sandboxed) unless the final session is wrapped with `readonly_filesystem`.

## Alternatives Considered

- Real filesystem with chroot: rejected, requires root, not portable, no WASM.
- tokio::fs wrapper: rejected, always hits real FS, can't isolate or virtualize.

## See also

- [Bashkit Architecture](architecture.md), how the VFS is owned and shared
- [Threat Model](../security/threat-model.md), path-escape threats the sandbox invariants answer
- [Git Support](../integrations/git-support.md), Git operations layered on the VFS
- [SQLite Builtin](../runtimes/sqlite-builtin.md), VfsIO backend bridging SQLite onto the VFS
- [Python Package](../runtimes/python-package.md), binding-side mount API parity
