//! In-memory filesystem implementation.
//!
//! [`InMemoryFs`] provides a simple, fast, thread-safe filesystem that stores
//! all data in memory using a `HashMap`.
//!
//! # Fail Points (enabled with `failpoints` feature)
//!
//! For testing error handling, the following fail points are available:
//!
//! - `fs::read_file` - Inject failures in file reads
//! - `fs::write_file` - Inject failures in file writes
//! - `fs::mkdir` - Inject failures in directory creation
//! - `fs::remove` - Inject failures in file/directory removal
//! - `fs::lock_read` - Inject failures in read lock acquisition
//! - `fs::lock_write` - Inject failures in write lock acquisition

// RwLock.read()/write().unwrap() only panics on lock poisoning (prior panic
// while holding lock). This is intentional - corrupted state should not propagate.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::collections::HashMap;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

use super::traits::{DirEntry, FileSystem, FileType, Metadata};
use crate::error::Result;

#[cfg(feature = "failpoints")]
use fail::fail_point;

/// In-memory filesystem implementation.
///
/// `InMemoryFs` is the default filesystem used by [`Bash::new()`](crate::Bash::new).
/// It stores all files and directories in memory using a `HashMap`, making it
/// ideal for sandboxed execution where no real filesystem access is needed.
///
/// # Features
///
/// - **Thread-safe**: Uses `RwLock` for concurrent read/write access
/// - **Binary-safe**: Fully supports binary data including null bytes
/// - **Default directories**: Creates `/`, `/tmp`, `/home`, `/home/user`, `/dev`
/// - **Special devices**: `/dev/null` discards writes and returns empty on read
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, FileSystem, InMemoryFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// // InMemoryFs is the default when using Bash::new()
/// let mut bash = Bash::new();
///
/// // Or create explicitly for direct filesystem access
/// let fs = Arc::new(InMemoryFs::new());
///
/// // Write files
/// fs.write_file(Path::new("/tmp/test.txt"), b"hello").await?;
///
/// // Read files
/// let content = fs.read_file(Path::new("/tmp/test.txt")).await?;
/// assert_eq!(content, b"hello");
///
/// // Create directories
/// fs.mkdir(Path::new("/data/nested/dir"), true).await?;
///
/// // Check existence
/// assert!(fs.exists(Path::new("/data/nested/dir")).await?);
///
/// // Use with Bash
/// let mut bash = Bash::builder().fs(fs.clone()).build();
/// bash.exec("echo 'from bash' >> /tmp/test.txt").await?;
///
/// let content = fs.read_file(Path::new("/tmp/test.txt")).await?;
/// assert_eq!(content, b"hellofrom bash\n");
/// # Ok(())
/// # }
/// ```
///
/// # Default Directory Structure
///
/// `InMemoryFs::new()` creates these directories:
///
/// ```text
/// /
/// ├── tmp/
/// ├── home/
/// │   └── user/
/// └── dev/
///     └── null  (special device)
/// ```
///
/// # Binary Data
///
/// The filesystem fully supports binary data:
///
/// ```rust
/// use bashkit::{FileSystem, InMemoryFs};
/// use std::path::Path;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let fs = InMemoryFs::new();
///
/// // Write binary with null bytes
/// let data = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF];
/// fs.write_file(Path::new("/tmp/binary.bin"), &data).await?;
///
/// // Read it back unchanged
/// let read = fs.read_file(Path::new("/tmp/binary.bin")).await?;
/// assert_eq!(read, data);
/// # Ok(())
/// # }
/// ```
pub struct InMemoryFs {
    entries: RwLock<HashMap<PathBuf, FsEntry>>,
}

#[derive(Debug, Clone)]
enum FsEntry {
    File {
        content: Vec<u8>,
        metadata: Metadata,
    },
    Directory {
        metadata: Metadata,
    },
    Symlink {
        target: PathBuf,
        metadata: Metadata,
    },
}

impl Default for InMemoryFs {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryFs {
    /// Create a new in-memory filesystem with default directories.
    ///
    /// Creates the following directory structure:
    /// - `/` - Root directory
    /// - `/tmp` - Temporary files
    /// - `/home` - Home directories
    /// - `/home/user` - Default user home
    /// - `/dev` - Device files
    /// - `/dev/null` - Null device (discards writes, returns empty)
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{FileSystem, InMemoryFs};
    /// use std::path::Path;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let fs = InMemoryFs::new();
    ///
    /// // Default directories exist
    /// assert!(fs.exists(Path::new("/tmp")).await?);
    /// assert!(fs.exists(Path::new("/home/user")).await?);
    /// assert!(fs.exists(Path::new("/dev/null")).await?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Self {
        let mut entries = HashMap::new();

        // Create root directory
        entries.insert(
            PathBuf::from("/"),
            FsEntry::Directory {
                metadata: Metadata {
                    file_type: FileType::Directory,
                    size: 0,
                    mode: 0o755,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        // Create common directories
        for dir in &["/tmp", "/home", "/home/user", "/dev"] {
            entries.insert(
                PathBuf::from(dir),
                FsEntry::Directory {
                    metadata: Metadata {
                        file_type: FileType::Directory,
                        size: 0,
                        mode: 0o755,
                        modified: SystemTime::now(),
                        created: SystemTime::now(),
                    },
                },
            );
        }

        // Create special device files
        // /dev/null - discards all writes, returns empty on read
        entries.insert(
            PathBuf::from("/dev/null"),
            FsEntry::File {
                content: Vec::new(),
                metadata: Metadata {
                    file_type: FileType::File,
                    size: 0,
                    mode: 0o666,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        // /dev/fd - directory for process substitution file descriptors
        entries.insert(
            PathBuf::from("/dev/fd"),
            FsEntry::Directory {
                metadata: Metadata {
                    file_type: FileType::Directory,
                    size: 0,
                    mode: 0o755,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        Self {
            entries: RwLock::new(entries),
        }
    }

    fn normalize_path(path: &Path) -> PathBuf {
        let mut result = PathBuf::new();

        for component in path.components() {
            match component {
                std::path::Component::RootDir => {
                    result.push("/");
                }
                std::path::Component::Normal(name) => {
                    result.push(name);
                }
                std::path::Component::ParentDir => {
                    result.pop();
                }
                std::path::Component::CurDir => {}
                std::path::Component::Prefix(_) => {}
            }
        }

        if result.as_os_str().is_empty() {
            result.push("/");
        }

        result
    }

    /// Add a file with specific mode (synchronous, for initial setup).
    ///
    /// This method is primarily used by [`BashBuilder`](crate::BashBuilder) to
    /// pre-populate the filesystem during construction. For runtime file operations,
    /// use the async [`FileSystem::write_file`] method instead.
    ///
    /// Parent directories are created automatically.
    ///
    /// # Arguments
    ///
    /// * `path` - Absolute path where the file will be created
    /// * `content` - File content (will be converted to bytes)
    /// * `mode` - Unix permission mode (e.g., `0o644` for writable, `0o444` for readonly)
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::InMemoryFs;
    ///
    /// let fs = InMemoryFs::new();
    ///
    /// // Add a writable config file
    /// fs.add_file("/config/app.conf", "debug=true\n", 0o644);
    ///
    /// // Add a readonly file
    /// fs.add_file("/etc/version", "1.0.0", 0o444);
    /// ```
    pub fn add_file(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>, mode: u32) {
        let path = Self::normalize_path(path.as_ref());
        let content = content.as_ref();
        let mut entries = self.entries.write().unwrap();

        // Ensure parent directories exist
        if let Some(parent) = path.parent() {
            let mut current = PathBuf::from("/");
            for component in parent.components().skip(1) {
                current.push(component);
                if !entries.contains_key(&current) {
                    entries.insert(
                        current.clone(),
                        FsEntry::Directory {
                            metadata: Metadata {
                                file_type: FileType::Directory,
                                size: 0,
                                mode: 0o755,
                                modified: SystemTime::now(),
                                created: SystemTime::now(),
                            },
                        },
                    );
                }
            }
        }

        entries.insert(
            path,
            FsEntry::File {
                content: content.to_vec(),
                metadata: Metadata {
                    file_type: FileType::File,
                    size: content.len() as u64,
                    mode,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );
    }
}

#[async_trait]
impl FileSystem for InMemoryFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        // Fail point: simulate read failures
        #[cfg(feature = "failpoints")]
        fail_point!("fs::read_file", |action| {
            match action.as_deref() {
                Some("io_error") => {
                    return Err(IoError::other("injected I/O error").into());
                }
                Some("permission_denied") => {
                    return Err(
                        IoError::new(ErrorKind::PermissionDenied, "permission denied").into(),
                    );
                }
                Some("corrupt_data") => {
                    // Return garbage data instead of actual content
                    return Ok(vec![0xFF, 0xFE, 0x00, 0x01]);
                }
                _ => {}
            }
            Err(IoError::other("fail point triggered").into())
        });

        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::File { content, .. }) => Ok(content.clone()),
            Some(FsEntry::Directory { .. }) => Err(IoError::other("is a directory").into()),
            Some(FsEntry::Symlink { .. }) => {
                // TODO: Follow symlinks
                Err(IoError::new(ErrorKind::NotFound, "file not found").into())
            }
            None => Err(IoError::new(ErrorKind::NotFound, "file not found").into()),
        }
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        // Fail point: simulate write failures
        #[cfg(feature = "failpoints")]
        fail_point!("fs::write_file", |action| {
            match action.as_deref() {
                Some("io_error") => {
                    return Err(IoError::other("injected I/O error").into());
                }
                Some("disk_full") => {
                    return Err(IoError::other("no space left on device").into());
                }
                Some("permission_denied") => {
                    return Err(
                        IoError::new(ErrorKind::PermissionDenied, "permission denied").into(),
                    );
                }
                Some("partial_write") => {
                    // Simulate partial write - this tests data integrity handling
                    // In a real scenario, this could corrupt data
                    return Err(IoError::new(ErrorKind::Interrupted, "partial write").into());
                }
                _ => {}
            }
            Err(IoError::other("fail point triggered").into())
        });

        let path = Self::normalize_path(path);

        // Special handling for /dev/null - discard all writes
        if path == Path::new("/dev/null") {
            return Ok(());
        }

        let mut entries = self.entries.write().unwrap();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !entries.contains_key(parent) && parent != Path::new("/") {
                return Err(IoError::new(ErrorKind::NotFound, "parent directory not found").into());
            }
        }

        entries.insert(
            path,
            FsEntry::File {
                content: content.to_vec(),
                metadata: Metadata {
                    file_type: FileType::File,
                    size: content.len() as u64,
                    mode: 0o644,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        Ok(())
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize_path(path);

        // Special handling for /dev/null - discard all writes
        if path == Path::new("/dev/null") {
            return Ok(());
        }

        // Check if file exists and handle accordingly
        // We need to release the lock before potentially calling write_file
        let should_create = {
            let mut entries = self.entries.write().unwrap();

            match entries.get_mut(&path) {
                Some(FsEntry::File {
                    content: existing,
                    metadata,
                }) => {
                    existing.extend_from_slice(content);
                    metadata.size = existing.len() as u64;
                    metadata.modified = SystemTime::now();
                    return Ok(());
                }
                Some(FsEntry::Directory { .. }) => {
                    return Err(IoError::other("is a directory").into());
                }
                Some(FsEntry::Symlink { .. }) => {
                    return Err(IoError::new(ErrorKind::NotFound, "file not found").into());
                }
                None => true,
            }
        };

        if should_create {
            self.write_file(&path, content).await
        } else {
            Ok(())
        }
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut entries = self.entries.write().unwrap();

        if recursive {
            let mut current = PathBuf::from("/");
            for component in path.components().skip(1) {
                current.push(component);
                if !entries.contains_key(&current) {
                    entries.insert(
                        current.clone(),
                        FsEntry::Directory {
                            metadata: Metadata {
                                file_type: FileType::Directory,
                                size: 0,
                                mode: 0o755,
                                modified: SystemTime::now(),
                                created: SystemTime::now(),
                            },
                        },
                    );
                }
            }
        } else {
            // Check parent exists
            if let Some(parent) = path.parent() {
                if !entries.contains_key(parent) && parent != Path::new("/") {
                    return Err(
                        IoError::new(ErrorKind::NotFound, "parent directory not found").into(),
                    );
                }
            }

            if entries.contains_key(&path) {
                return Err(IoError::new(ErrorKind::AlreadyExists, "directory exists").into());
            }

            entries.insert(
                path,
                FsEntry::Directory {
                    metadata: Metadata {
                        file_type: FileType::Directory,
                        size: 0,
                        mode: 0o755,
                        modified: SystemTime::now(),
                        created: SystemTime::now(),
                    },
                },
            );
        }

        Ok(())
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut entries = self.entries.write().unwrap();

        match entries.get(&path) {
            Some(FsEntry::Directory { .. }) => {
                if recursive {
                    // Remove all entries under this path
                    let to_remove: Vec<PathBuf> = entries
                        .keys()
                        .filter(|p| p.starts_with(&path))
                        .cloned()
                        .collect();

                    for p in to_remove {
                        entries.remove(&p);
                    }
                } else {
                    // Check if directory is empty
                    let has_children = entries
                        .keys()
                        .any(|p| p != &path && p.parent() == Some(&path));

                    if has_children {
                        return Err(IoError::other("directory not empty").into());
                    }

                    entries.remove(&path);
                }
            }
            Some(FsEntry::File { .. }) | Some(FsEntry::Symlink { .. }) => {
                entries.remove(&path);
            }
            None => {
                return Err(IoError::new(ErrorKind::NotFound, "not found").into());
            }
        }

        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::File { metadata, .. })
            | Some(FsEntry::Directory { metadata })
            | Some(FsEntry::Symlink { metadata, .. }) => Ok(metadata.clone()),
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::Directory { .. }) => {
                let mut result = Vec::new();

                for (entry_path, entry) in entries.iter() {
                    if entry_path.parent() == Some(&path) && entry_path != &path {
                        let name = entry_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();

                        let metadata = match entry {
                            FsEntry::File { metadata, .. }
                            | FsEntry::Directory { metadata }
                            | FsEntry::Symlink { metadata, .. } => metadata.clone(),
                        };

                        result.push(DirEntry { name, metadata });
                    }
                }

                Ok(result)
            }
            Some(_) => Err(IoError::other("not a directory").into()),
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();
        Ok(entries.contains_key(&path))
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);
        let mut entries = self.entries.write().unwrap();

        let entry = entries
            .remove(&from)
            .ok_or_else(|| IoError::new(ErrorKind::NotFound, "not found"))?;

        entries.insert(to, entry);
        Ok(())
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);
        let mut entries = self.entries.write().unwrap();

        let entry = entries
            .get(&from)
            .cloned()
            .ok_or_else(|| IoError::new(ErrorKind::NotFound, "not found"))?;

        entries.insert(to, entry);
        Ok(())
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        let link = Self::normalize_path(link);
        let mut entries = self.entries.write().unwrap();

        entries.insert(
            link,
            FsEntry::Symlink {
                target: target.to_path_buf(),
                metadata: Metadata {
                    file_type: FileType::Symlink,
                    size: 0,
                    mode: 0o777,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        Ok(())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::Symlink { target, .. }) => Ok(target.clone()),
            Some(_) => Err(IoError::other("not a symlink").into()),
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut entries = self.entries.write().unwrap();

        match entries.get_mut(&path) {
            Some(FsEntry::File { metadata, .. })
            | Some(FsEntry::Directory { metadata })
            | Some(FsEntry::Symlink { metadata, .. }) => {
                metadata.mode = mode;
                Ok(())
            }
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_and_read_file() {
        let fs = InMemoryFs::new();

        fs.write_file(Path::new("/tmp/test.txt"), b"hello world")
            .await
            .unwrap();

        let content = fs.read_file(Path::new("/tmp/test.txt")).await.unwrap();
        assert_eq!(content, b"hello world");
    }

    #[tokio::test]
    async fn test_mkdir_and_read_dir() {
        let fs = InMemoryFs::new();

        fs.mkdir(Path::new("/tmp/mydir"), false).await.unwrap();
        fs.write_file(Path::new("/tmp/mydir/file.txt"), b"test")
            .await
            .unwrap();

        let entries = fs.read_dir(Path::new("/tmp/mydir")).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file.txt");
    }

    #[tokio::test]
    async fn test_exists() {
        let fs = InMemoryFs::new();

        assert!(fs.exists(Path::new("/tmp")).await.unwrap());
        assert!(!fs.exists(Path::new("/tmp/nonexistent")).await.unwrap());
    }

    #[tokio::test]
    async fn test_add_file_basic() {
        let fs = InMemoryFs::new();
        fs.add_file("/tmp/added.txt", "hello from add_file", 0o644);

        let content = fs.read_file(Path::new("/tmp/added.txt")).await.unwrap();
        assert_eq!(content, b"hello from add_file");
    }

    #[tokio::test]
    async fn test_add_file_with_mode() {
        let fs = InMemoryFs::new();
        fs.add_file("/etc/readonly.conf", "secret", 0o444);

        let stat = fs.stat(Path::new("/etc/readonly.conf")).await.unwrap();
        assert_eq!(stat.mode, 0o444);
    }

    #[tokio::test]
    async fn test_add_file_creates_parent_directories() {
        let fs = InMemoryFs::new();
        fs.add_file("/a/b/c/d/nested.txt", "deep content", 0o644);

        // File should exist
        assert!(fs.exists(Path::new("/a/b/c/d/nested.txt")).await.unwrap());

        // Parent directories should exist
        assert!(fs.exists(Path::new("/a")).await.unwrap());
        assert!(fs.exists(Path::new("/a/b")).await.unwrap());
        assert!(fs.exists(Path::new("/a/b/c")).await.unwrap());
        assert!(fs.exists(Path::new("/a/b/c/d")).await.unwrap());

        // Verify content
        let content = fs
            .read_file(Path::new("/a/b/c/d/nested.txt"))
            .await
            .unwrap();
        assert_eq!(content, b"deep content");
    }

    #[tokio::test]
    async fn test_add_file_binary() {
        let fs = InMemoryFs::new();
        let binary_data = vec![0x00, 0xFF, 0x89, 0x50, 0x4E, 0x47];
        fs.add_file("/data/binary.bin", &binary_data, 0o644);

        let content = fs.read_file(Path::new("/data/binary.bin")).await.unwrap();
        assert_eq!(content, binary_data);
    }
}
