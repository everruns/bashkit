//! Virtual filesystem for Bashkit.
//!
//! This module provides a virtual filesystem abstraction that allows Bashkit to
//! operate in a virtual environment without accessing the real filesystem.
//!
//! # Which Trait/Type Should I Use?
//!
//! ```text
//! Do you need a custom filesystem?
//!     │
//!     ├─ NO → Use InMemoryFs (default with Bash::new())
//!     │
//!     └─ YES → Is your storage simple (key-value, database, cloud)?
//!               │
//!               ├─ YES → Implement FsBackend + wrap with PosixFs
//!               │        (POSIX checks are automatic)
//!               │
//!               └─ NO → Implement FileSystem directly
//!                       (full control, you handle all checks)
//! ```
//!
//! # Architecture
//!
//! The filesystem abstraction has two layers:
//!
//! | Layer | Trait/Type | What You Implement |
//! |-------|------------|-------------------|
//! | Backend | [`FsBackend`] | Raw storage only (read/write/list) |
//! | POSIX | [`FileSystem`] | Full POSIX semantics (type checks, parent dirs) |
//!
//! **[`PosixFs`]** bridges these: it wraps any `FsBackend` and provides `FileSystem`.
//!
//! # Implementing Custom Filesystems
//!
//! ## Option 1: `FsBackend` + `PosixFs` (Recommended)
//!
//! Best for: databases, cloud storage, simple key-value stores.
//!
//! ```rust,ignore
//! use bashkit::{async_trait, FsBackend, PosixFs, Bash, Result, Metadata, DirEntry};
//! use std::sync::Arc;
//!
//! // Implement raw storage operations
//! struct MyStorage { /* ... */ }
//!
//! #[async_trait]
//! impl FsBackend for MyStorage {
//!     async fn read(&self, path: &Path) -> Result<Vec<u8>> { /* ... */ }
//!     async fn write(&self, path: &Path, content: &[u8]) -> Result<()> { /* ... */ }
//!     // ... other methods
//! }
//!
//! // Wrap with PosixFs - POSIX semantics are automatic!
//! let fs = Arc::new(PosixFs::new(MyStorage::new()));
//! let mut bash = Bash::builder().fs(fs).build();
//! ```
//!
//! ## Option 2: `FileSystem` Directly
//!
//! Best for: complex behavior, custom caching, specialized semantics.
//!
//! ```rust,ignore
//! use bashkit::{async_trait, FileSystem, Bash};
//!
//! struct MyFs { /* ... */ }
//!
//! #[async_trait]
//! impl FileSystem for MyFs {
//!     async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
//!         // You MUST check: is path a directory?
//!         if self.is_directory(path) {
//!             return Err(fs_errors::is_a_directory());
//!         }
//!         // ... write logic
//!     }
//!     // ... other methods with POSIX checks
//! }
//! ```
//!
//! See `examples/custom_backend.rs` and `examples/custom_filesystem_impl.rs`.
//!
//! # Built-in Implementations
//!
//! | Type | Description | Use Case |
//! |------|-------------|----------|
//! | [`InMemoryFs`] | HashMap-based storage with POSIX checks | Default, isolated execution |
//! | [`OverlayFs`] | Copy-on-write layered filesystem | Templates, immutable bases |
//! | [`MountableFs`] | Multiple filesystems at mount points | Complex multi-source setups |
//!
//! All implementations are thread-safe (`Send + Sync`) and fully async.
//!
//! # Quick Start
//!
//! ## Using InMemoryFs (Default)
//!
//! [`InMemoryFs`] is the default filesystem when using [`Bash::new()`](crate::Bash::new):
//!
//! ```rust
//! use bashkit::Bash;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//!
//! // Files are stored entirely in memory
//! bash.exec("echo 'hello' > /tmp/test.txt").await?;
//! let result = bash.exec("cat /tmp/test.txt").await?;
//! assert_eq!(result.stdout, "hello\n");
//! # Ok(())
//! # }
//! ```
//!
//! ## Using OverlayFs
//!
//! [`OverlayFs`] provides copy-on-write semantics - reads fall through to a base
//! layer while writes go to an overlay layer:
//!
//! ```rust
//! use bashkit::{Bash, FileSystem, InMemoryFs, OverlayFs};
//! use std::path::Path;
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! // Create a base filesystem with template files
//! let base = Arc::new(InMemoryFs::new());
//! base.mkdir(Path::new("/templates"), false).await?;
//! base.write_file(Path::new("/templates/config.txt"), b"default=true").await?;
//!
//! // Create overlay - base is read-only, changes go to overlay
//! let overlay = Arc::new(OverlayFs::new(base.clone()));
//!
//! let mut bash = Bash::builder().fs(overlay).build();
//!
//! // Read from base layer
//! let result = bash.exec("cat /templates/config.txt").await?;
//! assert_eq!(result.stdout, "default=true");
//!
//! // Modify - changes go to overlay, base is unchanged
//! bash.exec("echo 'modified=true' > /templates/config.txt").await?;
//!
//! // Base still has original content
//! let original = base.read_file(Path::new("/templates/config.txt")).await?;
//! assert_eq!(original, b"default=true");
//! # Ok(())
//! # }
//! ```
//!
//! ## Using MountableFs
//!
//! [`MountableFs`] allows mounting different filesystems at specific paths:
//!
//! ```rust
//! use bashkit::{Bash, FileSystem, InMemoryFs, MountableFs};
//! use std::path::Path;
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! // Create root and a separate data filesystem
//! let root = Arc::new(InMemoryFs::new());
//! let data_fs = Arc::new(InMemoryFs::new());
//!
//! // Pre-populate the data filesystem
//! data_fs.write_file(Path::new("/users.json"), br#"["alice", "bob"]"#).await?;
//!
//! // Create mountable filesystem and mount data_fs at /mnt/data
//! let mountable = MountableFs::new(root);
//! mountable.mount("/mnt/data", data_fs)?;
//!
//! let mut bash = Bash::builder().fs(Arc::new(mountable)).build();
//!
//! // Access the mounted filesystem
//! let result = bash.exec("cat /mnt/data/users.json").await?;
//! assert!(result.stdout.contains("alice"));
//! # Ok(())
//! # }
//! ```
//!
//! # Direct Filesystem Access
//!
//! Access the filesystem directly for pre-populating files or reading output:
//!
//! ```rust
//! use bashkit::{Bash, FileSystem};
//! use std::path::Path;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//! let fs = bash.fs();
//!
//! // Create directories
//! fs.mkdir(Path::new("/data"), false).await?;
//! fs.mkdir(Path::new("/data/input"), false).await?;
//! fs.mkdir(Path::new("/data/output"), false).await?;
//!
//! // Pre-populate input files
//! fs.write_file(Path::new("/data/input/data.csv"), b"name,value\nalice,100").await?;
//!
//! // Run a script that processes the data
//! bash.exec("cat /data/input/data.csv | grep alice > /data/output/result.txt").await?;
//!
//! // Read the output directly
//! let output = fs.read_file(Path::new("/data/output/result.txt")).await?;
//! assert_eq!(output, b"alice,100\n");
//!
//! // Check file exists
//! assert!(fs.exists(Path::new("/data/output/result.txt")).await?);
//!
//! // Get file metadata
//! let stat = fs.stat(Path::new("/data/output/result.txt")).await?;
//! assert!(stat.file_type.is_file());
//!
//! // List directory contents
//! let entries = fs.read_dir(Path::new("/data")).await?;
//! let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
//! assert!(names.contains(&"input"));
//! assert!(names.contains(&"output"));
//! # Ok(())
//! # }
//! ```
//!
//! # Binary File Support
//!
//! The filesystem fully supports binary data including null bytes:
//!
//! ```rust
//! use bashkit::{Bash, FileSystem};
//! use std::path::Path;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let bash = Bash::new();
//! let fs = bash.fs();
//!
//! // Write binary data (e.g., PNG header)
//! let binary_data = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF];
//! fs.write_file(Path::new("/tmp/image.bin"), &binary_data).await?;
//!
//! // Read it back
//! let content = fs.read_file(Path::new("/tmp/image.bin")).await?;
//! assert_eq!(content, binary_data);
//! # Ok(())
//! # }
//! ```
//!
//! # Implementing Custom Filesystems
//!
//! Implement the [`FileSystem`] trait to create custom storage backends:
//!
//! ```rust
//! use bashkit::{async_trait, FileSystem, DirEntry, Metadata, FileType, Result, Error};
//! use std::path::{Path, PathBuf};
//! use std::collections::HashMap;
//! use std::sync::RwLock;
//! use std::time::SystemTime;
//!
//! /// A simple custom filesystem example
//! pub struct SimpleFs {
//!     files: RwLock<HashMap<PathBuf, Vec<u8>>>,
//! }
//!
//! impl SimpleFs {
//!     pub fn new() -> Self {
//!         let mut files = HashMap::new();
//!         // Initialize with root and common directories
//!         files.insert(PathBuf::from("/"), Vec::new());
//!         files.insert(PathBuf::from("/tmp"), Vec::new());
//!         Self { files: RwLock::new(files) }
//!     }
//! }
//!
//! #[async_trait]
//! impl FileSystem for SimpleFs {
//!     async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
//!         let files = self.files.read().unwrap();
//!         files.get(path)
//!             .cloned()
//!             .ok_or_else(|| Error::Io(std::io::Error::new(
//!                 std::io::ErrorKind::NotFound,
//!                 "file not found"
//!             )))
//!     }
//!
//!     async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
//!         let mut files = self.files.write().unwrap();
//!         files.insert(path.to_path_buf(), content.to_vec());
//!         Ok(())
//!     }
//!
//!     // ... implement remaining methods
//! #   async fn append_file(&self, _path: &Path, _content: &[u8]) -> Result<()> { Ok(()) }
//! #   async fn mkdir(&self, _path: &Path, _recursive: bool) -> Result<()> { Ok(()) }
//! #   async fn remove(&self, _path: &Path, _recursive: bool) -> Result<()> { Ok(()) }
//! #   async fn stat(&self, _path: &Path) -> Result<Metadata> {
//! #       Ok(Metadata::default())
//! #   }
//! #   async fn read_dir(&self, _path: &Path) -> Result<Vec<DirEntry>> { Ok(vec![]) }
//! #   async fn exists(&self, _path: &Path) -> Result<bool> { Ok(false) }
//! #   async fn rename(&self, _from: &Path, _to: &Path) -> Result<()> { Ok(()) }
//! #   async fn copy(&self, _from: &Path, _to: &Path) -> Result<()> { Ok(()) }
//! #   async fn symlink(&self, _target: &Path, _link: &Path) -> Result<()> { Ok(()) }
//! #   async fn read_link(&self, _path: &Path) -> Result<PathBuf> { Ok(PathBuf::new()) }
//! #   async fn chmod(&self, _path: &Path, _mode: u32) -> Result<()> { Ok(()) }
//! }
//! ```
//!
//! For a complete custom filesystem implementation example, see
//! `examples/custom_filesystem_impl.rs`.
//!
//! # Default Directory Structure
//!
//! [`InMemoryFs::new()`] creates these directories by default:
//!
//! - `/` - Root directory
//! - `/tmp` - Temporary files
//! - `/home` - Home directories
//! - `/home/user` - Default user home
//! - `/dev` - Device files
//! - `/dev/null` - Null device (discards writes, returns empty on read)
//!
//! # Requirements for Custom FileSystem Implementations
//!
//! When implementing [`FileSystem`] for custom storage backends, your implementation
//! **must** ensure:
//!
//! 1. **Root directory exists**: `exists("/")` must return `true`
//! 2. **Path normalization**: Paths like `/.`, `/tmp/..`, etc. must resolve correctly
//! 3. **Root is listable**: `read_dir("/")` must return the root's contents
//!
//! Without these, commands like `cd /` and `ls /` will fail with "No such file or directory".
//!
//! Use [`verify_filesystem_requirements`] to test your implementation:
//!
//! ```rust
//! use bashkit::{verify_filesystem_requirements, InMemoryFs};
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let fs = Arc::new(InMemoryFs::new());
//! verify_filesystem_requirements(&*fs).await?;
//! # Ok(())
//! # }
//! ```

mod backend;
mod limits;
mod memory;
mod mountable;
mod overlay;
mod posix;
mod traits;

pub use backend::FsBackend;
pub use limits::{FsLimitExceeded, FsLimits, FsUsage};
pub use memory::InMemoryFs;
pub use mountable::MountableFs;
pub use overlay::OverlayFs;
pub use posix::PosixFs;
#[allow(unused_imports)]
pub use traits::{fs_errors, DirEntry, FileSystem, FileType, Metadata};

use crate::error::Result;
use std::io::{Error as IoError, ErrorKind};
use std::path::Path;

/// Verify that a filesystem implementation meets minimum requirements for Bashkit.
///
/// This function checks that your custom [`FileSystem`] implementation:
/// - Has root directory `/` that exists
/// - Can stat the root directory
/// - Can list the root directory contents
/// - Handles path normalization (e.g., `/.` resolves to `/`)
///
/// # Errors
///
/// Returns an error describing what requirement is not met.
///
/// # Example
///
/// ```rust
/// use bashkit::{verify_filesystem_requirements, InMemoryFs};
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let fs = Arc::new(InMemoryFs::new());
/// verify_filesystem_requirements(&*fs).await?;
/// println!("Filesystem meets all requirements!");
/// # Ok(())
/// # }
/// ```
pub async fn verify_filesystem_requirements(fs: &dyn FileSystem) -> Result<()> {
    // Check 1: Root directory must exist
    if !fs.exists(Path::new("/")).await? {
        return Err(IoError::new(
            ErrorKind::NotFound,
            "FileSystem requirement not met: root directory '/' does not exist. \
             Custom FileSystem implementations must ensure '/' exists on creation.",
        )
        .into());
    }

    // Check 2: Root must be a directory
    let stat = fs.stat(Path::new("/")).await.map_err(|_| {
        IoError::new(
            ErrorKind::NotFound,
            "FileSystem requirement not met: cannot stat root directory '/'. \
             Ensure stat() works for the root path.",
        )
    })?;

    if !stat.file_type.is_dir() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "FileSystem requirement not met: root '/' is not a directory.",
        )
        .into());
    }

    // Check 3: Root must be listable
    fs.read_dir(Path::new("/")).await.map_err(|_| {
        IoError::new(
            ErrorKind::NotFound,
            "FileSystem requirement not met: cannot list root directory '/'. \
             Ensure read_dir() works for the root path.",
        )
    })?;

    // Check 4: Path normalization - "/." should resolve to "/"
    if !fs.exists(Path::new("/.")).await? {
        return Err(IoError::new(
            ErrorKind::NotFound,
            "FileSystem requirement not met: path '/.' does not resolve to root. \
             Ensure your implementation normalizes paths (removes '.' components).",
        )
        .into());
    }

    Ok(())
}
