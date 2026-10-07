//! Symlink-following view of the session filesystem.
//!
//! Decisions:
//! - [`FollowFs`] is the outermost layer of a session's filesystem stack, so
//!   resolution happens in the VFS namespace, after mounts and overlays are
//!   composed. A link's target is always a VFS path: an absolute target is
//!   resolved from the VFS root, `..` clamps there, and every resolved path
//!   goes back through the normal layers (mount tables, `RealFs` containment,
//!   read-only wrappers). THREAT[TM-ESC-002]: a link can therefore never name
//!   a host path, only another VFS path the script could already open.
//! - Inner layers keep their non-following semantics: their `stat` is an
//!   `lstat`. Only this layer follows, with POSIX rules: every operation that
//!   opens a path follows links in all components; `lstat`, `read_link`,
//!   `remove`, `rename` and `symlink` follow only the parent components.
//! - THREAT[TM-DOS-011]: at most [`MAX_SYMLINK_HOPS`] links per lookup (Linux
//!   `MAXSYMLINKS`); more fail with "Too many levels of symbolic links".
//! - Fast path: operations run against the inner filesystem first and only
//!   resolve links when that fails (inner layers refuse to read or write
//!   through a link) or when `stat` reports a link, so scripts that never
//!   create links pay nothing. Writes check the final component with one
//!   `lstat` so they never replace a link with a file.
//! - Permission and resource-limit errors are returned as-is, never retried
//!   through link resolution, so containment errors stay visible.
//! - `..` in a path is resolved lexically by the interpreter before it reaches
//!   the filesystem (`/link/..` is the link's parent, not the target's), the
//!   same logical view `cd` and `pwd` use.
//! - Turned off with `BashBuilder::follow_symlinks(false)`.

use async_trait::async_trait;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::limits::{FsLimits, FsUsage};
use super::traits::{DirEntry, FileSystem, FileSystemExt, Metadata};
use super::{SearchCapable, normalize_path};
use crate::error::{Error, Result};
use crate::time_compat::SystemTime;

/// Links followed per lookup before failing with ELOOP (Linux MAXSYMLINKS).
pub(crate) const MAX_SYMLINK_HOPS: usize = 40;

fn eloop() -> Error {
    IoError::other("Too many levels of symbolic links").into()
}

/// Errors that may be caused by a link somewhere on the path.
fn may_be_link_error(e: &Error) -> bool {
    matches!(e, Error::Io(io) if !matches!(
        io.kind(),
        ErrorKind::PermissionDenied | ErrorKind::Unsupported | ErrorKind::OutOfMemory
    ))
}

fn split(path: &Path) -> Vec<String> {
    normalize_path(path)
        .to_str()
        .unwrap_or("/")
        .split('/')
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect()
}

fn join(parts: &[String]) -> PathBuf {
    PathBuf::from(format!("/{}", parts.join("/")))
}

/// Filesystem wrapper that follows symbolic links.
pub(crate) struct FollowFs {
    inner: Arc<dyn FileSystem>,
}

impl FollowFs {
    pub(crate) fn new(inner: Arc<dyn FileSystem>) -> Self {
        Self { inner }
    }

    /// Resolve every link on `path`. With `follow_last == false` the final
    /// component is kept as-is (lstat semantics). Resolution stops at the
    /// first missing component; the rest of the path is appended unchanged.
    async fn resolve(&self, path: &Path, follow_last: bool) -> Result<PathBuf> {
        let mut pending: Vec<String> = split(path);
        pending.reverse();
        let mut done: Vec<String> = Vec::new();
        let mut hops = 0usize;
        while let Some(comp) = pending.pop() {
            match comp.as_str() {
                "." => continue,
                ".." => {
                    done.pop();
                    continue;
                }
                _ => {}
            }
            done.push(comp);
            if pending.is_empty() && !follow_last {
                break;
            }
            let here = join(&done);
            match self.inner.stat(&here).await {
                Ok(meta) if meta.file_type.is_symlink() => {
                    hops += 1;
                    if hops > MAX_SYMLINK_HOPS {
                        return Err(eloop());
                    }
                    let target = self.inner.read_link(&here).await?;
                    done.pop();
                    let target = target.to_str().unwrap_or_default();
                    if target.starts_with('/') {
                        done.clear();
                    }
                    pending.extend(
                        target
                            .split('/')
                            .filter(|c| !c.is_empty())
                            .rev()
                            .map(str::to_owned),
                    );
                }
                Ok(_) => {}
                Err(_) => {
                    while let Some(rest) = pending.pop() {
                        done.push(rest);
                    }
                    break;
                }
            }
        }
        Ok(join(&done))
    }

    /// Path with all links followed, or `None` when nothing changed.
    async fn followed(&self, path: &Path) -> Result<Option<PathBuf>> {
        let resolved = self.resolve(path, true).await?;
        Ok((resolved != normalize_path(path)).then_some(resolved))
    }

    /// Path with links followed in the parent components only.
    async fn parent_followed(&self, path: &Path) -> Result<Option<PathBuf>> {
        let resolved = self.resolve(path, false).await?;
        Ok((resolved != normalize_path(path)).then_some(resolved))
    }

    /// Path to write: the link's target when the final component is a link.
    async fn write_target(&self, path: &Path) -> Result<PathBuf> {
        match self.inner.stat(path).await {
            Ok(meta) if meta.file_type.is_symlink() => self.resolve(path, true).await,
            Ok(_) => Ok(path.to_path_buf()),
            Err(_) => Ok(self
                .parent_followed(path)
                .await?
                .unwrap_or_else(|| path.to_path_buf())),
        }
    }
}

/// Run `$op` on the inner fs with `$path`; on a link-related error, retry once
/// on the path with links resolved by `$resolver`.
macro_rules! retry_resolved {
    ($self:ident, $resolver:ident, $path:expr, |$p:ident| $op:expr) => {{
        let $p: &Path = $path;
        match $op {
            Err(e) if may_be_link_error(&e) => match $self.$resolver($p).await? {
                Some(resolved) => {
                    let $p: &Path = &resolved;
                    $op
                }
                None => Err(e),
            },
            other => other,
        }
    }};
}

#[async_trait]
impl FileSystemExt for FollowFs {
    fn usage(&self) -> FsUsage {
        self.inner.usage()
    }

    async fn mkfifo(&self, path: &Path, mode: u32) -> Result<()> {
        retry_resolved!(self, parent_followed, path, |p| self
            .inner
            .mkfifo(p, mode)
            .await)
    }

    fn limits(&self) -> FsLimits {
        self.inner.limits()
    }

    fn vfs_snapshot(&self) -> Option<super::VfsSnapshot> {
        self.inner.vfs_snapshot()
    }

    fn vfs_restore(&self, snapshot: &super::VfsSnapshot) -> Result<()> {
        self.inner.vfs_restore(snapshot)
    }

    fn backend_kind(&self) -> &'static str {
        self.inner.backend_kind()
    }
}

#[async_trait]
impl FileSystem for FollowFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        retry_resolved!(self, followed, path, |p| self.inner.read_file(p).await)
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let target = self.write_target(path).await?;
        self.inner.write_file(&target, content).await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let target = self.write_target(path).await?;
        self.inner.append_file(&target, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        // `mkdir -p` walks through linked directories; plain `mkdir` on an
        // existing link fails with "File exists" like Linux.
        if recursive {
            retry_resolved!(self, followed, path, |p| self.inner.mkdir(p, true).await)
        } else {
            retry_resolved!(self, parent_followed, path, |p| self
                .inner
                .mkdir(p, false)
                .await)
        }
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        retry_resolved!(self, parent_followed, path, |p| self
            .inner
            .remove(p, recursive)
            .await)
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        match self.inner.stat(path).await {
            Ok(meta) if meta.file_type.is_symlink() => {
                let resolved = self.resolve(path, true).await?;
                self.inner.stat(&resolved).await
            }
            Err(e) if may_be_link_error(&e) => match self.followed(path).await? {
                Some(resolved) => self.inner.stat(&resolved).await,
                None => Err(e),
            },
            other => other,
        }
    }

    async fn lstat(&self, path: &Path) -> Result<Metadata> {
        retry_resolved!(self, parent_followed, path, |p| self.inner.stat(p).await)
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        retry_resolved!(self, followed, path, |p| self.inner.read_dir(p).await)
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        // `test -e` follows links: a dangling link does not exist.
        match self.stat(path).await {
            Ok(_) => Ok(true),
            Err(e) if may_be_link_error(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        match self.inner.rename(from, to).await {
            Err(e) if may_be_link_error(&e) => {
                let from_r = self.parent_followed(from).await?;
                let to_r = self.parent_followed(to).await?;
                if from_r.is_none() && to_r.is_none() {
                    return Err(e);
                }
                self.inner
                    .rename(
                        from_r.as_deref().unwrap_or(from),
                        to_r.as_deref().unwrap_or(to),
                    )
                    .await
            }
            other => other,
        }
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from_r = self.resolve(from, true).await?;
        let to_r = self.write_target(to).await?;
        self.inner.copy(&from_r, &to_r).await
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        retry_resolved!(self, parent_followed, link, |p| self
            .inner
            .symlink(target, p)
            .await)
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        retry_resolved!(self, parent_followed, path, |p| self
            .inner
            .read_link(p)
            .await)
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        let resolved = self.resolve(path, true).await?;
        self.inner.chmod(&resolved, mode).await
    }

    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        let resolved = self.resolve(path, true).await?;
        self.inner.set_modified_time(&resolved, time).await
    }

    fn as_search_capable(&self) -> Option<&dyn SearchCapable> {
        self.inner.as_search_capable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;

    fn fs() -> FollowFs {
        FollowFs::new(Arc::new(InMemoryFs::new()))
    }

    #[tokio::test]
    async fn reads_and_writes_follow_links() {
        let fs = fs();
        fs.mkdir(Path::new("/d/real"), true).await.unwrap();
        fs.write_file(Path::new("/d/real/f"), b"x").await.unwrap();
        fs.symlink(Path::new("real"), Path::new("/d/link"))
            .await
            .unwrap();
        fs.symlink(Path::new("/d/link/f"), Path::new("/f2"))
            .await
            .unwrap();
        assert_eq!(fs.read_file(Path::new("/f2")).await.unwrap(), b"x");
        fs.append_file(Path::new("/f2"), b"y").await.unwrap();
        assert_eq!(fs.read_file(Path::new("/d/real/f")).await.unwrap(), b"xy");
        assert!(
            fs.stat(Path::new("/d/link"))
                .await
                .unwrap()
                .file_type
                .is_dir()
        );
        assert!(
            fs.lstat(Path::new("/d/link"))
                .await
                .unwrap()
                .file_type
                .is_symlink()
        );
        assert_eq!(fs.read_dir(Path::new("/d/link")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn loops_fail_with_eloop() {
        let fs = fs();
        fs.symlink(Path::new("/b"), Path::new("/a")).await.unwrap();
        fs.symlink(Path::new("/a"), Path::new("/b")).await.unwrap();
        let err = fs.read_file(Path::new("/a")).await.unwrap_err();
        assert!(err.to_string().contains("Too many levels"), "{err}");
        assert!(!fs.exists(Path::new("/a")).await.unwrap());
        // The link itself is still visible and removable.
        assert!(fs.lstat(Path::new("/a")).await.is_ok());
        fs.remove(Path::new("/a"), false).await.unwrap();
    }

    #[tokio::test]
    async fn absolute_targets_stay_in_vfs() {
        // THREAT[TM-ESC-002]: `..` past the root clamps at the VFS root.
        let fs = fs();
        fs.mkdir(Path::new("/tmp"), true).await.unwrap();
        fs.write_file(Path::new("/secret"), b"vfs").await.unwrap();
        fs.symlink(Path::new("../../../../secret"), Path::new("/tmp/l"))
            .await
            .unwrap();
        assert_eq!(fs.read_file(Path::new("/tmp/l")).await.unwrap(), b"vfs");
    }

    #[tokio::test]
    async fn dangling_write_creates_target_and_rm_keeps_target() {
        let fs = fs();
        fs.symlink(Path::new("/new"), Path::new("/l"))
            .await
            .unwrap();
        assert!(!fs.exists(Path::new("/l")).await.unwrap());
        fs.write_file(Path::new("/l"), b"made").await.unwrap();
        assert_eq!(fs.read_file(Path::new("/new")).await.unwrap(), b"made");
        fs.remove(Path::new("/l"), false).await.unwrap();
        assert!(fs.exists(Path::new("/new")).await.unwrap());
    }

    #[tokio::test]
    async fn hop_limit_allows_long_chains() {
        let fs = fs();
        fs.write_file(Path::new("/l0"), b"end").await.unwrap();
        for i in 1..=MAX_SYMLINK_HOPS {
            fs.symlink(
                Path::new(&format!("/l{}", i - 1)),
                Path::new(&format!("/l{i}")),
            )
            .await
            .unwrap();
        }
        let last = format!("/l{MAX_SYMLINK_HOPS}");
        assert_eq!(fs.read_file(Path::new(&last)).await.unwrap(), b"end");
        let over = format!("/l{}", MAX_SYMLINK_HOPS + 1);
        fs.symlink(Path::new(&last), Path::new(&over))
            .await
            .unwrap();
        assert!(fs.read_file(Path::new(&over)).await.is_err());
    }
}
