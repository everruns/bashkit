// Snapshot: the post-run state a mira `Scorer` needs to evaluate bashkit
// expectations. Mira scorers only see `&Sample` + `&Transcript`, so everything
// the deterministic checks read must be carried on the Transcript:
//
//   - VFS files (path -> contents) go in `transcript.files` (also surfaces them
//     in mira's HTML/JSON reports and feeds mira's own file scorers).
//   - VFS directories + per-tool-call stdout/stderr/exit_code go in
//     `transcript.metadata["bashkit"]` as this `Snapshot` (kept out of `files`
//     so the file map stays a clean view of the workspace).
//
// See specs/eval.md ("Scoring") for why the snapshot, not a live filesystem
// handle, is the scoring substrate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bashkit::{FileSystem, FileType};
use serde::{Deserialize, Serialize};

/// One tool invocation's captured output, used by the trace-oriented checks
/// (`exit_code`, `stdout_contains`, `stdout_regex`, `stderr_empty`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolOutput {
    pub commands: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Non-file state carried in `transcript.metadata["bashkit"]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    /// Output of every tool call, in order.
    pub tool_outputs: Vec<ToolOutput>,
    /// Exit code of the final tool call (`None` if no tool was ever called).
    pub last_exit_code: Option<i32>,
    /// Absolute paths of every directory in the VFS after the run.
    pub dirs: Vec<String>,
}

/// Metadata key under which the snapshot rides on the Transcript.
pub const SNAPSHOT_KEY: &str = "bashkit";

impl Snapshot {
    /// Decode a snapshot from a Transcript's metadata, if present.
    pub fn from_metadata(metadata: &BTreeMap<String, serde_json::Value>) -> Option<Snapshot> {
        metadata
            .get(SNAPSHOT_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Encode this snapshot to a JSON value for `transcript.metadata`.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Walk an entire bashkit VFS into a flat `(files, dirs)` pair. `files` maps
/// absolute path -> UTF-8-lossy contents; `dirs` is the sorted set of absolute
/// directory paths. Symlinks/FIFOs are ignored (no eval scores on them).
pub async fn snapshot_fs(fs: &dyn FileSystem) -> (BTreeMap<String, String>, Vec<String>) {
    let mut files = BTreeMap::new();
    let mut dirs = Vec::new();
    walk(fs, PathBuf::from("/"), &mut files, &mut dirs).await;
    dirs.sort();
    (files, dirs)
}

fn walk<'a>(
    fs: &'a dyn FileSystem,
    dir: PathBuf,
    files: &'a mut BTreeMap<String, String>,
    dirs: &'a mut Vec<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let entries = match fs.read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries {
            let path = dir.join(&entry.name);
            match entry.metadata.file_type {
                FileType::Directory => {
                    dirs.push(path_string(&path));
                    walk(fs, path, files, dirs).await;
                }
                FileType::File => {
                    if let Ok(bytes) = fs.read_file(&path).await {
                        files.insert(
                            path_string(&path),
                            String::from_utf8_lossy(&bytes).into_owned(),
                        );
                    }
                }
                FileType::Symlink | FileType::Fifo => {}
            }
        }
    })
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
