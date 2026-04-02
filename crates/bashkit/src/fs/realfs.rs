// Decision: RealFs is a FsBackend that delegates to the real host filesystem,
// scoped to a root directory. It supports readonly and readwrite modes.
// Security: path traversal is prevented by canonicalizing and checking the prefix.
// This module is only available with the `realfs` feature flag.

//! Real filesystem backend.
//!
//! [`RealFs`] provides access to a directory on the host filesystem as an
//! [`FsBackend`]. It is gated behind the `realfs` feature flag because it
//! intentionally breaks the sandbox boundary.
//!
//! # Security
//!
//! - All paths are resolved relative to a configured root directory.
//! - Path traversal via `..` is blocked by canonicalizing and checking the
//!   resolved path stays under the root.
//! - Readonly mode rejects all write operations at the backend level.
//!
//! # Modes
//!
//! | Mode | Reads | Writes | Use case |
//! |------|-------|--------|----------|
//! | `RealFsMode::ReadOnly` | Yes | No | Expose host files to scripts safely |
//! | `RealFsMode::ReadWrite` | Yes | Yes | Let scripts modify host files (dangerous) |
//!
//! # Builder API (Recommended)
//!
//! The easiest way to use RealFs is through the builder on [`Bash`](crate::Bash):
//!
//! ```rust,no_run
//! use bashkit::Bash;
//!
//! // Readonly: host files visible at /mnt/data, writes go to in-memory overlay
//! let bash = Bash::builder()
//!     .mount_real_readonly_at("/tmp", "/mnt/data")
//!     .build();
//!
//! // Read-write: scripts can modify host files (dangerous!)
//! let bash = Bash::builder()
//!     .mount_real_readwrite_at("/tmp", "/mnt/workspace")
//!     .build();
//! ```
//!
//! # Direct Usage
//!
//! For full control, create a `RealFs` backend and wrap it with
//! [`PosixFs`](super::PosixFs):
//!
//! ```rust,no_run
//! use bashkit::PosixFs;
//! use bashkit::{RealFs, RealFsMode};
//! use std::sync::Arc;
//!
//! let backend = RealFs::new("/tmp", RealFsMode::ReadOnly).unwrap();
//! let fs = Arc::new(PosixFs::new(backend));
//! let bash = bashkit::Bash::builder().fs(fs).build();
//! ```
//!
//! # CLI
//!
//! ```bash
//! bashkit --mount-ro /path/to/data:/mnt/data -c 'cat /mnt/data/file.txt'
//! bashkit --mount-rw /path/to/out:/mnt/out -c 'echo hi > /mnt/out/result.txt'
//! ```

use async_trait::async_trait;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::backend::FsBackend;
use super::limits::{FsLimits, FsUsage};
use super::traits::{DirEntry, FileType, Metadata};
use crate::error::Result;

/// Access mode for the real filesystem backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealFsMode {
    /// Read-only access. All write operations return permission denied.
    ReadOnly,
    /// Read-write access. Scripts can modify files on the host filesystem.
    ///
    /// # Warning
    ///
    /// This breaks the sandbox boundary. Only use when the script is trusted
    /// and the root directory is scoped appropriately.
    ReadWrite,
}

/// Real filesystem backend scoped to a root directory.
///
/// Wraps host filesystem access with path containment and optional readonly
/// enforcement. Use with [`PosixFs`](super::PosixFs) for POSIX semantics.
///
/// # Example
///
/// ```rust,no_run
/// use bashkit::{RealFs, RealFsMode};
/// use bashkit::PosixFs;
/// use std::sync::Arc;
///
/// let backend = RealFs::new("/tmp", RealFsMode::ReadOnly).unwrap();
/// let fs = Arc::new(PosixFs::new(backend));
/// let bash = bashkit::Bash::builder().fs(fs).build();
/// ```
pub struct RealFs {
    /// Canonicalized root directory on the host.
    root: PathBuf,
    mode: RealFsMode,
}

impl RealFs {
    /// Create a new RealFs backend rooted at the given directory.
    ///
    /// The root path is canonicalized on creation. Returns an error if the
    /// path does not exist or is not a directory.
    pub fn new(root: impl AsRef<Path>, mode: RealFsMode) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root.as_ref())?;
        if !root.is_dir() {
            return Err(IoError::new(
                ErrorKind::NotADirectory,
                format!("realfs root is not a directory: {}", root.display()),
            ));
        }
        Ok(Self { root, mode })
    }

    /// Resolve a virtual path to a real host path, ensuring it stays under root.
    ///
    /// Virtual paths are absolute (e.g. `/foo/bar`). We strip the leading `/`
    /// and join onto the root. Then we canonicalize (for existing paths) or
    /// check the parent (for new paths) to prevent traversal.
    fn resolve(&self, vpath: &Path) -> std::io::Result<PathBuf> {
        let normalized = normalize_vpath(vpath);
        // Strip leading "/" to make it relative
        let relative = normalized.strip_prefix("/").unwrap_or(&normalized);

        // For root path itself
        if relative == Path::new("") {
            return Ok(self.root.clone());
        }

        let joined = self.root.join(relative);

        // If the path exists, canonicalize and check
        if joined.exists() {
            let canon = std::fs::canonicalize(&joined)?;
            if !canon.starts_with(&self.root) {
                return Err(IoError::new(
                    ErrorKind::PermissionDenied,
                    "path escapes realfs root",
                ));
            }
            return Ok(canon);
        }

        // Path doesn't exist yet - check that its parent is within root
        if let Some(parent) = joined.parent()
            && parent.exists()
        {
            let canon_parent = std::fs::canonicalize(parent)?;
            if !canon_parent.starts_with(&self.root) {
                return Err(IoError::new(
                    ErrorKind::PermissionDenied,
                    "path escapes realfs root",
                ));
            }
            // Re-join the filename onto the canonicalized parent
            if let Some(file_name) = joined.file_name() {
                return Ok(canon_parent.join(file_name));
            }
        }

        // Fallback: just use the joined path (will fail at the OS level
        // if parent doesn't exist, which is the correct POSIX behavior)
        Ok(joined)
    }

    /// Check that the mode allows writes. Returns PermissionDenied if readonly.
    fn check_writable(&self) -> std::io::Result<()> {
        if self.mode == RealFsMode::ReadOnly {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "realfs is mounted readonly",
            ));
        }
        Ok(())
    }

    /// Get the root directory path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the access mode.
    pub fn mode(&self) -> RealFsMode {
        self.mode
    }
}

impl std::fmt::Debug for RealFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealFs")
            .field("root", &self.root)
            .field("mode", &self.mode)
            .finish()
    }
}

fn file_type_from_std(ft: std::fs::FileType) -> FileType {
    if ft.is_dir() {
        FileType::Directory
    } else if ft.is_symlink() {
        FileType::Symlink
    } else {
        FileType::File
    }
}

fn metadata_from_std(m: &std::fs::Metadata) -> Metadata {
    let file_type = file_type_from_std(m.file_type());
    let size = if file_type.is_dir() { 0 } else { m.len() };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o7777
    };
    #[cfg(not(unix))]
    let mode = if m.permissions().readonly() {
        0o444
    } else {
        0o644
    };
    Metadata {
        file_type,
        size,
        mode,
        modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        created: m.created().unwrap_or(SystemTime::UNIX_EPOCH),
    }
}

/// Normalize a virtual path: collapse `.` and `..`, ensure absolute.
fn normalize_vpath(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {
                components.clear();
                components.push(std::path::Component::RootDir);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if components.len() > 1 {
                    components.pop();
                }
            }
            c => components.push(c),
        }
    }
    if components.is_empty() {
        PathBuf::from("/")
    } else {
        components.iter().collect()
    }
}

#[async_trait]
impl FsBackend for RealFs {
    async fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let real = self.resolve(path)?;
        let data = tokio::fs::read(&real).await?;
        Ok(data)
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.check_writable()?;
        let real = self.resolve(path)?;
        if let Some(parent) = real.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&real, content).await?;
        Ok(())
    }

    async fn append(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.check_writable()?;
        let real = self.resolve(path)?;
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&real)
            .await?;
        file.write_all(content).await?;
        file.flush().await?;
        Ok(())
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        self.check_writable()?;
        let real = self.resolve(path)?;
        if recursive {
            tokio::fs::create_dir_all(&real).await?;
        } else {
            tokio::fs::create_dir(&real).await?;
        }
        Ok(())
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.check_writable()?;
        let real = self.resolve(path)?;
        let meta = tokio::fs::metadata(&real).await?;
        if meta.is_dir() {
            if recursive {
                tokio::fs::remove_dir_all(&real).await?;
            } else {
                tokio::fs::remove_dir(&real).await?;
            }
        } else {
            tokio::fs::remove_file(&real).await?;
        }
        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let real = self.resolve(path)?;
        // Use symlink_metadata to not follow symlinks
        let meta = tokio::fs::symlink_metadata(&real).await?;
        Ok(metadata_from_std(&meta))
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let real = self.resolve(path)?;
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&real).await?;
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await?;
            entries.push(DirEntry {
                name,
                metadata: metadata_from_std(&meta),
            });
        }
        // Sort for deterministic output
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let real = self.resolve(path)?;
        Ok(tokio::fs::try_exists(&real).await.unwrap_or(false))
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.check_writable()?;
        let real_from = self.resolve(from)?;
        let real_to = self.resolve(to)?;
        tokio::fs::rename(&real_from, &real_to).await?;
        Ok(())
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.check_writable()?;
        let real_from = self.resolve(from)?;
        let real_to = self.resolve(to)?;
        tokio::fs::copy(&real_from, &real_to).await?;
        Ok(())
    }

    /// THREAT[TM-ESC-003]: Symlink creation is blocked in RealFs to prevent
    /// sandbox escape. Even though bashkit itself doesn't follow symlinks
    /// (TM-ESC-002), any external process sharing the directory tree would
    /// follow them, enabling reads/writes to arbitrary host paths.
    async fn symlink(&self, _target: &Path, _link: &Path) -> Result<()> {
        Err(IoError::new(
            ErrorKind::PermissionDenied,
            "symlink creation is not allowed in RealFs (sandbox security)",
        )
        .into())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let real = self.resolve(path)?;
        let target = tokio::fs::read_link(&real).await?;
        Ok(target)
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.check_writable()?;
        let real = self.resolve(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(mode);
            tokio::fs::set_permissions(&real, perms).await?;
        }
        #[cfg(not(unix))]
        {
            let _ = (mode, &real);
        }
        Ok(())
    }

    fn usage(&self) -> FsUsage {
        // Could walk the real directory, but that's expensive. Return zeros.
        FsUsage::default()
    }

    fn limits(&self) -> FsLimits {
        FsLimits::unlimited()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        // Create some test files
        std::fs::write(dir.path().join("hello.txt"), b"hello world").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        std::fs::write(dir.path().join("subdir/nested.txt"), b"nested content").unwrap();
        dir
    }

    #[tokio::test]
    async fn read_file() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let data = fs.read(Path::new("/hello.txt")).await.unwrap();
        assert_eq!(data, b"hello world");
    }

    #[tokio::test]
    async fn read_nested() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let data = fs.read(Path::new("/subdir/nested.txt")).await.unwrap();
        assert_eq!(data, b"nested content");
    }

    #[tokio::test]
    async fn read_root_dir() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let entries = fs.read_dir(Path::new("/")).await.unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"hello.txt"));
        assert!(names.contains(&"subdir"));
    }

    #[tokio::test]
    async fn stat_file() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let meta = fs.stat(Path::new("/hello.txt")).await.unwrap();
        assert!(meta.file_type.is_file());
        assert_eq!(meta.size, 11); // "hello world"
    }

    #[tokio::test]
    async fn stat_dir() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let meta = fs.stat(Path::new("/subdir")).await.unwrap();
        assert!(meta.file_type.is_dir());
        assert_eq!(meta.size, 0);
    }

    #[tokio::test]
    async fn exists_checks() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        assert!(fs.exists(Path::new("/hello.txt")).await.unwrap());
        assert!(fs.exists(Path::new("/subdir")).await.unwrap());
        assert!(fs.exists(Path::new("/")).await.unwrap());
        assert!(!fs.exists(Path::new("/nope")).await.unwrap());
    }

    #[tokio::test]
    async fn readonly_rejects_write() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let err = fs.write(Path::new("/new.txt"), b"data").await;
        assert!(err.is_err());
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("readonly"), "error was: {msg}");
    }

    #[tokio::test]
    async fn readonly_rejects_mkdir() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let err = fs.mkdir(Path::new("/newdir"), false).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn readonly_rejects_remove() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let err = fs.remove(Path::new("/hello.txt"), false).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn readwrite_can_write() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        fs.write(Path::new("/new.txt"), b"new data").await.unwrap();
        let data = fs.read(Path::new("/new.txt")).await.unwrap();
        assert_eq!(data, b"new data");
    }

    #[tokio::test]
    async fn readwrite_can_mkdir() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        fs.mkdir(Path::new("/newdir"), false).await.unwrap();
        assert!(fs.exists(Path::new("/newdir")).await.unwrap());
    }

    #[tokio::test]
    async fn readwrite_can_remove() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        fs.remove(Path::new("/hello.txt"), false).await.unwrap();
        assert!(!fs.exists(Path::new("/hello.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn readwrite_append() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        fs.append(Path::new("/hello.txt"), b" appended")
            .await
            .unwrap();
        let data = fs.read(Path::new("/hello.txt")).await.unwrap();
        assert_eq!(data, b"hello world appended");
    }

    #[tokio::test]
    async fn path_traversal_blocked() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        // Attempt to read outside root via ..
        let result = fs.read(Path::new("/../../../etc/passwd")).await;
        // Should either fail with permission denied or not found (depending on
        // whether /etc/passwd exists), but must not succeed in reading it
        if let Ok(data) = &result {
            // If it somehow succeeded, the content must not be /etc/passwd
            assert!(
                data == b"hello world" || data.is_empty(),
                "path traversal should not leak host files"
            );
        }
    }

    #[tokio::test]
    async fn normalize_collapses_dots() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let data = fs.read(Path::new("/subdir/../hello.txt")).await.unwrap();
        assert_eq!(data, b"hello world");
    }

    #[tokio::test]
    async fn rename_readwrite() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        fs.rename(Path::new("/hello.txt"), Path::new("/renamed.txt"))
            .await
            .unwrap();
        assert!(!fs.exists(Path::new("/hello.txt")).await.unwrap());
        let data = fs.read(Path::new("/renamed.txt")).await.unwrap();
        assert_eq!(data, b"hello world");
    }

    #[tokio::test]
    async fn copy_readwrite() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        fs.copy(Path::new("/hello.txt"), Path::new("/copied.txt"))
            .await
            .unwrap();
        let data = fs.read(Path::new("/copied.txt")).await.unwrap();
        assert_eq!(data, b"hello world");
        // Original still exists
        assert!(fs.exists(Path::new("/hello.txt")).await.unwrap());
    }

    #[test]
    fn new_rejects_nonexistent() {
        let result = RealFs::new(
            "/nonexistent/path/that/does/not/exist",
            RealFsMode::ReadOnly,
        );
        assert!(result.is_err());
    }

    #[test]
    fn new_rejects_file_as_root() {
        let dir = setup();
        let file_path = dir.path().join("hello.txt");
        let result = RealFs::new(&file_path, RealFsMode::ReadOnly);
        assert!(result.is_err());
    }

    #[test]
    fn debug_display() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let dbg = format!("{:?}", fs);
        assert!(dbg.contains("RealFs"));
        assert!(dbg.contains("ReadOnly"));
    }
}
