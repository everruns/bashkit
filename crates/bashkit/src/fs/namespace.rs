//! Static filesystem namespaces composed from arbitrary filesystem subtrees.
//!
//! Important decision: namespace paths are normalized before mount selection,
//! then appended beneath a normalized source root. This ordering makes `..`
//! unable to escape a source root or bypass a nested/read-only mount.

use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use super::{
    DirEntry, FileSystem, FileSystemExt, FileType, FsLimits, FsUsage, Metadata, ReadOnlyFs,
    normalize_path,
};
use crate::Result;
use crate::time_compat::SystemTime;

/// Access granted to a filesystem mounted in a [`NamespaceFs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceAccess {
    /// Reads are allowed and every mutation is denied.
    ReadOnly,
    /// Reads and mutations are delegated to the mounted filesystem.
    ReadWrite,
}

struct NamespaceMount {
    target: PathBuf,
    source_root: PathBuf,
    fs: Arc<dyn FileSystem>,
    access: NamespaceAccess,
}

struct PendingMount {
    source_root: PathBuf,
    fs: Arc<dyn FileSystem>,
    access: NamespaceAccess,
}

/// Builder for a static [`NamespaceFs`].
#[derive(Default)]
pub struct NamespaceFsBuilder {
    mounts: BTreeMap<PathBuf, PendingMount>,
}

impl NamespaceFsBuilder {
    /// Mount the source filesystem's root at `target` with explicit access.
    pub fn mount(
        self,
        target: impl AsRef<Path>,
        fs: Arc<dyn FileSystem>,
        access: NamespaceAccess,
    ) -> Result<Self> {
        self.mount_from(target, fs, "/", access)
    }

    /// Mount `source_root` from `fs` at `target` with explicit access.
    ///
    /// Mount paths and source roots must be absolute POSIX paths. Adding the
    /// same target again replaces the earlier binding deterministically.
    pub fn mount_from(
        mut self,
        target: impl AsRef<Path>,
        fs: Arc<dyn FileSystem>,
        source_root: impl AsRef<Path>,
        access: NamespaceAccess,
    ) -> Result<Self> {
        let target = validate_mount_path(target.as_ref(), "namespace target")?;
        let source_root = validate_mount_path(source_root.as_ref(), "source root")?;
        let fs = match access {
            NamespaceAccess::ReadOnly => Arc::new(ReadOnlyFs::new(fs)) as Arc<dyn FileSystem>,
            NamespaceAccess::ReadWrite => fs,
        };
        self.mounts.insert(
            target,
            PendingMount {
                source_root,
                fs,
                access,
            },
        );
        Ok(self)
    }

    /// Mount the source filesystem's root read-only at `target`.
    pub fn mount_readonly(self, target: impl AsRef<Path>, fs: Arc<dyn FileSystem>) -> Result<Self> {
        self.mount(target, fs, NamespaceAccess::ReadOnly)
    }

    /// Mount `source_root` read-only at `target`.
    pub fn mount_readonly_from(
        self,
        target: impl AsRef<Path>,
        fs: Arc<dyn FileSystem>,
        source_root: impl AsRef<Path>,
    ) -> Result<Self> {
        self.mount_from(target, fs, source_root, NamespaceAccess::ReadOnly)
    }

    /// Mount the source filesystem's root read-write at `target`.
    pub fn mount_readwrite(
        self,
        target: impl AsRef<Path>,
        fs: Arc<dyn FileSystem>,
    ) -> Result<Self> {
        self.mount(target, fs, NamespaceAccess::ReadWrite)
    }

    /// Mount `source_root` read-write at `target`.
    pub fn mount_readwrite_from(
        self,
        target: impl AsRef<Path>,
        fs: Arc<dyn FileSystem>,
        source_root: impl AsRef<Path>,
    ) -> Result<Self> {
        self.mount_from(target, fs, source_root, NamespaceAccess::ReadWrite)
    }

    /// Finish the namespace. The builder owns every mounted filesystem.
    pub fn build(self) -> NamespaceFs {
        let mut synthetic_dirs = BTreeSet::from([PathBuf::from("/")]);
        let mounts = self
            .mounts
            .into_iter()
            .map(|(target, pending)| {
                let mut ancestor = target.parent();
                while let Some(path) = ancestor {
                    synthetic_dirs.insert(normalize_path(path));
                    ancestor = path.parent();
                }
                NamespaceMount {
                    target,
                    source_root: pending.source_root,
                    fs: pending.fs,
                    access: pending.access,
                }
            })
            .collect();

        NamespaceFs {
            mounts,
            synthetic_dirs,
            synthetic_metadata: Metadata {
                file_type: FileType::Directory,
                size: 0,
                mode: 0o755,
                modified: SystemTime::now(),
                created: SystemTime::now(),
            },
            limits: FsLimits::new(),
        }
    }
}

/// One visible path tree composed from arbitrary [`FileSystem`] instances.
///
/// The namespace is static after construction. Longest target-prefix wins for
/// nested mounts. Missing ancestors are exposed as synthetic directories.
/// Cross-mount file and symlink copies are supported; cross-mount rename is
/// rejected with [`ErrorKind::CrossesDevices`] because it cannot be atomic.
///
/// # Example
///
/// ```rust
/// use bashkit::{FileSystem, InMemoryFs, NamespaceFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let source = Arc::new(InMemoryFs::new());
/// source.mkdir(Path::new("/project"), false).await?;
/// source
///     .write_file(Path::new("/project/input.txt"), b"input")
///     .await?;
/// let output = Arc::new(InMemoryFs::new());
///
/// let namespace = NamespaceFs::builder()
///     .mount_readonly_from("/src", source, "/project")?
///     .mount_readwrite("/build", output.clone())?
///     .build();
///
/// assert_eq!(namespace.read_file(Path::new("/src/input.txt")).await?, b"input");
/// namespace.write_file(Path::new("/build/output.txt"), b"output").await?;
/// assert_eq!(output.read_file(Path::new("/output.txt")).await?, b"output");
/// # Ok(())
/// # }
/// ```
pub struct NamespaceFs {
    mounts: Vec<NamespaceMount>,
    synthetic_dirs: BTreeSet<PathBuf>,
    synthetic_metadata: Metadata,
    limits: FsLimits,
}

struct ResolvedPath<'a> {
    index: usize,
    mount: &'a NamespaceMount,
    source_path: PathBuf,
}

impl NamespaceFs {
    /// Start an empty namespace builder.
    pub fn builder() -> NamespaceFsBuilder {
        NamespaceFsBuilder::default()
    }

    fn normalized(&self, path: &Path) -> Result<PathBuf> {
        self.limits
            .validate_path(path)
            .map_err(|error| IoError::new(ErrorKind::InvalidInput, error.to_string()))?;
        Ok(normalize_path(path))
    }

    // THREAT[TM-ESC-031]: selection happens on the normalized visible path;
    // only a component-stripped relative suffix is appended to source_root.
    fn resolve(&self, path: &Path) -> Option<ResolvedPath<'_>> {
        self.mounts
            .iter()
            .enumerate()
            .filter(|(_, mount)| path.starts_with(&mount.target))
            .max_by_key(|(_, mount)| mount.target.components().count())
            .map(|(index, mount)| {
                let relative = path.strip_prefix(&mount.target).unwrap_or(Path::new(""));
                let source_path = if relative.as_os_str().is_empty() {
                    mount.source_root.clone()
                } else {
                    mount.source_root.join(relative)
                };
                ResolvedPath {
                    index,
                    mount,
                    source_path,
                }
            })
    }

    fn is_exact_mount(&self, path: &Path) -> bool {
        self.mounts.iter().any(|mount| mount.target == path)
    }

    fn is_synthetic(&self, path: &Path) -> bool {
        self.synthetic_dirs.contains(path) && !self.is_exact_mount(path)
    }

    fn is_namespace_node(&self, path: &Path) -> bool {
        self.synthetic_dirs.contains(path) || self.is_exact_mount(path)
    }

    fn outside_read_error() -> crate::Error {
        IoError::new(ErrorKind::NotFound, "path is outside namespace mounts").into()
    }

    fn outside_write_error() -> crate::Error {
        IoError::new(
            ErrorKind::PermissionDenied,
            "path is outside writable namespace mounts",
        )
        .into()
    }

    fn cross_mount_error() -> crate::Error {
        IoError::new(
            ErrorKind::CrossesDevices,
            "cross-mount rename is not atomic",
        )
        .into()
    }

    fn require_writable(resolved: &ResolvedPath<'_>) -> Result<()> {
        if resolved.mount.access == NamespaceAccess::ReadOnly {
            return Err(
                IoError::new(ErrorKind::PermissionDenied, "filesystem is read-only").into(),
            );
        }
        Ok(())
    }

    fn child_mount_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut children = BTreeSet::new();
        for mount in &self.mounts {
            let Ok(relative) = mount.target.strip_prefix(path) else {
                continue;
            };
            let Some(Component::Normal(name)) = relative.components().next() else {
                continue;
            };
            children.insert(if path == Path::new("/") {
                PathBuf::from("/").join(name)
            } else {
                path.join(name)
            });
        }
        children.into_iter().collect()
    }

    fn resolve_writable(&self, path: &Path) -> Result<ResolvedPath<'_>> {
        let path = self.normalized(path)?;
        if self.is_synthetic(&path) {
            return Err(Self::outside_write_error());
        }
        let resolved = self.resolve(&path).ok_or_else(Self::outside_write_error)?;
        Self::require_writable(&resolved)?;
        Ok(resolved)
    }
}

#[async_trait]
impl FileSystem for NamespaceFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path = self.normalized(path)?;
        if self.is_synthetic(&path) {
            return Err(super::fs_errors::is_a_directory());
        }
        let resolved = self.resolve(&path).ok_or_else(Self::outside_read_error)?;
        resolved.mount.fs.read_file(&resolved.source_path).await
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let resolved = self.resolve_writable(path)?;
        resolved
            .mount
            .fs
            .write_file(&resolved.source_path, content)
            .await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let resolved = self.resolve_writable(path)?;
        resolved
            .mount
            .fs
            .append_file(&resolved.source_path, content)
            .await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let resolved = self.resolve_writable(path)?;
        resolved
            .mount
            .fs
            .mkdir(&resolved.source_path, recursive)
            .await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = self.normalized(path)?;
        if self.is_namespace_node(&path) {
            return Err(Self::outside_write_error());
        }
        let resolved = self.resolve_writable(&path)?;
        resolved
            .mount
            .fs
            .remove(&resolved.source_path, recursive)
            .await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = self.normalized(path)?;
        if self.is_synthetic(&path) {
            return Ok(self.synthetic_metadata.clone());
        }
        let resolved = self.resolve(&path).ok_or_else(Self::outside_read_error)?;
        resolved.mount.fs.stat(&resolved.source_path).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = self.normalized(path)?;
        let mut entries = if self.is_synthetic(&path) {
            Vec::new()
        } else {
            let resolved = self.resolve(&path).ok_or_else(Self::outside_read_error)?;
            resolved.mount.fs.read_dir(&resolved.source_path).await?
        };

        for child_path in self.child_mount_paths(&path) {
            let name = child_path
                .file_name()
                .expect("child mount path has a name")
                .to_string_lossy()
                .into_owned();
            let metadata = self.stat(&child_path).await?;
            if let Some(entry) = entries.iter_mut().find(|entry| entry.name == name) {
                entry.metadata = metadata;
            } else {
                entries.push(DirEntry { name, metadata });
            }
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = self.normalized(path)?;
        if self.is_synthetic(&path) {
            return Ok(true);
        }
        let Some(resolved) = self.resolve(&path) else {
            return Ok(false);
        };
        resolved.mount.fs.exists(&resolved.source_path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = self.normalized(from)?;
        let to = self.normalized(to)?;
        if self.is_namespace_node(&from) || self.is_namespace_node(&to) {
            return Err(Self::outside_write_error());
        }
        let from = self.resolve(&from).ok_or_else(Self::outside_write_error)?;
        let to = self.resolve(&to).ok_or_else(Self::outside_write_error)?;
        Self::require_writable(&from)?;
        Self::require_writable(&to)?;
        if from.index != to.index {
            return Err(Self::cross_mount_error());
        }
        from.mount
            .fs
            .rename(&from.source_path, &to.source_path)
            .await
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = self.normalized(from)?;
        let to = self.normalized(to)?;
        if self.is_namespace_node(&to) {
            return Err(Self::outside_write_error());
        }
        if self.is_synthetic(&from) {
            return Err(super::fs_errors::is_a_directory());
        }
        let from = self.resolve(&from).ok_or_else(Self::outside_read_error)?;
        let to = self.resolve(&to).ok_or_else(Self::outside_write_error)?;
        Self::require_writable(&to)?;
        if from.index == to.index {
            return from.mount.fs.copy(&from.source_path, &to.source_path).await;
        }

        let metadata = from.mount.fs.stat(&from.source_path).await?;
        match metadata.file_type {
            FileType::File => {
                let content = from.mount.fs.read_file(&from.source_path).await?;
                to.mount.fs.write_file(&to.source_path, &content).await
            }
            FileType::Symlink => {
                let target = from.mount.fs.read_link(&from.source_path).await?;
                to.mount.fs.symlink(&target, &to.source_path).await
            }
            FileType::Directory => Err(IoError::new(
                ErrorKind::Unsupported,
                "cross-mount directory copy is not supported",
            )
            .into()),
            FileType::Fifo => Err(IoError::new(
                ErrorKind::Unsupported,
                "cross-mount FIFO copy is not supported",
            )
            .into()),
        }
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        let resolved = self.resolve_writable(link)?;
        resolved
            .mount
            .fs
            .symlink(target, &resolved.source_path)
            .await
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = self.normalized(path)?;
        if self.is_synthetic(&path) {
            return Err(IoError::new(ErrorKind::InvalidInput, "not a symlink").into());
        }
        let resolved = self.resolve(&path).ok_or_else(Self::outside_read_error)?;
        resolved.mount.fs.read_link(&resolved.source_path).await
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        let resolved = self.resolve_writable(path)?;
        resolved.mount.fs.chmod(&resolved.source_path, mode).await
    }

    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        let resolved = self.resolve_writable(path)?;
        resolved
            .mount
            .fs
            .set_modified_time(&resolved.source_path, time)
            .await
    }
}

#[async_trait]
impl FileSystemExt for NamespaceFs {
    fn usage(&self) -> FsUsage {
        self.mounts
            .iter()
            .fold(FsUsage::default(), |mut total, mount| {
                let usage = mount.fs.usage();
                total.total_bytes += usage.total_bytes;
                total.file_count += usage.file_count;
                total.dir_count += usage.dir_count;
                total
            })
    }

    fn limits(&self) -> FsLimits {
        self.limits.clone()
    }

    async fn mkfifo(&self, path: &Path, mode: u32) -> Result<()> {
        let resolved = self.resolve_writable(path)?;
        resolved.mount.fs.mkfifo(&resolved.source_path, mode).await
    }
}

fn validate_mount_path(path: &Path, label: &str) -> Result<PathBuf> {
    if !path.as_os_str().as_encoded_bytes().starts_with(b"/") {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!("{label} must be an absolute POSIX path"),
        )
        .into());
    }
    FsLimits::new()
        .validate_path(path)
        .map_err(|error| IoError::new(ErrorKind::InvalidInput, error.to_string()))?;
    Ok(normalize_path(path))
}
