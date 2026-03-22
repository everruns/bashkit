# Live Mount/Unmount Guide

Bashkit supports attaching and detaching filesystems on a **running** interpreter
without rebuilding it. Shell state — environment variables, working directory,
history, aliases — is fully preserved across mount operations.

## Motivation

Before live mounts, the only way to add a filesystem after `build()` was to
accumulate mount configs and call `reset()`, which rebuilds the entire
interpreter and loses all in-flight state. Live mounts solve this by exposing
the internal [`MountableFs`] layer that wraps every `Bash` instance.

Common use cases:

- **Agent workflows**: attach a host directory mid-session when a tool needs it
- **Plugin systems**: mount/unmount plugin filesystems without restarting
- **Hot-swap deployments**: replace a mounted app filesystem with a new version
- **Testing**: inject mock data at specific paths during a test

## Quick Start

```rust
use bashkit::{Bash, FileSystem, InMemoryFs};
use std::path::Path;
use std::sync::Arc;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::new();

// Create and populate a filesystem
let data_fs = Arc::new(InMemoryFs::new());
data_fs.write_file(Path::new("/users.json"), br#"["alice"]"#).await?;

// Mount it live — no rebuild needed
bash.mount("/mnt/data", data_fs)?;

let result = bash.exec("cat /mnt/data/users.json").await?;
assert!(result.stdout.contains("alice"));

// Unmount when done
bash.unmount("/mnt/data")?;
# Ok(())
# }
```

## API

### `Bash::mount(vfs_path, fs)`

Mounts `fs` at `vfs_path`. The mount takes effect immediately — subsequent
`exec()` calls see files from the mounted filesystem. If a mount already exists
at `vfs_path`, it is replaced (hot-swap).

```rust
# use bashkit::{Bash, FileSystem, InMemoryFs};
# use std::sync::Arc;
# use std::path::Path;
# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let bash = Bash::new();
let fs = Arc::new(InMemoryFs::new());
bash.mount("/mnt/data", fs)?;
# Ok(())
# }
```

**Errors:** Returns `Err` if `vfs_path` is not absolute (after normalization).

### `Bash::unmount(vfs_path)`

Removes the mount at `vfs_path`. Paths that previously resolved to the mounted
filesystem fall back to the root filesystem or the next shorter mount prefix.

```rust
# use bashkit::{Bash, FileSystem, InMemoryFs};
# use std::sync::Arc;
# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
# let bash = Bash::new();
# let fs = Arc::new(InMemoryFs::new());
# bash.mount("/mnt/data", fs)?;
bash.unmount("/mnt/data")?;
# Ok(())
# }
```

**Errors:** Returns `Err` if nothing is mounted at `vfs_path`.

## How It Works

Every `Bash` instance wraps its filesystem stack in a [`MountableFs`] as the
outermost layer. This layer uses longest-prefix matching to route path
operations to the correct mounted filesystem:

```text
┌──────────────────────────────┐
│  MountableFs (live mounts)   │  ← Bash::mount() / unmount()
├──────────────────────────────┤
│  OverlayFs (text mounts)     │  ← BashBuilder::mount_text()
├──────────────────────────────┤
│  MountableFs (real mounts)   │  ← BashBuilder::mount_real_*_at()
├──────────────────────────────┤
│  Base filesystem             │  ← InMemoryFs or custom
└──────────────────────────────┘
```

Because the interpreter holds an `Arc<dyn FileSystem>` pointing to the
outermost `MountableFs`, any mount/unmount operation is visible to the
interpreter immediately — no rebuild or state transfer required.

## Builder Mounts vs Live Mounts

| | Builder mounts | Live mounts |
|---|---|---|
| **When** | Before `build()` | After `build()` |
| **Method** | `BashBuilder::mount_text()`, `mount_real_*()` | `Bash::mount()` |
| **State** | N/A (no interpreter yet) | Fully preserved |
| **Use case** | Initial configuration | Dynamic attachment |

Both approaches can be combined: configure initial mounts with the builder, then
add/remove mounts at runtime.

## Examples

### Multiple Mounts

```rust
use bashkit::{Bash, FileSystem, InMemoryFs};
use std::path::Path;
use std::sync::Arc;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::new();

let logs = Arc::new(InMemoryFs::new());
logs.write_file(Path::new("/app.log"), b"started\n").await?;

let config = Arc::new(InMemoryFs::new());
config.write_file(Path::new("/app.toml"), b"port = 8080\n").await?;

bash.mount("/var/log", logs)?;
bash.mount("/etc/app", config)?;

let result = bash.exec("cat /var/log/app.log").await?;
assert_eq!(result.stdout, "started\n");

let result = bash.exec("cat /etc/app/app.toml").await?;
assert_eq!(result.stdout, "port = 8080\n");
# Ok(())
# }
```

### Hot-Swap

Re-mounting at the same path replaces the filesystem atomically:

```rust
use bashkit::{Bash, FileSystem, InMemoryFs};
use std::path::Path;
use std::sync::Arc;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::new();

let v1 = Arc::new(InMemoryFs::new());
v1.write_file(Path::new("/version"), b"1.0").await?;
bash.mount("/app", v1)?;

let result = bash.exec("cat /app/version").await?;
assert_eq!(result.stdout, "1.0");

// Hot-swap to v2
let v2 = Arc::new(InMemoryFs::new());
v2.write_file(Path::new("/version"), b"2.0").await?;
bash.mount("/app", v2)?;

let result = bash.exec("cat /app/version").await?;
assert_eq!(result.stdout, "2.0");
# Ok(())
# }
```

## See Also

- [`MountableFs`] — the underlying mount infrastructure
- [`BashBuilder::mount_text`] — pre-build text file mounts
- [`BashBuilder::fs`] — custom filesystem injection
- [`Bash::fs`] — direct filesystem access
- [VFS specification](https://github.com/everruns/bashkit/blob/main/specs/003-vfs.md)
