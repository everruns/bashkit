//! Low-level filesystem backend trait.
//!
//! This module provides the [`FsBackend`] trait for implementing raw storage
//! operations without POSIX semantics enforcement.
//!
//! # When to Use `FsBackend`
//!
//! Use `FsBackend` when you want to implement a **simple storage backend**
//! and let [`PosixFs`](super::PosixFs) handle all the POSIX semantics (type
//! checking, parent directory validation, etc.).
//!
//! | You want to... | Use |
//! |----------------|-----|
//! | Simple storage with automatic POSIX checks | `FsBackend` + `PosixFs` |
//! | Full control over all behavior | [`FileSystem`](super::FileSystem) directly |
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │                    Bash                          │
//! │                 (interpreter)                    │
//! └───────────────────────┬─────────────────────────┘
//!                         │ uses
//! ┌───────────────────────▼─────────────────────────┐
//! │              FileSystem trait                    │
//! │         (POSIX semantics enforced)               │
//! └───────────────────────┬─────────────────────────┘
//!                         │
//!        ┌────────────────┼────────────────┐
//!        │                │                │
//! ┌──────▼──────┐  ┌──────▼──────┐  ┌──────▼──────┐
//! │ InMemoryFs  │  │  PosixFs    │  │ OverlayFs   │
//! │ (built-in)  │  │  (wrapper)  │  │ (built-in)  │
//! └─────────────┘  └──────┬──────┘  └─────────────┘
//!                         │ wraps
//!                  ┌──────▼──────┐
//!                  │ FsBackend   │
//!                  │ (your impl) │
//!                  └─────────────┘
//! ```
//!
//! # Example: Simple Key-Value Storage
//!
//! ```rust,ignore
//! use bashkit::{async_trait, FsBackend, Result, Metadata, DirEntry, FileType};
//! use std::collections::HashMap;
//! use std::path::{Path, PathBuf};
//! use std::sync::RwLock;
//!
//! /// Simple storage backed by a HashMap.
//! pub struct KvStorage {
//!     data: RwLock<HashMap<PathBuf, Vec<u8>>>,
//! }
//!
//! #[async_trait]
//! impl FsBackend for KvStorage {
//!     async fn read(&self, path: &Path) -> Result<Vec<u8>> {
//!         let data = self.data.read().unwrap();
//!         data.get(path)
//!             .cloned()
//!             .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound).into())
//!     }
//!
//!     async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
//!         let mut data = self.data.write().unwrap();
//!         data.insert(path.to_path_buf(), content.to_vec());
//!         Ok(())
//!     }
//!
//!     // ... implement remaining methods
//! }
//! ```
//!
//! # Using Your Backend
//!
//! Wrap with [`PosixFs`](super::PosixFs) to get POSIX semantics:
//!
//! ```rust,ignore
//! use bashkit::{Bash, PosixFs};
//! use std::sync::Arc;
//!
//! let backend = KvStorage::new();
//! let fs = Arc::new(PosixFs::new(backend));
//! let mut bash = Bash::builder().fs(fs).build();
//!
//! // POSIX checks are automatic:
//! // - Writing to a directory fails
//! // - mkdir on existing file fails
//! // - Parent directory must exist
//! ```
//!
//! See `examples/custom_backend.rs` for a complete working example.

use async_trait::async_trait;
use std::path::{Path, PathBuf};

use super::limits::{FsLimits, FsUsage};
use super::traits::{DirEntry, Metadata};
use crate::error::Result;

/// Low-level filesystem backend trait.
///
/// This trait defines raw storage operations without enforcing POSIX semantics.
/// Implementations handle storage only - type checking and semantic enforcement
/// are provided by [`PosixFs`] wrapper.
///
/// # Contract
///
/// Backends are expected to:
/// - Store and retrieve bytes at paths
/// - Track file metadata (type, size, mode, timestamps)
/// - Handle path normalization consistently
///
/// Backends do NOT need to:
/// - Check if writing to a directory (PosixFs handles this)
/// - Prevent mkdir over existing file (PosixFs handles this)
/// - Validate parent directory existence (PosixFs handles this)
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` for concurrent access.
#[async_trait]
pub trait FsBackend: Send + Sync {
    /// Read raw bytes from a path.
    ///
    /// Returns the file contents as bytes.
    ///
    /// # Errors
    /// - `NotFound` if path doesn't exist
    async fn read(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write raw bytes to a path.
    ///
    /// Creates file if it doesn't exist, overwrites if it does.
    /// The backend may overwrite any entry type (file, dir, symlink).
    ///
    /// # Errors
    /// - Storage-specific errors
    async fn write(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Append bytes to a path.
    ///
    /// Creates file if it doesn't exist.
    ///
    /// # Errors
    /// - Storage-specific errors
    async fn append(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Create a directory.
    ///
    /// If `recursive` is true, create parent directories as needed.
    ///
    /// # Errors
    /// - Storage-specific errors
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Remove a file or directory.
    ///
    /// If `recursive` is true, remove directory contents.
    ///
    /// # Errors
    /// - `NotFound` if path doesn't exist
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()>;

    /// Get metadata for a path.
    ///
    /// # Errors
    /// - `NotFound` if path doesn't exist
    async fn stat(&self, path: &Path) -> Result<Metadata>;

    /// List directory contents.
    ///
    /// # Errors
    /// - `NotFound` if path doesn't exist
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Check if path exists.
    async fn exists(&self, path: &Path) -> Result<bool>;

    /// Rename/move a path.
    async fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Copy a file.
    async fn copy(&self, from: &Path, to: &Path) -> Result<()>;

    /// Create a symbolic link.
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()>;

    /// Read symbolic link target.
    async fn read_link(&self, path: &Path) -> Result<PathBuf>;

    /// Change file permissions.
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()>;

    /// Get storage usage statistics.
    fn usage(&self) -> FsUsage {
        FsUsage::default()
    }

    /// Get storage limits.
    fn limits(&self) -> FsLimits {
        FsLimits::unlimited()
    }
}
