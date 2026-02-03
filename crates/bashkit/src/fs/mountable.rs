//! Mountable filesystem implementation.
//!
//! [`MountableFs`] allows mounting multiple filesystems at different paths,
//! similar to Unix mount semantics.

// RwLock.read()/write().unwrap() only panics on lock poisoning (prior panic
// while holding lock). This is intentional - corrupted state should not propagate.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::io::Error as IoError;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use super::limits::{FsLimits, FsUsage};
use super::traits::{DirEntry, FileSystem, FileType, Metadata};
use crate::error::Result;

/// Filesystem with Unix-style mount points.
///
/// `MountableFs` allows mounting different filesystem implementations at
/// specific paths, similar to how Unix systems mount devices at directories.
/// This enables complex multi-source filesystem setups.
///
/// # Features
///
/// - **Multiple mount points**: Mount different filesystems at different paths
/// - **Nested mounts**: Mount filesystems within other mounts (longest-prefix matching)
/// - **Dynamic mounting**: Add/remove mounts at runtime
/// - **Cross-mount operations**: Copy/move files between different mounted filesystems
///
/// # Use Cases
///
/// - **Hybrid storage**: Combine in-memory temp storage with persistent data stores
/// - **Multi-tenant isolation**: Mount separate filesystems for different tenants
/// - **Plugin systems**: Each plugin gets its own mounted filesystem
/// - **Testing**: Mount mock filesystems for specific paths
///
/// # Example: Basic Mounting
///
/// ```rust
/// use bashkit::{Bash, FileSystem, InMemoryFs, MountableFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// // Create root and separate data filesystem
/// let root = Arc::new(InMemoryFs::new());
/// let data_fs = Arc::new(InMemoryFs::new());
///
/// // Pre-populate data filesystem
/// data_fs.write_file(Path::new("/users.json"), br#"["alice", "bob"]"#).await?;
///
/// // Create mountable filesystem
/// let mountable = MountableFs::new(root.clone());
///
/// // Mount data_fs at /mnt/data
/// mountable.mount("/mnt/data", data_fs.clone())?;
///
/// // Use with Bash
/// let mut bash = Bash::builder().fs(Arc::new(mountable)).build();
///
/// // Access mounted filesystem
/// let result = bash.exec("cat /mnt/data/users.json").await?;
/// assert!(result.stdout.contains("alice"));
///
/// // Access root filesystem
/// bash.exec("echo hello > /root.txt").await?;
/// # Ok(())
/// # }
/// ```
///
/// # Example: Nested Mounts
///
/// ```rust
/// use bashkit::{FileSystem, InMemoryFs, MountableFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let root = Arc::new(InMemoryFs::new());
/// let outer = Arc::new(InMemoryFs::new());
/// let inner = Arc::new(InMemoryFs::new());
///
/// outer.write_file(Path::new("/outer.txt"), b"outer").await?;
/// inner.write_file(Path::new("/inner.txt"), b"inner").await?;
///
/// let mountable = MountableFs::new(root);
/// mountable.mount("/mnt", outer)?;
/// mountable.mount("/mnt/nested", inner)?;
///
/// // Access outer mount
/// let content = mountable.read_file(Path::new("/mnt/outer.txt")).await?;
/// assert_eq!(content, b"outer");
///
/// // Access nested mount (longest-prefix matching)
/// let content = mountable.read_file(Path::new("/mnt/nested/inner.txt")).await?;
/// assert_eq!(content, b"inner");
/// # Ok(())
/// # }
/// ```
///
/// # Example: Dynamic Mount/Unmount
///
/// ```rust
/// use bashkit::{FileSystem, InMemoryFs, MountableFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let root = Arc::new(InMemoryFs::new());
/// let plugin_fs = Arc::new(InMemoryFs::new());
/// plugin_fs.write_file(Path::new("/plugin.so"), b"binary").await?;
///
/// let mountable = MountableFs::new(root);
///
/// // Mount plugin filesystem
/// mountable.mount("/plugins", plugin_fs)?;
/// assert!(mountable.exists(Path::new("/plugins/plugin.so")).await?);
///
/// // Unmount when done
/// mountable.unmount("/plugins")?;
/// assert!(!mountable.exists(Path::new("/plugins/plugin.so")).await?);
/// # Ok(())
/// # }
/// ```
///
/// # Path Resolution
///
/// When resolving a path, `MountableFs` uses longest-prefix matching to find
/// the appropriate filesystem. For example, with mounts at `/mnt` and `/mnt/data`:
///
/// - `/mnt/file.txt` → resolves to `/mnt` mount
/// - `/mnt/data/file.txt` → resolves to `/mnt/data` mount (longer prefix wins)
/// - `/other/file.txt` → resolves to root filesystem
pub struct MountableFs {
    /// Root filesystem (for paths not covered by any mount)
    root: Arc<dyn FileSystem>,
    /// Mount points: path -> filesystem
    /// BTreeMap ensures iteration in path order
    mounts: RwLock<BTreeMap<PathBuf, Arc<dyn FileSystem>>>,
}

impl MountableFs {
    /// Create a new `MountableFs` with the given root filesystem.
    ///
    /// The root filesystem is used for all paths that don't match any mount point.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{FileSystem, InMemoryFs, MountableFs};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let root = Arc::new(InMemoryFs::new());
    /// let mountable = MountableFs::new(root);
    ///
    /// // Paths not covered by mounts go to root
    /// mountable.write_file(Path::new("/tmp/test.txt"), b"hello").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(root: Arc<dyn FileSystem>) -> Self {
        Self {
            root,
            mounts: RwLock::new(BTreeMap::new()),
        }
    }

    /// Mount a filesystem at the given path.
    ///
    /// After mounting, all operations on paths under the mount point will be
    /// directed to the mounted filesystem.
    ///
    /// # Arguments
    ///
    /// * `path` - The mount point (must be an absolute path)
    /// * `fs` - The filesystem to mount
    ///
    /// # Errors
    ///
    /// Returns an error if the path is not absolute.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{FileSystem, InMemoryFs, MountableFs};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let root = Arc::new(InMemoryFs::new());
    /// let data_fs = Arc::new(InMemoryFs::new());
    /// data_fs.write_file(Path::new("/data.txt"), b"data").await?;
    ///
    /// let mountable = MountableFs::new(root);
    /// mountable.mount("/data", data_fs)?;
    ///
    /// // Access via mount point
    /// let content = mountable.read_file(Path::new("/data/data.txt")).await?;
    /// assert_eq!(content, b"data");
    /// # Ok(())
    /// # }
    /// ```
    pub fn mount(&self, path: impl AsRef<Path>, fs: Arc<dyn FileSystem>) -> Result<()> {
        let path = Self::normalize_path(path.as_ref());

        if !path.is_absolute() {
            return Err(IoError::other("mount path must be absolute").into());
        }

        let mut mounts = self.mounts.write().unwrap();
        mounts.insert(path, fs);
        Ok(())
    }

    /// Unmount a filesystem at the given path.
    ///
    /// After unmounting, paths that previously resolved to the mounted filesystem
    /// will fall back to the root filesystem or a shorter mount prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if no filesystem is mounted at the given path.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{FileSystem, InMemoryFs, MountableFs};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let root = Arc::new(InMemoryFs::new());
    /// let plugin = Arc::new(InMemoryFs::new());
    /// plugin.write_file(Path::new("/lib.so"), b"binary").await?;
    ///
    /// let mountable = MountableFs::new(root);
    /// mountable.mount("/plugin", plugin)?;
    ///
    /// // File is accessible
    /// assert!(mountable.exists(Path::new("/plugin/lib.so")).await?);
    ///
    /// // Unmount
    /// mountable.unmount("/plugin")?;
    ///
    /// // No longer accessible
    /// assert!(!mountable.exists(Path::new("/plugin/lib.so")).await?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn unmount(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = Self::normalize_path(path.as_ref());

        let mut mounts = self.mounts.write().unwrap();
        mounts
            .remove(&path)
            .ok_or_else(|| IoError::other("mount not found"))?;
        Ok(())
    }

    /// Normalize a path for consistent lookups
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

    /// Resolve a path to the appropriate filesystem and relative path.
    ///
    /// Returns (filesystem, path_within_mount).
    fn resolve(&self, path: &Path) -> (Arc<dyn FileSystem>, PathBuf) {
        let path = Self::normalize_path(path);
        let mounts = self.mounts.read().unwrap();

        // Find the longest matching mount point
        // BTreeMap iteration is in key order, but we need longest match
        // So we iterate and keep track of the best match
        let mut best_mount: Option<(&PathBuf, &Arc<dyn FileSystem>)> = None;

        for (mount_path, fs) in mounts.iter() {
            if path.starts_with(mount_path) {
                match best_mount {
                    None => best_mount = Some((mount_path, fs)),
                    Some((best_path, _)) => {
                        if mount_path.components().count() > best_path.components().count() {
                            best_mount = Some((mount_path, fs));
                        }
                    }
                }
            }
        }

        match best_mount {
            Some((mount_path, fs)) => {
                // Calculate relative path within mount
                let relative = path
                    .strip_prefix(mount_path)
                    .unwrap_or(Path::new(""))
                    .to_path_buf();

                // Ensure we have an absolute path
                let resolved = if relative.as_os_str().is_empty() {
                    PathBuf::from("/")
                } else {
                    PathBuf::from("/").join(relative)
                };

                (Arc::clone(fs), resolved)
            }
            None => {
                // Use root filesystem
                (Arc::clone(&self.root), path)
            }
        }
    }
}

#[async_trait]
impl FileSystem for MountableFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let (fs, resolved) = self.resolve(path);
        fs.read_file(&resolved).await
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let (fs, resolved) = self.resolve(path);
        fs.write_file(&resolved, content).await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let (fs, resolved) = self.resolve(path);
        fs.append_file(&resolved, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let (fs, resolved) = self.resolve(path);
        fs.mkdir(&resolved, recursive).await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let (fs, resolved) = self.resolve(path);
        fs.remove(&resolved, recursive).await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let (fs, resolved) = self.resolve(path);
        fs.stat(&resolved).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = Self::normalize_path(path);
        let (fs, resolved) = self.resolve(&path);

        let mut entries = fs.read_dir(&resolved).await?;

        // Add mount points that are direct children of this directory
        let mounts = self.mounts.read().unwrap();
        for mount_path in mounts.keys() {
            if mount_path.parent() == Some(&path) {
                if let Some(name) = mount_path.file_name() {
                    // Check if this entry already exists
                    let name_str = name.to_string_lossy().to_string();
                    if !entries.iter().any(|e| e.name == name_str) {
                        entries.push(DirEntry {
                            name: name_str,
                            metadata: Metadata {
                                file_type: FileType::Directory,
                                size: 0,
                                mode: 0o755,
                                modified: std::time::SystemTime::now(),
                                created: std::time::SystemTime::now(),
                            },
                        });
                    }
                }
            }
        }

        Ok(entries)
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = Self::normalize_path(path);

        // Check if this is a mount point
        {
            let mounts = self.mounts.read().unwrap();
            if mounts.contains_key(&path) {
                return Ok(true);
            }
        }

        let (fs, resolved) = self.resolve(&path);
        fs.exists(&resolved).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let (from_fs, from_resolved) = self.resolve(from);
        let (to_fs, to_resolved) = self.resolve(to);

        // Check if both paths resolve to the same filesystem
        // We can only do efficient rename within the same filesystem
        // For cross-mount rename, we need to copy + delete
        if Arc::ptr_eq(&from_fs, &to_fs) {
            from_fs.rename(&from_resolved, &to_resolved).await
        } else {
            // Cross-mount rename: copy then delete
            let content = from_fs.read_file(&from_resolved).await?;
            to_fs.write_file(&to_resolved, &content).await?;
            from_fs.remove(&from_resolved, false).await
        }
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let (from_fs, from_resolved) = self.resolve(from);
        let (to_fs, to_resolved) = self.resolve(to);

        if Arc::ptr_eq(&from_fs, &to_fs) {
            from_fs.copy(&from_resolved, &to_resolved).await
        } else {
            // Cross-mount copy
            let content = from_fs.read_file(&from_resolved).await?;
            to_fs.write_file(&to_resolved, &content).await
        }
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        let (fs, resolved) = self.resolve(link);
        fs.symlink(target, &resolved).await
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let (fs, resolved) = self.resolve(path);
        fs.read_link(&resolved).await
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        let (fs, resolved) = self.resolve(path);
        fs.chmod(&resolved, mode).await
    }

    fn usage(&self) -> FsUsage {
        // Aggregate usage from root and all mounts
        let mut total = self.root.usage();

        let mounts = self.mounts.read().unwrap();
        for fs in mounts.values() {
            let mount_usage = fs.usage();
            total.total_bytes += mount_usage.total_bytes;
            total.file_count += mount_usage.file_count;
            total.dir_count += mount_usage.dir_count;
        }

        total
    }

    fn limits(&self) -> FsLimits {
        // Return root filesystem limits as the overall limits
        self.root.limits()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;

    #[tokio::test]
    async fn test_mount_and_access() {
        let root = Arc::new(InMemoryFs::new());
        let mounted = Arc::new(InMemoryFs::new());

        // Write to mounted fs
        mounted
            .write_file(Path::new("/data.txt"), b"mounted data")
            .await
            .unwrap();

        let mfs = MountableFs::new(root.clone());
        mfs.mount("/mnt/data", mounted.clone()).unwrap();

        // Access through mountable fs
        let content = mfs
            .read_file(Path::new("/mnt/data/data.txt"))
            .await
            .unwrap();
        assert_eq!(content, b"mounted data");
    }

    #[tokio::test]
    async fn test_write_to_mount() {
        let root = Arc::new(InMemoryFs::new());
        let mounted = Arc::new(InMemoryFs::new());

        let mfs = MountableFs::new(root);
        mfs.mount("/mnt", mounted.clone()).unwrap();

        // Create directory and write file through mountable
        mfs.mkdir(Path::new("/mnt/subdir"), false).await.unwrap();
        mfs.write_file(Path::new("/mnt/subdir/test.txt"), b"hello")
            .await
            .unwrap();

        // Verify it's in the mounted fs
        let content = mounted
            .read_file(Path::new("/subdir/test.txt"))
            .await
            .unwrap();
        assert_eq!(content, b"hello");
    }

    #[tokio::test]
    async fn test_nested_mounts() {
        let root = Arc::new(InMemoryFs::new());
        let outer = Arc::new(InMemoryFs::new());
        let inner = Arc::new(InMemoryFs::new());

        outer
            .write_file(Path::new("/outer.txt"), b"outer")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/inner.txt"), b"inner")
            .await
            .unwrap();

        let mfs = MountableFs::new(root);
        mfs.mount("/mnt", outer).unwrap();
        mfs.mount("/mnt/nested", inner).unwrap();

        // Access outer mount
        let content = mfs.read_file(Path::new("/mnt/outer.txt")).await.unwrap();
        assert_eq!(content, b"outer");

        // Access nested mount
        let content = mfs
            .read_file(Path::new("/mnt/nested/inner.txt"))
            .await
            .unwrap();
        assert_eq!(content, b"inner");
    }

    #[tokio::test]
    async fn test_root_fallback() {
        let root = Arc::new(InMemoryFs::new());
        root.write_file(Path::new("/root.txt"), b"root data")
            .await
            .unwrap();

        let mfs = MountableFs::new(root);

        // Should access root fs
        let content = mfs.read_file(Path::new("/root.txt")).await.unwrap();
        assert_eq!(content, b"root data");
    }

    #[tokio::test]
    async fn test_mount_point_in_readdir() {
        let root = Arc::new(InMemoryFs::new());
        let mounted = Arc::new(InMemoryFs::new());

        let mfs = MountableFs::new(root);
        mfs.mount("/mnt", mounted).unwrap();

        // Read root directory should show mnt
        let entries = mfs.read_dir(Path::new("/")).await.unwrap();
        let names: Vec<_> = entries.iter().map(|e| &e.name).collect();
        assert!(names.contains(&&"mnt".to_string()));
    }

    #[tokio::test]
    async fn test_unmount() {
        let root = Arc::new(InMemoryFs::new());
        let mounted = Arc::new(InMemoryFs::new());
        mounted
            .write_file(Path::new("/data.txt"), b"data")
            .await
            .unwrap();

        let mfs = MountableFs::new(root);
        mfs.mount("/mnt", mounted).unwrap();

        // Should exist
        assert!(mfs.exists(Path::new("/mnt/data.txt")).await.unwrap());

        // Unmount
        mfs.unmount("/mnt").unwrap();

        // Should no longer exist (falls back to root which doesn't have it)
        assert!(!mfs.exists(Path::new("/mnt/data.txt")).await.unwrap());
    }
}
