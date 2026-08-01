// Decision: the fingerprint is its own object, referenced by the commit rather
// than inlined into it. A session's environment is identical across every
// commit it produces, so content addressing stores the (~2 KB) builtin list
// once per session instead of once per message.
//
// Decision: the fingerprint records builtin *names*, not a hash of them,
// because `Superset` has to compute which names are missing in order to say so.

//! Capability fingerprinting: what environment produced a snapshot, and
//! whether the environment restoring it is compatible.

use std::collections::BTreeSet;

/// Environment that produced a commit.
///
/// Deserialization ignores unknown fields so a newer producer can add
/// capability dimensions without breaking older readers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CapabilityFingerprint {
    /// Bashkit version string. Informational — it does not gate restore,
    /// because the format versions and capability sets are what actually
    /// determine compatibility.
    pub bashkit_version: String,
    /// Every dispatchable builtin name, sorted.
    pub builtins: Vec<String>,
    /// Compile-time features that change interpreter semantics, sorted.
    pub features: Vec<String>,
    /// Filesystem backend kind, e.g. `in-memory` or `overlay`.
    pub fs_backend: String,
}

impl CapabilityFingerprint {
    /// Fingerprint a live instance.
    ///
    /// Useful for previewing whether a stored snapshot will pass a given
    /// [`CheckoutPolicy`] before attempting the restore.
    pub fn capture(bash: &crate::Bash) -> Self {
        let mut builtins = bash.builtin_names();
        builtins.sort();
        builtins.dedup();

        Self {
            bashkit_version: env!("CARGO_PKG_VERSION").to_string(),
            builtins,
            features: enabled_features(),
            fs_backend: bash.fs().backend_kind().to_string(),
        }
    }

    /// Compare against the environment restoring this snapshot.
    pub fn compare(&self, live: &Self) -> CapabilityDelta {
        let snap_builtins: BTreeSet<&str> = self.builtins.iter().map(String::as_str).collect();
        let live_builtins: BTreeSet<&str> = live.builtins.iter().map(String::as_str).collect();
        let snap_features: BTreeSet<&str> = self.features.iter().map(String::as_str).collect();
        let live_features: BTreeSet<&str> = live.features.iter().map(String::as_str).collect();

        CapabilityDelta {
            missing_builtins: diff_names(&snap_builtins, &live_builtins),
            extra_builtins: diff_names(&live_builtins, &snap_builtins),
            missing_features: diff_names(&snap_features, &live_features),
            extra_features: diff_names(&live_features, &snap_features),
            fs_backend_changed: self.fs_backend != live.fs_backend,
        }
    }
}

fn diff_names(from: &BTreeSet<&str>, to: &BTreeSet<&str>) -> Vec<String> {
    from.difference(to).map(|s| (*s).to_string()).collect()
}

fn enabled_features() -> Vec<String> {
    // Only features that change what a restored script can actually do.
    // Cosmetic or build-only features are deliberately excluded so they cannot
    // spuriously fail a Strict restore.
    // Each entry is `Some` only when its feature is on, so the list reflects
    // this build exactly. Written as a fixed array rather than conditional
    // pushes so the set is visible in one place.
    let mut out: Vec<String> = [
        cfg!(feature = "python").then_some("python"),
        cfg!(feature = "typescript").then_some("typescript"),
        cfg!(feature = "sqlite").then_some("sqlite"),
        cfg!(feature = "ssh").then_some("ssh"),
        cfg!(feature = "http_client").then_some("http_client"),
        cfg!(feature = "realfs").then_some("realfs"),
    ]
    .into_iter()
    .flatten()
    .map(str::to_owned)
    .collect();
    out.sort();
    out
}

/// What differs between a snapshot's environment and the live one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityDelta {
    /// Builtins the snapshot had that the live instance lacks.
    pub missing_builtins: Vec<String>,
    /// Builtins the live instance has that the snapshot did not.
    pub extra_builtins: Vec<String>,
    /// Features the snapshot had that the live instance lacks.
    pub missing_features: Vec<String>,
    /// Features the live instance has that the snapshot did not.
    pub extra_features: Vec<String>,
    /// Whether the filesystem backend kind differs.
    pub fs_backend_changed: bool,
}

impl CapabilityDelta {
    /// True when the environments are identical.
    pub fn is_empty(&self) -> bool {
        self.missing_builtins.is_empty()
            && self.extra_builtins.is_empty()
            && self.missing_features.is_empty()
            && self.extra_features.is_empty()
            && !self.fs_backend_changed
    }

    /// True when the live environment is at least as capable as the snapshot's.
    pub fn is_superset(&self) -> bool {
        self.missing_builtins.is_empty()
            && self.missing_features.is_empty()
            && !self.fs_backend_changed
    }
}

// TM-INF-022: this text reaches callers and logs. Display only, and capped, so
// a snapshot carrying thousands of builtin names cannot turn a restore failure
// into an unbounded diagnostic.
impl std::fmt::Display for CapabilityDelta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if !self.missing_builtins.is_empty() {
            parts.push(format!(
                "missing builtins: {}",
                summarize(&self.missing_builtins)
            ));
        }
        if !self.extra_builtins.is_empty() {
            parts.push(format!(
                "extra builtins: {}",
                summarize(&self.extra_builtins)
            ));
        }
        if !self.missing_features.is_empty() {
            parts.push(format!(
                "missing features: {}",
                summarize(&self.missing_features)
            ));
        }
        if !self.extra_features.is_empty() {
            parts.push(format!(
                "extra features: {}",
                summarize(&self.extra_features)
            ));
        }
        if self.fs_backend_changed {
            parts.push("filesystem backend differs".to_string());
        }
        if parts.is_empty() {
            return f.write_str("no differences");
        }
        let joined = parts.join("; ");
        f.write_str(truncate(&joined, 1024))
    }
}

fn summarize(names: &[String]) -> String {
    const SHOWN: usize = 8;
    if names.len() <= SHOWN {
        return names.join(", ");
    }
    format!(
        "{}, and {} more",
        names[..SHOWN].join(", "),
        names.len() - SHOWN
    )
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// Decision: `Superset` is the default checkout policy, not `Strict`.
//
// The dangerous direction is asymmetric. Restoring into an environment that
// lacks a tool the snapshot's environment had can produce a session that
// cannot run; restoring into one that has extra tools cannot. Defaulting to
// `Strict` would also mean every stored snapshot stops restoring the moment
// bashkit ships a new builtin, since the fingerprint records the whole builtin
// set — which would push long-lived callers straight to `Force` and lose the
// check entirely. `Superset` enforces the property that actually matters and
// survives upgrades; `Strict` stays available for exact pinning.

/// How strictly a checkout enforces the capability fingerprint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CheckoutPolicy {
    /// Environments must match exactly, in both directions. Use to pin a
    /// session to one exact build; expect it to reject snapshots taken by any
    /// other bashkit version.
    Strict,
    /// The live environment must be at least as capable as the snapshot's.
    /// Missing builtins, missing features, or a changed filesystem backend are
    /// errors; additions are not.
    #[default]
    Superset,
    /// Restore regardless. The caller is responsible for the consequences.
    Force,
}

impl CheckoutPolicy {
    pub(crate) fn check(self, delta: &CapabilityDelta) -> crate::Result<()> {
        let ok = match self {
            Self::Strict => delta.is_empty(),
            Self::Superset => delta.is_superset(),
            Self::Force => true,
        };
        if ok {
            return Ok(());
        }
        // Typed variant lands with the release that exposes `CheckoutPolicy`;
        // `Error` is not `#[non_exhaustive]` so it cannot ride in a patch.
        Err(crate::Error::Internal(format!(
            "snapshot capability mismatch: {delta}"
        )))
    }
}

/// SQLite database header, used for the state-evidence check below.
///
/// Only referenced when the `sqlite` feature is off — with the feature on there
/// is nothing to refuse.
#[cfg(not(feature = "sqlite"))]
const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";

/// Reject state that provably needs a feature the live build lacks.
///
/// The fingerprint proves the environments match; it cannot prove a restored
/// session will work. Where captured *state* implies a requirement, check the
/// state directly — a VFS holding SQLite databases is unusable without the
/// `sqlite` feature no matter which policy the caller chose.
pub(crate) fn check_state_evidence(vfs: &crate::fs::VfsSnapshot) -> crate::Result<()> {
    let _ = vfs;
    #[cfg(not(feature = "sqlite"))]
    {
        for entry in vfs.entries() {
            if let crate::fs::VfsEntryKind::File { content } = &entry.kind
                && content.starts_with(SQLITE_MAGIC)
            {
                return Err(crate::Error::Internal(format!(
                    "snapshot contains a SQLite database ({}) but this build lacks the sqlite feature",
                    entry.path.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(builtins: &[&str], features: &[&str], backend: &str) -> CapabilityFingerprint {
        CapabilityFingerprint {
            bashkit_version: "0.0.0".to_string(),
            builtins: builtins.iter().map(|s| s.to_string()).collect(),
            features: features.iter().map(|s| s.to_string()).collect(),
            fs_backend: backend.to_string(),
        }
    }

    #[test]
    fn identical_environments_have_no_delta() {
        let a = fp(&["echo", "ls"], &["sqlite"], "in-memory");
        assert!(a.compare(&a).is_empty());
    }

    #[test]
    fn detects_missing_and_extra_builtins() {
        let snap = fp(&["echo", "custom_tool"], &[], "in-memory");
        let live = fp(&["echo", "other_tool"], &[], "in-memory");
        let delta = snap.compare(&live);
        assert_eq!(delta.missing_builtins, vec!["custom_tool"]);
        assert_eq!(delta.extra_builtins, vec!["other_tool"]);
        assert!(!delta.is_superset());
    }

    #[test]
    fn superset_allows_extra_but_not_missing() {
        let snap = fp(&["echo"], &[], "in-memory");
        let live = fp(&["echo", "jq"], &[], "in-memory");
        let delta = snap.compare(&live);
        assert!(delta.is_superset());
        assert!(!delta.is_empty());

        assert!(CheckoutPolicy::Superset.check(&delta).is_ok());
        assert!(CheckoutPolicy::Strict.check(&delta).is_err());
        assert!(CheckoutPolicy::Force.check(&delta).is_ok());
    }

    #[test]
    fn missing_builtin_fails_every_policy_but_force() {
        let snap = fp(&["echo", "jq"], &[], "in-memory");
        let live = fp(&["echo"], &[], "in-memory");
        let delta = snap.compare(&live);
        assert!(CheckoutPolicy::Strict.check(&delta).is_err());
        assert!(CheckoutPolicy::Superset.check(&delta).is_err());
        assert!(CheckoutPolicy::Force.check(&delta).is_ok());
    }

    #[test]
    fn feature_and_backend_changes_are_detected() {
        let snap = fp(&[], &["sqlite"], "in-memory");
        let live = fp(&[], &[], "overlay");
        let delta = snap.compare(&live);
        assert_eq!(delta.missing_features, vec!["sqlite"]);
        assert!(delta.fs_backend_changed);
        assert!(!delta.is_superset());
    }

    #[test]
    fn display_is_bounded_and_summarized() {
        let many: Vec<String> = (0..500).map(|i| format!("builtin_{i}")).collect();
        let delta = CapabilityDelta {
            missing_builtins: many,
            ..Default::default()
        };
        let text = delta.to_string();
        assert!(text.len() <= 1024, "diagnostic must stay bounded");
        assert!(text.contains("and 492 more"));
    }

    #[test]
    fn display_reports_no_differences_when_empty() {
        assert_eq!(CapabilityDelta::default().to_string(), "no differences");
    }

    #[test]
    fn capture_reports_a_populated_environment() {
        let bash = crate::Bash::new();
        let caps = CapabilityFingerprint::capture(&bash);
        assert!(caps.builtins.contains(&"echo".to_string()));
        assert_eq!(caps.fs_backend, "in-memory");
        // Sorted and deduped, so the fingerprint is order-independent.
        let mut sorted = caps.builtins.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, caps.builtins);
    }

    #[cfg(not(feature = "sqlite"))]
    #[test]
    fn state_evidence_rejects_sqlite_db_without_the_feature() {
        use crate::fs::{VfsEntry, VfsEntryKind, VfsSnapshot};
        let mut content = SQLITE_MAGIC.to_vec();
        content.extend_from_slice(&[0u8; 32]);
        let vfs = VfsSnapshot::from_entries(vec![VfsEntry {
            path: std::path::PathBuf::from("/data/app.db"),
            kind: VfsEntryKind::File { content },
            mode: 0o644,
        }]);
        let err = check_state_evidence(&vfs).unwrap_err().to_string();
        assert!(err.contains("sqlite"), "unexpected message: {err}");
    }

    #[test]
    fn state_evidence_allows_ordinary_files() {
        use crate::fs::{VfsEntry, VfsEntryKind, VfsSnapshot};
        let vfs = VfsSnapshot::from_entries(vec![VfsEntry {
            path: std::path::PathBuf::from("/notes.txt"),
            kind: VfsEntryKind::File {
                content: b"plain text".to_vec(),
            },
            mode: 0o644,
        }]);
        assert!(check_state_evidence(&vfs).is_ok());
    }
}
