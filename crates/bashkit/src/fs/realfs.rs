// Decision: RealFs is a FsBackend that delegates to the real host filesystem,
// scoped to a root directory. Host filesystem work runs on Tokio's blocking
// pool so current-thread runtimes cannot be blocked by host I/O.
// `open` is the safe default; deprecated `new` remains only as a migration shim.
// Security (TM-ESC-003, TM-ESC-035): every host operation is performed
// relative to a directory handle for the mount root via `cap-std`, never via
// a re-joined host pathname. Resolution happens inside the same kernel call
// as the operation (openat2 RESOLVE_BENEATH on Linux, per-component no-follow
// walks elsewhere), so a script cannot win a race by swapping a checked
// directory for an outward symlink between a containment check and the I/O.
// Absolute symlink targets and `..` hops above the root fail resolution.
// This module is only available with the `realfs` feature flag.
// Relative paths below the root handle are built with `Path::join` on
// purpose: they are host-relative, where the host separator is correct (see
// crates/bashkit/clippy.toml).
#![allow(clippy::disallowed_methods)]

//! Real filesystem backend.
//!
//! [`RealFs`] provides access to a directory on the host filesystem as an
//! [`FsBackend`]. It is gated behind the `realfs` feature flag because it
//! intentionally breaks the sandbox boundary.
//!
//! # Security
//!
//! - All paths are resolved relative to an open handle on the configured root
//!   directory. Resolution and the filesystem operation happen together, so
//!   concurrent renames or symlink swaps on a writable mount cannot redirect
//!   an operation outside the root.
//! - Path traversal via `..`, absolute symlinks, or symlinks whose target
//!   climbs above the root is rejected with a permission error.
//! - Whole-file reads of host files are limited to 10 MB (10,000,000 bytes).
//! - Readonly mode rejects all write operations at the backend level.
//! - The configured host root cannot be removed or renamed through the mount.
//!
//! # Ownership
//!
//! On Unix, metadata reports the host file's UID/GID. The shell still uses
//! sandbox UID/GID 1000 (`id`, `UID`, `EUID`); `-O`/`-G` compare against those
//! sandbox IDs, not the embedding process's effective IDs. Thus a host file
//! owned by another account can fail these tests even when that account runs
//! the process. This does not impersonate users or change host permissions.
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
//! # tokio_test::block_on(async {
//! let backend = RealFs::open("/tmp", RealFsMode::ReadOnly).await.unwrap();
//! let fs = Arc::new(PosixFs::new(backend));
//! let bash = bashkit::Bash::builder().fs(fs).build();
//! # });
//! ```
//!
//! # CLI
//!
//! ```bash
//! bashkit --mount-ro /path/to/data:/mnt/data -c 'cat /mnt/data/file.txt'
//! bashkit --mount-rw /path/to/out:/mnt/out -c 'echo hi > /mnt/out/result.txt'
//! ```

use crate::time_compat::SystemTime;
use async_trait::async_trait;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use cap_std::time::SystemTime as HostTime; // std-time-ok: realfs is native-only
use rand::Rng;
use std::io::{Error as IoError, ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::backend::FsBackend;
use super::limits::{FsLimits, FsUsage};
use super::traits::{DirEntry, FileType, Metadata};
use crate::error::Result;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
/// # tokio_test::block_on(async {
/// let backend = RealFs::open("/tmp", RealFsMode::ReadOnly).await.unwrap();
/// let fs = Arc::new(PosixFs::new(backend));
/// let bash = bashkit::Bash::builder().fs(fs).build();
/// # });
/// ```
pub struct RealFs {
    /// Canonicalized root directory on the host (reporting only; never used
    /// to build host paths for I/O).
    root: PathBuf,
    /// Handle on the root. Every host operation resolves beneath it.
    dir: Arc<Dir>,
    mode: RealFsMode,
}

/// The current directory of a [`Dir`]: the mount root itself.
const ROOT: &str = ".";

fn escape_error() -> IoError {
    IoError::new(ErrorKind::PermissionDenied, "path escapes realfs root")
}

/// Report a `cap-std` sandbox escape with the stable RealFs wording.
fn normalize_escape(error: IoError) -> IoError {
    if error.kind() == ErrorKind::PermissionDenied
        // cap-std's escape error; match the stable fragment, not the whole
        // sentence, so a rewording upstream still maps to our message.
        && error.to_string().contains("led outside of the filesystem")
    {
        escape_error()
    } else {
        error
    }
}

fn symlink_target_escape_error() -> IoError {
    IoError::new(
        ErrorKind::PermissionDenied,
        "symlink target escapes realfs root (sandbox security)",
    )
}

fn root_busy_error() -> IoError {
    IoError::new(
        ErrorKind::ResourceBusy,
        "cannot remove or rename the root of a host mount",
    )
}

/// Path handed to `cap-std`: the relative path, or `.` for the root.
fn at(rel: &Path) -> &Path {
    if rel.as_os_str().is_empty() {
        Path::new(ROOT)
    } else {
        rel
    }
}

/// Reject a final component that is a symlink, dangling or otherwise, before
/// a creating write (issue #1575). Containment no longer depends on this —
/// `cap-std` refuses to follow a link outside the root — but writes stay
/// defined on the entry the script named rather than a link target.
fn reject_leaf_symlink(dir: &Dir, rel: &Path) -> std::io::Result<()> {
    if rel.as_os_str().is_empty() {
        return Ok(());
    }
    match dir.symlink_metadata(rel) {
        Ok(meta) if meta.is_symlink() => Err(IoError::new(
            ErrorKind::PermissionDenied,
            "refusing to create or write through a symlink (sandbox security)",
        )),
        _ => Ok(()),
    }
}

fn temporary_sibling(dir: &Dir, rel: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    let parent = rel.parent().unwrap_or(Path::new(""));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    for _ in 0..128 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nonce = rand::rng().next_u64();
        let candidate = parent.join(format!(".bashkit-tmp-{sequence:016x}-{nonce:016x}"));
        match dir.open_with(&candidate, &options) {
            Ok(file) => return Ok((candidate, file.into_std())),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(IoError::new(
        ErrorKind::AlreadyExists,
        "unable to allocate atomic write staging file",
    ))
}

/// Finish a staged file and rename it over `rel`. On error the staging entry
/// is removed and the old destination is left untouched.
fn commit_staged(
    dir: &Dir,
    temporary: &Path,
    rel: &Path,
    fill: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<()> {
    let result = fill().and_then(|()| dir.rename(temporary, dir, rel));
    if result.is_err() {
        let _ = dir.remove_file(temporary);
    }
    result
}

/// THREAT[TM-FS-014]: Replacement writes are staged beside the target and
/// renamed only after a complete flush.
fn write_atomically(dir: &Dir, rel: &Path, content: &[u8]) -> std::io::Result<()> {
    if rel.as_os_str().is_empty() {
        return Err(IoError::new(ErrorKind::IsADirectory, "is a directory"));
    }
    let existing_permissions = dir.metadata(rel).ok().map(|m| m.permissions());
    let (temporary, mut file) = temporary_sibling(dir, rel)?;
    commit_staged(dir, &temporary, rel, || {
        file.write_all(content)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        if let Some(permissions) = existing_permissions {
            dir.set_permissions(&temporary, permissions)?;
        }
        Ok(())
    })
}

/// THREAT[TM-DOS-105]: Stream the existing file into sibling staging so
/// atomic append uses bounded memory even when the host file is large.
fn append_atomically(dir: &Dir, rel: &Path, content: &[u8]) -> std::io::Result<()> {
    if rel.as_os_str().is_empty() {
        return Err(IoError::new(ErrorKind::IsADirectory, "is a directory"));
    }
    let existing = match dir.open(rel) {
        Ok(file) => Some(file.into_std()),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let existing_permissions = match &existing {
        Some(file) => Some(file.metadata()?.permissions()),
        None => None,
    };
    let (temporary, mut staged) = temporary_sibling(dir, rel)?;
    commit_staged(dir, &temporary, rel, || {
        if let Some(mut existing) = existing {
            std::io::copy(&mut existing, &mut staged)?;
        }
        staged.write_all(content)?;
        staged.flush()?;
        staged.sync_all()?;
        if let Some(permissions) = existing_permissions {
            staged.set_permissions(permissions)?;
        }
        Ok(())
    })
}

fn copy_atomically(dir: &Dir, from: &Path, to: &Path) -> std::io::Result<()> {
    if to.as_os_str().is_empty() {
        return Err(IoError::new(ErrorKind::IsADirectory, "is a directory"));
    }
    let mut source = dir.open(at(from))?.into_std();
    let source_meta = source.metadata()?;
    if source_meta.is_dir() {
        return Err(IoError::new(ErrorKind::IsADirectory, "is a directory"));
    }
    let (temporary, mut staged) = temporary_sibling(dir, to)?;
    commit_staged(dir, &temporary, to, || {
        std::io::copy(&mut source, &mut staged)?;
        #[allow(unused_mut)]
        let mut permissions = source_meta.permissions();
        // THREAT[TM-ESC-010]: a copy keeps only the rwx bits, as chmod does,
        // so a copy cannot mint a setuid file on the host.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(permissions.mode() & 0o777);
        }
        staged.flush()?;
        staged.sync_all()?;
        staged.set_permissions(permissions)?;
        Ok(())
    })
}

impl RealFs {
    fn from_root(root: PathBuf, mode: RealFsMode) -> std::io::Result<Self> {
        let not_dir = || {
            IoError::new(
                ErrorKind::NotADirectory,
                format!("realfs root is not a directory: {}", root.display()),
            )
        };
        // Match Path::is_dir(): metadata lookup failures become
        // NotADirectory after canonicalization for both constructors.
        if !std::fs::metadata(&root)
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            return Err(not_dir());
        }
        let dir = Dir::open_ambient_dir(&root, ambient_authority())?;
        Ok(Self {
            root,
            dir: Arc::new(dir),
            mode,
        })
    }

    /// Create a new RealFs backend rooted at the given directory.
    ///
    /// The root path is canonicalized on creation. Returns an error if the
    /// path does not exist or is not a directory.
    #[deprecated(
        since = "0.15.0",
        note = "blocks on host filesystem I/O; use RealFs::open(...).await"
    )]
    pub fn new(root: impl AsRef<Path>, mode: RealFsMode) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root.as_ref())?;
        Self::from_root(root, mode)
    }

    /// Open a RealFs backend without blocking the async runtime.
    ///
    /// The root is canonicalized and validated as a directory before use.
    pub async fn open(root: impl AsRef<Path>, mode: RealFsMode) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || {
            let root = std::fs::canonicalize(root)?;
            Self::from_root(root, mode)
        })
        .await
        .map_err(|error| IoError::other(format!("realfs worker failed: {error}")))?
    }

    /// Map a virtual path to a path relative to the mount root.
    ///
    /// THREAT[TM-ESC-033]: Use the shared POSIX VFS normalizer, then keep only
    /// normal components. Host prefixes, roots, and `..` never reach the
    /// host. An empty result is the root itself. The relative path is only
    /// ever resolved beneath the root handle.
    fn relative(vpath: &Path) -> std::io::Result<PathBuf> {
        super::reject_empty_path(vpath)?;
        let normalized = super::normalize_path(vpath);
        let mut rel = PathBuf::new();
        for component in normalized.components() {
            match component {
                Component::Normal(name) => rel.push(name),
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir | Component::Prefix(_) => return Err(escape_error()),
            }
        }
        Ok(rel)
    }

    /// Run `op` against the root handle on Tokio's blocking pool.
    async fn with_dir<T, F>(&self, op: F) -> std::io::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Dir) -> std::io::Result<T> + Send + 'static,
    {
        let dir = Arc::clone(&self.dir);
        tokio::task::spawn_blocking(move || op(&dir).map_err(normalize_escape))
            .await
            .map_err(|error| IoError::other(format!("realfs worker failed: {error}")))?
    }

    /// List a directory opened beneath the root handle. Entries are pulled
    /// from the blocking pool in small batches so the caller's read charge
    /// (not `Send`/`'static`) admits each one before it is retained.
    async fn read_dir_admitted(
        &self,
        path: &Path,
        max_entries: usize,
        charge: super::ReadCharge<'_>,
    ) -> Result<Vec<DirEntry>> {
        const BATCH: usize = 256;
        let rel = Self::relative(path)?;
        let mut iter = Some(
            self.with_dir(move |dir| dir.open_dir(at(&rel))?.entries())
                .await?,
        );
        let mut entries = Vec::new();
        while let Some(mut read_dir) = iter.take() {
            let (batch, more) = tokio::task::spawn_blocking(move || {
                let mut batch = Vec::new();
                for entry in read_dir.by_ref() {
                    let entry = entry.map_err(normalize_escape)?;
                    batch.push((entry.file_name(), entry.metadata()?));
                    if batch.len() == BATCH {
                        return Ok::<_, IoError>((batch, Some(read_dir)));
                    }
                }
                Ok((batch, None))
            })
            .await
            .map_err(|error| IoError::other(format!("realfs worker failed: {error}")))??;
            for (name, meta) in batch {
                if entries.len() == max_entries {
                    return Err(super::traits::fs_errors::directory_entry_limit());
                }
                super::charge_dir_entry(charge, name.len())?;
                entries.push(DirEntry {
                    name: name.to_string_lossy().into_owned(),
                    metadata: metadata_from_cap(&meta),
                });
            }
            iter = more;
        }
        // Sort for deterministic output
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
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

fn metadata_from_cap(m: &cap_std::fs::Metadata) -> Metadata {
    #[cfg(unix)]
    let (uid, gid) = {
        use cap_std::fs::MetadataExt;
        (m.uid(), m.gid())
    };
    #[cfg(not(unix))]
    let (uid, gid) = (1000, 1000);
    let ft = m.file_type();
    let file_type = if ft.is_dir() {
        FileType::Directory
    } else if ft.is_symlink() {
        FileType::Symlink
    } else {
        FileType::File
    };
    let size = if file_type.is_dir() { 0 } else { m.len() };
    #[cfg(unix)]
    let mode = {
        use cap_std::fs::PermissionsExt;
        m.permissions().mode() & 0o7777
    };
    #[cfg(not(unix))]
    let mode = if m.permissions().readonly() {
        0o444
    } else {
        0o644
    };
    let time =
        |t: std::io::Result<HostTime>| t.map(|t| t.into_std()).unwrap_or(SystemTime::UNIX_EPOCH);
    Metadata {
        uid,
        gid,
        file_type,
        size,
        mode,
        modified: time(m.modified()),
        created: time(m.created()),
    }
}

fn symlink_metadata(dir: &Dir, rel: &Path) -> std::io::Result<cap_std::fs::Metadata> {
    if rel.as_os_str().is_empty() {
        dir.dir_metadata()
    } else {
        dir.symlink_metadata(rel)
    }
}

#[async_trait]
impl FsBackend for RealFs {
    async fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.read_bounded(path, None).await
    }

    async fn read_bounded(&self, path: &Path, charge: super::ReadCharge<'_>) -> Result<Vec<u8>> {
        use super::limits::DEFAULT_MAX_FILE_SIZE;
        use tokio::io::AsyncReadExt;

        let rel = Self::relative(path)?;
        // Resolve and open beneath the root handle; only the opened file
        // leaves the blocking worker, so the read cannot be redirected.
        let std_file = self
            .with_dir(move |dir| {
                let metadata = symlink_metadata(dir, &rel)?;
                if metadata.is_symlink() {
                    return Err(IoError::new(ErrorKind::NotFound, "file not found"));
                }
                if metadata.is_dir() {
                    return Err(IoError::new(ErrorKind::IsADirectory, "is a directory"));
                }
                let file = dir.open(at(&rel))?.into_std();
                if file.metadata()?.is_dir() {
                    return Err(IoError::new(ErrorKind::IsADirectory, "is a directory"));
                }
                Ok(file)
            })
            .await?;

        let too_large = || {
            IoError::new(
                ErrorKind::FileTooLarge,
                format!("host file exceeds read limit of {DEFAULT_MAX_FILE_SIZE} bytes"),
            )
        };
        let mut file = tokio::fs::File::from_std(std_file);
        let size = file.metadata().await?.len();
        if size > DEFAULT_MAX_FILE_SIZE {
            return Err(too_large().into());
        }

        // THREAT[TM-DOS-148]: Bound actual reads as the file can grow after
        // metadata is sampled or report a size smaller than its contents.
        if charge.is_some() {
            super::charge_read(charge, size as usize)?;
            let mut data = Vec::with_capacity(size as usize);
            let mut chunk = [0; 8192];
            loop {
                let count = file.read(&mut chunk).await?;
                if count == 0 {
                    return Ok(data);
                }
                let required = data.len() + count;
                if required as u64 > DEFAULT_MAX_FILE_SIZE {
                    return Err(too_large().into());
                }
                if required > data.capacity() {
                    let capacity = required
                        .max(data.capacity().saturating_mul(2))
                        .min(DEFAULT_MAX_FILE_SIZE as usize);
                    super::charge_read(charge, capacity - data.capacity())?;
                    data.reserve_exact(capacity - data.len());
                }
                data.extend_from_slice(&chunk[..count]);
            }
        }
        let mut data = Vec::with_capacity(size as usize + 1);
        file.take(DEFAULT_MAX_FILE_SIZE + 1)
            .read_to_end(&mut data)
            .await?;
        if data.len() as u64 > DEFAULT_MAX_FILE_SIZE {
            return Err(too_large().into());
        }
        Ok(data)
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.check_writable()?;
        let rel = Self::relative(path)?;
        let content = content.to_vec();
        self.with_dir(move |dir| {
            // Issue #1575: refuse to write through a leaf symlink.
            reject_leaf_symlink(dir, &rel)?;
            if let Some(parent) = rel.parent()
                && !parent.as_os_str().is_empty()
            {
                dir.create_dir_all(parent)?;
            }
            write_atomically(dir, &rel, &content)
        })
        .await?;
        Ok(())
    }

    async fn append(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.check_writable()?;
        let rel = Self::relative(path)?;
        let content = content.to_vec();
        self.with_dir(move |dir| {
            // Issue #1575: same leaf-symlink rejection as write().
            reject_leaf_symlink(dir, &rel)?;
            append_atomically(dir, &rel, &content)
        })
        .await?;
        Ok(())
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        self.check_writable()?;
        let rel = Self::relative(path)?;
        self.with_dir(move |dir| {
            if rel.as_os_str().is_empty() {
                return if recursive {
                    Ok(())
                } else {
                    Err(IoError::new(ErrorKind::AlreadyExists, "file exists"))
                };
            }
            if recursive {
                dir.create_dir_all(&rel)
            } else {
                dir.create_dir(&rel)
            }
        })
        .await?;
        Ok(())
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.check_writable()?;
        let rel = Self::relative(path)?;
        // THREAT[TM-FS-018]: A mount root may appear at a non-root shell path.
        if rel.as_os_str().is_empty() {
            return Err(root_busy_error().into());
        }
        self.with_dir(move |dir| {
            // Issue #1578: act on the entry itself so removing a symlink with
            // `recursive` unlinks it instead of wiping its target.
            let ft = dir.symlink_metadata(&rel)?.file_type();
            if ft.is_dir() {
                if recursive {
                    dir.remove_dir_all(&rel)
                } else {
                    dir.remove_dir(&rel)
                }
            } else {
                dir.remove_file(&rel)
            }
        })
        .await?;
        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        // Issue #1578: don't dereference a final symlink — stat must
        // describe the link itself.
        let rel = Self::relative(path)?;
        let meta = self
            .with_dir(move |dir| symlink_metadata(dir, &rel))
            .await?;
        Ok(metadata_from_cap(&meta))
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.read_dir_admitted(path, usize::MAX, None).await
    }

    async fn read_dir_limited(&self, path: &Path, max_entries: usize) -> Result<Vec<DirEntry>> {
        self.read_dir_admitted(path, max_entries, None).await
    }

    async fn read_dir_bounded(
        &self,
        path: &Path,
        charge: super::ReadCharge<'_>,
    ) -> Result<Vec<DirEntry>> {
        self.read_dir_admitted(path, usize::MAX, charge).await
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let rel = Self::relative(path)?;
        Ok(self.with_dir(move |dir| dir.try_exists(at(&rel))).await?)
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.check_writable()?;
        let from = Self::relative(from)?;
        let to = Self::relative(to)?;
        if from.as_os_str().is_empty() || to.as_os_str().is_empty() {
            return Err(root_busy_error().into());
        }
        self.with_dir(move |dir| {
            reject_leaf_symlink(dir, &to)?;
            dir.rename(&from, dir, &to)
        })
        .await?;
        Ok(())
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.check_writable()?;
        let from_rel = Self::relative(from)?;
        let to_rel = Self::relative(to)?;
        let link_target = self
            .with_dir(move |dir| {
                // Issue #1575: refuse to copy onto a leaf symlink.
                reject_leaf_symlink(dir, &to_rel)?;
                if symlink_metadata(dir, &from_rel)?.is_symlink() {
                    return dir.read_link_contents(&from_rel).map(Some);
                }
                copy_atomically(dir, &from_rel, &to_rel).map(|()| None)
            })
            .await?;
        if let Some(target) = link_target {
            // `to` is the caller's virtual path; reuse it rather than
            // rebuilding one from host components.
            self.symlink(&target, to).await?;
        }
        Ok(())
    }

    /// THREAT[TM-ESC-003]: Symlink creation in RealFs is allowed only in
    /// ReadWrite mode. Host processes may follow a link later without the
    /// root handle, so the stored target must stay within the mount root on
    /// disk. Absolute targets and `..` are rejected; existing components of
    /// the effective target are resolved beneath the root handle.
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.check_writable()?;
        let link = Self::relative(link)?;

        // THREAT[TM-ESC-033]: `C:target` is drive-relative, not absolute, but
        // still carries host namespace semantics and cannot be a VFS target.
        // `has_root()` rather than `is_absolute()`: a VFS-absolute target like
        // `/etc/passwd` has no drive prefix, so on Windows `is_absolute()` is
        // false (issue #2425's class).
        if target.has_root()
            || target.is_absolute()
            || target
                .components()
                .any(|component| matches!(component, Component::Prefix(_)))
        {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "symlink with absolute or drive-relative target not allowed in RealFs (sandbox security)",
            )
            .into());
        }

        // Relative RealFs symlinks must not contain `..`. The stored bytes are
        // reinterpreted by external host processes after later renames, so a
        // creation-time containment proof is only stable for child-only paths.
        if target
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(IoError::new(
                ErrorKind::PermissionDenied,
                "symlink target with parent components not allowed in RealFs (sandbox security)",
            )
            .into());
        }

        let target = target.to_path_buf();
        self.with_dir(move |dir| {
            // Existing symlink components inside the effective target must
            // not lead outside root. Walk to the nearest existing ancestor
            // and resolve it beneath the root handle; a dangling link on the
            // way is refused because its target is unchecked.
            let effective = link.parent().unwrap_or(Path::new("")).join(&target);
            let mut probe = effective.as_path();
            while !probe.as_os_str().is_empty() {
                match dir.canonicalize(probe) {
                    Ok(_) => break,
                    Err(error) if error.kind() == ErrorKind::NotFound => {
                        if dir
                            .symlink_metadata(probe)
                            .is_ok_and(|meta| meta.is_symlink())
                        {
                            return Err(symlink_target_escape_error());
                        }
                        probe = probe.parent().unwrap_or(Path::new(""));
                    }
                    Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                        return Err(symlink_target_escape_error());
                    }
                    Err(error) => return Err(error),
                }
            }

            #[cfg(unix)]
            {
                dir.symlink(&target, at(&link))
            }
            #[cfg(not(unix))]
            {
                dir.write(at(&link), "")
            }
        })
        .await?;
        Ok(())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        // Issue #1578: report the link's own stored target, including
        // absolute targets of pre-existing host links.
        let rel = Self::relative(path)?;
        if rel.as_os_str().is_empty() {
            return Err(IoError::new(ErrorKind::InvalidInput, "not a symbolic link").into());
        }
        Ok(self
            .with_dir(move |dir| dir.read_link_contents(&rel))
            .await?)
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.check_writable()?;
        let rel = Self::relative(path)?;
        #[cfg(unix)]
        {
            use cap_std::fs::{Permissions, PermissionsExt};
            // THREAT[TM-ESC-010]: only rwx bits reach the host. A script
            // must not set setuid/setgid/sticky on a host file through a
            // read-write mount (a setuid binary there runs as its owner).
            self.with_dir(move |dir| {
                dir.set_permissions(at(&rel), Permissions::from_mode(mode & 0o777))
            })
            .await?;
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            self.with_dir(move |dir| dir.metadata(at(&rel)).map(drop))
                .await?;
        }
        Ok(())
    }

    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        self.check_writable()?;
        let rel = Self::relative(path)?;
        // Windows `SetFileTime` requires `FILE_WRITE_ATTRIBUTES` on the handle,
        // so a read-only open fails with `ERROR_ACCESS_DENIED` (os error 5).
        // Unix `futimens` ignores the fd open mode.
        self.with_dir(move |dir| {
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use cap_std::fs::OpenOptionsExt;
                const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
                options.access_mode(FILE_WRITE_ATTRIBUTES);
            }
            dir.open_with(at(&rel), &options)?
                .into_std()
                .set_modified(time)
        })
        .await?;
        Ok(())
    }

    fn usage(&self) -> FsUsage {
        // Could walk the real directory, but that's expensive. Return zeros.
        FsUsage::default()
    }

    fn limits(&self) -> FsLimits {
        // These quotas describe retained filesystem content. The read-buffer
        // ceiling does not restrict host writes or native copies.
        FsLimits::unlimited()
    }
}

#[cfg(test)]
#[allow(deprecated)] // The suite also verifies the blocking migration shim.
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
    async fn bounded_directory_listing_requires_room_for_every_entry() {
        use crate::FileSystem;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a"), b"a").unwrap();
        std::fs::write(root.path().join("b"), b"b").unwrap();
        let backend = RealFs::new(root.path(), RealFsMode::ReadOnly).unwrap();
        let fs = super::super::PosixFs::new(backend);

        assert!(fs.read_dir_limited(Path::new("/"), 1).await.is_err());
        let entries = fs.read_dir_limited(Path::new("/"), 2).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a");
        assert_eq!(entries[1].name, "b");
    }

    #[tokio::test]
    async fn read_file() {
        let dir = setup();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap();
        let data = fs.read(Path::new("/hello.txt")).await.unwrap();
        assert_eq!(data, b"hello world");
    }

    #[tokio::test]
    async fn read_enforces_host_file_size_limit() {
        use super::super::limits::DEFAULT_MAX_FILE_SIZE;

        let dir = setup();
        let host_file = std::fs::File::create(dir.path().join("data")).unwrap();
        let path = Path::new("/data");

        for mode in [RealFsMode::ReadOnly, RealFsMode::ReadWrite] {
            let fs = RealFs::open(dir.path(), mode).await.unwrap();
            host_file.set_len(DEFAULT_MAX_FILE_SIZE).unwrap();
            assert_eq!(
                fs.read(path).await.unwrap().len() as u64,
                DEFAULT_MAX_FILE_SIZE
            );

            host_file.set_len(DEFAULT_MAX_FILE_SIZE + 1).unwrap();
            let error = fs.read(path).await.unwrap_err();
            assert!(
                matches!(error, crate::Error::Io(ref err) if err.kind() == ErrorKind::FileTooLarge)
            );
        }
    }

    #[tokio::test]
    async fn bounded_reads_honor_admission_through_posix() {
        use crate::FileSystem;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let dir = setup();
        let fs = super::super::PosixFs::new(RealFs::new(dir.path(), RealFsMode::ReadOnly).unwrap());
        let requested = AtomicUsize::new(0);
        let reject = |bytes| {
            requested.store(bytes, Ordering::Relaxed);
            Err(IoError::other("storage rejected").into())
        };
        let error = fs
            .read_file_bounded(Path::new("/hello.txt"), Some(&reject))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("storage rejected"));
        assert_eq!(requested.load(Ordering::Relaxed), b"hello world".len());

        let error = fs
            .read_dir_bounded(Path::new("/"), Some(&reject))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("storage rejected"));
        assert!(requested.load(Ordering::Relaxed) >= std::mem::size_of::<DirEntry>());

        let allow = |_| Ok(());
        assert_eq!(
            fs.read_file_bounded(Path::new("/hello.txt"), Some(&allow))
                .await
                .unwrap(),
            b"hello world"
        );
        assert_eq!(
            fs.read_dir_bounded(Path::new("/"), Some(&allow))
                .await
                .unwrap()
                .len(),
            2
        );
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
    async fn append_large_sparse_file_without_materializing_it() {
        const LARGE_FILE_SIZE: u64 = 128 * 1024 * 1024;

        let dir = setup();
        let path = dir.path().join("large.bin");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(LARGE_FILE_SIZE)
            .unwrap();
        let fs = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();

        fs.append(Path::new("/large.bin"), b"ok").await.unwrap();

        let mut file = std::fs::File::open(path).unwrap();
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(LARGE_FILE_SIZE)).unwrap();
        let mut suffix = Vec::new();
        file.read_to_end(&mut suffix).unwrap();
        assert_eq!(suffix, b"ok");
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

    #[tokio::test]
    #[allow(deprecated)]
    async fn open_matches_deprecated_constructor() {
        let dir = setup();
        let sync = RealFs::new(dir.path(), RealFsMode::ReadWrite).unwrap();
        let asynchronous = RealFs::open(dir.path(), RealFsMode::ReadWrite)
            .await
            .unwrap();

        assert_eq!(asynchronous.root(), sync.root());
        assert_eq!(asynchronous.mode(), sync.mode());

        let file_path = dir.path().join("hello.txt");
        let sync_error = RealFs::new(&file_path, RealFsMode::ReadOnly).unwrap_err();
        let async_error = RealFs::open(&file_path, RealFsMode::ReadOnly)
            .await
            .unwrap_err();
        assert_eq!(async_error.kind(), sync_error.kind());
    }

    // --- Security tests for issue #980: TOCTOU fallback sandbox escape ---

    #[test]
    fn relative_drops_roots_and_parent_hops() {
        let rel = |p: &str| RealFs::relative(Path::new(p)).unwrap();
        assert_eq!(rel("/"), PathBuf::new());
        assert_eq!(
            rel("/a/b/../c/file.txt"),
            Path::new("a").join("c").join("file.txt")
        );
        assert_eq!(rel("/../../../etc/passwd"), Path::new("etc").join("passwd"));
        assert_eq!(rel("/./x/."), PathBuf::from("x"));
        assert!(RealFs::relative(Path::new("")).is_err());
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

    /// Race a checked directory against a pre-existing outward symlink.
    ///
    /// A racer thread keeps swapping `/a` between a real directory and a
    /// symlink to a host directory outside the root. Every RealFs operation
    /// must resolve and act on the same object, so no read may return the
    /// outside secret and no write may land outside the root.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn security_parent_swap_race_cannot_escape_root() {
        use std::os::unix::fs::symlink;
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::time::{Duration, Instant}; // std-time-ok: native-only realfs test

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"OUTSIDE").unwrap();
        // Give chmod a distinguishable initial mode even under a 077 umask.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            outside.path().join("secret"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        std::fs::create_dir(root.path().join("a")).unwrap();
        std::fs::write(root.path().join("a/secret"), b"inside").unwrap();
        symlink(outside.path(), root.path().join("evil")).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let racer = {
            let stop = Arc::clone(&stop);
            let base = root.path().to_path_buf();
            std::thread::spawn(move || {
                let (a, real, evil) = (base.join("a"), base.join("a_real"), base.join("evil"));
                while !stop.load(Ordering::Relaxed) {
                    let _ = std::fs::rename(&a, &real);
                    let _ = std::fs::rename(&evil, &a);
                    std::thread::yield_now();
                    let _ = std::fs::rename(&a, &evil);
                    let _ = std::fs::rename(&real, &a);
                }
            })
        };

        let fs = RealFs::open(root.path(), RealFsMode::ReadWrite)
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut escaped = None;
        let mut inside_reads = 0u64;
        let mut i = 0u64;
        while Instant::now() < deadline && escaped.is_none() {
            i += 1;
            match fs.read(Path::new("/a/secret")).await.as_deref() {
                Ok(b"OUTSIDE") => escaped = Some(format!("read escaped root on iteration {i}")),
                Ok(b"inside") => inside_reads += 1,
                _ => {}
            }
            let _ = fs.write(Path::new("/a/pwned"), b"x").await;
            let _ = fs.chmod(Path::new("/a/secret"), 0o600).await;
            let _ = fs.remove(Path::new("/a/pwned"), false).await;
            if outside.path().join("pwned").exists() {
                escaped = Some(format!("write escaped root on iteration {i}"));
            }
        }
        stop.store(true, Ordering::Relaxed);
        racer.join().unwrap();

        let outside_mode = std::fs::metadata(outside.path().join("secret"))
            .unwrap()
            .permissions()
            .mode();
        assert!(escaped.is_none(), "{}", escaped.unwrap());
        assert!(inside_reads > 0, "race never exercised the in-root path");
        assert_ne!(outside_mode & 0o777, 0o600, "chmod escaped root");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn escape_through_host_symlink_uses_stable_wording() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"OUTSIDE").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("evil")).unwrap();

        let fs = RealFs::open(root.path(), RealFsMode::ReadOnly)
            .await
            .unwrap();
        let error = fs.read(Path::new("/evil/secret")).await.unwrap_err();
        assert!(
            matches!(error, crate::Error::Io(ref err)
                if err.kind() == ErrorKind::PermissionDenied
                    && err.to_string() == "path escapes realfs root"),
            "unexpected error: {error}"
        );
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
