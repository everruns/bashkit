//! Tests for /dev/null support
//!
//! /dev/null is handled at the interpreter level to ensure it cannot be
//! bypassed by custom filesystem implementations, overlays, or mount points.
//!
//! Security invariant: Redirects to /dev/null NEVER reach the filesystem layer.

use bashkit::{
    async_trait, Bash, DirEntry, Error, FileSystem, FileType, InMemoryFs, Metadata, MountableFs,
    OverlayFs, Result,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

// =============================================================================
// BASIC /dev/null OUTPUT TESTS
// =============================================================================

#[tokio::test]
async fn test_dev_null_stdout_redirect() {
    let mut bash = Bash::builder().build();
    let result = bash.exec("echo hello > /dev/null").await.unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_stderr_redirect() {
    let mut bash = Bash::builder().build();
    // Use a command that produces stderr
    let result = bash.exec("echo error >&2 2>/dev/null").await.unwrap();
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_append_stdout() {
    let mut bash = Bash::builder().build();
    let result = bash.exec("echo hello >> /dev/null").await.unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_append_stderr() {
    let mut bash = Bash::builder().build();
    let result = bash.exec("echo error >&2 2>>/dev/null").await.unwrap();
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_output_both() {
    let mut bash = Bash::builder().build();
    // &> redirects both stdout and stderr - test with a simple echo
    let result = bash.exec("echo test &>/dev/null").await.unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
}

// =============================================================================
// /dev/null INPUT TESTS
// =============================================================================

#[tokio::test]
async fn test_dev_null_input_redirect() {
    let mut bash = Bash::builder().build();
    // Reading from /dev/null should give EOF (empty input)
    let result = bash.exec("cat < /dev/null").await.unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_input_with_wc() {
    let mut bash = Bash::builder().build();
    // wc -l on empty input should be 0
    let result = bash.exec("wc -l < /dev/null").await.unwrap();
    assert_eq!(result.stdout.trim(), "0");
}

// =============================================================================
// PATH NORMALIZATION BYPASS ATTEMPTS
// =============================================================================

#[tokio::test]
async fn test_dev_null_path_normalization_parent_dir() {
    let mut bash = Bash::builder().build();
    // Attempt to bypass via /dev/../dev/null
    let result = bash.exec("echo test > /dev/../dev/null").await.unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_path_normalization_current_dir() {
    let mut bash = Bash::builder().build();
    // Attempt to bypass via /dev/./null
    let result = bash.exec("echo test > /dev/./null").await.unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_path_normalization_complex() {
    let mut bash = Bash::builder().build();
    // Complex path normalization attempt
    let result = bash
        .exec("echo test > /tmp/../dev/../dev/./null")
        .await
        .unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

// =============================================================================
// SECURITY: CUSTOM FILESYSTEM CANNOT INTERCEPT /dev/null
// =============================================================================

/// A filesystem that tracks all write operations.
/// Used to verify /dev/null writes NEVER reach the filesystem.
struct TrackingFs {
    inner: InMemoryFs,
    write_count: AtomicUsize,
    dev_null_intercepted: AtomicBool,
    writes: RwLock<Vec<PathBuf>>,
}

impl TrackingFs {
    fn new() -> Self {
        Self {
            inner: InMemoryFs::new(),
            write_count: AtomicUsize::new(0),
            dev_null_intercepted: AtomicBool::new(false),
            writes: RwLock::new(Vec::new()),
        }
    }

    fn get_write_count(&self) -> usize {
        self.write_count.load(Ordering::SeqCst)
    }

    fn was_dev_null_intercepted(&self) -> bool {
        self.dev_null_intercepted.load(Ordering::SeqCst)
    }

    fn get_writes(&self) -> Vec<PathBuf> {
        self.writes.read().unwrap().clone()
    }
}

#[async_trait]
impl FileSystem for TrackingFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        self.writes.write().unwrap().push(path.to_path_buf());

        // Check if /dev/null was intercepted (security violation!)
        let normalized = normalize_path(path);
        if normalized == Path::new("/dev/null") {
            self.dev_null_intercepted.store(true, Ordering::SeqCst);
        }

        self.inner.write_file(path, content).await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        self.writes.write().unwrap().push(path.to_path_buf());

        let normalized = normalize_path(path);
        if normalized == Path::new("/dev/null") {
            self.dev_null_intercepted.store(true, Ordering::SeqCst);
        }

        self.inner.append_file(path, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        self.inner.mkdir(path, recursive).await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.inner.remove(path, recursive).await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        self.inner.stat(path).await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.inner.read_dir(path).await
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        self.inner.exists(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.inner.rename(from, to).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.inner.copy(from, to).await
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.inner.symlink(target, link).await
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.inner.read_link(path).await
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.inner.chmod(path, mode).await
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir => result.push("/"),
            std::path::Component::Normal(name) => result.push(name),
            std::path::Component::ParentDir => {
                result.pop();
            }
            _ => {}
        }
    }
    if result.as_os_str().is_empty() {
        result.push("/");
    }
    result
}

#[tokio::test]
async fn test_custom_fs_cannot_intercept_dev_null_output() {
    let fs = Arc::new(TrackingFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    // Write to /dev/null
    bash.exec("echo test > /dev/null").await.unwrap();

    // Verify /dev/null was NOT intercepted by the filesystem
    assert!(
        !fs.was_dev_null_intercepted(),
        "Security violation: /dev/null write reached filesystem!"
    );
}

#[tokio::test]
async fn test_custom_fs_cannot_intercept_dev_null_append() {
    let fs = Arc::new(TrackingFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    // Append to /dev/null
    bash.exec("echo test >> /dev/null").await.unwrap();

    // Verify /dev/null was NOT intercepted
    assert!(
        !fs.was_dev_null_intercepted(),
        "Security violation: /dev/null append reached filesystem!"
    );
}

#[tokio::test]
async fn test_custom_fs_cannot_intercept_dev_null_stderr() {
    let fs = Arc::new(TrackingFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    // Redirect stderr to /dev/null
    bash.exec("echo error >&2 2>/dev/null").await.unwrap();

    // Verify /dev/null was NOT intercepted
    assert!(
        !fs.was_dev_null_intercepted(),
        "Security violation: /dev/null stderr redirect reached filesystem!"
    );
}

#[tokio::test]
async fn test_custom_fs_cannot_intercept_dev_null_with_normalization() {
    let fs = Arc::new(TrackingFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    // Try various path normalization bypasses
    bash.exec("echo test > /dev/../dev/null").await.unwrap();
    bash.exec("echo test > /dev/./null").await.unwrap();
    bash.exec("echo test > /tmp/../dev/null").await.unwrap();

    // None should have reached the filesystem
    assert!(
        !fs.was_dev_null_intercepted(),
        "Security violation: normalized /dev/null path reached filesystem!"
    );
}

#[tokio::test]
async fn test_regular_file_writes_still_work() {
    let fs = Arc::new(TrackingFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    // Write to a regular file
    bash.exec("echo hello > /tmp/test.txt").await.unwrap();

    // This SHOULD have reached the filesystem
    assert!(fs.get_write_count() > 0, "Regular writes should work");
    assert!(
        fs.get_writes().contains(&PathBuf::from("/tmp/test.txt")),
        "Write to /tmp/test.txt should be tracked"
    );
}

// =============================================================================
// OVERLAY FILESYSTEM CANNOT BYPASS /dev/null
// =============================================================================

#[tokio::test]
async fn test_overlay_fs_cannot_bypass_dev_null() {
    let base = Arc::new(TrackingFs::new());
    let overlay = Arc::new(OverlayFs::new(base.clone()));
    let mut bash = Bash::builder().fs(overlay).build();

    // Write to /dev/null through overlay
    bash.exec("echo test > /dev/null").await.unwrap();

    // Base filesystem should NOT have seen the write
    assert!(
        !base.was_dev_null_intercepted(),
        "Security violation: /dev/null bypassed through OverlayFs!"
    );
}

#[tokio::test]
async fn test_overlay_fs_with_path_normalization() {
    let base = Arc::new(TrackingFs::new());
    let overlay = Arc::new(OverlayFs::new(base.clone()));
    let mut bash = Bash::builder().fs(overlay).build();

    // Try normalization bypass through overlay
    bash.exec("echo test > /dev/../dev/null").await.unwrap();

    assert!(
        !base.was_dev_null_intercepted(),
        "Security violation: normalized /dev/null bypassed through OverlayFs!"
    );
}

// =============================================================================
// MOUNTABLE FILESYSTEM CANNOT BYPASS /dev/null
// =============================================================================

/// A malicious filesystem that tries to intercept /dev/null
struct MaliciousDevFs {
    intercepted: AtomicBool,
}

impl MaliciousDevFs {
    fn new() -> Self {
        Self {
            intercepted: AtomicBool::new(false),
        }
    }

    fn was_intercepted(&self) -> bool {
        self.intercepted.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl FileSystem for MaliciousDevFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        if path.ends_with("null") {
            self.intercepted.store(true, Ordering::SeqCst);
        }
        Ok(vec![])
    }

    async fn write_file(&self, path: &Path, _content: &[u8]) -> Result<()> {
        if path.ends_with("null") {
            self.intercepted.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    async fn append_file(&self, path: &Path, _content: &[u8]) -> Result<()> {
        if path.ends_with("null") {
            self.intercepted.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    async fn mkdir(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Ok(())
    }

    async fn remove(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        Ok(Metadata {
            file_type: if path.ends_with("null") {
                FileType::File
            } else {
                FileType::Directory
            },
            size: 0,
            mode: 0o666,
            modified: SystemTime::now(),
            created: SystemTime::now(),
        })
    }

    async fn read_dir(&self, _path: &Path) -> Result<Vec<DirEntry>> {
        Ok(vec![DirEntry {
            name: "null".to_string(),
            metadata: Metadata {
                file_type: FileType::File,
                size: 0,
                mode: 0o666,
                modified: SystemTime::now(),
                created: SystemTime::now(),
            },
        }])
    }

    async fn exists(&self, _path: &Path) -> Result<bool> {
        Ok(true)
    }

    async fn rename(&self, _from: &Path, _to: &Path) -> Result<()> {
        Ok(())
    }

    async fn copy(&self, _from: &Path, _to: &Path) -> Result<()> {
        Ok(())
    }

    async fn symlink(&self, _target: &Path, _link: &Path) -> Result<()> {
        Ok(())
    }

    async fn read_link(&self, _path: &Path) -> Result<PathBuf> {
        Err(Error::Io(std::io::Error::other("not a symlink")))
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_mounted_dev_fs_cannot_intercept_dev_null() {
    let root = Arc::new(InMemoryFs::new());
    let malicious_dev = Arc::new(MaliciousDevFs::new());

    let mountable = MountableFs::new(root);
    // Mount malicious filesystem at /dev
    mountable.mount("/dev", malicious_dev.clone()).unwrap();

    let mut bash = Bash::builder().fs(Arc::new(mountable)).build();

    // Write to /dev/null - should NOT reach the mounted filesystem
    bash.exec("echo test > /dev/null").await.unwrap();

    assert!(
        !malicious_dev.was_intercepted(),
        "Security violation: /dev/null was intercepted by mounted filesystem!"
    );
}

#[tokio::test]
async fn test_mounted_dev_fs_cannot_intercept_normalized_paths() {
    let root = Arc::new(InMemoryFs::new());
    let malicious_dev = Arc::new(MaliciousDevFs::new());

    let mountable = MountableFs::new(root);
    mountable.mount("/dev", malicious_dev.clone()).unwrap();

    let mut bash = Bash::builder().fs(Arc::new(mountable)).build();

    // Try path normalization bypasses
    bash.exec("echo test > /dev/../dev/null").await.unwrap();
    bash.exec("echo test > /tmp/../dev/null").await.unwrap();

    assert!(
        !malicious_dev.was_intercepted(),
        "Security violation: normalized /dev/null paths were intercepted!"
    );
}

// =============================================================================
// COMBINED REDIRECTIONS
// =============================================================================

#[tokio::test]
async fn test_dev_null_with_other_redirects() {
    let mut bash = Bash::builder().build();

    // stdout to file, stderr to /dev/null
    let _result = bash
        .exec("echo out; echo err >&2; echo out > /tmp/out.txt 2>/dev/null")
        .await
        .unwrap();

    // Read the file to verify stdout went there
    let content = bash.exec("cat /tmp/out.txt").await.unwrap();
    assert_eq!(content.stdout.trim(), "out");
}

#[tokio::test]
async fn test_multiple_dev_null_redirects() {
    let mut bash = Bash::builder().build();

    // Both stdout and stderr to /dev/null separately
    let result = bash
        .exec("echo out >/dev/null 2>/dev/null; echo done")
        .await
        .unwrap();
    assert_eq!(result.stdout, "done\n");
}

// =============================================================================
// EDGE CASES
// =============================================================================

#[tokio::test]
async fn test_dev_null_empty_output() {
    let mut bash = Bash::builder().build();
    // Command with no output to /dev/null
    let result = bash.exec("true > /dev/null").await.unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_large_output() {
    let mut bash = Bash::builder().build();
    // Generate larger output and discard
    let result = bash
        .exec("echo 'line1 line2 line3 line4 line5 line6 line7 line8 line9 line10' > /dev/null")
        .await
        .unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_dev_null_in_pipeline() {
    let mut bash = Bash::builder().build();
    // /dev/null in a pipeline context
    let result = bash
        .exec("echo hello | cat > /dev/null; echo done")
        .await
        .unwrap();
    assert_eq!(result.stdout, "done\n");
}

#[tokio::test]
async fn test_dev_null_in_subshell() {
    let mut bash = Bash::builder().build();
    let result = bash.exec("(echo hidden > /dev/null); echo visible").await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert_eq!(result.stdout, "visible\n");
}
