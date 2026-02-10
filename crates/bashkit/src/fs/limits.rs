//! Filesystem resource limits for virtual execution.
//!
//! These limits prevent scripts from exhausting memory via filesystem operations.
//!
//! # Security Mitigations
//!
//! This module mitigates the following threats (see `specs/006-threat-model.md`):
//!
//! - **TM-DOS-005**: Large file creation → `max_file_size`
//! - **TM-DOS-006**: Many small files → `max_file_count`
//! - **TM-DOS-007**: Zip bomb decompression → limits checked during extraction
//! - **TM-DOS-008**: Tar bomb extraction → `max_total_bytes`, `max_file_count`
//! - **TM-DOS-009**: Recursive copy → `max_total_bytes`
//! - **TM-DOS-010**: Append flood → `max_total_bytes`, `max_file_size`
//! - **TM-DOS-012**: Deep directory nesting → `max_path_depth`
//! - **TM-DOS-013**: Long filenames → `max_filename_length`, `max_path_length`
//! - **TM-DOS-014**: Many directory entries → `max_file_count`
//! - **TM-DOS-015**: Unicode path attacks → `validate_path()` control char rejection

use std::fmt;
use std::path::Path;

/// Default maximum total filesystem size: 100MB
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 100_000_000;

/// Default maximum single file size: 10MB
pub const DEFAULT_MAX_FILE_SIZE: u64 = 10_000_000;

/// Default maximum file count: 10,000
pub const DEFAULT_MAX_FILE_COUNT: u64 = 10_000;

/// Default maximum path depth (directory nesting): 100
pub const DEFAULT_MAX_PATH_DEPTH: usize = 100;

/// Default maximum filename (single component) length: 255 bytes
pub const DEFAULT_MAX_FILENAME_LENGTH: usize = 255;

/// Default maximum total path length: 4096 bytes
pub const DEFAULT_MAX_PATH_LENGTH: usize = 4096;

/// Filesystem resource limits.
///
/// Controls maximum resource consumption for in-memory filesystems.
/// Applied to both [`InMemoryFs`](crate::InMemoryFs) and [`OverlayFs`](crate::OverlayFs).
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, FsLimits, InMemoryFs};
/// use std::sync::Arc;
///
/// // Create filesystem with custom limits
/// let limits = FsLimits::new()
///     .max_total_bytes(50_000_000)  // 50MB total
///     .max_file_size(5_000_000)     // 5MB per file
///     .max_file_count(1000);        // 1000 files max
///
/// let fs = Arc::new(InMemoryFs::with_limits(limits));
/// let bash = Bash::builder().fs(fs).build();
/// ```
///
/// # Default Limits
///
/// | Limit | Default | Purpose |
/// |-------|---------|---------|
/// | `max_total_bytes` | 100MB | Total filesystem memory |
/// | `max_file_size` | 10MB | Single file size |
/// | `max_file_count` | 10,000 | Number of files |
/// | `max_path_depth` | 100 | Directory nesting depth |
/// | `max_filename_length` | 255 | Single path component |
/// | `max_path_length` | 4096 | Total path length |
#[derive(Debug, Clone)]
pub struct FsLimits {
    /// Maximum total bytes across all files.
    /// Default: 100MB (100,000,000 bytes)
    pub max_total_bytes: u64,

    /// Maximum size of a single file in bytes.
    /// Default: 10MB (10,000,000 bytes)
    pub max_file_size: u64,

    /// Maximum number of files (not including directories).
    /// Default: 10,000
    pub max_file_count: u64,

    // THREAT[TM-DOS-012]: Deep directory nesting can cause stack/memory exhaustion
    // Mitigation: Limit maximum path component count
    /// Maximum directory nesting depth.
    /// Default: 100
    pub max_path_depth: usize,

    // THREAT[TM-DOS-013]: Long filenames can cause memory exhaustion
    // Mitigation: Limit filename and total path length
    /// Maximum length of a single filename (path component) in bytes.
    /// Default: 255 bytes
    pub max_filename_length: usize,

    /// Maximum total path length in bytes.
    /// Default: 4096 bytes
    pub max_path_length: usize,
}

impl Default for FsLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_file_count: DEFAULT_MAX_FILE_COUNT,
            max_path_depth: DEFAULT_MAX_PATH_DEPTH,
            max_filename_length: DEFAULT_MAX_FILENAME_LENGTH,
            max_path_length: DEFAULT_MAX_PATH_LENGTH,
        }
    }
}

impl FsLimits {
    /// Create new limits with defaults.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FsLimits;
    ///
    /// let limits = FsLimits::new();
    /// assert_eq!(limits.max_total_bytes, 100_000_000);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Create unlimited limits (no restrictions).
    ///
    /// # Warning
    ///
    /// Using unlimited limits removes protection against memory exhaustion.
    /// Only use in trusted environments.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FsLimits;
    ///
    /// let limits = FsLimits::unlimited();
    /// assert_eq!(limits.max_total_bytes, u64::MAX);
    /// ```
    pub fn unlimited() -> Self {
        Self {
            max_total_bytes: u64::MAX,
            max_file_size: u64::MAX,
            max_file_count: u64::MAX,
            max_path_depth: usize::MAX,
            max_filename_length: usize::MAX,
            max_path_length: usize::MAX,
        }
    }

    /// Set maximum total filesystem size.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FsLimits;
    ///
    /// let limits = FsLimits::new().max_total_bytes(50_000_000); // 50MB
    /// ```
    pub fn max_total_bytes(mut self, bytes: u64) -> Self {
        self.max_total_bytes = bytes;
        self
    }

    /// Set maximum single file size.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FsLimits;
    ///
    /// let limits = FsLimits::new().max_file_size(1_000_000); // 1MB
    /// ```
    pub fn max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = bytes;
        self
    }

    /// Set maximum file count.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::FsLimits;
    ///
    /// let limits = FsLimits::new().max_file_count(100);
    /// ```
    pub fn max_file_count(mut self, count: u64) -> Self {
        self.max_file_count = count;
        self
    }

    /// Set maximum path depth (directory nesting).
    pub fn max_path_depth(mut self, depth: usize) -> Self {
        self.max_path_depth = depth;
        self
    }

    /// Set maximum filename (single component) length.
    pub fn max_filename_length(mut self, len: usize) -> Self {
        self.max_filename_length = len;
        self
    }

    /// Set maximum total path length.
    pub fn max_path_length(mut self, len: usize) -> Self {
        self.max_path_length = len;
        self
    }

    // THREAT[TM-DOS-012]: Deep directory nesting can exhaust stack/memory
    // THREAT[TM-DOS-013]: Long filenames/paths can exhaust memory
    // THREAT[TM-DOS-015]: Unicode control chars can cause path confusion
    // Mitigation: Validate all three properties before accepting a path
    /// Validate a path against depth, length, and character safety limits.
    ///
    /// Returns `Err(FsLimitExceeded)` if the path violates any limit.
    pub fn validate_path(&self, path: &Path) -> Result<(), FsLimitExceeded> {
        let path_str = path.to_string_lossy();
        let path_len = path_str.len();

        // TM-DOS-013: Check total path length
        if path_len > self.max_path_length {
            return Err(FsLimitExceeded::PathTooLong {
                length: path_len,
                limit: self.max_path_length,
            });
        }

        let mut depth: usize = 0;
        for component in path.components() {
            match component {
                std::path::Component::Normal(name) => {
                    let name_str = name.to_string_lossy();

                    // TM-DOS-013: Check individual filename length
                    if name_str.len() > self.max_filename_length {
                        return Err(FsLimitExceeded::FilenameTooLong {
                            length: name_str.len(),
                            limit: self.max_filename_length,
                        });
                    }

                    // TM-DOS-015: Reject control characters and bidi overrides
                    if let Some(bad_char) = find_unsafe_path_char(&name_str) {
                        return Err(FsLimitExceeded::UnsafePathChar {
                            character: bad_char,
                            component: name_str.to_string(),
                        });
                    }

                    depth += 1;
                }
                std::path::Component::ParentDir => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        // TM-DOS-012: Check path depth
        if depth > self.max_path_depth {
            return Err(FsLimitExceeded::PathTooDeep {
                depth,
                limit: self.max_path_depth,
            });
        }

        Ok(())
    }

    /// Check if adding bytes would exceed total limit.
    ///
    /// Returns `Ok(())` if within limits, `Err(FsLimitExceeded)` otherwise.
    pub fn check_total_bytes(&self, current: u64, additional: u64) -> Result<(), FsLimitExceeded> {
        let new_total = current.saturating_add(additional);
        if new_total > self.max_total_bytes {
            return Err(FsLimitExceeded::TotalBytes {
                current,
                additional,
                limit: self.max_total_bytes,
            });
        }
        Ok(())
    }

    /// Check if a file size exceeds the limit.
    pub fn check_file_size(&self, size: u64) -> Result<(), FsLimitExceeded> {
        if size > self.max_file_size {
            return Err(FsLimitExceeded::FileSize {
                size,
                limit: self.max_file_size,
            });
        }
        Ok(())
    }

    /// Check if adding a file would exceed the count limit.
    pub fn check_file_count(&self, current: u64) -> Result<(), FsLimitExceeded> {
        if current >= self.max_file_count {
            return Err(FsLimitExceeded::FileCount {
                current,
                limit: self.max_file_count,
            });
        }
        Ok(())
    }
}

// THREAT[TM-DOS-015]: Unicode control chars and bidi overrides can cause path confusion
// Mitigation: Reject paths containing these characters
/// Check if a path component contains unsafe characters.
///
/// Returns `Some(description)` for the first unsafe character found.
/// Rejects: ASCII control chars (0x00-0x1F, 0x7F), C1 controls (0x80-0x9F),
/// and Unicode bidi override characters (U+202A-U+202E, U+2066-U+2069).
fn find_unsafe_path_char(name: &str) -> Option<String> {
    for ch in name.chars() {
        // ASCII control characters (except we allow nothing - null is already
        // impossible in Rust strings)
        if ch.is_ascii_control() {
            return Some(format!("U+{:04X}", ch as u32));
        }
        // C1 control characters
        if ('\u{0080}'..='\u{009F}').contains(&ch) {
            return Some(format!("U+{:04X}", ch as u32));
        }
        // Bidi override characters - can cause visual path confusion
        if ('\u{202A}'..='\u{202E}').contains(&ch) || ('\u{2066}'..='\u{2069}').contains(&ch) {
            return Some(format!("U+{:04X} (bidi override)", ch as u32));
        }
    }
    None
}

/// Error returned when a filesystem limit is exceeded.
#[derive(Debug, Clone)]
pub enum FsLimitExceeded {
    /// Total filesystem size would exceed limit.
    TotalBytes {
        current: u64,
        additional: u64,
        limit: u64,
    },
    /// Single file size exceeds limit.
    FileSize { size: u64, limit: u64 },
    /// File count would exceed limit.
    FileCount { current: u64, limit: u64 },
    /// Path depth (nesting) exceeds limit (TM-DOS-012).
    PathTooDeep { depth: usize, limit: usize },
    /// Single filename component exceeds length limit (TM-DOS-013).
    FilenameTooLong { length: usize, limit: usize },
    /// Total path exceeds length limit (TM-DOS-013).
    PathTooLong { length: usize, limit: usize },
    /// Path contains unsafe character (TM-DOS-015).
    UnsafePathChar {
        character: String,
        component: String,
    },
}

impl fmt::Display for FsLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsLimitExceeded::TotalBytes {
                current,
                additional,
                limit,
            } => {
                write!(
                    f,
                    "filesystem full: {} + {} bytes exceeds {} byte limit",
                    current, additional, limit
                )
            }
            FsLimitExceeded::FileSize { size, limit } => {
                write!(
                    f,
                    "file too large: {} bytes exceeds {} byte limit",
                    size, limit
                )
            }
            FsLimitExceeded::FileCount { current, limit } => {
                write!(
                    f,
                    "too many files: {} files at {} file limit",
                    current, limit
                )
            }
            FsLimitExceeded::PathTooDeep { depth, limit } => {
                write!(
                    f,
                    "path too deep: {} levels exceeds {} level limit",
                    depth, limit
                )
            }
            FsLimitExceeded::FilenameTooLong { length, limit } => {
                write!(
                    f,
                    "filename too long: {} bytes exceeds {} byte limit",
                    length, limit
                )
            }
            FsLimitExceeded::PathTooLong { length, limit } => {
                write!(
                    f,
                    "path too long: {} bytes exceeds {} byte limit",
                    length, limit
                )
            }
            FsLimitExceeded::UnsafePathChar {
                character,
                component,
            } => {
                write!(
                    f,
                    "unsafe character {} in path component '{}'",
                    character, component
                )
            }
        }
    }
}

impl std::error::Error for FsLimitExceeded {}

/// Current filesystem usage statistics.
///
/// Returned by [`FileSystem::usage()`](crate::FileSystem::usage).
#[derive(Debug, Clone, Default)]
pub struct FsUsage {
    /// Total bytes used by all files.
    pub total_bytes: u64,
    /// Number of files (not including directories).
    pub file_count: u64,
    /// Number of directories.
    pub dir_count: u64,
}

impl FsUsage {
    /// Create new usage stats.
    pub fn new(total_bytes: u64, file_count: u64, dir_count: u64) -> Self {
        Self {
            total_bytes,
            file_count,
            dir_count,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_default_limits() {
        let limits = FsLimits::default();
        assert_eq!(limits.max_total_bytes, 100_000_000);
        assert_eq!(limits.max_file_size, 10_000_000);
        assert_eq!(limits.max_file_count, 10_000);
        assert_eq!(limits.max_path_depth, 100);
        assert_eq!(limits.max_filename_length, 255);
        assert_eq!(limits.max_path_length, 4096);
    }

    #[test]
    fn test_unlimited() {
        let limits = FsLimits::unlimited();
        assert_eq!(limits.max_total_bytes, u64::MAX);
        assert_eq!(limits.max_file_size, u64::MAX);
        assert_eq!(limits.max_file_count, u64::MAX);
        assert_eq!(limits.max_path_depth, usize::MAX);
        assert_eq!(limits.max_filename_length, usize::MAX);
        assert_eq!(limits.max_path_length, usize::MAX);
    }

    #[test]
    fn test_builder() {
        let limits = FsLimits::new()
            .max_total_bytes(50_000_000)
            .max_file_size(1_000_000)
            .max_file_count(100);

        assert_eq!(limits.max_total_bytes, 50_000_000);
        assert_eq!(limits.max_file_size, 1_000_000);
        assert_eq!(limits.max_file_count, 100);
    }

    #[test]
    fn test_check_total_bytes() {
        let limits = FsLimits::new().max_total_bytes(1000);

        assert!(limits.check_total_bytes(500, 400).is_ok());
        assert!(limits.check_total_bytes(500, 500).is_ok());
        assert!(limits.check_total_bytes(500, 501).is_err());
        assert!(limits.check_total_bytes(1000, 1).is_err());
    }

    #[test]
    fn test_check_file_size() {
        let limits = FsLimits::new().max_file_size(1000);

        assert!(limits.check_file_size(999).is_ok());
        assert!(limits.check_file_size(1000).is_ok());
        assert!(limits.check_file_size(1001).is_err());
    }

    #[test]
    fn test_check_file_count() {
        let limits = FsLimits::new().max_file_count(10);

        assert!(limits.check_file_count(9).is_ok());
        assert!(limits.check_file_count(10).is_err());
        assert!(limits.check_file_count(11).is_err());
    }

    #[test]
    fn test_error_display() {
        let err = FsLimitExceeded::TotalBytes {
            current: 90,
            additional: 20,
            limit: 100,
        };
        assert!(err.to_string().contains("90"));
        assert!(err.to_string().contains("20"));
        assert!(err.to_string().contains("100"));

        let err = FsLimitExceeded::FileSize {
            size: 200,
            limit: 100,
        };
        assert!(err.to_string().contains("200"));
        assert!(err.to_string().contains("100"));

        let err = FsLimitExceeded::FileCount {
            current: 10,
            limit: 10,
        };
        assert!(err.to_string().contains("10"));
    }

    // TM-DOS-012: Path depth validation
    #[test]
    fn test_validate_path_depth_ok() {
        let limits = FsLimits::new().max_path_depth(3);
        assert!(limits.validate_path(Path::new("/a/b/c")).is_ok());
    }

    #[test]
    fn test_validate_path_depth_exceeded() {
        let limits = FsLimits::new().max_path_depth(3);
        assert!(limits.validate_path(Path::new("/a/b/c/d")).is_err());
        let err = limits.validate_path(Path::new("/a/b/c/d")).unwrap_err();
        assert!(err.to_string().contains("path too deep"));
    }

    #[test]
    fn test_validate_path_depth_with_parent_refs() {
        let limits = FsLimits::new().max_path_depth(3);
        // /a/b/../c/d = /a/c/d which is depth 3 — OK
        assert!(limits.validate_path(Path::new("/a/b/../c/d")).is_ok());
    }

    // TM-DOS-013: Filename length validation
    #[test]
    fn test_validate_filename_length_ok() {
        let limits = FsLimits::new().max_filename_length(10);
        assert!(limits.validate_path(Path::new("/tmp/short.txt")).is_ok());
    }

    #[test]
    fn test_validate_filename_length_exceeded() {
        let limits = FsLimits::new().max_filename_length(10);
        let long_name = "a".repeat(11);
        let path = PathBuf::from(format!("/tmp/{}", long_name));
        assert!(limits.validate_path(&path).is_err());
        let err = limits.validate_path(&path).unwrap_err();
        assert!(err.to_string().contains("filename too long"));
    }

    // TM-DOS-013: Total path length validation
    #[test]
    fn test_validate_path_length_exceeded() {
        let limits = FsLimits::new().max_path_length(20);
        let path = PathBuf::from("/this/is/a/very/long/path/that/exceeds");
        assert!(limits.validate_path(&path).is_err());
        let err = limits.validate_path(&path).unwrap_err();
        assert!(err.to_string().contains("path too long"));
    }

    // TM-DOS-015: Unicode path safety
    #[test]
    fn test_validate_path_control_char_rejected() {
        let limits = FsLimits::new();
        let path = PathBuf::from("/tmp/file\x01name");
        assert!(limits.validate_path(&path).is_err());
        let err = limits.validate_path(&path).unwrap_err();
        assert!(err.to_string().contains("unsafe character"));
    }

    #[test]
    fn test_validate_path_bidi_override_rejected() {
        let limits = FsLimits::new();
        let path = PathBuf::from("/tmp/file\u{202E}name");
        assert!(limits.validate_path(&path).is_err());
        let err = limits.validate_path(&path).unwrap_err();
        assert!(err.to_string().contains("bidi override"));
    }

    #[test]
    fn test_validate_path_normal_unicode_ok() {
        let limits = FsLimits::new();
        // Normal unicode (accented chars, CJK, emoji) should be fine
        assert!(limits.validate_path(Path::new("/tmp/café")).is_ok());
        assert!(limits.validate_path(Path::new("/tmp/文件")).is_ok());
    }
}
