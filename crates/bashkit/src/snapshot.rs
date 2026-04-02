// Decision: Snapshot format uses serde_json for Phase 1 (debuggable, human-readable).
// Phase 2 can add bincode/postcard for compactness.
// VFS contents are included by default (opt-out via SnapshotOptions in future).
// Session limit budgets are transferred (not reset) to preserve resource accounting.

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
//! - Shell aliases
//! - Trap handlers
//! - VFS contents (files, directories, symlinks)
//! - Session-level resource counters (commands used, exec calls)
//!
//! # What is NOT captured
//!
//! - Function definitions (AST is not serializable; define functions after restore)
//! - Builtins (immutable configuration, not state)
//! - Active execution stack (snapshot only between `exec()` calls)
//! - Tokio runtime state
//! - File descriptors, pipes, background jobs (ephemeral)
//! - Execution limits configuration (caller should configure on restore)

use crate::fs::VfsSnapshot;
use crate::interpreter::ShellState;

/// Schema version for snapshot format compatibility.
const SNAPSHOT_VERSION: u32 = 1;

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

impl Snapshot {
    /// Serialize this snapshot to JSON bytes.
    pub fn to_bytes(&self) -> crate::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| crate::Error::Internal(e.to_string()))
    }

    /// Deserialize a snapshot from JSON bytes.
    pub fn from_bytes(data: &[u8]) -> crate::Result<Self> {
        let snap: Self =
            serde_json::from_slice(data).map_err(|e| crate::Error::Internal(e.to_string()))?;
        if snap.version != SNAPSHOT_VERSION {
            return Err(crate::Error::Internal(format!(
                "unsupported snapshot version {} (expected {})",
                snap.version, SNAPSHOT_VERSION
            )));
        }
        Ok(snap)
    }
}

impl crate::Bash {
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
        let shell = self.interpreter.shell_state();
        let vfs = self.fs.vfs_snapshot();
        let counters = self.interpreter.counters();
        let snap = Snapshot {
            version: SNAPSHOT_VERSION,
            shell,
            vfs,
            session_commands: counters.session_commands,
            session_exec_calls: counters.session_exec_calls,
        };
        snap.to_bytes()
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
        bash.restore_snapshot_inner(&snap);
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
        self.restore_snapshot_inner(&snap);
        Ok(())
    }

    fn restore_snapshot_inner(&mut self, snap: &Snapshot) {
        self.interpreter.restore_shell_state(&snap.shell);
        if let Some(ref vfs) = snap.vfs {
            self.fs.vfs_restore(vfs);
        }
        self.interpreter
            .restore_session_counters(snap.session_commands, snap.session_exec_calls);
    }
}
