//! WASM-specific smoke tests for bashkit platform-compatible time types.
//!
//! Run with: wasm-pack test --node
//!     or: wasm-pack test --headless --chrome

use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

/// Polyfill IndexedDB in Node.js test environments.
#[wasm_bindgen_test]
fn setup_fake_indexeddb() {
    let _ = js_sys::eval(
        "try { if (typeof indexedDB === 'undefined' && typeof require !== 'undefined') { require('fake-indexeddb/auto'); } } catch(e) {}",
    );
}

#[wasm_bindgen_test]
fn system_time_now_does_not_panic() {
    let _now = bashkit::time::SystemTime::now();
}

#[wasm_bindgen_test]
fn unix_epoch_is_before_now() {
    let epoch = bashkit::time::UNIX_EPOCH;
    let now = bashkit::time::SystemTime::now();
    let duration = now
        .duration_since(epoch)
        .expect("now should be after epoch");
    assert!(duration.as_secs() > 1_000_000_000, "expected year 2001+");
}

#[wasm_bindgen_test]
fn chrono_roundtrip_utc() {
    let now = bashkit::time::SystemTime::now();
    let dt = bashkit::time::to_chrono_utc(now);
    let back = bashkit::time::from_chrono(dt);

    let diff = now
        .duration_since(back)
        .unwrap_or_else(|e| e.duration())
        .as_millis();
    assert!(diff < 2, "roundtrip drift should be < 2ms, got {}ms", diff);
}

#[wasm_bindgen_test]
fn duration_arithmetic() {
    let a = bashkit::time::Duration::from_secs(10);
    let b = bashkit::time::Duration::from_secs(5);
    assert_eq!((a + b).as_secs(), 15);
}

// ---------------------------------------------------------------------------
// IndexedDB filesystem tests
// ---------------------------------------------------------------------------

use bashkit::{FileSystem, FsBackend, IndexedDbFs, PosixFs};
use std::path::Path;
use std::sync::Arc;

/// Unique DB name per test to avoid collisions.
fn db_name(test: &str) -> String {
    format!("bashkit_test_{}", test)
}

#[wasm_bindgen_test]
async fn indexeddb_fs_write_and_read_file() {
    let fs = IndexedDbFs::new(db_name("write_read"));
    fs.write(Path::new("/tmp/test.txt"), b"hello world")
        .await
        .unwrap();
    let content = fs.read(Path::new("/tmp/test.txt")).await.unwrap();
    assert_eq!(content, b"hello world");
}

#[wasm_bindgen_test]
async fn indexeddb_fs_append_file() {
    let fs = IndexedDbFs::new(db_name("append"));
    fs.write(Path::new("/tmp/log.txt"), b"line1\n")
        .await
        .unwrap();
    fs.append(Path::new("/tmp/log.txt"), b"line2\n")
        .await
        .unwrap();
    let content = fs.read(Path::new("/tmp/log.txt")).await.unwrap();
    assert_eq!(content, b"line1\nline2\n");
}

#[wasm_bindgen_test]
async fn indexeddb_fs_mkdir_and_exists() {
    let fs = IndexedDbFs::new(db_name("mkdir"));
    fs.mkdir(Path::new("/data/nested"), true).await.unwrap();
    assert!(fs.exists(Path::new("/data")).await.unwrap());
    assert!(fs.exists(Path::new("/data/nested")).await.unwrap());
    assert!(!fs.exists(Path::new("/data/nested/missing")).await.unwrap());
}

#[wasm_bindgen_test]
async fn indexeddb_fs_read_dir() {
    let fs = IndexedDbFs::new(db_name("read_dir"));
    fs.mkdir(Path::new("/tmp/sub"), true).await.unwrap();
    fs.write(Path::new("/tmp/a.txt"), b"a").await.unwrap();
    fs.write(Path::new("/tmp/b.txt"), b"b").await.unwrap();

    let entries = fs.read_dir(Path::new("/tmp")).await.unwrap();
    let mut names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["a.txt", "b.txt", "sub"]);
}

#[wasm_bindgen_test]
async fn indexeddb_fs_remove_file() {
    let fs = IndexedDbFs::new(db_name("remove_file"));
    fs.write(Path::new("/tmp/del.txt"), b"x").await.unwrap();
    assert!(fs.exists(Path::new("/tmp/del.txt")).await.unwrap());
    fs.remove(Path::new("/tmp/del.txt"), false).await.unwrap();
    assert!(!fs.exists(Path::new("/tmp/del.txt")).await.unwrap());
}

#[wasm_bindgen_test]
async fn indexeddb_fs_remove_dir_recursive() {
    let fs = IndexedDbFs::new(db_name("remove_dir"));
    fs.mkdir(Path::new("/tmp/deep/nested"), true).await.unwrap();
    fs.write(Path::new("/tmp/deep/file.txt"), b"x")
        .await
        .unwrap();
    fs.remove(Path::new("/tmp/deep"), true).await.unwrap();
    assert!(!fs.exists(Path::new("/tmp/deep")).await.unwrap());
    assert!(!fs.exists(Path::new("/tmp/deep/nested")).await.unwrap());
    assert!(!fs.exists(Path::new("/tmp/deep/file.txt")).await.unwrap());
}

#[wasm_bindgen_test]
async fn indexeddb_fs_stat() {
    let fs = IndexedDbFs::new(db_name("stat"));
    fs.write(Path::new("/tmp/stats.txt"), b"12345")
        .await
        .unwrap();
    let meta = fs.stat(Path::new("/tmp/stats.txt")).await.unwrap();
    assert!(meta.file_type.is_file());
    assert_eq!(meta.size, 5);
    assert_eq!(meta.mode, 0o644);
}

#[wasm_bindgen_test]
async fn indexeddb_fs_rename_file() {
    let fs = IndexedDbFs::new(db_name("rename_file"));
    fs.write(Path::new("/tmp/old.txt"), b"data").await.unwrap();
    fs.rename(Path::new("/tmp/old.txt"), Path::new("/tmp/new.txt"))
        .await
        .unwrap();
    assert!(!fs.exists(Path::new("/tmp/old.txt")).await.unwrap());
    assert!(fs.exists(Path::new("/tmp/new.txt")).await.unwrap());
    assert_eq!(fs.read(Path::new("/tmp/new.txt")).await.unwrap(), b"data");
}

#[wasm_bindgen_test]
async fn indexeddb_fs_copy_file() {
    let fs = IndexedDbFs::new(db_name("copy_file"));
    fs.write(Path::new("/tmp/src.txt"), b"copy me")
        .await
        .unwrap();
    fs.copy(Path::new("/tmp/src.txt"), Path::new("/tmp/dst.txt"))
        .await
        .unwrap();
    assert_eq!(
        fs.read(Path::new("/tmp/src.txt")).await.unwrap(),
        b"copy me"
    );
    assert_eq!(
        fs.read(Path::new("/tmp/dst.txt")).await.unwrap(),
        b"copy me"
    );
}

#[wasm_bindgen_test]
async fn indexeddb_fs_symlink() {
    let fs = IndexedDbFs::new(db_name("symlink"));
    fs.symlink(Path::new("/tmp/target.txt"), Path::new("/tmp/link.txt"))
        .await
        .unwrap();
    let target = fs.read_link(Path::new("/tmp/link.txt")).await.unwrap();
    assert_eq!(target, Path::new("/tmp/target.txt"));
}

#[wasm_bindgen_test]
async fn indexeddb_fs_chmod() {
    let fs = IndexedDbFs::new(db_name("chmod"));
    fs.write(Path::new("/tmp/perms.txt"), b"x").await.unwrap();
    fs.chmod(Path::new("/tmp/perms.txt"), 0o755).await.unwrap();
    let meta = fs.stat(Path::new("/tmp/perms.txt")).await.unwrap();
    assert_eq!(meta.mode, 0o755);
}

#[wasm_bindgen_test]
async fn indexeddb_fs_posix_wrapper() {
    let backend = IndexedDbFs::new(db_name("posix"));
    let fs = Arc::new(PosixFs::new(backend));

    // Create parent dir first — IndexedDB fs doesn't auto-create parents
    fs.mkdir(Path::new("/tmp"), false).await.unwrap();

    // POSIX semantics: write -> read roundtrip
    fs.write_file(Path::new("/tmp/posix.txt"), b"posix")
        .await
        .unwrap();
    let content = fs.read_file(Path::new("/tmp/posix.txt")).await.unwrap();
    assert_eq!(content, b"posix");

    // POSIX semantics: cannot write to a directory
    fs.mkdir(Path::new("/tmp/dir"), false).await.unwrap();
    let result = fs.write_file(Path::new("/tmp/dir"), b"x").await;
    assert!(result.is_err(), "writing to a directory should fail");

    // verify_filesystem_requirements smoke test
    bashkit::verify_filesystem_requirements(&*fs).await.unwrap();
}
