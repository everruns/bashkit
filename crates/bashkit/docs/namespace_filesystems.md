# Filesystem Namespace Guide

[`NamespaceFs`] composes arbitrary [`FileSystem`] instances into one static,
visible path tree. It is useful for build sandboxes, tests, plugin hosts,
serverless functions, CI jobs, and shell sessions that need different storage
and permissions at different paths.

## Build a Namespace

```rust
use bashkit::{Bash, FileSystem, InMemoryFs, NamespaceFs};
use std::path::Path;
use std::sync::Arc;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let source = Arc::new(InMemoryFs::new());
source.write_file(Path::new("/lib.rs"), b"pub fn run() {}\n").await?;
let build = Arc::new(InMemoryFs::new());

let namespace = NamespaceFs::builder()
    .mount_readonly("/src", source)?
    .mount_readwrite("/build", build.clone())?
    .build();
let mut bash = Bash::builder().fs(Arc::new(namespace)).build();

bash.exec("cp /src/lib.rs /build/lib.rs").await?;
assert_eq!(build.read_file(Path::new("/lib.rs")).await?, b"pub fn run() {}\n");
# Ok(())
# }
```

The builder accepts any `Arc<dyn FileSystem>`. Object ownership determines the
namespace lifetime: create a namespace per operation or retain and reuse it.

## Rebase a Source Subtree

Use `mount_readonly_from` or `mount_readwrite_from` when the visible target and
the source root differ:

```rust
use bashkit::{FileSystem, InMemoryFs, NamespaceFs};
use std::path::Path;
use std::sync::Arc;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let repository = Arc::new(InMemoryFs::new());
repository.mkdir(Path::new("/repo/src/generated"), true).await?;
repository.write_file(Path::new("/repo/src/lib.rs"), b"source").await?;
let generated = Arc::new(InMemoryFs::new());

let namespace = NamespaceFs::builder()
    .mount_readonly_from("/workspace", repository, "/repo/src")?
    .mount_readwrite("/workspace/generated", generated.clone())?
    .build();

assert_eq!(namespace.read_file(Path::new("/workspace/lib.rs")).await?, b"source");
namespace
    .write_file(Path::new("/workspace/generated/output.rs"), b"output")
    .await?;
assert_eq!(generated.read_file(Path::new("/output.rs")).await?, b"output");
# Ok(())
# }
```

Nested mounts use deterministic longest-prefix precedence. Missing ancestors
and mount points appear as directories in `exists`, `stat`, and `read_dir`.
Directory entries carry the same metadata returned by `stat`.

## Cross-Mount Operations

- File and symlink `copy` works across mounts. The destination must be writable.
- Directory and FIFO copies across mounts return `ErrorKind::Unsupported`.
- `rename` within one mount delegates to its filesystem.
- `rename` across mounts returns `ErrorKind::CrossesDevices`; Bashkit does not
  emulate it with copy-delete because that cannot be atomic.

Visible paths are normalized before mount selection and rebasing. A path cannot
use `..` to escape its source root, skip a nested mount, or bypass read-only
access.

## See also

- [`FileSystem`] — filesystem operation contract.
- [`MountableFs`] — dynamic live mounts with a fallback root filesystem.
- [`ReadOnlyFs`] — deny mutations for an entire filesystem.
- [Live mounts](live-mounts.md) — attach and detach filesystems after build.
- [VFS specification](https://github.com/everruns/bashkit/blob/main/specs/vfs.md).
