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

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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
    /// Absolute paths of expectation-relevant directories in the VFS after the run.
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

/// Maximum bytes retained from any one expected file in a transcript snapshot.
pub const MAX_SNAPSHOT_FILE_BYTES: usize = 1024 * 1024;

/// File and directory paths needed by the deterministic expectation checks.
#[derive(Debug, Clone, Default)]
pub struct SnapshotTargets {
    pub files: BTreeSet<String>,
    pub dirs: BTreeSet<String>,
}

impl SnapshotTargets {
    /// Derive the minimal VFS snapshot target set from task expectations.
    pub fn from_expectations(expectations: &[(String, f64)]) -> Self {
        let mut targets = Self::default();
        for (check, _) in expectations {
            let (check_type, check_value) = check.split_once(':').unwrap_or((check, ""));
            match check_type {
                "file_exists" => {
                    targets.files.insert(check_value.to_string());
                    targets.dirs.insert(check_value.to_string());
                }
                "dir_exists" => {
                    targets.dirs.insert(check_value.to_string());
                }
                "file_contains" | "file_line_regex" => {
                    if let Some((path, _)) = check_value.split_once(':') {
                        targets.files.insert(path.to_string());
                    }
                }
                _ => {}
            }
        }
        targets
    }
}

/// Snapshot only expectation-relevant VFS state into transcript data. Full VFS
/// capture is intentionally avoided: model-controlled evals can create many
/// large files, and transcript/reporting code retains this map in memory.
pub async fn snapshot_fs(
    fs: &dyn FileSystem,
    targets: &SnapshotTargets,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut files = BTreeMap::new();
    for path in &targets.files {
        let path_buf = PathBuf::from(path);
        let Ok(metadata) = fs.stat(&path_buf).await else {
            continue;
        };
        if metadata.file_type != FileType::File {
            continue;
        }
        if let Ok(bytes) = fs.read_file(&path_buf).await {
            files.insert(path.clone(), lossy_truncated(&bytes));
        }
    }

    let mut dirs = Vec::new();
    for path in &targets.dirs {
        let path_buf = PathBuf::from(path);
        let Ok(metadata) = fs.stat(&path_buf).await else {
            continue;
        };
        if metadata.file_type == FileType::Directory {
            dirs.push(path.clone());
        }
    }
    dirs.sort();
    (files, dirs)
}

fn lossy_truncated(bytes: &[u8]) -> String {
    let capped = bytes.len().min(MAX_SNAPSHOT_FILE_BYTES);
    let mut content = String::from_utf8_lossy(&bytes[..capped]).into_owned();
    if bytes.len() > capped {
        content.push_str("\n[... truncated by bashkit-eval transcript snapshot ...]");
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use bashkit::InMemoryFs;

    #[tokio::test]
    async fn snapshot_reads_only_expectation_files() {
        let fs = InMemoryFs::new();
        fs.mkdir(std::path::Path::new("/wanted"), false)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/wanted/file.txt"), b"keep")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/ignored.txt"), b"drop")
            .await
            .unwrap();

        let expectations = vec![
            ("file_contains:/wanted/file.txt:keep".to_string(), 1.0),
            ("dir_exists:/wanted".to_string(), 1.0),
        ];
        let targets = SnapshotTargets::from_expectations(&expectations);
        let (files, dirs) = snapshot_fs(&fs, &targets).await;

        assert_eq!(
            files.get("/wanted/file.txt").map(String::as_str),
            Some("keep")
        );
        assert!(!files.contains_key("/ignored.txt"));
        assert_eq!(dirs, vec!["/wanted".to_string()]);
    }

    #[tokio::test]
    async fn snapshot_truncates_expected_large_files() {
        let fs = InMemoryFs::new();
        let bytes = vec![b'a'; MAX_SNAPSHOT_FILE_BYTES + 1024];
        fs.write_file(std::path::Path::new("/large.txt"), &bytes)
            .await
            .unwrap();

        let expectations = vec![("file_contains:/large.txt:a".to_string(), 1.0)];
        let targets = SnapshotTargets::from_expectations(&expectations);
        let (files, _) = snapshot_fs(&fs, &targets).await;
        let content = files.get("/large.txt").unwrap();

        assert!(content.len() < String::from_utf8_lossy(&bytes).len());
        assert!(content.ends_with("[... truncated by bashkit-eval transcript snapshot ...]"));
    }
}
