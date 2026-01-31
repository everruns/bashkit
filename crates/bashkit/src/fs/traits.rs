//! Filesystem trait definitions

use async_trait::async_trait;
use std::path::Path;
use std::time::SystemTime;

use crate::error::Result;

/// Async filesystem trait.
///
/// All filesystem implementations must implement this trait.
#[async_trait]
pub trait FileSystem: Send + Sync {
    /// Read a file's contents.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write contents to a file.
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Append contents to a file.
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Create a directory.
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Remove a file or directory.
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Get file metadata.
    async fn stat(&self, path: &Path) -> Result<Metadata>;

    /// Read directory entries.
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Check if a path exists.
    async fn exists(&self, path: &Path) -> Result<bool>;

    /// Rename/move a file or directory.
    async fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Copy a file.
    async fn copy(&self, from: &Path, to: &Path) -> Result<()>;

    /// Create a symbolic link.
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()>;

    /// Read a symbolic link's target.
    async fn read_link(&self, path: &Path) -> Result<std::path::PathBuf>;

    /// Change file permissions.
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()>;
}

/// File metadata.
#[derive(Debug, Clone)]
pub struct Metadata {
    /// File type
    pub file_type: FileType,
    /// File size in bytes
    pub size: u64,
    /// File permissions (Unix mode)
    pub mode: u32,
    /// Last modification time
    pub modified: SystemTime,
    /// Creation time
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

/// File type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileType {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
}

impl FileType {
    /// Check if this is a file.
    pub fn is_file(&self) -> bool {
        matches!(self, FileType::File)
    }

    /// Check if this is a directory.
    pub fn is_dir(&self) -> bool {
        matches!(self, FileType::Directory)
    }

    /// Check if this is a symlink.
    pub fn is_symlink(&self) -> bool {
        matches!(self, FileType::Symlink)
    }
}

/// Directory entry.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Entry name (not full path)
    pub name: String,
    /// Entry metadata
    pub metadata: Metadata,
}
