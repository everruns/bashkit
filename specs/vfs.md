# Virtual Filesystem Design

## Status
Active

## Decision

Bashkit uses a two-layer filesystem abstraction:

| Layer | Trait/Type | Responsibility |
|-------|------------|----------------|
| Backend | `FsBackend` | Raw storage operations (minimal contract) |
| POSIX | `FileSystem` / `PosixFs` | POSIX-like semantics enforcement |

`FsBackend` handles raw storage without enforcing POSIX semantics.
Implementations can be wrapped with `PosixFs` to get type-safe behavior.

See `crates/bashkit/src/fs/` for trait definitions and implementations.

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
- All files stored in `HashMap<PathBuf, FsEntry>`, thread-safe via `RwLock`
- Initial directories: `/`, `/tmp`, `/home`, `/home/user`, `/dev`
- Special handling for `/dev/null`, `/dev/urandom`, `/dev/random`
- No persistence — state lost on drop

##### Mounting Files with BashBuilder

```rust
let mut bash = Bash::builder()
    .mount_text("/config/app.conf", "debug=true\nport=8080\n")
    .mount_readonly_text("/etc/version", "1.2.3")
    .build();
```

#### OverlayFs
- Copy-on-write layer over another FileSystem
- Whiteout tracking for deleted files
- Useful for: temp modifications, testing, isolation

#### MountableFs
- Mount multiple filesystems at different paths
- Longest-prefix matching for nested mounts
- Always used as outermost FS layer for live mount/unmount support

#### RealFs (Optional, `realfs` feature)
- Direct access to a host directory as an `FsBackend`
- Two modes: `ReadOnly` (safe) and `ReadWrite` (dangerous)
- Path traversal prevented via canonicalization + root prefix check
- New-path writes canonicalize the nearest existing ancestor before attaching a
  missing suffix, blocking symlink escapes through non-existent subpaths

##### Builder Methods

```rust
Bash::builder().mount_real_readonly("/path/to/dir")
Bash::builder().mount_real_readonly_at("/path/to/dir", "/mnt/data")
Bash::builder().mount_real_readwrite("/path/to/dir")
```

##### CLI Usage

```bash
bashkit --mount-ro /path/to/project -c 'cat /README.md'
bashkit --mount-rw /path/to/output:/mnt/output -c 'echo result > /mnt/output/result.txt'
```

#### Live Mount/Unmount

Every `Bash` instance wraps its filesystem stack in a `MountableFs`. This
enables post-build mount/unmount without rebuilding the interpreter:

```rust
bash.mount("/mnt/data", data_fs)?;
bash.unmount("/mnt/data")?;
```

### FS Layering Stack

```text
┌──────────────────────────────────┐
│  MountableFs (live mounts)       │  ← Bash::mount() / unmount()
├──────────────────────────────────┤
│  OverlayFs (text mounts)         │  ← BashBuilder::mount_text()
├──────────────────────────────────┤
│  MountableFs (real mounts)       │  ← BashBuilder::mount_real_*_at()
├──────────────────────────────────┤
│  Base filesystem                 │  ← InMemoryFs or custom
└──────────────────────────────────┘
```

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

### Symlink Handling

Symlinks are stored but intentionally not followed for security:
- Prevents symlink escape attacks (TM-ESC-002)
- Prevents symlink loop DoS (TM-DOS-011)

## Binding API Parity

All language bindings must expose the same mount API:

```
files:  { "/path": "content" }                # text files (writable, in-memory)
mounts: [{ host_path, vfs_path?, writable? }] # real FS (read-only by default)
```

Runtime methods: `mount(host_path, vfs_path, writable=false)`, `unmount(vfs_path)`.

Safety: real mounts are **read-only by default**. Text files are writable (sandboxed).

## Alternatives Considered

### Real filesystem with chroot
Rejected: requires root, not portable, doesn't work in WASM.

### tokio::fs wrapper
Rejected: always hits real FS, can't isolate or virtualize.
