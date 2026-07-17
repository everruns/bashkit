# Virtual filesystem

Every Bashkit script runs against an in-memory **virtual filesystem** (VFS), not
the host disk. `cat`, `ls`, `cp`, redirections, `mkdir` — they all work exactly
as a script expects, but the bytes live in memory and disappear when the
interpreter is dropped. Path traversal like `../../../etc/passwd` is normalised
away, and symlinks are stored but never followed. The host is invisible by
default; you grant access deliberately, never by accident.

This is the foundation of the [security model](security.md): there is no real
filesystem to escape to unless you mount one.

## The layering stack

A `Bash` instance composes its filesystem from layers. Each layer wraps the one
below it, so you can stack read-only enforcement, text mounts, and host mounts
over an in-memory base — and swap mounts at runtime.

<svg viewBox="0 0 560 320" role="img" aria-label="Filesystem layering stack from MountableFs at the top down to the base filesystem" xmlns="http://www.w3.org/2000/svg" style="max-width:100%;height:auto;margin:1rem 0;">
  <g font-family="ui-monospace,monospace" font-size="13">
    <rect x="40" y="16" width="280" height="44" rx="4" fill="#0a1636"/>
    <text x="180" y="42" text-anchor="middle" fill="#ffffff">MountableFs · live mounts</text>
    <rect x="40" y="68" width="280" height="44" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
    <text x="180" y="94" text-anchor="middle" fill="#0a1636">ReadOnlyFs · optional</text>
    <rect x="40" y="120" width="280" height="44" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
    <text x="180" y="146" text-anchor="middle" fill="#0a1636">OverlayFs · text mounts</text>
    <rect x="40" y="172" width="280" height="44" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
    <text x="180" y="198" text-anchor="middle" fill="#0a1636">MountableFs · real mounts</text>
    <rect x="40" y="224" width="280" height="44" rx="4" fill="#fff" stroke="#d4a43a" stroke-width="1.5"/>
    <text x="180" y="250" text-anchor="middle" fill="#0a1636">base · InMemoryFs or custom</text>

    <g font-size="11" fill="#404040">
      <text x="338" y="42">Bash::mount() / unmount()</text>
      <text x="338" y="94">readonly_filesystem()</text>
      <text x="338" y="146">mount_text()</text>
      <text x="338" y="198">mount_real_*_at()</text>
      <text x="338" y="250">Bash::new()</text>
    </g>
    <g stroke="#0a1636" stroke-opacity="0.25">
      <line x1="180" y1="60" x2="180" y2="68"/>
      <line x1="180" y1="112" x2="180" y2="120"/>
      <line x1="180" y1="164" x2="180" y2="172"/>
      <line x1="180" y1="216" x2="180" y2="224"/>
    </g>
  </g>
</svg>

## Two-layer trait model

Internally the VFS splits raw storage from POSIX semantics:

| Layer | Trait | Responsibility |
|-------|-------|----------------|
| Backend | `FsBackend` | Raw storage operations (minimal contract) |
| POSIX | `FileSystem` / `PosixFs` | POSIX-like semantics (no duplicate names, type-safe ops, parent-dir rules) |

If you want a custom backend (a database, object store, key-value store),
implement the small `FsBackend` and wrap it in `PosixFs` — the POSIX checks come
for free. Implement `FileSystem` directly only when you need full control over
semantics.

## Built-in implementations

| Implementation | Purpose |
|----------------|---------|
| **InMemoryFs** | Default (`Bash::new()`). `HashMap`-backed, thread-safe, no persistence. Seeds `/`, `/tmp`, `/home`, `/home/user`, `/dev`. |
| **OverlayFs** | Copy-on-write over another filesystem, with whiteout tracking for deletes. |
| **MountableFs** | Mount multiple filesystems at different paths (longest-prefix match). Always the outermost layer, enabling [live mounts](live-mounts.md). |
| **NamespaceFs** | Compose a static visible tree from rebased filesystem subtrees, with per-mount access and synthetic ancestors. |
| **ReadOnlyFs** | Delegates reads, denies every mutation with `PermissionDenied` — even writes to `/tmp`, `cp`, `mv`, `rm`, `chmod`. For inspection-only sessions. |
| **RealFs** (`realfs` feature) | Direct access to a host directory. Read-only (safe) or read-write (dangerous); path traversal blocked by canonicalisation + root-prefix checks. |

## Mounting host directories

Real host access is **opt-in and read-only by default**. The `realfs` feature
adds builder and CLI entry points:

```rust,no_run
use bashkit::Bash;

# fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .mount_real_readonly("/host/data")        // visible read-only inside the VFS
    .build();
# let _ = bash;
# Ok(())
# }
```

```bash
bashkit --mount-ro /host/data:/data -c 'ls /data'   # read-only
bashkit --mount-rw /host/out:/out  -c 'echo hi > /out/f'  # writable (dangerous)
```

To freeze a session — including in-memory writes — wrap it with
`readonly_filesystem()`.

## Composing a static namespace

`NamespaceFs` creates an intentionally bounded path tree instead of a fallback
root plus live mounts. Each source may be rebased and independently read-only or
read-write:

```rust
use bashkit::{Bash, FileSystem, InMemoryFs, NamespaceFs};
use std::path::Path;
use std::sync::Arc;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let repository = Arc::new(InMemoryFs::new());
repository.mkdir(Path::new("/repo/src"), true).await?;
repository.write_file(Path::new("/repo/src/lib.rs"), b"source").await?;
let output = Arc::new(InMemoryFs::new());

let namespace = NamespaceFs::builder()
    .mount_readonly_from("/src", repository, "/repo/src")?
    .mount_readwrite("/build", output)?
    .build();
let mut bash = Bash::builder().fs(Arc::new(namespace)).build();
assert_eq!(bash.exec("cat /src/lib.rs").await?.stdout, "source");
# Ok(())
# }
```

Nested targets use longest-prefix precedence. Missing ancestors and mount points
are visible as directories. Files and symlinks can be copied across mounts;
cross-mount rename reports a typed cross-device error because copy-delete is not
atomic.

## Special files and symlinks

- **`/dev/null`** is handled at the interpreter level (not the filesystem), so a
  custom backend can't intercept it. **`/dev/urandom`** / **`/dev/random`**
  return bounded random data.
- **Symlinks** are stored but never followed — this closes symlink-escape
  (TM-ESC-002) and symlink-loop DoS (TM-DOS-011).

## Binding parity

Every language binding exposes the same concepts, so the model is identical from
Rust, Python, and Node:

```text
files:  { "/path": "content" }                 # writable in-memory text files
mounts: [{ host_path, vfs_path?, writable? }]  # real FS (read-only by default)
readonly_filesystem: bool                       # deny all VFS mutations after setup
```

## See also

- [Security](security.md) — the boundaries built on top of the VFS.
- [Live mounts](live-mounts.md) — attach and detach filesystems at runtime.
- [Filesystem namespaces](https://docs.rs/bashkit/latest/bashkit/namespace_filesystems_guide/) — compose and rebase static mount trees.
- [Snapshotting](snapshotting.md) — serialise and restore VFS + shell state.
- Spec: [`specs/vfs.md`](https://github.com/everruns/bashkit/blob/main/specs/vfs.md).
