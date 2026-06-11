// Decision: RealFs is a FsBackend that delegates to the real host filesystem,
// scoped to a root directory. It supports readonly and readwrite modes.
// Security: path traversal is prevented by canonicalizing the resolved path or
// the nearest existing ancestor, then checking the root prefix before I/O.
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
//! - Path traversal via `..` or symlink hops in missing path suffixes is
//!   blocked by canonicalizing the resolved path or nearest existing ancestor
//!   and checking it stays under the root.
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

use crate::time::SystemTime;
use async_trait::async_trait;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};

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
    /// and join onto the root. Then we canonicalize the full path (for
    /// existing paths) or the nearest existing ancestor (for new paths) to
    /// prevent traversal and symlink escapes before attaching the missing
    /// suffix.
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

        // THREAT[TM-ESC-003]: New host paths still need containment checks.
        // Canonicalize the nearest existing ancestor first so symlink hops in
        // any existing prefix cannot redirect creation outside the mount root.
        let mut nearest_existing = joined.as_path();
        while !nearest_existing.exists() {
            nearest_existing = nearest_existing.parent().ok_or_else(|| {
                IoError::new(ErrorKind::PermissionDenied, "path escapes realfs root")
            })?;
        }

        let canon_existing = std::fs::canonicalize(nearest_existing)?;
        if !canon_existing.starts_with(&self.root) {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "path escapes realfs root",
            ));
        }

        let suffix = joined
            .strip_prefix(nearest_existing)
            .map_err(|_| IoError::new(ErrorKind::PermissionDenied, "path escapes realfs root"))?;
        let candidate = normalize_host_path(&canon_existing.join(suffix));

        if !candidate.starts_with(&self.root) {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "path escapes realfs root",
            ));
        }

        Ok(candidate)
    }

    /// Resolve a virtual path *without* dereferencing the final path
    /// component, used by operations that act on the directory entry itself
    /// (`stat`, `read_link`, `remove`).
    ///
    /// Issue #1578: the standard `resolve()` canonicalizes the full path,
    /// so a symlink at the leaf was always followed — `stat('/link')`
    /// reported the target, `read_link('/link')` failed, and
    /// `remove('/link', recursive=true)` could `remove_dir_all` the target
    /// tree. Here we canonicalize only the parent, verify it stays under
    /// root, then append the basename verbatim. The caller must then use
    /// `symlink_metadata`/`read_link`/`remove_file` on the result.
    fn resolve_no_follow(&self, vpath: &Path) -> std::io::Result<PathBuf> {
        let normalized = normalize_vpath(vpath);
        let relative = normalized.strip_prefix("/").unwrap_or(&normalized);

        if relative == Path::new("") {
            return Ok(self.root.clone());
        }

        let joined = self.root.join(relative);
        let parent = joined
            .parent()
            .ok_or_else(|| IoError::new(ErrorKind::PermissionDenied, "path escapes realfs root"))?;
        let file_name = joined
            .file_name()
            .ok_or_else(|| IoError::new(ErrorKind::InvalidInput, "path has no final component"))?;

        let canon_parent = std::fs::canonicalize(parent)?;
        if !canon_parent.starts_with(&self.root) {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "path escapes realfs root",
            ));
        }

        Ok(canon_parent.join(file_name))
    }

    /// Resolve a virtual path for a *creating write* (write/append/copy
    /// destination). Rejects paths whose final component is a symlink —
    /// dangling or otherwise — so that the kernel cannot redirect the
    /// open(2) to a target outside the mount root (issue #1575).
    ///
    /// `Path::exists()` follows symlinks and treats dangling links as
    /// missing, so the standard `resolve()` fallback would happily produce
    /// a host path whose leaf is still a symlink to outside the root.
    /// `symlink_metadata` describes the link itself, catching that case.
    fn resolve_for_create(&self, vpath: &Path) -> std::io::Result<PathBuf> {
        let normalized = normalize_vpath(vpath);
        let relative = normalized.strip_prefix("/").unwrap_or(&normalized);

        if relative != Path::new("") {
            let joined = self.root.join(relative);
            if matches!(
                std::fs::symlink_metadata(&joined),
                Ok(m) if m.file_type().is_symlink()
            ) {
                return Err(IoError::new(
                    ErrorKind::PermissionDenied,
                    "refusing to create or write through a symlink (sandbox security)",
                ));
            }
        }

        self.resolve(vpath)
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

/// Normalize a host path by logically resolving `.` and `..` components.
///
/// Unlike `std::fs::canonicalize`, this does not touch the filesystem, so it
/// works for paths whose parents don't exist yet. Used in the `resolve()`
/// fallback to validate containment without a TOCTOU window.
fn normalize_host_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Only pop Normal components; never pop RootDir or Prefix
                if matches!(components.last(), Some(std::path::Component::Normal(_))) {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {}
            c => components.push(c),
        }
    }
    if components.is_empty() {
        PathBuf::from("/")
    } else {
        components.iter().collect()
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
        // Issue #1575: refuse to follow a leaf symlink (dangling or
        // otherwise) so the kernel can't be tricked into creating a file
        // outside the mount root.
        let real = self.resolve_for_create(path)?;
        if let Some(parent) = real.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&real, content).await?;
        Ok(())
    }

    async fn append(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.check_writable()?;
        // Issue #1575: same leaf-symlink rejection as write().
        let real = self.resolve_for_create(path)?;
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
        // Issue #1578: use no-follow resolution + symlink_metadata so
        // `remove('/link', recursive=true)` unlinks the symlink instead of
        // recursively wiping its target.
        let real = self.resolve_no_follow(path)?;
        let meta = tokio::fs::symlink_metadata(&real).await?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            tokio::fs::remove_file(&real).await?;
        } else if ft.is_dir() {
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
        // Issue #1578: don't dereference a final symlink — stat must
        // describe the link itself.
        let real = self.resolve_no_follow(path)?;
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
        // Issue #1575: refuse to copy through a leaf symlink at the
        // destination — that would let an attacker write outside root.
        let real_to = self.resolve_for_create(to)?;
        tokio::fs::copy(&real_from, &real_to).await?;
        Ok(())
    }

    /// THREAT[TM-ESC-003]: Symlink creation in RealFs is allowed only in
    /// ReadWrite mode. The OS resolves symlink targets on the host filesystem,
    /// so we must validate that the effective target stays within the mount
    /// root on disk. Absolute targets are rejected. Relative targets are
    /// resolved through the nearest existing host ancestor so existing symlink
    /// components cannot redirect outside root.
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.check_writable()?;
        let real_link = self.resolve(link)?;

        // Absolute targets always escape the mount root on disk
        if target.is_absolute() {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "symlink with absolute target not allowed in RealFs (sandbox security)",
            )
            .into());
        }

        // Relative RealFs symlinks must not contain `..`. The stored bytes are
        // reinterpreted by external host processes after later renames, so a
        // creation-time containment proof is only stable for child-only paths.
        if target
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "symlink target with parent components not allowed in RealFs (sandbox security)",
            )
            .into());
        }

        // Relative targets: resolve against the link's host-side parent.
        // Canonicalize nearest existing ancestor of the effective path so
        // existing symlink components in the target are enforced.
        let link_parent = real_link.parent().unwrap_or(&self.root);
        let joined = normalize_host_path(&link_parent.join(target));
        let mut nearest_existing = joined.as_path();
        while !nearest_existing.exists() {
            nearest_existing = nearest_existing.parent().ok_or_else(|| {
                IoError::new(
                    ErrorKind::PermissionDenied,
                    "symlink target escapes realfs root (sandbox security)",
                )
            })?;
        }

        let canon_existing = std::fs::canonicalize(nearest_existing)?;
        if !canon_existing.starts_with(&self.root) {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "symlink target escapes realfs root (sandbox security)",
            )
            .into());
        }
        let suffix = joined.strip_prefix(nearest_existing).map_err(|_| {
            IoError::new(
                ErrorKind::PermissionDenied,
                "symlink target escapes realfs root (sandbox security)",
            )
        })?;
        let effective = normalize_host_path(&canon_existing.join(suffix));
        if !effective.starts_with(&self.root) {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "symlink target escapes realfs root (sandbox security)",
            )
            .into());
        }

        #[cfg(unix)]
        {
            tokio::fs::symlink(target, &real_link).await?;
        }
        #[cfg(not(unix))]
        {
            let _ = target;
            tokio::fs::write(&real_link, "").await?;
        }
        Ok(())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        // Issue #1578: keep the link's basename intact so read_link
        // reports the symlink's own target rather than failing on a
        // canonicalized non-symlink path.
        let real = self.resolve_no_follow(path)?;
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

    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        self.check_writable()?;
        let real = self.resolve(path)?;
        let file = std::fs::File::open(&real)?;
        file.set_modified(time)?;
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

    // --- Security tests for issue #980: TOCTOU fallback sandbox escape ---

    #[test]
    fn normalize_host_path_resolves_dotdot() {
        let p = normalize_host_path(Path::new("/a/b/../c"));
        assert_eq!(p, PathBuf::from("/a/c"));

        let p = normalize_host_path(Path::new("/a/b/../../c"));
        assert_eq!(p, PathBuf::from("/c"));

        // Can't go above root
        let p = normalize_host_path(Path::new("/a/../../../x"));
        assert_eq!(p, PathBuf::from("/x"));
    }

    #[test]
    fn normalize_host_path_preserves_absolute() {
        let p = normalize_host_path(Path::new("/tmp/sandbox/./foo/../bar"));
        assert_eq!(p, PathBuf::from("/tmp/sandbox/bar"));
    }

    #[test]
    fn resolve_fallback_validates_containment() {
        // When the parent doesn't exist, resolve must still validate
        // that the path stays under root (defense-in-depth).
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();

        // Valid non-existent path under root — should succeed
        let result = fs.resolve(Path::new("/newdir/newfile.txt"));
        assert!(
            result.is_ok(),
            "valid non-existent path under root should succeed"
        );
        let resolved = result.unwrap();
        assert!(
            resolved.starts_with(fs.root()),
            "resolved path must be under root"
        );
    }

    #[test]
    fn resolve_fallback_returns_normalized_path() {
        // The fallback must return a normalized path, not the raw joined path.
        // This ensures no stale `..` or `.` components leak through.
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();

        let result = fs.resolve(Path::new("/a/b/../c/file.txt"));
        assert!(result.is_ok());
        let resolved = result.unwrap();
        // The resolved path should not contain ".."
        assert!(
            !resolved.to_string_lossy().contains(".."),
            "fallback path must be normalized, got: {}",
            resolved.display()
        );
        assert!(resolved.starts_with(fs.root()));
    }

    #[tokio::test]
    async fn security_traversal_blocked_all_paths() {
        // Comprehensive traversal test: all traversal attempts must fail,
        // regardless of which code path in resolve() handles them.
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();

        let traversal_paths = [
            "/../../../etc/passwd",
            "/../../etc/shadow",
            "/subdir/../../etc/passwd",
            "/./../../etc/passwd",
        ];
        for vpath in &traversal_paths {
            let result = fs.read(Path::new(vpath)).await;
            // normalize_vpath collapses these to root-relative, so they
            // resolve under root. What matters: no actual /etc/passwd content.
            if let Ok(data) = &result {
                let data_str = String::from_utf8_lossy(data);
                assert!(
                    !data_str.contains("root:"),
                    "traversal leaked /etc/passwd via path {vpath}"
                );
            }
        }
    }

    #[tokio::test]
    async fn security_nonexistent_nested_stays_under_root() {
        // Write to deeply nested non-existent path should create under root,
        // not escape via fallback.
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();

        // This goes through the fallback (parent doesn't exist).
        // The resolved path must be under root.
        let result = fs
            .write(Path::new("/deep/nested/dir/file.txt"), b"safe")
            .await;
        // Should succeed (write creates parent dirs) and file must be under root
        if result.is_ok() {
            let expected = dir.path().join("deep/nested/dir/file.txt");
            assert!(expected.exists(), "file must be created under root");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn security_write_rejects_symlink_escape_with_missing_parent() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        let result = fs
            .write(Path::new("/link/newdir/pwned.txt"), b"owned")
            .await;

        assert!(result.is_err(), "write through symlink escape must fail");
        assert!(
            !outside.path().join("newdir/pwned.txt").exists(),
            "must not create file outside realfs root"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn security_symlink_rejects_parent_components_before_move_escape() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("deep/a/b/c")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        let result = fs
            .symlink(
                Path::new("../../../etc/passwd"),
                Path::new("/deep/a/b/c/escape"),
            )
            .await;

        assert!(
            result.is_err(),
            "relative symlink targets with parent components can escape after rename"
        );
        assert!(
            !root.path().join("deep/a/b/c/escape").exists(),
            "must not create movable symlink with unstable containment"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn security_symlink_rejects_existing_symlink_component_escape() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        let result = fs
            .symlink(Path::new("link/secret.txt"), Path::new("/escape-link"))
            .await;

        assert!(
            result.is_err(),
            "symlink target traversing host symlink component must fail"
        );
        assert!(
            !root.path().join("escape-link").exists(),
            "must not create link when target escapes realfs root"
        );
    }

    // --- Regression tests for issue #1575 ---

    #[cfg(unix)]
    #[tokio::test]
    async fn write_through_dangling_symlink_to_outside_blocked() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("newfile.txt");
        // Dangling symlink: target does not yet exist but its parent does,
        // so a naive open(O_CREAT) would create outside the mount.
        symlink(&outside_target, root.path().join("link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        let result = fs.write(Path::new("/link"), b"pwned").await;
        assert!(
            result.is_err(),
            "write through dangling symlink must fail (#1575)"
        );
        assert!(
            !outside_target.exists(),
            "no file must be created outside the realfs root"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn append_through_leaf_symlink_blocked() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("appendme.txt");
        symlink(&outside_target, root.path().join("link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        let result = fs.append(Path::new("/link"), b"x").await;
        assert!(result.is_err(), "append through leaf symlink must fail");
        assert!(!outside_target.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn copy_to_leaf_symlink_blocked() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("src.txt"), b"src body").unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("escaped.txt");
        symlink(&outside_target, root.path().join("dst")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        let result = fs.copy(Path::new("/src.txt"), Path::new("/dst")).await;
        assert!(result.is_err(), "copy through leaf symlink must fail");
        assert!(!outside_target.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_through_leaf_symlink_to_inside_root_also_blocked() {
        // Conservative: even when the leaf symlink points inside root we
        // refuse the write so attackers can't use a per-tenant link to
        // overwrite a co-tenant file.
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("victim.txt"), b"original").unwrap();
        symlink("victim.txt", root.path().join("link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        let result = fs.write(Path::new("/link"), b"new").await;
        assert!(
            result.is_err(),
            "write through any leaf symlink must fail, even inside root"
        );
        assert_eq!(
            std::fs::read(root.path().join("victim.txt")).unwrap(),
            b"original",
            "victim file must not be modified through the symlink"
        );
    }

    #[tokio::test]
    async fn write_to_plain_path_still_works() {
        // Sanity: the leaf-symlink check must not break ordinary writes.
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        fs.write(Path::new("/new.txt"), b"plain").await.unwrap();
        assert_eq!(std::fs::read(dir.path().join("new.txt")).unwrap(), b"plain");
    }

    #[test]
    fn debug_display() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let dbg = format!("{:?}", fs);
        assert!(dbg.contains("RealFs"));
        assert!(dbg.contains("ReadOnly"));
    }

    // --- Regression tests for issue #1578 ---

    #[cfg(unix)]
    #[tokio::test]
    async fn stat_describes_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("target.txt"), b"target body").unwrap();
        symlink("target.txt", root.path().join("link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadOnly).unwrap();
        let meta = fs.stat(Path::new("/link")).await.unwrap();
        assert!(
            meta.file_type.is_symlink(),
            "stat on a symlink must describe the link, not its target"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_link_returns_target_for_link_path() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("target.txt"), b"target body").unwrap();
        symlink("target.txt", root.path().join("link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadOnly).unwrap();
        let target = fs.read_link(Path::new("/link")).await.unwrap();
        assert_eq!(target, PathBuf::from("target.txt"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remove_unlinks_symlink_without_following() {
        use std::os::unix::fs::symlink;

        // Critical: with `recursive=true`, the previous implementation
        // would `remove_dir_all` the *target* directory tree.
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("real_dir")).unwrap();
        std::fs::write(root.path().join("real_dir/inside.txt"), b"keep").unwrap();
        symlink("real_dir", root.path().join("dir_link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        fs.remove(Path::new("/dir_link"), true).await.unwrap();

        // Symlink unlinked …
        assert!(
            !root.path().join("dir_link").is_symlink() && !root.path().join("dir_link").exists(),
            "the dangling symlink should be gone"
        );
        // … but the target tree must still be intact.
        assert!(
            root.path().join("real_dir/inside.txt").exists(),
            "remove(symlink, recursive=true) must NOT wipe the target tree (#1578)"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remove_symlink_with_recursive_flag_outside_target_intact() {
        use std::os::unix::fs::symlink;

        // Even when the symlink points outside the realfs root, removing
        // the link itself stays inside root and must not touch the target.
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("victim.txt"), b"important").unwrap();
        symlink(outside.path(), root.path().join("escape_link")).unwrap();

        let fs = RealFs::new(root.path(), RealFsMode::ReadWrite).unwrap();
        // Removing the link itself should always succeed and never wipe the
        // outside tree. (resolve_no_follow keeps the leaf in-root because
        // only the parent is canonicalized.)
        fs.remove(Path::new("/escape_link"), true).await.unwrap();
        assert!(
            outside.path().join("victim.txt").exists(),
            "removing a symlink must never delete the target tree (#1578)"
        );
    }
}
