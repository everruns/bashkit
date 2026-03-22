//! Tests for live mount/unmount on a running Bash instance (issue #782).
//!
//! Verifies that `Bash::mount()` and `Bash::unmount()` work without
//! rebuilding the interpreter, preserving shell state across operations.

use bashkit::{Bash, FileSystem, InMemoryFs};
use std::path::Path;
use std::sync::Arc;

#[tokio::test]
async fn live_mount_makes_files_visible() {
    let mut bash = Bash::new();

    let data_fs = Arc::new(InMemoryFs::new());
    data_fs
        .write_file(Path::new("/hello.txt"), b"world")
        .await
        .unwrap();

    bash.mount("/mnt/data", data_fs).unwrap();

    let result = bash.exec("cat /mnt/data/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "world");
}

#[tokio::test]
async fn live_unmount_removes_files() {
    let mut bash = Bash::new();

    let tmp_fs = Arc::new(InMemoryFs::new());
    tmp_fs
        .write_file(Path::new("/file.txt"), b"temp")
        .await
        .unwrap();

    bash.mount("/scratch", tmp_fs).unwrap();
    let result = bash.exec("cat /scratch/file.txt").await.unwrap();
    assert_eq!(result.stdout, "temp");

    bash.unmount("/scratch").unwrap();
    let result = bash.exec("cat /scratch/file.txt 2>&1").await.unwrap();
    assert_ne!(result.exit_code, 0);
}

#[tokio::test]
async fn live_mount_preserves_shell_state() {
    let mut bash = Bash::new();

    // Set up shell state before mount
    bash.exec("export MY_VAR=preserved").await.unwrap();
    bash.exec("cd /tmp").await.unwrap();

    let data_fs = Arc::new(InMemoryFs::new());
    data_fs
        .write_file(Path::new("/data.txt"), b"content")
        .await
        .unwrap();

    bash.mount("/mnt/data", data_fs).unwrap();

    // Shell state should be preserved
    let result = bash.exec("echo $MY_VAR").await.unwrap();
    assert_eq!(result.stdout, "preserved\n");

    let result = bash.exec("pwd").await.unwrap();
    assert_eq!(result.stdout, "/tmp\n");

    // Mounted data is accessible
    let result = bash.exec("cat /mnt/data/data.txt").await.unwrap();
    assert_eq!(result.stdout, "content");
}

#[tokio::test]
async fn live_mount_works_with_builder() {
    let mut bash = Bash::builder()
        .mount_text("/config/app.conf", "debug=true\n")
        .env("HOME", "/home/user")
        .build();

    // Pre-existing text mount still works
    let result = bash.exec("cat /config/app.conf").await.unwrap();
    assert_eq!(result.stdout, "debug=true\n");

    // Live mount on top of builder config
    let plugin_fs = Arc::new(InMemoryFs::new());
    plugin_fs
        .write_file(Path::new("/init.sh"), b"echo plugin loaded")
        .await
        .unwrap();

    bash.mount("/plugins", plugin_fs).unwrap();

    let result = bash.exec("cat /plugins/init.sh").await.unwrap();
    assert_eq!(result.stdout, "echo plugin loaded");

    // Builder env preserved
    let result = bash.exec("echo $HOME").await.unwrap();
    assert_eq!(result.stdout, "/home/user\n");
}

#[tokio::test]
async fn unmount_nonexistent_returns_error() {
    let bash = Bash::new();
    assert!(bash.unmount("/nonexistent").is_err());
}

#[tokio::test]
async fn multiple_live_mounts() {
    let mut bash = Bash::new();

    let fs_a = Arc::new(InMemoryFs::new());
    fs_a.write_file(Path::new("/a.txt"), b"AAA").await.unwrap();

    let fs_b = Arc::new(InMemoryFs::new());
    fs_b.write_file(Path::new("/b.txt"), b"BBB").await.unwrap();

    bash.mount("/mnt/a", fs_a).unwrap();
    bash.mount("/mnt/b", fs_b).unwrap();

    let result = bash.exec("cat /mnt/a/a.txt").await.unwrap();
    assert_eq!(result.stdout, "AAA");

    let result = bash.exec("cat /mnt/b/b.txt").await.unwrap();
    assert_eq!(result.stdout, "BBB");

    // Unmount one, other stays
    bash.unmount("/mnt/a").unwrap();
    let result = bash.exec("cat /mnt/b/b.txt").await.unwrap();
    assert_eq!(result.stdout, "BBB");
}

#[tokio::test]
async fn live_mount_replace() {
    let mut bash = Bash::new();

    let fs_v1 = Arc::new(InMemoryFs::new());
    fs_v1
        .write_file(Path::new("/version"), b"v1")
        .await
        .unwrap();

    let fs_v2 = Arc::new(InMemoryFs::new());
    fs_v2
        .write_file(Path::new("/version"), b"v2")
        .await
        .unwrap();

    bash.mount("/app", fs_v1).unwrap();
    let result = bash.exec("cat /app/version").await.unwrap();
    assert_eq!(result.stdout, "v1");

    // Re-mount replaces the filesystem
    bash.mount("/app", fs_v2).unwrap();
    let result = bash.exec("cat /app/version").await.unwrap();
    assert_eq!(result.stdout, "v2");
}
