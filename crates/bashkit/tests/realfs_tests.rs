//! Integration tests for RealFs feature.
//!
//! Tests the full pipeline: host directory → RealFs → PosixFs → Bash interpreter.

#![cfg(feature = "realfs")]

use bashkit::Bash;
use std::path::Path;

fn setup_host_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();
    std::fs::create_dir(dir.path().join("subdir")).unwrap();
    std::fs::write(dir.path().join("subdir/nested.txt"), "nested\n").unwrap();
    std::fs::write(dir.path().join("data.csv"), "a,1\nb,2\nc,3\n").unwrap();
    dir
}

// --- Use case 1: readonly overlay at root ---

#[tokio::test]
async fn readonly_root_overlay_cat() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder().mount_real_readonly(dir.path()).build();

    let result = bash.exec("cat /hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn readonly_root_overlay_ls() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder().mount_real_readonly(dir.path()).build();

    let result = bash.exec("ls /").await.unwrap();
    assert!(result.stdout.contains("hello.txt"));
    assert!(result.stdout.contains("subdir"));
}

#[tokio::test]
async fn readonly_root_overlay_nested() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder().mount_real_readonly(dir.path()).build();

    let result = bash.exec("cat /subdir/nested.txt").await.unwrap();
    assert_eq!(result.stdout, "nested\n");
}

#[tokio::test]
async fn readonly_root_overlay_write_goes_to_memory() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder().mount_real_readonly(dir.path()).build();

    // Write a new file - should go to in-memory overlay
    bash.exec("echo 'vfs only' > /new_file.txt").await.unwrap();
    let result = bash.exec("cat /new_file.txt").await.unwrap();
    assert_eq!(result.stdout, "vfs only\n");

    // Host should NOT have this file
    assert!(!dir.path().join("new_file.txt").exists());
}

#[tokio::test]
async fn readonly_root_overlay_pipes() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder().mount_real_readonly(dir.path()).build();

    let result = bash.exec("cat /data.csv | grep b").await.unwrap();
    assert_eq!(result.stdout, "b,2\n");
}

#[tokio::test]
async fn readonly_root_overlay_wc() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder().mount_real_readonly(dir.path()).build();

    let result = bash.exec("wc -l < /data.csv").await.unwrap();
    assert_eq!(result.stdout.trim(), "3");
}

// --- Use case 2: readonly mount at specific path ---

#[tokio::test]
async fn readonly_mount_at_path_cat() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let result = bash.exec("cat /mnt/data/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");
}

#[tokio::test]
async fn readonly_mount_at_path_ls() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let result = bash.exec("ls /mnt/data").await.unwrap();
    assert!(result.stdout.contains("hello.txt"));
    assert!(result.stdout.contains("subdir"));
}

#[tokio::test]
async fn readonly_mount_at_path_vfs_root_intact() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    // VFS root should still have default dirs
    let result = bash
        .exec("test -d /tmp && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "yes");

    // Can write to VFS normally
    bash.exec("echo test > /tmp/test.txt").await.unwrap();
    let result = bash.exec("cat /tmp/test.txt").await.unwrap();
    assert_eq!(result.stdout, "test\n");
}

// --- Use case 3: readwrite mount ---

#[tokio::test]
async fn readwrite_mount_modifies_host() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readwrite_at(dir.path(), "/workspace")
        .build();

    // Read existing file
    let result = bash.exec("cat /workspace/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    // Write to host file (overwrite)
    bash.exec("echo 'modified by bash' > /workspace/hello.txt")
        .await
        .unwrap();

    // Verify on host
    let content = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
    assert_eq!(content, "modified by bash\n");

    // Append to host file
    bash.exec("echo 'appended line' >> /workspace/hello.txt")
        .await
        .unwrap();

    let content = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
    assert!(
        content.contains("appended line"),
        "append should modify host file, got: {:?}",
        content
    );
}

#[tokio::test]
async fn readwrite_mount_creates_files_on_host() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readwrite_at(dir.path(), "/workspace")
        .build();

    bash.exec("echo 'new' > /workspace/created.txt")
        .await
        .unwrap();

    assert!(dir.path().join("created.txt").exists());
    let content = std::fs::read_to_string(dir.path().join("created.txt")).unwrap();
    assert_eq!(content, "new\n");
}

#[tokio::test]
async fn readwrite_mount_creates_dirs_on_host() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readwrite_at(dir.path(), "/workspace")
        .build();

    bash.exec("mkdir -p /workspace/a/b/c").await.unwrap();
    assert!(dir.path().join("a/b/c").is_dir());
}

#[tokio::test]
async fn readwrite_root_overlay() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder().mount_real_readwrite(dir.path()).build();

    let result = bash.exec("cat /hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    // Write goes to overlay (in-memory), not host, because OverlayFs wraps it
    bash.exec("echo 'overlay' > /overlay_file.txt")
        .await
        .unwrap();
    let result = bash.exec("cat /overlay_file.txt").await.unwrap();
    assert_eq!(result.stdout, "overlay\n");
}

// --- Multiple mounts ---

#[tokio::test]
async fn multiple_readonly_mounts() {
    let dir1 = setup_host_dir();
    let dir2 = tempfile::tempdir().unwrap();
    std::fs::write(dir2.path().join("other.txt"), "from dir2\n").unwrap();

    let mut bash = Bash::builder()
        .mount_real_readonly_at(dir1.path(), "/mnt/a")
        .mount_real_readonly_at(dir2.path(), "/mnt/b")
        .build();

    let result = bash.exec("cat /mnt/a/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    let result = bash.exec("cat /mnt/b/other.txt").await.unwrap();
    assert_eq!(result.stdout, "from dir2\n");
}

#[tokio::test]
async fn mixed_readonly_and_text_mounts() {
    let dir = setup_host_dir();

    let mut bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/host")
        .mount_text("/config/app.toml", "key = 'value'\n")
        .build();

    let result = bash.exec("cat /mnt/host/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    let result = bash.exec("cat /config/app.toml").await.unwrap();
    assert_eq!(result.stdout, "key = 'value'\n");
}

// --- Security: path traversal ---

#[tokio::test]
async fn path_traversal_blocked_via_bash() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    // Attempt traversal - should not leak files outside the mount root
    let result = bash
        .exec("cat /mnt/data/../../etc/passwd 2>&1")
        .await
        .unwrap();
    // This should fail or return content from VFS, not from host /etc/passwd
    assert!(result.exit_code != 0 || !result.stdout.contains("root:"));
}

// --- Direct filesystem API ---

#[tokio::test]
async fn direct_fs_api_read() {
    let dir = setup_host_dir();
    let bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let fs = bash.fs();
    let content = fs
        .read_file(Path::new("/mnt/data/hello.txt"))
        .await
        .unwrap();
    assert_eq!(content, b"hello world\n");
}

#[tokio::test]
async fn direct_fs_api_stat() {
    let dir = setup_host_dir();
    let bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let fs = bash.fs();
    let meta = fs.stat(Path::new("/mnt/data/hello.txt")).await.unwrap();
    assert!(meta.file_type.is_file());
    assert_eq!(meta.size, 12); // "hello world\n"
}

#[tokio::test]
async fn direct_fs_api_exists() {
    let dir = setup_host_dir();
    let bash = Bash::builder()
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let fs = bash.fs();
    assert!(fs.exists(Path::new("/mnt/data/hello.txt")).await.unwrap());
    assert!(!fs.exists(Path::new("/mnt/data/nope.txt")).await.unwrap());
}

// ==================== Symlink sandbox escape prevention (Issue #979) ====================

#[tokio::test]
async fn realfs_symlink_absolute_escape_blocked() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readwrite_at(dir.path(), "/mnt/workspace")
        .build();

    // Attempt to create a symlink pointing to /etc/passwd
    let r = bash
        .exec("ln -s /etc/passwd /mnt/workspace/escape 2>&1; echo $?")
        .await
        .unwrap();
    // Should fail with non-zero exit code
    assert!(
        r.stdout.trim().ends_with('1')
            || r.stdout.contains("not allowed")
            || r.stdout.contains("Permission denied"),
        "Symlink creation should be blocked, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn realfs_symlink_relative_escape_blocked() {
    let dir = setup_host_dir();
    let mut bash = Bash::builder()
        .mount_real_readwrite_at(dir.path(), "/mnt/workspace")
        .build();

    // Attempt relative path traversal via symlink
    let r = bash
        .exec("ln -s ../../../../etc/passwd /mnt/workspace/escape 2>&1; echo $?")
        .await
        .unwrap();
    assert!(
        r.stdout.trim().ends_with('1')
            || r.stdout.contains("not allowed")
            || r.stdout.contains("Permission denied"),
        "Relative symlink escape should be blocked, got: {}",
        r.stdout
    );
}
