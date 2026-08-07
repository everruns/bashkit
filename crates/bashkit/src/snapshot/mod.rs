// Decision: v2 snapshots are a content-addressed object graph (see `objects`),
// packed into a versioned binary container (see `container`). v1 was a single
// serde_json blob; it re-encoded the whole VFS on every call, inflated binary
// content ~3.5x by writing `Vec<u8>` as JSON integer arrays, and serialized
// `HashMap`s in iteration order so identical state produced different bytes.
//
// Decision: v1 payloads stay readable forever. `from_bytes` dispatches on the
// body prefix, so stored v1 snapshots survive the upgrade — the whole point of
// the version policy in knowledge/foundations/snapshot-history.md.
//
// Decision: v2 restore funnels into the same `restore_snapshot_inner` as v1, by
// reconstructing a `Snapshot` from the object graph. Limit validation, atomic
// VFS replacement, builtin cache invalidation, and monotonic counter merging
// are therefore identical across both formats by construction, not by parallel
// implementations that could drift.

//! Snapshot/resume — serialize interpreter state between `exec()` calls.
//!
//! Two APIs over one format:
//!
//! - **Packed snapshots** ([`Bash::snapshot`], [`Bash::from_snapshot`]) return
//!   one self-contained blob. Use these for checkpoint/resume.
//! - **Commit/checkout** ([`Bash::commit`], [`Bash::checkout`]) exchange
//!   individual content-addressed objects with a store you own. Use these for
//!   session history, where consecutive snapshots share almost all their
//!   content and forks share it with their ancestors.
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
//! # History and forks
//!
//! ```rust
//! use bashkit::{Bash, CheckoutPolicy, CommitOptions, ObjectId, SnapshotGraph};
//! use std::collections::HashMap;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut store: HashMap<ObjectId, Vec<u8>> = HashMap::new();
//! let mut bash = Bash::new();
//!
//! bash.exec("echo one > /log.txt").await?;
//! let first = bash.commit(CommitOptions::new())?;
//! let first_id = first.id();
//! store.extend(first.into_objects());
//!
//! bash.exec("echo two >> /log.txt").await?;
//! // `have` keeps the commit incremental: unchanged content is not re-emitted.
//! let second = bash.commit(CommitOptions::new().parent(first_id).have(store.keys()))?;
//! let second_id = second.id();
//! store.extend(second.into_objects());
//!
//! // Fork from the first commit — no copy, no replay.
//! let mut branch = Bash::new();
//! branch.checkout(first_id, &store, CheckoutPolicy::default())?;
//! assert_eq!(branch.exec("cat /log.txt").await?.stdout, "one\n");
//!
//! let diff = SnapshotGraph::diff(first_id, second_id, &store)?;
//! assert_eq!(diff.files_modified, vec!["/log.txt".to_string()]);
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

mod capabilities;
mod chunker;
mod container;
mod graph;
mod objects;

pub use capabilities::{CapabilityDelta, CapabilityFingerprint, CheckoutPolicy};
pub use graph::{CommitOptions, ObjectSource, PackedCommit, SnapshotDiff, SnapshotGraph};
pub use objects::{CommitId, CommitObject, ObjectId};

use sha2::{Digest, Sha256};

use crate::fs::VfsSnapshot;
use crate::interpreter::{ShellState, ShellStateOptions};

/// Schema version for the legacy v1 JSON payload.
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
    /// Accepts both the current object-graph format and legacy v1 JSON
    /// payloads. Verifies integrity before decoding, and rejects tampering.
    pub fn from_bytes(data: &[u8]) -> crate::Result<Self> {
        Ok(decode_sealed(data, None, &crate::FsLimits::default())?.0)
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
        Ok(decode_sealed(data, Some(key), &crate::FsLimits::default())?.0)
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

/// Prepend the integrity digest to a payload.
///
/// With `key`, the digest is an HMAC and forgery requires the key. Without one
/// it is the public-tag SHA-256 from v1, which detects corruption but not
/// deliberate tampering (TM-SNAP-001) — that limitation is unchanged.
pub(crate) fn seal(body: &[u8], key: Option<&[u8]>) -> Vec<u8> {
    let digest = match key {
        Some(key) => Snapshot::compute_hmac(key, body),
        None => Snapshot::compute_digest(body),
    };
    let mut out = Vec::with_capacity(DIGEST_LEN + body.len());
    out.extend_from_slice(&digest);
    out.extend_from_slice(body);
    out
}

/// Verify the integrity digest and return the body it covers.
fn unseal<'a>(data: &'a [u8], key: Option<&[u8]>) -> crate::Result<&'a [u8]> {
    if data.len() < DIGEST_LEN {
        return Err(crate::Error::Internal(
            "snapshot too short: missing integrity digest".to_string(),
        ));
    }
    let (stored, body) = data.split_at(DIGEST_LEN);
    let ok = match key {
        Some(key) => {
            use hmac::{Hmac, KeyInit, Mac};
            type HmacSha256 = Hmac<Sha256>;
            let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
            mac.update(body);
            mac.verify_slice(stored).is_ok()
        }
        None => stored == Snapshot::compute_digest(body).as_slice(),
    };
    if !ok {
        return Err(crate::Error::Internal(match key {
            Some(_) => {
                "snapshot integrity check failed: HMAC mismatch (wrong key or tampered data)"
                    .to_string()
            }
            None => "snapshot integrity check failed: data may have been tampered with".to_string(),
        }));
    }
    Ok(body)
}

/// Verify, then decode either format.
///
/// The capability fingerprint is `None` for v1 payloads, which predate it —
/// there is nothing to compare against, so v1 restores skip the policy gate.
/// That is a deliberate consequence of keeping old snapshots readable.
fn decode_sealed(
    data: &[u8],
    key: Option<&[u8]>,
    limits: &crate::FsLimits,
) -> crate::Result<(Snapshot, Option<CapabilityFingerprint>)> {
    let body = unseal(data, key)?;

    if container::is_v2(body) {
        let parsed = container::decode(body)?;
        // THREAT[TM-SNAP-004]: reject declared file sizes and totals against
        // live VFS limits before expanding content-addressed chunks.
        let (snapshot, caps) = SnapshotGraph::materialize(parsed.root, &parsed.objects, limits)?;
        return Ok((snapshot, Some(caps)));
    }

    let snap: Snapshot =
        serde_json::from_slice(body).map_err(|e| crate::Error::Internal(e.to_string()))?;
    if snap.version != SNAPSHOT_VERSION {
        return Err(crate::Error::Internal(format!(
            "unsupported snapshot version {} (expected {})",
            snap.version, SNAPSHOT_VERSION
        )));
    }
    Ok((snap, None))
}

impl crate::Bash {
    /// Capture state as a [`Snapshot`] value, without serializing it.
    ///
    /// Useful for inspection and for tests that need to construct legacy v1
    /// bytes via [`Snapshot::to_bytes`].
    pub fn snapshot_state(&self, options: SnapshotOptions) -> Snapshot {
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
        self.packed_commit(options)?.to_bytes()
    }

    /// Build a self-contained commit carrying the whole state.
    fn packed_commit(&self, options: SnapshotOptions) -> crate::Result<PackedCommit> {
        self.commit(
            CommitOptions::new()
                .exclude_filesystem(options.exclude_filesystem)
                .exclude_functions(options.exclude_functions),
        )
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
        let mut bash = Self::new();
        bash.restore_snapshot(data)?;
        Ok(bash)
    }

    /// Restore state from a snapshot into this Bash instance.
    ///
    /// Preserves the current instance's configuration (limits, builtins,
    /// filesystem type) while restoring shell state and VFS contents.
    ///
    /// Enforces the default [`CheckoutPolicy::Superset`]: a snapshot taken by
    /// an instance with builtins, features, or a filesystem backend this one
    /// lacks is rejected rather than restored into an environment that may not
    /// be able to run it. Extra local capabilities are fine, so snapshots keep
    /// restoring across bashkit upgrades. Use
    /// [`restore_snapshot_with_policy`](Self::restore_snapshot_with_policy) for
    /// exact pinning or to override. Legacy v1 snapshots carry no fingerprint
    /// and skip the check.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization, integrity verification, or the
    /// capability check fails. The instance is untouched on failure.
    pub fn restore_snapshot(&mut self, data: &[u8]) -> crate::Result<()> {
        self.restore_snapshot_with_policy(data, CheckoutPolicy::default())
    }

    /// Restore from a snapshot under an explicit capability policy.
    pub fn restore_snapshot_with_policy(
        &mut self,
        data: &[u8],
        policy: CheckoutPolicy,
    ) -> crate::Result<()> {
        let (snap, caps) = decode_sealed(data, None, &self.fs.limits())?;
        self.apply_restore(&snap, caps.as_ref(), policy)
    }

    fn apply_restore(
        &mut self,
        snap: &Snapshot,
        caps: Option<&CapabilityFingerprint>,
        policy: CheckoutPolicy,
    ) -> crate::Result<()> {
        match caps {
            Some(caps) => self.apply_checked(snap, caps, policy),
            None => self.restore_snapshot_inner(snap),
        }
    }

    fn restore_snapshot_inner(&mut self, snap: &Snapshot) -> crate::Result<()> {
        // Issue #1576: validate everything that can fail before mutating
        // either the shell or the VFS, so a malformed/forged snapshot can't
        // leave the instance in a half-restored state.
        self.interpreter
            .validate_shell_state_restore_limits(&snap.shell)?;
        if let Some(ref vfs) = snap.vfs {
            self.fs.vfs_restore(vfs)?;
        }
        // Security: restore invalidates builtin caches after the VFS swap so
        // hidden state cannot leak across snapshot boundaries or overwrite the
        // restored filesystem on the next command.
        self.interpreter.reset_builtin_session_state();
        // Shell state cannot fail past validation, and the VFS has already
        // been restored atomically (or rejected) above.
        self.interpreter.restore_shell_state(&snap.shell);
        // Session counters are part of session accounting. Merge them
        // monotonically: authenticated snapshot/resume carries used budget
        // forward, while tampered unkeyed bytes cannot lower live counters.
        self.interpreter
            .restore_session_counters(snap.session_commands, snap.session_exec_calls);
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
        self.packed_commit(options)?.to_bytes_keyed(key)
    }

    /// Create a new Bash instance from a keyed (HMAC-protected) snapshot.
    ///
    /// Rejects snapshots where the HMAC doesn't match the provided key.
    pub fn from_snapshot_keyed(data: &[u8], key: &[u8]) -> crate::Result<Self> {
        let mut bash = Self::new();
        bash.restore_snapshot_keyed(data, key)?;
        Ok(bash)
    }

    /// Restore state from a keyed snapshot into this Bash instance.
    pub fn restore_snapshot_keyed(&mut self, data: &[u8], key: &[u8]) -> crate::Result<()> {
        self.restore_snapshot_keyed_with_policy(data, key, CheckoutPolicy::default())
    }

    /// Restore from a keyed snapshot under an explicit capability policy.
    pub fn restore_snapshot_keyed_with_policy(
        &mut self,
        data: &[u8],
        key: &[u8],
        policy: CheckoutPolicy,
    ) -> crate::Result<()> {
        let (snap, caps) = decode_sealed(data, Some(key), &self.fs.limits())?;
        self.apply_restore(&snap, caps.as_ref(), policy)
    }
}
