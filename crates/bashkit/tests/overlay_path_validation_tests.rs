//! Tests for OverlayFs path validation in all methods (issue #652)

use bashkit::{FileSystem, FsLimits, InMemoryFs, OverlayFs};
use std::path::Path;
use std::sync::Arc;

fn make_overlay() -> OverlayFs {
    let lower = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
    let limits = FsLimits::default();
    OverlayFs::with_limits(lower, limits)
}

// Unicode bidi override character (U+202E) in path
fn bidi_path() -> &'static str {
    "/tmp/evil\u{202E}txt"
}

// Path deeper than max_path_depth
fn deep_path() -> String {
    let mut p = String::from("/");
    for i in 0..200 {
        p.push_str(&format!("d{}/", i));
    }
    p.push_str("file.txt");
    p
}

#[tokio::test]
async fn overlayfs_read_file_rejects_bidi_path() {
    let fs = make_overlay();
    let result = fs.read_file(Path::new(bidi_path())).await;
    assert!(result.is_err(), "read_file should reject bidi path");
}

#[tokio::test]
async fn overlayfs_stat_rejects_bidi_path() {
    let fs = make_overlay();
    let result = fs.stat(Path::new(bidi_path())).await;
    assert!(result.is_err(), "stat should reject bidi path");
}

#[tokio::test]
async fn overlayfs_exists_rejects_bidi_path() {
    let fs = make_overlay();
    let result = fs.exists(Path::new(bidi_path())).await;
    assert!(result.is_err(), "exists should reject bidi path");
}

#[tokio::test]
async fn overlayfs_read_dir_rejects_bidi_path() {
    let fs = make_overlay();
    let result = fs.read_dir(Path::new(bidi_path())).await;
    assert!(result.is_err(), "read_dir should reject bidi path");
}

#[tokio::test]
async fn overlayfs_remove_rejects_bidi_path() {
    let fs = make_overlay();
    let result = fs.remove(Path::new(bidi_path()), false).await;
    assert!(result.is_err(), "remove should reject bidi path");
}

#[tokio::test]
async fn overlayfs_rename_rejects_bidi_source() {
    let fs = make_overlay();
    let result = fs
        .rename(Path::new(bidi_path()), Path::new("/tmp/ok.txt"))
        .await;
    assert!(result.is_err(), "rename should reject bidi source path");
}

#[tokio::test]
async fn overlayfs_rename_rejects_bidi_dest() {
    let fs = make_overlay();
    fs.write_file(Path::new("/tmp/src.txt"), b"data")
        .await
        .unwrap();
    let result = fs
        .rename(Path::new("/tmp/src.txt"), Path::new(bidi_path()))
        .await;
    assert!(result.is_err(), "rename should reject bidi dest path");
}

#[tokio::test]
async fn overlayfs_copy_rejects_bidi_source() {
    let fs = make_overlay();
    let result = fs
        .copy(Path::new(bidi_path()), Path::new("/tmp/ok.txt"))
        .await;
    assert!(result.is_err(), "copy should reject bidi source path");
}

#[tokio::test]
async fn overlayfs_read_link_rejects_bidi_path() {
    let fs = make_overlay();
    let result = fs.read_link(Path::new(bidi_path())).await;
    assert!(result.is_err(), "read_link should reject bidi path");
}

#[tokio::test]
async fn overlayfs_chmod_rejects_bidi_path() {
    let fs = make_overlay();
    let result = fs.chmod(Path::new(bidi_path()), 0o644).await;
    assert!(result.is_err(), "chmod should reject bidi path");
}

#[tokio::test]
async fn overlayfs_read_file_rejects_deep_path() {
    let fs = make_overlay();
    let result = fs.read_file(Path::new(&deep_path())).await;
    assert!(result.is_err(), "read_file should reject deep path");
}

#[tokio::test]
async fn overlayfs_stat_rejects_deep_path() {
    let fs = make_overlay();
    let result = fs.stat(Path::new(&deep_path())).await;
    assert!(result.is_err(), "stat should reject deep path");
}

// Normal paths still work
#[tokio::test]
async fn overlayfs_normal_paths_still_work() {
    let fs = make_overlay();
    fs.write_file(Path::new("/tmp/normal.txt"), b"hello")
        .await
        .unwrap();
    let content = fs.read_file(Path::new("/tmp/normal.txt")).await.unwrap();
    assert_eq!(content, b"hello");
    assert!(fs.exists(Path::new("/tmp/normal.txt")).await.unwrap());
    assert!(fs.stat(Path::new("/tmp/normal.txt")).await.is_ok());
}
