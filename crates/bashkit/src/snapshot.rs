// Decision: Snapshot format uses serde_json for Phase 1 (debuggable, human-readable).
// Phase 2 can add bincode/postcard for compactness.
// VFS contents are included by default; SnapshotOptions can opt out for shell-only restores.
// Session counters are serialized for observability; restore paths do not trust them.

//! Snapshot/resume — serialize interpreter state between `exec()` calls.
//!
//! Captures shell state (variables, env, cwd, arrays, aliases, traps) and
//! VFS contents into a serializable [`Snapshot`] that can be persisted to disk,
//! sent over a network, or used to restore a Bash instance later.
//!
//! # Example
//!
//! ```rust
//! use bashkit::Bash;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//! bash.exec("x=42; mkdir /tmp/work").await?;
//!
//! // Snapshot to bytes
//! let snapshot = bash.snapshot()?;
//!
//! // Resume from bytes (possibly in a different process)
//! let mut bash2 = Bash::from_snapshot(&snapshot)?;
//! let result = bash2.exec("echo $x").await?;
//! assert_eq!(result.stdout.trim(), "42");
//! # Ok(())
//! # }
//! ```
//!
//! # What is captured
//!
//! - Shell variables (scalar, indexed arrays, associative arrays)
//! - Environment variables
//! - Current working directory
//! - Last exit code (`$?`)
//! - Shell functions (AST plus original source when available)
//! - Shell aliases
//! - Trap handlers
//! - VFS contents (files, directories, symlinks)
//! - Session-level resource counters (commands used, exec calls)
//!
//! # What is NOT captured
//!
//! - Builtins (immutable configuration, not state)
//! - Active execution stack (snapshot only between `exec()` calls)
//! - Tokio runtime state
//! - File descriptors, pipes, background jobs (ephemeral)
//! - Execution limits configuration (caller should configure on restore)

use sha2::{Digest, Sha256};

use crate::fs::VfsSnapshot;
use crate::interpreter::{ShellState, ShellStateOptions};

/// Schema version for snapshot format compatibility.
const SNAPSHOT_VERSION: u32 = 1;

/// Domain-separation tag for the snapshot integrity digest.
///
/// # Security note (TM-SNAP-001)
///
/// This tag is a **public constant**, not a secret key. The digest
/// (`SHA-256(INTEGRITY_TAG || payload)`) detects **accidental corruption**
/// (bit flips, truncation) but does **NOT** prevent intentional forgery.
/// Anyone with access to the source code can compute a valid digest for
/// arbitrary payloads.
///
/// **Do NOT rely on `from_bytes` as a security boundary** when snapshots are
/// received from untrusted sources (network, shared storage, user upload).
/// For tamper-proof snapshots, wrap the bytes with your own HMAC or
/// authenticated encryption using a secret key, or use [`Snapshot::to_bytes_keyed`]
/// / [`Snapshot::from_bytes_keyed`] which accept a caller-provided key.
const INTEGRITY_TAG: &[u8; 8] = b"BKSNAP01";
/// Length of the SHA-256 digest prepended to snapshot bytes.
const DIGEST_LEN: usize = 32;

/// A serializable snapshot of a Bash interpreter's state.
///
/// Combines shell state (variables, env, cwd, etc.) with VFS contents
/// into a single serializable unit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Shell interpreter state (variables, env, cwd, aliases, traps, etc.).
    pub shell: ShellState,
    /// Virtual filesystem contents. `None` if the filesystem doesn't support snapshots.
    pub vfs: Option<VfsSnapshot>,
    /// Session-level command counter (total commands across all prior exec() calls).
    pub session_commands: u64,
    /// Session-level exec() call counter.
    pub session_exec_calls: u64,
}

/// Controls which interpreter state is captured in snapshot bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SnapshotOptions {
    /// Skip virtual filesystem contents and capture shell state only.
    pub exclude_filesystem: bool,
    /// Skip shell functions and avoid cloning AST-backed function state.
    pub exclude_functions: bool,
}

impl Snapshot {
    /// Serialize this snapshot to integrity-protected bytes.
    ///
    /// Format: `[32-byte SHA-256 digest][JSON payload]`
    /// The digest covers `INTEGRITY_TAG || JSON` to detect tampering.
    pub fn to_bytes(&self) -> crate::Result<Vec<u8>> {
        let json = serde_json::to_vec(self).map_err(|e| crate::Error::Internal(e.to_string()))?;
        let digest = Self::compute_digest(&json);
        let mut out = Vec::with_capacity(DIGEST_LEN + json.len());
        out.extend_from_slice(&digest);
        out.extend_from_slice(&json);
        Ok(out)
    }

    /// Deserialize a snapshot from integrity-protected bytes.
    ///
    /// Verifies the SHA-256 digest before deserializing. Rejects tampered snapshots.
    pub fn from_bytes(data: &[u8]) -> crate::Result<Self> {
        if data.len() < DIGEST_LEN {
            return Err(crate::Error::Internal(
                "snapshot too short: missing integrity digest".to_string(),
            ));
        }
        let (stored_digest, json) = data.split_at(DIGEST_LEN);
        let expected = Self::compute_digest(json);
        if stored_digest != expected.as_slice() {
            return Err(crate::Error::Internal(
                "snapshot integrity check failed: data may have been tampered with".to_string(),
            ));
        }
        let snap: Self =
            serde_json::from_slice(json).map_err(|e| crate::Error::Internal(e.to_string()))?;
        if snap.version != SNAPSHOT_VERSION {
            return Err(crate::Error::Internal(format!(
                "unsupported snapshot version {} (expected {})",
                snap.version, SNAPSHOT_VERSION
            )));
        }
        Ok(snap)
    }

    /// Serialize with a caller-provided secret key for tamper-proof integrity.
    ///
    /// Uses `HMAC-SHA256(key, payload)` instead of the public tag.
    /// Use this when snapshots cross trust boundaries (network, shared storage).
    pub fn to_bytes_keyed(&self, key: &[u8]) -> crate::Result<Vec<u8>> {
        let json = serde_json::to_vec(self).map_err(|e| crate::Error::Internal(e.to_string()))?;
        let digest = Self::compute_hmac(key, &json);
        let mut out = Vec::with_capacity(DIGEST_LEN + json.len());
        out.extend_from_slice(&digest);
        out.extend_from_slice(&json);
        Ok(out)
    }

    /// Deserialize and verify a snapshot using a caller-provided secret key.
    ///
    /// Rejects snapshots where the HMAC does not match, preventing forgery.
    pub fn from_bytes_keyed(data: &[u8], key: &[u8]) -> crate::Result<Self> {
        if data.len() < DIGEST_LEN {
            return Err(crate::Error::Internal(
                "snapshot too short: missing integrity digest".to_string(),
            ));
        }
        let (stored_digest, json) = data.split_at(DIGEST_LEN);
        let expected = Self::compute_hmac(key, json);
        if stored_digest != expected.as_slice() {
            return Err(crate::Error::Internal(
                "snapshot integrity check failed: HMAC mismatch (wrong key or tampered data)"
                    .to_string(),
            ));
        }
        let snap: Self =
            serde_json::from_slice(json).map_err(|e| crate::Error::Internal(e.to_string()))?;
        if snap.version != SNAPSHOT_VERSION {
            return Err(crate::Error::Internal(format!(
                "unsupported snapshot version {} (expected {})",
                snap.version, SNAPSHOT_VERSION
            )));
        }
        Ok(snap)
    }

    /// Compute SHA-256 digest over `INTEGRITY_TAG || payload`.
    fn compute_digest(payload: &[u8]) -> [u8; DIGEST_LEN] {
        let mut hasher = Sha256::new();
        hasher.update(INTEGRITY_TAG);
        hasher.update(payload);
        let result = hasher.finalize();
        let mut out = [0u8; DIGEST_LEN];
        out.copy_from_slice(&result);
        out
    }

    /// Compute HMAC-SHA256 using a caller-provided secret key.
    fn compute_hmac(key: &[u8], payload: &[u8]) -> [u8; DIGEST_LEN] {
        use hmac::{Hmac, KeyInit, Mac};
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(payload);
        let result = mac.finalize();
        let mut out = [0u8; DIGEST_LEN];
        out.copy_from_slice(&result.into_bytes());
        out
    }
}

impl crate::Bash {
    fn build_snapshot(&self, options: SnapshotOptions) -> Snapshot {
        let shell = self
            .interpreter
            .shell_state_with_options(ShellStateOptions {
                include_functions: !options.exclude_functions,
            });
        let vfs = if options.exclude_filesystem {
            None
        } else {
            self.fs.vfs_snapshot()
        };
        let counters = self.interpreter.counters();
        Snapshot {
            version: SNAPSHOT_VERSION,
            shell,
            vfs,
            session_commands: counters.session_commands,
            session_exec_calls: counters.session_exec_calls,
        }
    }

    /// Capture the current interpreter state as a serializable snapshot.
    ///
    /// The snapshot includes shell state (variables, env, cwd, arrays, aliases,
    /// traps) and VFS contents. It can be serialized to bytes with
    /// [`Snapshot::to_bytes()`] or directly via `serde_json`.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    /// bash.exec("x=42; mkdir /work").await?;
    ///
    /// let bytes = bash.snapshot()?;
    /// assert!(!bytes.is_empty());
    ///
    /// let mut bash2 = Bash::from_snapshot(&bytes)?;
    /// let r = bash2.exec("echo $x").await?;
    /// assert_eq!(r.stdout.trim(), "42");
    /// # Ok(())
    /// # }
    /// ```
    pub fn snapshot(&self) -> crate::Result<Vec<u8>> {
        self.snapshot_with_options(SnapshotOptions::default())
    }

    /// Capture the current interpreter state using caller-provided snapshot options.
    pub fn snapshot_with_options(&self, options: SnapshotOptions) -> crate::Result<Vec<u8>> {
        self.build_snapshot(options).to_bytes()
    }

    /// Create a new Bash instance restored from a snapshot.
    ///
    /// Restores shell state and VFS contents from previously captured bytes.
    /// The returned instance uses default execution limits and no custom builtins.
    /// Configure limits via the builder if needed, then call
    /// [`restore_snapshot()`](Self::restore_snapshot) instead.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails or the snapshot version is
    /// incompatible.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    /// bash.exec("greeting='hello world'").await?;
    /// let bytes = bash.snapshot()?;
    ///
    /// let mut restored = Bash::from_snapshot(&bytes)?;
    /// let r = restored.exec("echo $greeting").await?;
    /// assert_eq!(r.stdout.trim(), "hello world");
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_snapshot(data: &[u8]) -> crate::Result<Self> {
        let snap = Snapshot::from_bytes(data)?;
        let mut bash = Self::new();
        bash.restore_snapshot_inner(&snap)?;
        Ok(bash)
    }

    /// Restore state from a snapshot into this Bash instance.
    ///
    /// Preserves the current instance's configuration (limits, builtins,
    /// filesystem type) while restoring shell state and VFS contents.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn restore_snapshot(&mut self, data: &[u8]) -> crate::Result<()> {
        let snap = Snapshot::from_bytes(data)?;
        self.restore_snapshot_inner(&snap)
    }

    fn restore_snapshot_inner(&mut self, snap: &Snapshot) -> crate::Result<()> {
        self.interpreter
            .validate_shell_state_restore_limits(&snap.shell)?;
        self.interpreter.restore_shell_state(&snap.shell);
        if let Some(ref vfs) = snap.vfs {
            self.fs.vfs_restore(vfs);
        }
        Ok(())
    }

    /// Capture snapshot and serialize with HMAC-SHA256 using a secret key.
    ///
    /// Use this instead of [`snapshot()`](Self::snapshot) when snapshots cross
    /// trust boundaries (network, shared storage, untrusted input).
    pub fn snapshot_to_bytes_keyed(&self, key: &[u8]) -> crate::Result<Vec<u8>> {
        self.snapshot_to_bytes_keyed_with_options(key, SnapshotOptions::default())
    }

    /// Capture a keyed snapshot using caller-provided snapshot options.
    pub fn snapshot_to_bytes_keyed_with_options(
        &self,
        key: &[u8],
        options: SnapshotOptions,
    ) -> crate::Result<Vec<u8>> {
        self.build_snapshot(options).to_bytes_keyed(key)
    }

    /// Create a new Bash instance from a keyed (HMAC-protected) snapshot.
    ///
    /// Rejects snapshots where the HMAC doesn't match the provided key.
    pub fn from_snapshot_keyed(data: &[u8], key: &[u8]) -> crate::Result<Self> {
        let snap = Snapshot::from_bytes_keyed(data, key)?;
        let mut bash = Self::new();
        bash.restore_snapshot_inner(&snap)?;
        Ok(bash)
    }

    /// Restore state from a keyed snapshot into this Bash instance.
    pub fn restore_snapshot_keyed(&mut self, data: &[u8], key: &[u8]) -> crate::Result<()> {
        let snap = Snapshot::from_bytes_keyed(data, key)?;
        self.restore_snapshot_inner(&snap)
    }
}
