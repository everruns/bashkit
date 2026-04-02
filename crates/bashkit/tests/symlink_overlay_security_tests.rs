//! Security tests for symlink handling in overlay mode.
//!
//! THREAT[TM-ESC-002]: Validates that symlinks cannot be used to escape
//! mount boundaries, especially after rename/move operations.

use bashkit::{Bash, FileSystem, InMemoryFs, OverlayFs};
use std::path::Path;
use std::sync::Arc;

/// Renaming a symlink in overlay mode must preserve it as a symlink
/// (not silently fail or convert to a regular file).
#[tokio::test]
async fn overlay_rename_preserves_symlink() {
    let lower = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
    let overlay = Arc::new(OverlayFs::new(lower));

    // Create a file and a symlink to it
    overlay
        .write_file(Path::new("/target.txt"), b"hello")
        .await
        .unwrap();
    overlay
        .symlink(Path::new("/target.txt"), Path::new("/link1"))
        .await
        .unwrap();

    // Rename the symlink
    overlay
        .rename(Path::new("/link1"), Path::new("/link2"))
        .await
        .unwrap();

    // The renamed entry should still be a symlink
    let target = overlay.read_link(Path::new("/link2")).await.unwrap();
    assert_eq!(target, Path::new("/target.txt"));

    // Original should be gone
    assert!(!overlay.exists(Path::new("/link1")).await.unwrap());
}

/// Renaming a symlink from the lower layer into the upper layer must
/// preserve it as a symlink in the upper layer.
#[tokio::test]
async fn overlay_rename_symlink_from_lower_layer() {
    let lower = Arc::new(InMemoryFs::new());
    lower
        .write_file(Path::new("/data.txt"), b"secret")
        .await
        .unwrap();
    lower
        .symlink(Path::new("/data.txt"), Path::new("/link_lower"))
        .await
        .unwrap();

    let overlay = Arc::new(OverlayFs::new(lower.clone() as Arc<dyn FileSystem>));

    // Rename symlink from lower to a new name (goes to upper)
    overlay
        .rename(Path::new("/link_lower"), Path::new("/link_upper"))
        .await
        .unwrap();

    // Should be a symlink in the overlay
    let target = overlay.read_link(Path::new("/link_upper")).await.unwrap();
    assert_eq!(target, Path::new("/data.txt"));
}

/// Copying a symlink in overlay mode should copy it as a symlink.
#[tokio::test]
async fn overlay_copy_preserves_symlink() {
    let lower = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
    let overlay = Arc::new(OverlayFs::new(lower));

    overlay
        .write_file(Path::new("/target.txt"), b"data")
        .await
        .unwrap();
    overlay
        .symlink(Path::new("/target.txt"), Path::new("/link"))
        .await
        .unwrap();

    overlay
        .copy(Path::new("/link"), Path::new("/link_copy"))
        .await
        .unwrap();

    // Copy should be a symlink with same target
    let target = overlay.read_link(Path::new("/link_copy")).await.unwrap();
    assert_eq!(target, Path::new("/target.txt"));

    // Original still exists
    let orig_target = overlay.read_link(Path::new("/link")).await.unwrap();
    assert_eq!(orig_target, Path::new("/target.txt"));
}

/// A symlink pointing outside a mount should not allow reading the external
/// file through cat after being moved within the mount.
#[tokio::test]
async fn symlink_rename_cannot_escape_mount_via_read() {
    // Lower layer has /etc/passwd (simulating files outside the sandbox scope)
    let lower = Arc::new(InMemoryFs::new());
    lower.mkdir(Path::new("/etc"), false).await.unwrap();
    lower
        .write_file(Path::new("/etc/passwd"), b"root:x:0:0")
        .await
        .unwrap();
    lower.mkdir(Path::new("/sandbox"), false).await.unwrap();

    let overlay = Arc::new(OverlayFs::new(lower as Arc<dyn FileSystem>));

    // Create a symlink inside the overlay pointing to /etc/passwd
    overlay
        .symlink(Path::new("/etc/passwd"), Path::new("/sandbox/evil"))
        .await
        .unwrap();

    // Rename symlink within the sandbox
    overlay
        .rename(Path::new("/sandbox/evil"), Path::new("/sandbox/moved"))
        .await
        .unwrap();

    // The renamed symlink should exist and point to /etc/passwd
    let target = overlay
        .read_link(Path::new("/sandbox/moved"))
        .await
        .unwrap();
    assert_eq!(target, Path::new("/etc/passwd"));

    // But reading through read_file must NOT follow the symlink and return content
    // (symlinks are intentionally not followed — TM-ESC-002)
    let result = overlay.read_file(Path::new("/sandbox/moved")).await;
    assert!(result.is_err(), "read_file on symlink must not follow it");
}

/// mv of a symlink in a bash session should work and preserve the symlink.
#[tokio::test]
async fn bash_mv_symlink_in_overlay() {
    let lower = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
    let overlay = Arc::new(OverlayFs::new(lower));

    overlay
        .write_file(Path::new("/tmp/target.txt"), b"content")
        .await
        .unwrap();
    overlay.mkdir(Path::new("/tmp"), true).await.unwrap_or(()); // may already exist

    let mut bash = Bash::builder().fs(overlay.clone()).build();

    bash.exec("ln -s /tmp/target.txt /tmp/mylink")
        .await
        .unwrap();
    let result = bash.exec("mv /tmp/mylink /tmp/renamed_link").await.unwrap();
    assert_eq!(result.exit_code, 0, "mv should succeed: {}", result.stderr);

    // readlink should show the original target
    let result = bash.exec("readlink /tmp/renamed_link").await.unwrap();
    assert_eq!(result.stdout.trim(), "/tmp/target.txt");
}
