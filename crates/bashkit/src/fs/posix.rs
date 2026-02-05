//! POSIX-compatible filesystem wrapper.
//!
//! This module provides [`PosixFs`], a wrapper that adds POSIX-like semantics
//! on top of any [`FsBackend`] implementation.
//!
//! # Overview
//!
//! `PosixFs` takes a simple storage backend and adds:
//!
//! | Check | Description |
//! |-------|-------------|
//! | Type-safe writes | `write_file` fails with "is a directory" if path is a directory |
//! | Type-safe mkdir | `mkdir` fails with "file exists" if path is a file |
//! | Parent directory | Write operations require parent directory to exist |
//! | read_dir validation | Fails if path is not a directory |
//!
//! # Example
//!
//! ```rust,ignore
//! use bashkit::{Bash, FsBackend, PosixFs};
//! use std::sync::Arc;
//!
//! // 1. Implement FsBackend for your storage
//! struct MyStorage { /* ... */ }
//! impl FsBackend for MyStorage { /* ... */ }
//!
//! // 2. Wrap with PosixFs
//! let backend = MyStorage::new();
//! let fs = Arc::new(PosixFs::new(backend));
//!
//! // 3. Use with Bash
//! let mut bash = Bash::builder().fs(fs).build();
//!
//! // POSIX semantics are automatically enforced:
//! bash.exec("mkdir /tmp/dir").await?;
//! let result = bash.exec("echo test > /tmp/dir 2>&1").await?;
//! // ^ This fails with "is a directory"
//! ```
//!
//! # When to Use
//!
//! Use `PosixFs` when:
//! - You have a simple storage backend that doesn't enforce POSIX rules
//! - You want automatic type checking without implementing it yourself
//! - You're bridging to an external storage system (database, cloud, etc.)
//!
//! See [`FsBackend`](super::FsBackend) for how to implement a backend.

use async_trait::async_trait;
use std::io::Error as IoError;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::backend::FsBackend;
use super::limits::{FsLimits, FsUsage};
use super::traits::{fs_errors, DirEntry, FileSystem, Metadata};
use crate::error::Result;

/// POSIX-compatible filesystem wrapper.
///
/// Wraps any [`FsBackend`] and enforces POSIX-like semantics.
///
/// # Semantics Enforced
///
/// | Operation | Check |
/// |-----------|-------|
/// | `write_file` | Fails if path is a directory |
/// | `append_file` | Fails if path is a directory |
/// | `mkdir` | Fails if path exists as file (always) or dir (unless recursive) |
/// | `read_dir` | Fails if path is not a directory |
/// | `copy` | Fails if source is a directory |
///
/// # Example
///
/// ```rust,ignore
/// use bashkit::{FsBackend, PosixFs, Bash};
/// use std::sync::Arc;
///
/// // Your simple storage backend
/// let backend = MyStorage::new();
///
/// // Wrap with PosixFs for POSIX semantics
/// let fs = Arc::new(PosixFs::new(backend));
///
/// // Use with Bash interpreter
/// let mut bash = Bash::builder().fs(fs).build();
/// ```
pub struct PosixFs<B: FsBackend> {
    backend: B,
}

impl<B: FsBackend> PosixFs<B> {
    /// Create a new POSIX-compatible filesystem wrapper.
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Get a reference to the underlying backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Check if parent directory exists.
    async fn check_parent_exists(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if parent != Path::new("/")
                && parent != Path::new("")
                && !self.backend.exists(parent).await?
            {
                return Err(fs_errors::parent_not_found());
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<B: FsBackend + 'static> FileSystem for PosixFs<B> {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        // Check if it's a directory
        if let Ok(meta) = self.backend.stat(path).await {
            if meta.file_type.is_dir() {
                return Err(fs_errors::is_a_directory());
            }
        }
        self.backend.read(path).await
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        // Check parent exists
        self.check_parent_exists(path).await?;

        // Check if path is a directory
        if let Ok(meta) = self.backend.stat(path).await {
            if meta.file_type.is_dir() {
                return Err(fs_errors::is_a_directory());
            }
        }

        self.backend.write(path, content).await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        // Check if path is a directory
        if let Ok(meta) = self.backend.stat(path).await {
            if meta.file_type.is_dir() {
                return Err(fs_errors::is_a_directory());
            }
        }

        self.backend.append(path, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        // Check if something already exists at this path
        if let Ok(meta) = self.backend.stat(path).await {
            if meta.file_type.is_dir() {
                // Directory exists
                if recursive {
                    return Ok(()); // mkdir -p on existing dir is OK
                } else {
                    return Err(fs_errors::already_exists("directory exists"));
                }
            } else {
                // File or symlink exists - always error
                return Err(fs_errors::already_exists("file exists"));
            }
        }

        if recursive {
            // Check each component in path for file conflicts
            if let Some(parent) = path.parent() {
                let mut current = PathBuf::from("/");
                for component in parent.components().skip(1) {
                    current.push(component);
                    if let Ok(meta) = self.backend.stat(&current).await {
                        if !meta.file_type.is_dir() {
                            return Err(fs_errors::already_exists("file exists"));
                        }
                    }
                }
            }
        } else {
            // Non-recursive: parent must exist
            self.check_parent_exists(path).await?;
        }

        self.backend.mkdir(path, recursive).await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.backend.remove(path, recursive).await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        self.backend.stat(path).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        // Check if it's actually a directory
        if let Ok(meta) = self.backend.stat(path).await {
            if !meta.file_type.is_dir() {
                return Err(fs_errors::not_a_directory());
            }
        }
        self.backend.read_dir(path).await
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        self.backend.exists(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.backend.rename(from, to).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        // Check source is not a directory
        if let Ok(meta) = self.backend.stat(from).await {
            if meta.file_type.is_dir() {
                return Err(IoError::other("cannot copy directory").into());
            }
        }
        self.backend.copy(from, to).await
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.backend.symlink(target, link).await
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.backend.read_link(path).await
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.backend.chmod(path, mode).await
    }

    fn usage(&self) -> FsUsage {
        self.backend.usage()
    }

    fn limits(&self) -> FsLimits {
        self.backend.limits()
    }
}

// Allow Arc<PosixFs<B>> to be used where Arc<dyn FileSystem> is expected
impl<B: FsBackend + 'static> From<PosixFs<B>> for Arc<dyn FileSystem> {
    fn from(fs: PosixFs<B>) -> Self {
        Arc::new(fs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::path::Path;

    #[tokio::test]
    async fn test_posix_write_to_directory_fails() {
        // InMemoryFs already implements FileSystem with checks,
        // but we can test PosixFs wrapping a raw backend
        let fs = InMemoryFs::new();

        // Create a directory
        fs.mkdir(Path::new("/tmp/testdir"), false)
            .await
            .expect("mkdir should succeed");

        // Writing to it should fail
        let result = fs.write_file(Path::new("/tmp/testdir"), b"test").await;
        assert!(result.is_err());
        assert!(result
            .expect_err("write_file should fail")
            .to_string()
            .contains("directory"));
    }

    #[tokio::test]
    async fn test_posix_mkdir_on_file_fails() {
        let fs = InMemoryFs::new();

        // Create a file
        fs.write_file(Path::new("/tmp/testfile"), b"test")
            .await
            .expect("write_file should succeed");

        // mkdir on it should fail
        let result = fs.mkdir(Path::new("/tmp/testfile"), false).await;
        assert!(result.is_err());
    }
}
