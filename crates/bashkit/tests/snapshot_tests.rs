//! Tests for VFS snapshot/restore and shell state snapshot/restore

use bashkit::{Bash, FileSystem, InMemoryFs};
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
