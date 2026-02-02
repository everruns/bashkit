//! Overlay filesystem implementation.
//!
//! [`OverlayFs`] provides copy-on-write semantics by layering a writable upper
//! filesystem on top of a read-only lower (base) filesystem.

// RwLock.read()/write().unwrap() only panics on lock poisoning (prior panic
// while holding lock). This is intentional - corrupted state should not propagate.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::collections::HashSet;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use super::memory::InMemoryFs;
use super::traits::{DirEntry, FileSystem, FileType, Metadata};
use crate::error::Result;

/// Copy-on-write overlay filesystem.
///
/// `OverlayFs` layers a writable upper filesystem on top of a read-only base
/// (lower) filesystem, similar to Docker's overlay storage driver or Linux
/// overlayfs.
///
/// # Behavior
///
/// - **Reads**: Check upper layer first, fall back to lower layer
/// - **Writes**: Always go to the upper layer (copy-on-write)
/// - **Deletes**: Tracked via whiteouts - deleted files are hidden but the lower layer is unchanged
///
/// # Use Cases
///
/// - **Template systems**: Start from a read-only template, allow modifications
/// - **Immutable infrastructure**: Keep base images unchanged while allowing runtime modifications
/// - **Testing**: Run tests against a base state without modifying it
/// - **Undo support**: Discard the upper layer to "reset" to the base state
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, FileSystem, InMemoryFs, OverlayFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// // Create a base filesystem with template files
/// let base = Arc::new(InMemoryFs::new());
/// base.mkdir(Path::new("/config"), false).await?;
/// base.write_file(Path::new("/config/app.conf"), b"debug=false").await?;
///
/// // Create overlay - base is read-only, changes go to overlay
/// let overlay = Arc::new(OverlayFs::new(base.clone()));
///
/// // Use with Bash
/// let mut bash = Bash::builder().fs(overlay.clone()).build();
///
/// // Read from base layer
/// let result = bash.exec("cat /config/app.conf").await?;
/// assert_eq!(result.stdout, "debug=false");
///
/// // Modify - changes go to overlay only
/// bash.exec("echo 'debug=true' > /config/app.conf").await?;
///
/// // Overlay shows modified content
/// let result = bash.exec("cat /config/app.conf").await?;
/// assert_eq!(result.stdout, "debug=true\n");
///
/// // Base is unchanged!
/// let original = base.read_file(Path::new("/config/app.conf")).await?;
/// assert_eq!(original, b"debug=false");
/// # Ok(())
/// # }
/// ```
///
/// # Whiteouts (Deletion Handling)
///
/// When you delete a file that exists in the base layer, `OverlayFs` creates
/// a "whiteout" marker that hides the file without modifying the base:
///
/// ```rust
/// use bashkit::{Bash, FileSystem, InMemoryFs, OverlayFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let base = Arc::new(InMemoryFs::new());
/// base.write_file(Path::new("/tmp/secret.txt"), b"sensitive").await?;
///
/// let overlay = Arc::new(OverlayFs::new(base.clone()));
/// let mut bash = Bash::builder().fs(overlay.clone()).build();
///
/// // File exists initially
/// assert!(overlay.exists(Path::new("/tmp/secret.txt")).await?);
///
/// // Delete it
/// bash.exec("rm /tmp/secret.txt").await?;
///
/// // Gone from overlay's view
/// assert!(!overlay.exists(Path::new("/tmp/secret.txt")).await?);
///
/// // But base is unchanged
/// assert!(base.exists(Path::new("/tmp/secret.txt")).await?);
/// # Ok(())
/// # }
/// ```
///
/// # Directory Listing
///
/// When listing directories, entries from both layers are merged, with the
/// upper layer taking precedence for files that exist in both:
///
/// ```rust
/// use bashkit::{FileSystem, InMemoryFs, OverlayFs};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let base = Arc::new(InMemoryFs::new());
/// base.write_file(Path::new("/tmp/base.txt"), b"from base").await?;
///
/// let overlay = OverlayFs::new(base);
/// overlay.write_file(Path::new("/tmp/upper.txt"), b"from upper").await?;
///
/// // Both files visible
/// let entries = overlay.read_dir(Path::new("/tmp")).await?;
/// let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
/// assert!(names.contains(&"base.txt"));
/// assert!(names.contains(&"upper.txt"));
/// # Ok(())
/// # }
/// ```
pub struct OverlayFs {
    /// Lower (read-only base) filesystem
    lower: Arc<dyn FileSystem>,
    /// Upper (writable) filesystem - always InMemoryFs
    upper: InMemoryFs,
    /// Paths that have been deleted (whiteouts)
    whiteouts: RwLock<HashSet<PathBuf>>,
}

impl OverlayFs {
    /// Create a new overlay filesystem with the given base layer.
    ///
    /// The `lower` filesystem is treated as read-only - all reads will first
    /// check the upper layer, then fall back to the lower layer. All writes
    /// go to a new [`InMemoryFs`] upper layer.
    ///
    /// # Arguments
    ///
    /// * `lower` - The base (read-only) filesystem
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{FileSystem, InMemoryFs, OverlayFs};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// // Create base with some files
    /// let base = Arc::new(InMemoryFs::new());
    /// base.mkdir(Path::new("/data"), false).await?;
    /// base.write_file(Path::new("/data/readme.txt"), b"Read me!").await?;
    ///
    /// // Create overlay
    /// let overlay = OverlayFs::new(base);
    ///
    /// // Can read from base
    /// let content = overlay.read_file(Path::new("/data/readme.txt")).await?;
    /// assert_eq!(content, b"Read me!");
    ///
    /// // Writes go to upper layer
    /// overlay.write_file(Path::new("/data/new.txt"), b"New file").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(lower: Arc<dyn FileSystem>) -> Self {
        Self {
            lower,
            upper: InMemoryFs::new(),
            whiteouts: RwLock::new(HashSet::new()),
        }
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

    /// Check if a path has been deleted (whiteout)
    fn is_whiteout(&self, path: &Path) -> bool {
        let path = Self::normalize_path(path);
        let whiteouts = self.whiteouts.read().unwrap();
        whiteouts.contains(&path)
    }

    /// Mark a path as deleted (add whiteout)
    fn add_whiteout(&self, path: &Path) {
        let path = Self::normalize_path(path);
        let mut whiteouts = self.whiteouts.write().unwrap();
        whiteouts.insert(path);
    }

    /// Remove a whiteout (for when re-creating a deleted file)
    fn remove_whiteout(&self, path: &Path) {
        let path = Self::normalize_path(path);
        let mut whiteouts = self.whiteouts.write().unwrap();
        whiteouts.remove(&path);
    }
}

#[async_trait]
impl FileSystem for OverlayFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path = Self::normalize_path(path);

        // Check for whiteout (deleted file)
        if self.is_whiteout(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "file not found").into());
        }

        // Try upper first
        if self.upper.exists(&path).await.unwrap_or(false) {
            return self.upper.read_file(&path).await;
        }

        // Fall back to lower
        self.lower.read_file(&path).await
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize_path(path);

        // Remove any whiteout for this path
        self.remove_whiteout(&path);

        // Ensure parent directory exists in upper
        if let Some(parent) = path.parent() {
            if !self.upper.exists(parent).await.unwrap_or(false) {
                // Copy parent directory structure from lower if it exists
                if self.lower.exists(parent).await.unwrap_or(false) {
                    self.upper.mkdir(parent, true).await?;
                } else {
                    return Err(
                        IoError::new(ErrorKind::NotFound, "parent directory not found").into(),
                    );
                }
            }
        }

        // Write to upper
        self.upper.write_file(&path, content).await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize_path(path);

        // Check for whiteout
        if self.is_whiteout(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "file not found").into());
        }

        // If file exists in upper, append there
        if self.upper.exists(&path).await.unwrap_or(false) {
            return self.upper.append_file(&path, content).await;
        }

        // If file exists in lower, copy-on-write
        if self.lower.exists(&path).await.unwrap_or(false) {
            let existing = self.lower.read_file(&path).await?;

            // Ensure parent exists in upper
            if let Some(parent) = path.parent() {
                if !self.upper.exists(parent).await.unwrap_or(false) {
                    self.upper.mkdir(parent, true).await?;
                }
            }

            // Copy existing content and append new content
            let mut combined = existing;
            combined.extend_from_slice(content);
            return self.upper.write_file(&path, &combined).await;
        }

        // Create new file in upper
        self.upper.write_file(&path, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);

        // Remove any whiteout for this path
        self.remove_whiteout(&path);

        // Create in upper
        self.upper.mkdir(&path, recursive).await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);

        // Check if exists in either layer
        let in_upper = self.upper.exists(&path).await.unwrap_or(false);
        let in_lower = !self.is_whiteout(&path) && self.lower.exists(&path).await.unwrap_or(false);

        if !in_upper && !in_lower {
            return Err(IoError::new(ErrorKind::NotFound, "not found").into());
        }

        // Remove from upper if present
        if in_upper {
            self.upper.remove(&path, recursive).await?;
        }

        // If was in lower, add whiteout
        if in_lower {
            if recursive {
                // Add whiteouts for all paths under this directory
                // This is a simplification - real overlayfs uses opaque dirs
                self.add_whiteout(&path);
            } else {
                self.add_whiteout(&path);
            }
        }

        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = Self::normalize_path(path);

        // Check for whiteout
        if self.is_whiteout(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "not found").into());
        }

        // Try upper first
        if self.upper.exists(&path).await.unwrap_or(false) {
            return self.upper.stat(&path).await;
        }

        // Fall back to lower
        self.lower.stat(&path).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = Self::normalize_path(path);

        // Check for whiteout
        if self.is_whiteout(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "not found").into());
        }

        let mut entries: std::collections::HashMap<String, DirEntry> =
            std::collections::HashMap::new();

        // Get entries from lower (if not whited out)
        if self.lower.exists(&path).await.unwrap_or(false) {
            if let Ok(lower_entries) = self.lower.read_dir(&path).await {
                for entry in lower_entries {
                    // Skip whited out entries
                    let entry_path = path.join(&entry.name);
                    if !self.is_whiteout(&entry_path) {
                        entries.insert(entry.name.clone(), entry);
                    }
                }
            }
        }

        // Overlay with entries from upper (overriding lower)
        if self.upper.exists(&path).await.unwrap_or(false) {
            if let Ok(upper_entries) = self.upper.read_dir(&path).await {
                for entry in upper_entries {
                    entries.insert(entry.name.clone(), entry);
                }
            }
        }

        Ok(entries.into_values().collect())
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = Self::normalize_path(path);

        // Check for whiteout
        if self.is_whiteout(&path) {
            return Ok(false);
        }

        // Check upper first
        if self.upper.exists(&path).await.unwrap_or(false) {
            return Ok(true);
        }

        // Check lower
        self.lower.exists(&path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);

        // Read from source (checking both layers)
        let content = self.read_file(&from).await?;

        // Write to destination in upper
        self.write_file(&to, &content).await?;

        // Delete source (will add whiteout if needed)
        self.remove(&from, false).await?;

        Ok(())
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);

        // Read from source (checking both layers)
        let content = self.read_file(&from).await?;

        // Write to destination in upper
        self.write_file(&to, &content).await
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        let link = Self::normalize_path(link);

        // Remove any whiteout
        self.remove_whiteout(&link);

        // Create symlink in upper
        self.upper.symlink(target, &link).await
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = Self::normalize_path(path);

        // Check for whiteout
        if self.is_whiteout(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "not found").into());
        }

        // Try upper first
        if self.upper.exists(&path).await.unwrap_or(false) {
            return self.upper.read_link(&path).await;
        }

        // Fall back to lower
        self.lower.read_link(&path).await
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        let path = Self::normalize_path(path);

        // Check for whiteout
        if self.is_whiteout(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "not found").into());
        }

        // If exists in upper, chmod there
        if self.upper.exists(&path).await.unwrap_or(false) {
            return self.upper.chmod(&path, mode).await;
        }

        // If exists in lower, copy-on-write metadata
        if self.lower.exists(&path).await.unwrap_or(false) {
            let stat = self.lower.stat(&path).await?;

            // Create in upper with same content (for files)
            if stat.file_type == FileType::File {
                let content = self.lower.read_file(&path).await?;
                self.upper.write_file(&path, &content).await?;
            } else if stat.file_type == FileType::Directory {
                self.upper.mkdir(&path, true).await?;
            }

            return self.upper.chmod(&path, mode).await;
        }

        Err(IoError::new(ErrorKind::NotFound, "not found").into())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_from_lower() {
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/test.txt"), b"hello")
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let content = overlay.read_file(Path::new("/tmp/test.txt")).await.unwrap();
        assert_eq!(content, b"hello");
    }

    #[tokio::test]
    async fn test_write_to_upper() {
        let lower = Arc::new(InMemoryFs::new());
        let overlay = OverlayFs::new(lower.clone());

        overlay
            .write_file(Path::new("/tmp/new.txt"), b"new file")
            .await
            .unwrap();

        // Should be readable from overlay
        let content = overlay.read_file(Path::new("/tmp/new.txt")).await.unwrap();
        assert_eq!(content, b"new file");

        // Should NOT be in lower
        assert!(!lower.exists(Path::new("/tmp/new.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_copy_on_write() {
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/test.txt"), b"original")
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower.clone());

        // Modify through overlay
        overlay
            .write_file(Path::new("/tmp/test.txt"), b"modified")
            .await
            .unwrap();

        // Overlay should show modified
        let content = overlay.read_file(Path::new("/tmp/test.txt")).await.unwrap();
        assert_eq!(content, b"modified");

        // Lower should still have original
        let lower_content = lower.read_file(Path::new("/tmp/test.txt")).await.unwrap();
        assert_eq!(lower_content, b"original");
    }

    #[tokio::test]
    async fn test_delete_with_whiteout() {
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/test.txt"), b"hello")
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower.clone());

        // Delete through overlay
        overlay
            .remove(Path::new("/tmp/test.txt"), false)
            .await
            .unwrap();

        // Should not be visible through overlay
        assert!(!overlay.exists(Path::new("/tmp/test.txt")).await.unwrap());

        // But should still exist in lower
        assert!(lower.exists(Path::new("/tmp/test.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_recreate_after_delete() {
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/test.txt"), b"original")
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);

        // Delete
        overlay
            .remove(Path::new("/tmp/test.txt"), false)
            .await
            .unwrap();
        assert!(!overlay.exists(Path::new("/tmp/test.txt")).await.unwrap());

        // Recreate
        overlay
            .write_file(Path::new("/tmp/test.txt"), b"new content")
            .await
            .unwrap();

        // Should now exist with new content
        assert!(overlay.exists(Path::new("/tmp/test.txt")).await.unwrap());
        let content = overlay.read_file(Path::new("/tmp/test.txt")).await.unwrap();
        assert_eq!(content, b"new content");
    }

    #[tokio::test]
    async fn test_read_dir_merged() {
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/lower.txt"), b"lower")
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        overlay
            .write_file(Path::new("/tmp/upper.txt"), b"upper")
            .await
            .unwrap();

        let entries = overlay.read_dir(Path::new("/tmp")).await.unwrap();
        let names: Vec<_> = entries.iter().map(|e| &e.name).collect();

        assert!(names.contains(&&"lower.txt".to_string()));
        assert!(names.contains(&&"upper.txt".to_string()));
    }
}
