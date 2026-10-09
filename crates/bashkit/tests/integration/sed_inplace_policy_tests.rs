//! `sed -i` / `yq -i` under a filesystem that refuses dot-path writes.
//!
//! Embedders (e.g. a read/write workspace policy) may deny any path segment
//! starting with `.`. The atomic-replace helper stages a hidden
//! `.bashkit-<tool>-<hex>.tmp` sibling; when that is refused it must fall back
//! to a direct write of the target instead of failing with the file unchanged.

use bashkit::{
    Bash, DirEntry, FileSystem, FileSystemExt, InMemoryFs, Metadata, Result, async_trait,
};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Refuses every operation whose final path component starts with `.`.
struct NoDotFs {
    inner: Arc<InMemoryFs>,
}

fn guard(path: &Path) -> Result<()> {
    let hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'));
    if hidden {
        return Err(IoError::new(ErrorKind::PermissionDenied, "dot paths are not allowed").into());
    }
    Ok(())
}

#[async_trait]
impl FileSystemExt for NoDotFs {}

#[async_trait]
impl FileSystem for NoDotFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        guard(path)?;
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        guard(path)?;
        self.inner.write_file(path, content).await
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        guard(path)?;
        self.inner.append_file(path, content).await
    }
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        guard(path)?;
        self.inner.mkdir(path, recursive).await
    }
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        guard(path)?;
        self.inner.remove(path, recursive).await
    }
    async fn stat(&self, path: &Path) -> Result<Metadata> {
        guard(path)?;
        self.inner.stat(path).await
    }
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.inner.read_dir(path).await
    }
    async fn exists(&self, path: &Path) -> Result<bool> {
        self.inner.exists(path).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        guard(from)?;
        guard(to)?;
        self.inner.rename(from, to).await
    }
    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        guard(from)?;
        guard(to)?;
        self.inner.copy(from, to).await
    }
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        guard(link)?;
        self.inner.symlink(target, link).await
    }
    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        guard(path)?;
        self.inner.read_link(path).await
    }
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        guard(path)?;
        self.inner.chmod(path, mode).await
    }
    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        guard(path)?;
        self.inner.set_modified_time(path, time).await
    }
}

async fn setup(path: &str, content: &[u8], mode: u32) -> (Arc<InMemoryFs>, Bash) {
    let inner = Arc::new(InMemoryFs::new());
    inner.mkdir(Path::new("/workspace"), true).await.unwrap();
    inner.write_file(Path::new(path), content).await.unwrap();
    inner.chmod(Path::new(path), mode).await.unwrap();
    let fs = Arc::new(NoDotFs {
        inner: inner.clone(),
    });
    let bash = Bash::builder()
        .fs(fs as Arc<dyn FileSystem>)
        .cwd("/workspace")
        .build();
    (inner, bash)
}

async fn no_temp_left(inner: &InMemoryFs) -> bool {
    inner
        .read_dir(Path::new("/workspace"))
        .await
        .unwrap()
        .iter()
        .all(|e| !e.name.starts_with(".bashkit-"))
}

#[tokio::test]
async fn sed_in_place_falls_back_when_dot_paths_are_refused() {
    let (inner, mut bash) = setup("/workspace/notes.txt", b"version = 1\n", 0o640).await;
    let r = bash
        .exec("sed -i 's/1/2/' notes.txt && cat notes.txt")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "version = 2\n");
    let path = Path::new("/workspace/notes.txt");
    assert_eq!(inner.stat(path).await.unwrap().mode, 0o640);
    assert!(no_temp_left(&inner).await);
}

#[tokio::test]
async fn sed_in_place_with_backup_falls_back_when_dot_paths_are_refused() {
    let (inner, mut bash) = setup("/workspace/a.txt", b"x\n", 0o644).await;
    let r = bash.exec("sed -i.bak 's/x/y/' a.txt").await.unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    let read = |p: &'static str| {
        let inner = inner.clone();
        async move { inner.read_file(Path::new(p)).await.unwrap() }
    };
    assert_eq!(read("/workspace/a.txt").await, b"y\n");
    assert_eq!(read("/workspace/a.txt.bak").await, b"x\n");
}

#[tokio::test]
async fn sed_in_place_on_refused_target_still_fails_cleanly() {
    // The fallback must not bypass the policy for the target itself.
    let (inner, mut bash) = setup("/workspace/.env", b"K=1\n", 0o600).await;
    let r = bash.exec("sed -i 's/1/2/' .env").await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert_eq!(
        inner.read_file(Path::new("/workspace/.env")).await.unwrap(),
        b"K=1\n"
    );
}

#[cfg(feature = "jq")]
#[tokio::test]
async fn yq_in_place_falls_back_when_dot_paths_are_refused() {
    let (inner, mut bash) = setup("/workspace/c.yml", b"name: old\n", 0o644).await;
    let r = bash.exec("yq -i '.name = \"new\"' c.yml").await.unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    let out = inner
        .read_file(Path::new("/workspace/c.yml"))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&out).contains("new"));
    assert!(no_temp_left(&inner).await);
}
