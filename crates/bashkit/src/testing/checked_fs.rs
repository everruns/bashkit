//! Path-recording filesystem: checks the separator contract on every call.
//!
//! Decisions:
//! - The VFS is POSIX-rooted on every host (TM-ESC-033): every path the
//!   interpreter, builtins and wrapper filesystems hand to a `FileSystem`
//!   must use `/` only. `Path::join`/`PathBuf::push` silently join with `\`
//!   on Windows, so `/a` + `b` becomes `/a\b` (#2425, #2933). The bug is
//!   invisible on Linux and to every backend that normalizes, so it only
//!   shows up as a custom backend's key mismatch on Windows.
//! - `CheckedFs` records every path it receives and reports the ones that
//!   contain `\`. Tests that never put `\` in a file name can then assert the
//!   list is empty; on the Windows CI runner that catches host-separator
//!   joins anywhere above the filesystem. The `clippy::disallowed_methods`
//!   ban in `crates/bashkit/clippy.toml` is the matching static check.
//! - Records, never panics: a panic inside a builtin is caught and turned
//!   into an error, which would hide the offending path.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::error::Result;
use crate::fs::{
    DirEntry, DirListing, FileSystem, FileSystemExt, FsLimits, FsUsage, InMemoryFs, Metadata,
    SearchCapable, VfsSnapshot, WalkOptions, walk_with_read_dir,
};
use crate::time_compat::SystemTime;

/// Cap on recorded paths, so a long fuzz run cannot grow without bound.
const MAX_RECORDED: usize = 100_000;

/// A [`FileSystem`] wrapper that records every path it is handed.
pub struct CheckedFs {
    inner: Arc<dyn FileSystem>,
    seen: Arc<Mutex<Vec<(&'static str, String)>>>,
}

impl CheckedFs {
    /// Record calls into a fresh [`InMemoryFs`].
    pub fn new() -> Self {
        Self::wrap(Arc::new(InMemoryFs::new()))
    }

    /// Record calls into `inner`.
    pub fn wrap(inner: Arc<dyn FileSystem>) -> Self {
        Self {
            inner,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Every `(operation, path)` received so far, in call order.
    pub fn seen(&self) -> Vec<(&'static str, String)> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Received paths that contain `\`, the host separator on Windows.
    pub fn separator_violations(&self) -> Vec<(&'static str, String)> {
        self.seen()
            .into_iter()
            .filter(|(_, p)| p.contains('\\'))
            .collect()
    }

    /// Panic with the offending calls if any path contained `\`.
    #[track_caller]
    pub fn assert_slash_only(&self, ctx: &str) {
        let bad = self.separator_violations();
        assert!(
            bad.is_empty(),
            "[{ctx}] filesystem received host-separator paths (use vfs_join, not Path::join/PathBuf::push): {bad:?}" // debug-ok: test assertion
        );
    }

    /// Forget recorded paths.
    pub fn reset(&self) {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn record(&self, op: &'static str, path: &Path) {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() < MAX_RECORDED {
            seen.push((op, path.to_string_lossy().into_owned()));
        }
    }
}

impl Default for CheckedFs {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileSystemExt for CheckedFs {
    fn usage(&self) -> FsUsage {
        self.inner.usage()
    }
    async fn mkfifo(&self, path: &Path, mode: u32) -> Result<()> {
        self.record("mkfifo", path);
        self.inner.mkfifo(path, mode).await
    }
    fn limits(&self) -> FsLimits {
        self.inner.limits()
    }
    fn vfs_snapshot(&self) -> Option<VfsSnapshot> {
        self.inner.vfs_snapshot()
    }
    fn vfs_restore(&self, snapshot: &VfsSnapshot) -> Result<()> {
        self.inner.vfs_restore(snapshot)
    }
    fn backend_kind(&self) -> &'static str {
        self.inner.backend_kind()
    }
}

#[async_trait]
impl FileSystem for CheckedFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.record("read_file", path);
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.record("write_file", path);
        self.inner.write_file(path, content).await
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.record("append_file", path);
        self.inner.append_file(path, content).await
    }
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        self.record("mkdir", path);
        self.inner.mkdir(path, recursive).await
    }
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.record("remove", path);
        self.inner.remove(path, recursive).await
    }
    async fn stat(&self, path: &Path) -> Result<Metadata> {
        self.record("stat", path);
        self.inner.stat(path).await
    }
    async fn lstat(&self, path: &Path) -> Result<Metadata> {
        self.record("lstat", path);
        self.inner.lstat(path).await
    }
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.record("read_dir", path);
        self.inner.read_dir(path).await
    }
    async fn exists(&self, path: &Path) -> Result<bool> {
        self.record("exists", path);
        self.inner.exists(path).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.record("rename", from);
        self.record("rename", to);
        self.inner.rename(from, to).await
    }
    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.record("copy", from);
        self.record("copy", to);
        self.inner.copy(from, to).await
    }
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        // `target` is stored verbatim, not resolved: only `link` is a VFS path.
        self.record("symlink", link);
        self.inner.symlink(target, link).await
    }
    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.record("read_link", path);
        self.inner.read_link(path).await
    }
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.record("chmod", path);
        self.inner.chmod(path, mode).await
    }
    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        self.record("set_modified_time", path);
        self.inner.set_modified_time(path, time).await
    }
    async fn walk(&self, root: &Path, opts: WalkOptions) -> Result<Vec<DirListing>> {
        self.record("walk", root);
        // Walk through `self` so each listed directory is recorded too.
        walk_with_read_dir(self, root, opts).await
    }
    fn as_search_capable(&self) -> Option<&dyn SearchCapable> {
        self.inner.as_search_capable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_paths_and_flags_backslashes() {
        let fs = CheckedFs::new();
        fs.mkdir(Path::new("/a"), false).await.unwrap();
        fs.write_file(Path::new("/a/f"), b"x").await.unwrap();
        assert!(fs.separator_violations().is_empty());
        assert_eq!(fs.seen()[0], ("mkdir", "/a".to_string()));

        let _ = fs.stat(Path::new("/a\\f")).await;
        assert_eq!(
            fs.separator_violations(),
            vec![("stat", "/a\\f".to_string())]
        );
        fs.reset();
        assert!(fs.seen().is_empty());
    }
}
