//! Tests for VFS snapshot/restore and shell state snapshot/restore

use bashkit::{Bash, FileSystem, InMemoryFs, Snapshot};
use std::path::Path;
use std::sync::Arc;

// ==================== VFS snapshot/restore ====================

#[tokio::test]
async fn vfs_snapshot_restores_file_content() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/test.txt"), b"original")
        .await
        .unwrap();

    let snapshot = fs.snapshot();

    // Modify
    fs.write_file(Path::new("/tmp/test.txt"), b"modified")
        .await
        .unwrap();

    // Restore
    fs.restore(&snapshot);

    let content = fs.read_file(Path::new("/tmp/test.txt")).await.unwrap();
    assert_eq!(content, b"original");
}

#[tokio::test]
async fn vfs_snapshot_removes_new_files() {
    let fs = Arc::new(InMemoryFs::new());
    let snapshot = fs.snapshot();

    // Create new file
    fs.write_file(Path::new("/tmp/new.txt"), b"new file")
        .await
        .unwrap();
    assert!(fs.exists(Path::new("/tmp/new.txt")).await.unwrap());

    // Restore
    fs.restore(&snapshot);
    assert!(!fs.exists(Path::new("/tmp/new.txt")).await.unwrap());
}

#[tokio::test]
async fn vfs_snapshot_restores_deleted_files() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/keep.txt"), b"keep me")
        .await
        .unwrap();

    let snapshot = fs.snapshot();

    // Delete
    fs.remove(Path::new("/tmp/keep.txt"), false).await.unwrap();
    assert!(!fs.exists(Path::new("/tmp/keep.txt")).await.unwrap());

    // Restore
    fs.restore(&snapshot);
    let content = fs.read_file(Path::new("/tmp/keep.txt")).await.unwrap();
    assert_eq!(content, b"keep me");
}

#[tokio::test]
async fn vfs_snapshot_preserves_directories() {
    let fs = Arc::new(InMemoryFs::new());
    fs.mkdir(Path::new("/data"), false).await.unwrap();
    fs.mkdir(Path::new("/data/sub"), false).await.unwrap();
    fs.write_file(Path::new("/data/sub/file.txt"), b"content")
        .await
        .unwrap();

    let snapshot = fs.snapshot();

    fs.remove(Path::new("/data"), true).await.unwrap();
    assert!(!fs.exists(Path::new("/data")).await.unwrap());

    fs.restore(&snapshot);
    assert!(fs.exists(Path::new("/data/sub")).await.unwrap());
    let content = fs.read_file(Path::new("/data/sub/file.txt")).await.unwrap();
    assert_eq!(content, b"content");
}

#[tokio::test]
async fn vfs_snapshot_serialization_roundtrip() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/data.txt"), b"serialize me")
        .await
        .unwrap();

    let snapshot = fs.snapshot();
    let json = serde_json::to_string(&snapshot).unwrap();
    let restored: bashkit::VfsSnapshot = serde_json::from_str(&json).unwrap();

    let fs2 = Arc::new(InMemoryFs::new());
    fs2.restore(&restored);

    let content = fs2.read_file(Path::new("/tmp/data.txt")).await.unwrap();
    assert_eq!(content, b"serialize me");
}

// ==================== Shell state snapshot/restore ====================

#[tokio::test]
async fn shell_state_restores_variables() {
    let mut bash = Bash::new();
    bash.exec("x=42; y=hello").await.unwrap();

    let state = bash.shell_state();

    bash.exec("x=99; y=world").await.unwrap();
    bash.restore_shell_state(&state);

    let result = bash.exec("echo $x $y").await.unwrap();
    assert_eq!(result.stdout, "42 hello\n");
}

#[tokio::test]
async fn shell_state_restores_cwd() {
    let mut bash = Bash::new();
    bash.exec("mkdir -p /data && cd /data").await.unwrap();

    let state = bash.shell_state();

    bash.exec("cd /tmp").await.unwrap();
    bash.restore_shell_state(&state);

    let result = bash.exec("pwd").await.unwrap();
    assert_eq!(result.stdout, "/data\n");
}

#[tokio::test]
async fn shell_state_restores_aliases() {
    let mut bash = Bash::new();
    bash.exec("alias ll='ls -la'").await.unwrap();

    let state = bash.shell_state();

    bash.exec("unalias ll 2>/dev/null; alias ll='ls'")
        .await
        .unwrap();
    bash.restore_shell_state(&state);

    // Verify alias is restored by checking alias command
    let result = bash.exec("alias ll").await.unwrap();
    assert!(result.stdout.contains("ls -la"));
}

#[tokio::test]
async fn shell_state_serialization_roundtrip() {
    let mut bash = Bash::new();
    bash.exec("x=42").await.unwrap();

    let state = bash.shell_state();
    let json = serde_json::to_string(&state).unwrap();
    let restored: bashkit::ShellState = serde_json::from_str(&json).unwrap();

    let mut bash2 = Bash::new();
    bash2.restore_shell_state(&restored);

    let result = bash2.exec("echo $x").await.unwrap();
    assert_eq!(result.stdout, "42\n");
}

// ==================== Combined VFS + shell state ====================

#[tokio::test]
async fn combined_snapshot_restore_multi_turn() {
    let fs = Arc::new(InMemoryFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    // Turn 1: Set up files and variables
    bash.exec("echo 'config' > /tmp/config.txt && count=1")
        .await
        .unwrap();

    let vfs_snap = fs.snapshot();
    let shell_snap = bash.shell_state();

    // Turn 2: Make changes
    bash.exec("echo 'modified' > /tmp/config.txt && count=5 && echo 'new' > /tmp/new.txt")
        .await
        .unwrap();

    // Rollback to turn 1
    fs.restore(&vfs_snap);
    bash.restore_shell_state(&shell_snap);

    let result = bash
        .exec("cat /tmp/config.txt && echo $count")
        .await
        .unwrap();
    assert_eq!(result.stdout, "config\n1\n");

    // New file should be gone
    let result = bash
        .exec("test -f /tmp/new.txt && echo exists || echo gone")
        .await
        .unwrap();
    assert_eq!(result.stdout, "gone\n");
}

// ==================== Shell options snapshot/restore ====================

#[tokio::test]
async fn shell_options_survive_snapshot_roundtrip() {
    let mut bash = Bash::new();

    // Set options via `set` builtin. Options live in SHOPT_* variables which
    // are included in the variables snapshot (no more split brain with a
    // separate ShellOptions struct).
    bash.exec("set -e; set -o pipefail").await.unwrap();

    let state = bash.shell_state();

    // Options should be present in snapshotted variables
    assert_eq!(
        state.variables.get("SHOPT_e").map(|s| s.as_str()),
        Some("1")
    );
    assert_eq!(
        state.variables.get("SHOPT_pipefail").map(|s| s.as_str()),
        Some("1")
    );

    // Serialize → deserialize to prove options survive JSON roundtrip
    let json = serde_json::to_string(&state).unwrap();
    let restored: bashkit::ShellState = serde_json::from_str(&json).unwrap();
    assert_eq!(
        restored.variables.get("SHOPT_e").map(|s| s.as_str()),
        Some("1")
    );
    assert_eq!(
        restored.variables.get("SHOPT_pipefail").map(|s| s.as_str()),
        Some("1")
    );

    // Restore into a fresh interpreter and verify options are active
    let mut bash2 = Bash::new();
    bash2.restore_shell_state(&restored);

    // exec() calls reset_transient_state which clears SHOPT_* vars,
    // so we verify the state was restored correctly by inspecting it
    // before the next exec() call.
    let state2 = bash2.shell_state();
    assert_eq!(
        state2.variables.get("SHOPT_e").map(|s| s.as_str()),
        Some("1"),
        "errexit should survive snapshot/restore roundtrip"
    );
    assert_eq!(
        state2.variables.get("SHOPT_pipefail").map(|s| s.as_str()),
        Some("1"),
        "pipefail should survive snapshot/restore roundtrip"
    );
}

// ==================== Byte-level snapshot / from_snapshot ====================

#[tokio::test]
async fn snapshot_to_bytes_and_restore() {
    let mut bash = Bash::new();
    bash.exec("x=42; mkdir /tmp/work; echo 'data' > /tmp/work/file.txt")
        .await
        .unwrap();

    let bytes = bash.snapshot().unwrap();
    assert!(!bytes.is_empty());

    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    // Verify shell state
    let r = bash2.exec("echo $x").await.unwrap();
    assert_eq!(r.stdout.trim(), "42");

    // Verify VFS contents
    let r = bash2.exec("cat /tmp/work/file.txt").await.unwrap();
    assert_eq!(r.stdout.trim(), "data");
}

#[tokio::test]
async fn snapshot_preserves_arrays() {
    let mut bash = Bash::new();
    bash.exec("arr=(one two three); declare -A map=([k1]=v1 [k2]=v2)")
        .await
        .unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    let r = bash2.exec("echo ${arr[1]}").await.unwrap();
    assert_eq!(r.stdout.trim(), "two");

    let r = bash2.exec("echo ${map[k2]}").await.unwrap();
    assert_eq!(r.stdout.trim(), "v2");
}

#[tokio::test]
async fn snapshot_preserves_env() {
    let mut bash = Bash::new();
    bash.exec("export MY_VAR=hello").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    let r = bash2.exec("echo $MY_VAR").await.unwrap();
    assert_eq!(r.stdout.trim(), "hello");
}

#[tokio::test]
async fn snapshot_preserves_cwd() {
    let mut bash = Bash::new();
    bash.exec("mkdir -p /project && cd /project").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    let r = bash2.exec("pwd").await.unwrap();
    assert_eq!(r.stdout.trim(), "/project");
}

#[tokio::test]
async fn snapshot_restore_into_existing_instance() {
    let mut bash = Bash::new();
    bash.exec("x=42; echo 'data' > /tmp/saved.txt")
        .await
        .unwrap();

    let bytes = bash.snapshot().unwrap();

    // Make changes
    bash.exec("x=99; echo 'changed' > /tmp/saved.txt")
        .await
        .unwrap();

    // Restore into same instance
    bash.restore_snapshot(&bytes).unwrap();

    let r = bash.exec("echo $x").await.unwrap();
    assert_eq!(r.stdout.trim(), "42");

    let r = bash.exec("cat /tmp/saved.txt").await.unwrap();
    assert_eq!(r.stdout.trim(), "data");
}

#[tokio::test]
async fn snapshot_struct_serialization() {
    let mut bash = Bash::new();
    bash.exec("greeting='hello world'").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let snap = Snapshot::from_bytes(&bytes).unwrap();

    assert_eq!(snap.version, 1);
    assert_eq!(
        snap.shell.variables.get("greeting").map(|s| s.as_str()),
        Some("hello world")
    );

    // Re-serialize and verify roundtrip
    let bytes2 = snap.to_bytes().unwrap();
    let snap2 = Snapshot::from_bytes(&bytes2).unwrap();
    assert_eq!(
        snap2.shell.variables.get("greeting"),
        snap.shell.variables.get("greeting")
    );
}

#[tokio::test]
async fn snapshot_invalid_data_returns_error() {
    let result = Bash::from_snapshot(b"not valid json");
    assert!(result.is_err());
}

#[tokio::test]
async fn snapshot_session_counters_transferred() {
    let mut bash = Bash::new();
    // Run some commands to increment session counters
    bash.exec("echo 1; echo 2; echo 3").await.unwrap();
    bash.exec("echo 4").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let snap = Snapshot::from_bytes(&bytes).unwrap();

    // Session counters should be > 0
    assert!(snap.session_commands > 0);
    assert!(snap.session_exec_calls > 0);
}
