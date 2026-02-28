//! Filesystem trait definitions.
//!
//! # Overview
//!
//! This module defines [`FileSystem`], the high-level trait that enforces
//! POSIX-like semantics. For implementing custom storage backends, see also
//! [`FsBackend`](super::FsBackend) which provides a simpler contract.
//!
//! # POSIX Semantics Contract
//!
//! All [`FileSystem`] implementations MUST enforce these POSIX-like semantics:
//!
//! 1. **No duplicate names**: A file and directory cannot share the same path.
//!    The filesystem entry type (file/directory/symlink) is determined by
//!    whichever was created first.
//!
//! 2. **Type-safe writes**: [`FileSystem::write_file`] and [`FileSystem::append_file`]
//!    MUST fail with "is a directory" error when the path is a directory.
//!
//! 3. **Type-safe mkdir**: [`FileSystem::mkdir`] MUST fail with "already exists"
//!    when the path exists (file, directory, or symlink), unless `recursive=true`
//!    and the existing entry is a directory.
//!
//! 4. **Parent directory requirement**: Write operations require parent directory
//!    to exist (except with `recursive=true` for mkdir).
//!
//! # Implementing Custom Filesystems
//!
//! **Recommended**: Implement [`FsBackend`](super::FsBackend) and wrap with
//! [`PosixFs`](super::PosixFs) to get POSIX semantics automatically.
//!
//! **Alternative**: Implement `FileSystem` directly using [`fs_errors`] helpers:
//!
//! ```rust,ignore
//! use bashkit::fs::fs_errors;
//!
//! // In your write_file implementation:
//! if path_is_directory {
//!     return Err(fs_errors::is_a_directory());
//! }
//! ```

use async_trait::async_trait;
use std::io::{Error as IoError, ErrorKind};
use std::path::Path;
use std::time::SystemTime;

use super::limits::{FsLimits, FsUsage};
use crate::error::Result;

/// Standard filesystem errors for consistent error messages across implementations.
///
/// Use these helpers when implementing [`FileSystem`] to ensure consistent
/// error messages that match POSIX conventions.
#[allow(dead_code)]
pub mod fs_errors {
    use super::*;

    /// Error for attempting to write to a directory.
    ///
    /// Use when `write_file` or `append_file` is called on a directory path.
    #[inline]
    pub fn is_a_directory() -> crate::Error {
        IoError::other("is a directory").into()
    }

    /// Error for path already existing (for mkdir without recursive).
    ///
    /// Use when `mkdir` is called on a path that already exists.
    #[inline]
    pub fn already_exists(msg: &str) -> crate::Error {
        IoError::new(ErrorKind::AlreadyExists, msg).into()
    }

    /// Error for missing parent directory.
    ///
    /// Use when write operation is attempted but parent directory doesn't exist.
    #[inline]
    pub fn parent_not_found() -> crate::Error {
        IoError::new(ErrorKind::NotFound, "parent directory not found").into()
    }

    /// Error for file or directory not found.
    #[inline]
    pub fn not_found(msg: &str) -> crate::Error {
        IoError::new(ErrorKind::NotFound, msg).into()
    }

    /// Error for attempting directory operation on a file.
    ///
    /// Use when `read_dir` is called on a file path.
    #[inline]
    pub fn not_a_directory() -> crate::Error {
        IoError::other("not a directory").into()
    }

    /// Error for non-empty directory removal without recursive flag.
    #[inline]
    pub fn directory_not_empty() -> crate::Error {
        IoError::other("directory not empty").into()
    }
}

/// Async virtual filesystem trait.
///
/// This trait defines the interface for all filesystem implementations in Bashkit.
/// Implement this trait to create custom storage backends.
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` to support concurrent access from
/// multiple tasks. Use interior mutability patterns (e.g., `RwLock`, `Mutex`)
/// for mutable state.
///
/// # Implementing FileSystem
///
/// To create a custom filesystem, implement all methods in this trait.
/// See `examples/custom_filesystem_impl.rs` for a complete implementation.
///
/// ```rust,ignore
/// use bashkit::{async_trait, FileSystem, Result};
///
/// pub struct MyFileSystem { /* ... */ }
///
/// #[async_trait]
/// impl FileSystem for MyFileSystem {
///     async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
///         // Your implementation
///     }
///     // ... implement all other methods
/// }
/// ```
///
/// # Using Custom Filesystems
///
/// Pass your filesystem to [`Bash::builder()`](crate::Bash::builder):
///
/// ```rust,ignore
/// use bashkit::Bash;
/// use std::sync::Arc;
///
/// let custom_fs = Arc::new(MyFileSystem::new());
/// let mut bash = Bash::builder().fs(custom_fs).build();
/// ```
///
/// # Built-in Implementations
///
/// Bashkit provides three implementations:
///
/// - [`InMemoryFs`](crate::InMemoryFs) - HashMap-based in-memory storage
/// - [`OverlayFs`](crate::OverlayFs) - Copy-on-write layered filesystem
/// - [`MountableFs`](crate::MountableFs) - Multiple mount points
#[async_trait]
pub trait FileSystem: Send + Sync {
    /// Read a file's contents as bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file does not exist (`NotFound`)
    /// - The path is a directory
    /// - I/O error occurs
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write contents to a file, creating it if necessary.
    ///
    /// If the file exists, its contents are replaced. If it doesn't exist,
    /// a new file is created (parent directory must exist).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The parent directory does not exist
    /// - The path is a directory
    /// - I/O error occurs
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Append contents to a file, creating it if necessary.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is a directory
    /// - I/O error occurs
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Create a directory.
    ///
    /// # Arguments
    ///
    /// * `path` - The directory path to create
    /// * `recursive` - If true, create parent directories as needed (like `mkdir -p`)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `recursive` is false and parent directory doesn't exist
    /// - Path already exists as a file or symlink (always fails)
    /// - Path already exists as a directory (fails unless `recursive=true`)
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Remove a file or directory.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to remove
    /// * `recursive` - If true and path is a directory, remove all contents (like `rm -r`)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path does not exist
    /// - Path is a non-empty directory and `recursive` is false
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Get file or directory metadata.
    ///
    /// Returns information about the file including type, size, permissions,
    /// and timestamps.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist.
    async fn stat(&self, path: &Path) -> Result<Metadata>;

    /// List directory contents.
    ///
    /// Returns a list of entries (files, directories, symlinks) in the directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path does not exist
    /// - The path is not a directory
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Check if a path exists.
    ///
    /// Returns `true` if the path exists (file, directory, or symlink).
    async fn exists(&self, path: &Path) -> Result<bool>;

    /// Rename or move a file or directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The source path does not exist
    /// - The destination parent directory does not exist
    async fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Copy a file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The source file does not exist
    /// - The source is a directory
    async fn copy(&self, from: &Path, to: &Path) -> Result<()>;

    /// Create a symbolic link.
    ///
    /// Creates a symlink at `link` that points to `target`.
    ///
    /// # Arguments
    ///
    /// * `target` - The path the symlink will point to
    /// * `link` - The path where the symlink will be created
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()>;

    /// Read a symbolic link's target.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path does not exist
    /// - The path is not a symlink
    async fn read_link(&self, path: &Path) -> Result<std::path::PathBuf>;

    /// Change file permissions.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path
    /// * `mode` - Unix permission mode (e.g., `0o644`, `0o755`)
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist.
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()>;

    /// Get current filesystem usage statistics.
    ///
    /// Returns total bytes used, file count, and directory count.
    /// Used by `du` and `df` builtins.
    ///
    /// # Default Implementation
    ///
    /// Returns zeros. Implementations should override for accurate stats.
    fn usage(&self) -> FsUsage {
        FsUsage::default()
    }

    /// Get filesystem limits.
    ///
    /// Returns the configured limits for this filesystem.
    /// Used by `df` builtin to show available space.
    ///
    /// # Default Implementation
    ///
    /// Returns unlimited limits.
    fn limits(&self) -> FsLimits {
        FsLimits::unlimited()
    }
}

/// File or directory metadata.
///
/// Returned by [`FileSystem::stat()`] and included in [`DirEntry`].
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, FileSystem, FileType};
/// use std::path::Path;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let bash = Bash::new();
/// let fs = bash.fs();
///
/// fs.write_file(Path::new("/tmp/test.txt"), b"hello").await?;
///
/// let stat = fs.stat(Path::new("/tmp/test.txt")).await?;
/// assert!(stat.file_type.is_file());
/// assert_eq!(stat.size, 5);  // "hello" = 5 bytes
/// assert_eq!(stat.mode, 0o644);  // Default file permissions
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Metadata {
    /// The type of this entry (file, directory, or symlink).
    pub file_type: FileType,
    /// File size in bytes. For directories, this is typically 0.
    pub size: u64,
    /// Unix permission mode (e.g., `0o644` for files, `0o755` for directories).
    pub mode: u32,
    /// Last modification time.
    pub modified: SystemTime,
    /// Creation time.
    pub created: SystemTime,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            file_type: FileType::File,
            size: 0,
            mode: 0o644,
            modified: SystemTime::now(),
            created: SystemTime::now(),
        }
    }
}

/// Type of a filesystem entry.
///
/// Used in [`Metadata`] to indicate whether an entry is a file, directory,
/// or symbolic link.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileType {
    /// Regular file containing data.
    File,
    /// Directory that can contain other entries.
    Directory,
    /// Symbolic link pointing to another path.
    Symlink,
}

impl FileType {
    /// Returns `true` if this is a regular file.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FileType;
    ///
    /// assert!(FileType::File.is_file());
    /// assert!(!FileType::Directory.is_file());
    /// ```
    pub fn is_file(&self) -> bool {
        matches!(self, FileType::File)
    }

    /// Returns `true` if this is a directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FileType;
    ///
    /// assert!(FileType::Directory.is_dir());
    /// assert!(!FileType::File.is_dir());
    /// ```
    pub fn is_dir(&self) -> bool {
        matches!(self, FileType::Directory)
    }

    /// Returns `true` if this is a symbolic link.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FileType;
    ///
    /// assert!(FileType::Symlink.is_symlink());
    /// assert!(!FileType::File.is_symlink());
    /// ```
    pub fn is_symlink(&self) -> bool {
        matches!(self, FileType::Symlink)
    }
}

/// An entry in a directory listing.
///
/// Returned by [`FileSystem::read_dir()`]. Contains the entry name (not the
/// full path) and its metadata.
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, FileSystem};
/// use std::path::Path;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let bash = Bash::new();
/// let fs = bash.fs();
///
/// fs.mkdir(Path::new("/data"), false).await?;
/// fs.write_file(Path::new("/data/file.txt"), b"content").await?;
///
/// let entries = fs.read_dir(Path::new("/data")).await?;
/// for entry in entries {
///     println!("Name: {}, Size: {}", entry.name, entry.metadata.size);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Entry name (filename only, not the full path).
    pub name: String,
    /// Metadata for this entry.
    pub metadata: Metadata,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // --- fs_errors ---

    #[test]
    fn fs_error_is_a_directory_message() {
        let err = fs_errors::is_a_directory();
        let msg = format!("{err}");
        assert!(msg.contains("is a directory"), "got: {msg}");
    }

    #[test]
    fn fs_error_already_exists_message() {
        let err = fs_errors::already_exists("path /tmp exists");
        let msg = format!("{err}");
        assert!(msg.contains("path /tmp exists"), "got: {msg}");
    }

    #[test]
    fn fs_error_parent_not_found_message() {
        let err = fs_errors::parent_not_found();
        let msg = format!("{err}");
        assert!(msg.contains("parent directory not found"), "got: {msg}");
    }

    #[test]
    fn fs_error_not_found_message() {
        let err = fs_errors::not_found("no such file");
        let msg = format!("{err}");
        assert!(msg.contains("no such file"), "got: {msg}");
    }

    #[test]
    fn fs_error_not_a_directory_message() {
        let err = fs_errors::not_a_directory();
        let msg = format!("{err}");
        assert!(msg.contains("not a directory"), "got: {msg}");
    }

    #[test]
    fn fs_error_directory_not_empty_message() {
        let err = fs_errors::directory_not_empty();
        let msg = format!("{err}");
        assert!(msg.contains("directory not empty"), "got: {msg}");
    }

    // --- FileType ---

    #[test]
    fn file_type_is_file() {
        assert!(FileType::File.is_file());
        assert!(!FileType::Directory.is_file());
        assert!(!FileType::Symlink.is_file());
    }

    #[test]
    fn file_type_is_dir() {
        assert!(FileType::Directory.is_dir());
        assert!(!FileType::File.is_dir());
        assert!(!FileType::Symlink.is_dir());
    }

    #[test]
    fn file_type_is_symlink() {
        assert!(FileType::Symlink.is_symlink());
        assert!(!FileType::File.is_symlink());
        assert!(!FileType::Directory.is_symlink());
    }

    #[test]
    fn file_type_equality() {
        assert_eq!(FileType::File, FileType::File);
        assert_eq!(FileType::Directory, FileType::Directory);
        assert_eq!(FileType::Symlink, FileType::Symlink);
        assert_ne!(FileType::File, FileType::Directory);
        assert_ne!(FileType::File, FileType::Symlink);
        assert_ne!(FileType::Directory, FileType::Symlink);
    }

    #[test]
    fn file_type_debug() {
        let dbg = format!("{:?}", FileType::File);
        assert_eq!(dbg, "File");
    }

    // --- Metadata ---

    #[test]
    fn metadata_default_is_file() {
        let m = Metadata::default();
        assert!(m.file_type.is_file());
        assert_eq!(m.size, 0);
        assert_eq!(m.mode, 0o644);
    }

    #[test]
    fn metadata_custom_fields() {
        let now = SystemTime::now();
        let m = Metadata {
            file_type: FileType::Directory,
            size: 4096,
            mode: 0o755,
            modified: now,
            created: now,
        };
        assert!(m.file_type.is_dir());
        assert_eq!(m.size, 4096);
        assert_eq!(m.mode, 0o755);
    }

    #[test]
    fn metadata_clone() {
        let m = Metadata::default();
        let cloned = m.clone();
        assert_eq!(cloned.size, m.size);
        assert_eq!(cloned.mode, m.mode);
        assert!(cloned.file_type.is_file());
    }

    // --- DirEntry ---

    #[test]
    fn dir_entry_construction() {
        let entry = DirEntry {
            name: "test.txt".into(),
            metadata: Metadata::default(),
        };
        assert_eq!(entry.name, "test.txt");
        assert!(entry.metadata.file_type.is_file());
    }

    #[test]
    fn dir_entry_with_directory_type() {
        let now = SystemTime::now();
        let entry = DirEntry {
            name: "subdir".into(),
            metadata: Metadata {
                file_type: FileType::Directory,
                size: 0,
                mode: 0o755,
                modified: now,
                created: now,
            },
        };
        assert_eq!(entry.name, "subdir");
        assert!(entry.metadata.file_type.is_dir());
    }

    #[test]
    fn dir_entry_debug() {
        let entry = DirEntry {
            name: "f".into(),
            metadata: Metadata::default(),
        };
        let dbg = format!("{:?}", entry);
        assert!(dbg.contains("DirEntry"));
        assert!(dbg.contains("\"f\""));
    }

    // --- FileSystem default methods ---

    #[test]
    fn filesystem_default_usage_returns_zeros() {
        // Test via a minimal struct that only implements the defaults
        struct Dummy;

        #[async_trait]
        impl FileSystem for Dummy {
            async fn read_file(&self, _: &Path) -> crate::error::Result<Vec<u8>> {
                unimplemented!()
            }
            async fn write_file(&self, _: &Path, _: &[u8]) -> crate::error::Result<()> {
                unimplemented!()
            }
            async fn append_file(&self, _: &Path, _: &[u8]) -> crate::error::Result<()> {
                unimplemented!()
            }
            async fn mkdir(&self, _: &Path, _: bool) -> crate::error::Result<()> {
                unimplemented!()
            }
            async fn remove(&self, _: &Path, _: bool) -> crate::error::Result<()> {
                unimplemented!()
            }
            async fn stat(&self, _: &Path) -> crate::error::Result<Metadata> {
                unimplemented!()
            }
            async fn read_dir(&self, _: &Path) -> crate::error::Result<Vec<DirEntry>> {
                unimplemented!()
            }
            async fn exists(&self, _: &Path) -> crate::error::Result<bool> {
                unimplemented!()
            }
            async fn rename(&self, _: &Path, _: &Path) -> crate::error::Result<()> {
                unimplemented!()
            }
            async fn copy(&self, _: &Path, _: &Path) -> crate::error::Result<()> {
                unimplemented!()
            }
            async fn symlink(&self, _: &Path, _: &Path) -> crate::error::Result<()> {
                unimplemented!()
            }
            async fn read_link(&self, _: &Path) -> crate::error::Result<std::path::PathBuf> {
                unimplemented!()
            }
            async fn chmod(&self, _: &Path, _: u32) -> crate::error::Result<()> {
                unimplemented!()
            }
        }

        let d = Dummy;
        let usage = d.usage();
        assert_eq!(usage.total_bytes, 0);
        assert_eq!(usage.file_count, 0);
        assert_eq!(usage.dir_count, 0);

        let limits = d.limits();
        assert_eq!(limits.max_total_bytes, u64::MAX);
    }
}
