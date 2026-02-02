//! Filesystem trait definitions.

use async_trait::async_trait;
use std::path::Path;
use std::time::SystemTime;

use crate::error::Result;

/// Async virtual filesystem trait.
///
/// This trait defines the interface for all filesystem implementations in BashKit.
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
/// BashKit provides three implementations:
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
    /// - Directory already exists (when not recursive)
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
