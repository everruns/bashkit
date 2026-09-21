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
        if !fs
            .exists(&candidate)
            .await
            .map_err(|error| error.to_string())?
        {
            temp = Some(candidate);
            break;
        }
    }
    let temp = temp.ok_or_else(|| "cannot allocate temporary file".to_string())?;
    if let Err(error) = fs.write_file(&temp, content).await {
        let _ = fs.remove(&temp, false).await;
        return Err(format!("cannot write temporary file: {error}"));
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

#[cfg(feature = "failpoints")]
fn injected_failure(name: &str, expected: &str) -> bool {
    fail::eval(name, |action| action.as_deref() == Some(expected)).unwrap_or(false)
}
