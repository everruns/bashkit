//! Filesystem resource limits for sandboxed execution.
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
//! - **TM-DOS-014**: Many directory entries → `max_file_count`

use std::fmt;

/// Default maximum total filesystem size: 100MB
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 100_000_000;

/// Default maximum single file size: 10MB
pub const DEFAULT_MAX_FILE_SIZE: u64 = 10_000_000;

/// Default maximum file count: 10,000
pub const DEFAULT_MAX_FILE_COUNT: u64 = 10_000;

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
}

impl Default for FsLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_file_count: DEFAULT_MAX_FILE_COUNT,
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

    #[test]
    fn test_default_limits() {
        let limits = FsLimits::default();
        assert_eq!(limits.max_total_bytes, 100_000_000);
        assert_eq!(limits.max_file_size, 10_000_000);
        assert_eq!(limits.max_file_count, 10_000);
    }

    #[test]
    fn test_unlimited() {
        let limits = FsLimits::unlimited();
        assert_eq!(limits.max_total_bytes, u64::MAX);
        assert_eq!(limits.max_file_size, u64::MAX);
        assert_eq!(limits.max_file_count, u64::MAX);
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
}
