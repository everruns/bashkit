//! Shared atomic in-place file replacement for builtins that rewrite files.
//!
//! THREAT[TM-FS-016]: write a sibling temporary file, copy the original mode
//! onto it, and rename only after the new content is fully written. Any failure
//! leaves the source file untouched and removes the temporary.
//!
//! Extracted from `yq` so `sed -i` gets the same guarantees (issue #2427,
//! finding E): the previous `sed -i` wrote straight through
//! `FileSystem::write_file`, which recreates the entry with mode 0644 and loses
//! the original content if the write fails part-way.
//!
//! Decision: when the hidden sibling temporary cannot be created or written
//! (a filesystem or embedder policy that refuses dot paths, a file-count
//! limit with no room for one extra entry), fall back to a direct
//! `write_file` of the target and restore its mode. Every in-tree backend
//! makes `write_file` all-or-nothing (TM-FS-014 for `RealFs`, in-memory
//! replacement elsewhere), so the fallback still never leaves a half-written
//! target; it only gives up the rename step. Chmod and rename failures do not
//! fall back: by then the backend has accepted the temporary, so the failure
//! is real and is reported with the source untouched.

use std::path::Path;

use crate::fs::vfs_join;

/// Failpoint names for one call site. Each builtin supplies its own so the
/// security failpoint suite can target them independently.
#[cfg_attr(not(feature = "failpoints"), allow(dead_code))]
pub(crate) struct AtomicFailpoints {
    pub(crate) allocate: &'static str,
    pub(crate) chmod: &'static str,
    pub(crate) rename: &'static str,
}

pub(crate) async fn atomic_replace(
    fs: &dyn crate::fs::FileSystem,
    target: &Path,
    content: &[u8],
    tool: &str,
    failpoints: AtomicFailpoints,
) -> std::result::Result<(), String> {
    let parent = target.parent().unwrap_or_else(|| Path::new("/"));
    #[cfg(feature = "failpoints")]
    if injected_failure(failpoints.allocate, "exhausted") {
        return Err("cannot allocate temporary file".to_string());
    }
    let mut temp = None;
    for _ in 0..16 {
        let mut suffix = [0u8; 8];
        getrandom::fill(&mut suffix)
            .map_err(|_| "cannot generate temporary filename".to_string())?;
        let suffix = suffix
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let candidate = vfs_join(parent, format!(".bashkit-{tool}-{suffix}.tmp"));
        match fs.exists(&candidate).await {
            Ok(false) => {
                temp = Some(candidate);
                break;
            }
            Ok(true) => {}
            // The backend refuses to even look at the hidden path.
            Err(_) => return direct_replace(fs, target, content).await,
        }
    }
    let temp = temp.ok_or_else(|| "cannot allocate temporary file".to_string())?;
    if fs.write_file(&temp, content).await.is_err() {
        let _ = fs.remove(&temp, false).await;
        return direct_replace(fs, target, content).await;
    }
    #[cfg(feature = "failpoints")]
    if injected_failure(failpoints.chmod, "error") {
        let _ = fs.remove(&temp, false).await;
        return Err("cannot preserve file mode: injected failure".to_string());
    }
    if let Ok(metadata) = fs.stat(target).await
        && let Err(error) = fs.chmod(&temp, metadata.mode).await
    {
        let _ = fs.remove(&temp, false).await;
        return Err(format!("cannot preserve file mode: {error}"));
    }
    #[cfg(feature = "failpoints")]
    if injected_failure(failpoints.rename, "error") {
        let _ = fs.remove(&temp, false).await;
        return Err(format!(
            "cannot replace '{}': injected failure",
            target.display()
        ));
    }
    if let Err(error) = fs.rename(&temp, target).await {
        let _ = fs.remove(&temp, false).await;
        return Err(format!("cannot replace '{}': {error}", target.display()));
    }
    #[cfg(not(feature = "failpoints"))]
    let _ = failpoints;
    Ok(())
}

/// Fallback when the hidden temporary is refused: replace the target with one
/// `write_file` (all-or-nothing on every in-tree backend) and restore its mode.
async fn direct_replace(
    fs: &dyn crate::fs::FileSystem,
    target: &Path,
    content: &[u8],
) -> std::result::Result<(), String> {
    let mode = fs.stat(target).await.ok().map(|metadata| metadata.mode);
    fs.write_file(target, content)
        .await
        .map_err(|error| format!("cannot write '{}': {error}", target.display()))?;
    if let Some(mode) = mode
        && fs.stat(target).await.map(|m| m.mode).ok() != Some(mode)
    {
        fs.chmod(target, mode)
            .await
            .map_err(|error| format!("cannot preserve file mode: {error}"))?;
    }
    Ok(())
}

#[cfg(feature = "failpoints")]
fn injected_failure(name: &str, expected: &str) -> bool {
    fail::eval(name, |action| action.as_deref() == Some(expected)).unwrap_or(false)
}
