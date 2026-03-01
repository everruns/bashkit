//! Overlay filesystem implementation.
//!
//! [`OverlayFs`] provides copy-on-write semantics by layering a writable upper
//! filesystem on top of a read-only lower (base) filesystem.
//!
//! # Resource Limits
//!
//! Limits apply to the combined filesystem view (upper + lower).
//! See [`FsLimits`](crate::FsLimits) for configuration.

// RwLock.read()/write().unwrap() only panics on lock poisoning (prior panic
// while holding lock). This is intentional - corrupted state should not propagate.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::collections::HashSet;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use super::limits::{FsLimits, FsUsage};
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
    /// Combined limits for the overlay view
    limits: FsLimits,
    // Tracks lower-layer usage that is hidden by upper overrides or whiteouts.
    // Updated incrementally in async methods so compute_usage (sync) stays accurate.
    lower_hidden: RwLock<FsUsage>,
}

impl OverlayFs {
    /// Create a new overlay filesystem with the given base layer and default limits.
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
        Self::with_limits(lower, FsLimits::default())
    }

    /// Create a new overlay filesystem with custom limits.
    ///
    /// Limits apply to the combined view (upper layer writes + lower layer content).
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{FileSystem, InMemoryFs, OverlayFs, FsLimits};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let base = Arc::new(InMemoryFs::new());
    /// let limits = FsLimits::new().max_total_bytes(10_000_000); // 10MB
    ///
    /// let overlay = OverlayFs::with_limits(base, limits);
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_limits(lower: Arc<dyn FileSystem>, limits: FsLimits) -> Self {
        // Upper layer uses unlimited limits - we enforce limits at the OverlayFs level
        Self {
            lower,
            upper: InMemoryFs::with_limits(FsLimits::unlimited()),
            whiteouts: RwLock::new(HashSet::new()),
            limits,
            lower_hidden: RwLock::new(FsUsage::default()),
        }
    }

    /// Access the upper (writable) filesystem layer.
    ///
    /// This provides direct access to the [`InMemoryFs`] that stores all writes.
    /// Useful for pre-populating files during construction.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{InMemoryFs, OverlayFs};
    /// use std::sync::Arc;
    ///
    /// let base = Arc::new(InMemoryFs::new());
    /// let overlay = OverlayFs::new(base);
    ///
    /// // Add files directly to upper layer
    /// overlay.upper().add_file("/config/app.conf", "debug=true\n", 0o644);
    /// ```
    pub fn upper(&self) -> &InMemoryFs {
        &self.upper
    }

    /// Compute combined usage (upper + visible lower).
    ///
    /// Deducts lower-layer entries that are hidden by upper overrides or whiteouts
    /// to avoid double-counting. The `lower_hidden` accumulator is maintained
    /// incrementally by write_file, remove, chmod, and related async methods.
    fn compute_usage(&self) -> FsUsage {
        let upper_usage = self.upper.usage();
        let lower_usage = self.lower.usage();
        let hidden = self.lower_hidden.read().unwrap();

        let total_bytes = upper_usage
            .total_bytes
            .saturating_add(lower_usage.total_bytes)
            .saturating_sub(hidden.total_bytes);
        let file_count = upper_usage
            .file_count
            .saturating_add(lower_usage.file_count)
            .saturating_sub(hidden.file_count);
        let dir_count = upper_usage
            .dir_count
            .saturating_add(lower_usage.dir_count)
            .saturating_sub(hidden.dir_count);

        FsUsage::new(total_bytes, file_count, dir_count)
    }

    /// Check limits before writing.
    fn check_write_limits(&self, content_size: usize) -> Result<()> {
        // Check file size limit
        if content_size as u64 > self.limits.max_file_size {
            return Err(IoError::other(format!(
                "file too large: {} bytes exceeds {} byte limit",
                content_size, self.limits.max_file_size
            ))
            .into());
        }

        // Check total size limit (upper layer only, since lower is read-only)
        let usage = self.upper.usage();
        let new_total = usage.total_bytes + content_size as u64;
        if new_total > self.limits.max_total_bytes {
            return Err(IoError::other(format!(
                "filesystem full: {} bytes would exceed {} byte limit",
                new_total, self.limits.max_total_bytes
            ))
            .into());
        }

        // Check file count limit
        if usage.file_count >= self.limits.max_file_count {
            return Err(IoError::other(format!(
                "too many files: {} files at {} file limit",
                usage.file_count, self.limits.max_file_count
            ))
            .into());
        }

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

    /// Record that a lower-layer file is now hidden (overridden or whited out).
    fn hide_lower_file(&self, size: u64) {
        let mut hidden = self.lower_hidden.write().unwrap();
        hidden.total_bytes = hidden.total_bytes.saturating_add(size);
        hidden.file_count = hidden.file_count.saturating_add(1);
    }

    /// Record that a lower-layer directory is now hidden.
    fn hide_lower_dir(&self) {
        let mut hidden = self.lower_hidden.write().unwrap();
        hidden.dir_count = hidden.dir_count.saturating_add(1);
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
        // THREAT[TM-DOS-012, TM-DOS-013, TM-DOS-015]: Validate path before use
        self.limits
            .validate_path(path)
            .map_err(|e| IoError::other(e.to_string()))?;

        let path = Self::normalize_path(path);

        // Check limits before writing
        self.check_write_limits(content.len())?;

        // Track whether lower file becomes newly hidden by this write.
        // If the path is not already in upper AND not already whited out,
        // then writing to upper will newly shadow the lower entry.
        let already_in_upper = self.upper.exists(&path).await.unwrap_or(false);
        let already_whited = self.is_whiteout(&path);
        let lower_exists = self.lower.exists(&path).await.unwrap_or(false);

        // Remove any whiteout for this path (upper override takes over hiding)
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
        self.upper.write_file(&path, content).await?;

        // If this write newly hides a lower file (not previously hidden by
        // upper override or whiteout), record the hidden lower contribution.
        if lower_exists && !already_in_upper && !already_whited {
            if let Ok(meta) = self.lower.stat(&path).await {
                match meta.file_type {
                    FileType::File => self.hide_lower_file(meta.size),
                    FileType::Directory => self.hide_lower_dir(),
                    _ => {}
                }
            }
        }

        Ok(())
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        // THREAT[TM-DOS-012, TM-DOS-013, TM-DOS-015]: Validate path before use
        self.limits
            .validate_path(path)
            .map_err(|e| IoError::other(e.to_string()))?;

        let path = Self::normalize_path(path);

        // Check for whiteout
        if self.is_whiteout(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "file not found").into());
        }

        // If file exists in upper, append there
        if self.upper.exists(&path).await.unwrap_or(false) {
            // Check limits for appended content
            self.check_write_limits(content.len())?;
            return self.upper.append_file(&path, content).await;
        }

        // If file exists in lower, copy-on-write
        if self.lower.exists(&path).await.unwrap_or(false) {
            let lower_meta = self.lower.stat(&path).await?;
            let existing = self.lower.read_file(&path).await?;

            // Check limits for combined content
            self.check_write_limits(existing.len() + content.len())?;

            // Ensure parent exists in upper
            if let Some(parent) = path.parent() {
                if !self.upper.exists(parent).await.unwrap_or(false) {
                    self.upper.mkdir(parent, true).await?;
                }
            }

            // Copy existing content and append new content
            let mut combined = existing;
            combined.extend_from_slice(content);
            self.upper.write_file(&path, &combined).await?;

            // Lower file is now hidden by the upper copy
            self.hide_lower_file(lower_meta.size);
            return Ok(());
        }

        // Create new file in upper
        self.check_write_limits(content.len())?;
        self.upper.write_file(&path, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        // THREAT[TM-DOS-012, TM-DOS-013, TM-DOS-015]: Validate path before use
        self.limits
            .validate_path(path)
            .map_err(|e| IoError::other(e.to_string()))?;

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

        // If was in lower, add whiteout and track hiding.
        // If in_upper was also true, the lower was already hidden (by the upper
        // override). The whiteout replaces the override as the hiding mechanism,
        // so no additional deduction needed.
        if in_lower {
            // Newly hiding the lower entry only if there was no upper override
            if !in_upper {
                if let Ok(meta) = self.lower.stat(&path).await {
                    match meta.file_type {
                        FileType::File => self.hide_lower_file(meta.size),
                        FileType::Directory => self.hide_lower_dir(),
                        _ => {}
                    }
                }
            }
            self.add_whiteout(&path);
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
                self.check_write_limits(content.len())?;

                // Ensure parent dir exists in upper before write
                if let Some(parent) = path.parent() {
                    if !self.upper.exists(parent).await.unwrap_or(false) {
                        self.upper.mkdir(parent, true).await?;
                    }
                }

                self.upper.write_file(&path, &content).await?;
                self.hide_lower_file(stat.size);
            } else if stat.file_type == FileType::Directory {
                self.upper.mkdir(&path, true).await?;
                self.hide_lower_dir();
            }

            return self.upper.chmod(&path, mode).await;
        }

        Err(IoError::new(ErrorKind::NotFound, "not found").into())
    }

    fn usage(&self) -> FsUsage {
        self.compute_usage()
    }

    fn limits(&self) -> FsLimits {
        self.limits.clone()
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
    async fn test_chmod_cow_enforces_write_limits() {
        // Issue #417: chmod copy-on-write must check limits before writing to upper
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/big.txt"), &vec![b'x'; 5000])
            .await
            .unwrap();

        // Limit upper layer to 1000 bytes total - the 5000 byte file shouldn't fit
        let limits = FsLimits::new().max_total_bytes(1000);
        let overlay = OverlayFs::with_limits(lower, limits);

        // chmod triggers CoW from lower -> upper; must be rejected
        let result = overlay.chmod(Path::new("/tmp/big.txt"), 0o755).await;
        assert!(
            result.is_err(),
            "chmod CoW should fail when content exceeds write limits"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("filesystem full"),
            "expected 'filesystem full' error, got: {err}"
        );

        // File should NOT exist in upper layer
        assert!(
            !overlay
                .upper
                .exists(Path::new("/tmp/big.txt"))
                .await
                .unwrap(),
            "file should not have been copied to upper layer"
        );
    }

    #[tokio::test]
    async fn test_usage_no_double_count_override() {
        // Issue #418: overwriting a lower file in upper should not double-count
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/file.txt"), b"lower data") // 10 bytes
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);

        // Snapshot before override
        let usage_before = overlay.usage();

        // Override in upper with smaller content
        overlay
            .write_file(Path::new("/tmp/file.txt"), b"upper!") // 6 bytes
            .await
            .unwrap();

        let usage_after = overlay.usage();
        // File count should not change: same file, just overridden
        assert_eq!(
            usage_after.file_count, usage_before.file_count,
            "overridden file should not increase file_count"
        );
        // Bytes should decrease by (10 - 6) = 4 because lower's 10 bytes are
        // replaced by upper's 6 bytes
        assert_eq!(
            usage_after.total_bytes,
            usage_before.total_bytes - 4,
            "overridden file bytes should reflect upper size, not sum"
        );
    }

    #[tokio::test]
    async fn test_usage_no_double_count_whiteout() {
        // Issue #418: deleting a lower file should deduct it from usage
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/gone.txt"), b"12345") // 5 bytes
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower.clone());
        let usage_before = overlay.usage();

        // Delete through overlay (creates whiteout)
        overlay
            .remove(Path::new("/tmp/gone.txt"), false)
            .await
            .unwrap();

        let usage_after = overlay.usage();
        assert_eq!(
            usage_after.file_count,
            usage_before.file_count - 1,
            "whited-out file should not be counted"
        );
        assert_eq!(
            usage_after.total_bytes,
            usage_before.total_bytes - 5,
            "whited-out file bytes should be deducted"
        );
    }

    #[tokio::test]
    async fn test_usage_unique_files_both_layers() {
        // Files unique to each layer should each count once
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/lower.txt"), b"aaa") // 3 bytes
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let usage_before = overlay.usage();

        overlay
            .write_file(Path::new("/tmp/upper.txt"), b"bbbbb") // 5 bytes
            .await
            .unwrap();

        let usage_after = overlay.usage();
        // Adding a unique upper file: +1 file, +5 bytes
        assert_eq!(
            usage_after.file_count,
            usage_before.file_count + 1,
            "unique upper file adds one to count"
        );
        assert_eq!(
            usage_after.total_bytes,
            usage_before.total_bytes + 5,
            "unique upper file adds its bytes"
        );
    }

    #[tokio::test]
    async fn test_usage_recreate_after_whiteout() {
        // Delete then recreate: file should count once with new size
        let lower = Arc::new(InMemoryFs::new());
        lower
            .write_file(Path::new("/tmp/file.txt"), b"old data 10") // 11 bytes
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let usage_before = overlay.usage();

        // Delete
        overlay
            .remove(Path::new("/tmp/file.txt"), false)
            .await
            .unwrap();

        // Recreate with different size
        overlay
            .write_file(Path::new("/tmp/file.txt"), b"new") // 3 bytes
            .await
            .unwrap();

        let usage_after = overlay.usage();
        // Net effect: replaced 11-byte file with 3-byte file => -8 bytes, same count
        assert_eq!(
            usage_after.file_count, usage_before.file_count,
            "recreated file counted once"
        );
        assert_eq!(
            usage_after.total_bytes,
            usage_before.total_bytes - 8,
            "recreated file uses new size"
        );
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
