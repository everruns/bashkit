//! Read-only filesystem wrapper.
//!
//! This wrapper is intentionally applied as the outer filesystem layer when an
//! embedder wants a session that can inspect data but cannot persist or stage
//! any filesystem changes, including copies into the in-memory VFS.

use crate::time_compat::SystemTime;
use async_trait::async_trait;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::limits::{FsLimits, FsUsage};
use super::traits::{DirEntry, FileSystem, FileSystemExt, Metadata};
use crate::error::Result;

/// Denies all filesystem mutations while delegating read operations.
pub struct ReadOnlyFs {
    inner: Arc<dyn FileSystem>,
}

impl ReadOnlyFs {
    pub fn new(inner: Arc<dyn FileSystem>) -> Self {
        Self { inner }
    }

    fn readonly_error() -> crate::Error {
        IoError::new(ErrorKind::PermissionDenied, "filesystem is read-only").into()
    }
}

#[async_trait]
impl FileSystem for ReadOnlyFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }

    async fn write_file(&self, _path: &Path, _content: &[u8]) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn append_file(&self, _path: &Path, _content: &[u8]) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn mkdir(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn remove(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        self.inner.stat(path).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.inner.read_dir(path).await
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        self.inner.exists(path).await
    }

    async fn rename(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn copy(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn symlink(&self, _target: &Path, _link: &Path) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.inner.read_link(path).await
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> Result<()> {
        Err(Self::readonly_error())
    }

    async fn set_modified_time(&self, _path: &Path, _time: SystemTime) -> Result<()> {
        Err(Self::readonly_error())
    }

    fn as_search_capable(&self) -> Option<&dyn super::SearchCapable> {
        self.inner.as_search_capable()
    }
}

#[async_trait]
impl FileSystemExt for ReadOnlyFs {
    fn usage(&self) -> FsUsage {
        self.inner.usage()
    }

    fn limits(&self) -> FsLimits {
        self.inner.limits()
    }

    async fn mkfifo(&self, _path: &Path, _mode: u32) -> Result<()> {
        Err(Self::readonly_error())
    }

    fn vfs_snapshot(&self) -> Option<super::VfsSnapshot> {
        self.inner.vfs_snapshot()
    }

    fn vfs_restore(&self, _snapshot: &super::VfsSnapshot) -> Result<()> {
        Err(Self::readonly_error())
    }
}
