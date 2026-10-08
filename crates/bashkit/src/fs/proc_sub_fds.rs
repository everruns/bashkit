//! Per-shell `/dev/fd/N` namespace for process substitution.
//!
//! Decision: `<(cmd)` / `>(cmd)` expand to bash's `/dev/fd/63`, `/dev/fd/62`,
//! ... but the data never lives in the VFS. Several interpreters (tenants) can
//! share one filesystem; a shared `/dev/fd/63` file would let one read or
//! clobber another's substitution (TM-ISO-028). Each interpreter wraps its VFS
//! in [`ProcSubFs`], which answers `/dev/fd/N` (N >= 3) from this shell's own
//! fd table and never forwards such a path to the shared filesystem: an fd not
//! open in this shell is "No such file or directory", as on Linux.
//!
//! Fds 0-2 pass through (builtins see them via `std_streams`, redirects via
//! `dev_fd_alias`). The table is a stack: each simple command closes the
//! substitutions it opened (`ProcSubMark` in the interpreter). Background jobs
//! get a copy (fds are inherited at fork). Paths without `fd/` in them skip
//! the table without normalizing, so other VFS calls pay one byte scan.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;

use super::{
    DirEntry, FileSystem, FileSystemExt, FileType, FsLimits, FsUsage, Metadata, VfsSnapshot,
    fs_errors,
};
use crate::error::Result;
use crate::time_compat::{SystemTime, UNIX_EPOCH};

/// First fd bash hands out for a process substitution; later ones count down.
pub(crate) const PROC_SUB_FIRST_FD: i32 = 63;

/// What a command writes to a `>(cmd)` fd, fed to `cmd` afterwards.
pub(crate) type ProcSubBuffer = Arc<Mutex<Vec<u8>>>;

/// One open process substitution.
#[derive(Clone)]
pub(crate) enum ProcSubData {
    /// `<(cmd)`: the list's output, read by the command.
    Input(Arc<[u8]>),
    /// `>(cmd)`: what the command writes, fed to the list afterwards.
    Output(ProcSubBuffer),
}

pub(crate) struct ProcSubFs {
    inner: Arc<dyn FileSystem>,
    /// Open substitutions in allocation order.
    fds: Mutex<Vec<(i32, ProcSubData)>>,
}

/// What a path names in this shell's fd namespace.
enum Slot {
    /// Not `/dev/fd/N` with N >= 3: the VFS handles it.
    Vfs,
    Open(i32, ProcSubData),
    Closed,
}

/// `/dev/fd/N` with N >= 3 (after `.`/`..` normalization).
pub(crate) fn proc_sub_fd(path: &Path) -> Option<i32> {
    let raw = path.as_os_str().as_encoded_bytes();
    if !raw.windows(3).any(|w| w == b"fd/") {
        return None;
    }
    let normalized = super::normalize_path(path);
    let n = normalized.to_str()?.strip_prefix("/dev/fd/")?;
    if n.is_empty() || n.len() > 9 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fd: i32 = n.parse().ok()?;
    (fd >= 3).then_some(fd)
}

fn closed_error() -> crate::Error {
    fs_errors::not_found("No such file or directory")
}

fn not_permitted() -> crate::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "Operation not permitted",
    )
    .into()
}

impl ProcSubFs {
    pub(crate) fn new(inner: Arc<dyn FileSystem>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            fds: Mutex::new(Vec::new()),
        })
    }

    /// Same filesystem, a copy of the open fds (a forked job).
    pub(crate) fn fork(&self) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::clone(&self.inner),
            fds: Mutex::new(self.table().clone()),
        })
    }

    fn table(&self) -> MutexGuard<'_, Vec<(i32, ProcSubData)>> {
        self.fds.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn open_count(&self) -> usize {
        self.table().len()
    }

    pub(crate) fn is_open(&self, fd: i32) -> bool {
        self.table().iter().any(|(n, _)| *n == fd)
    }

    pub(crate) fn open(&self, fd: i32, data: ProcSubData) {
        self.table().push((fd, data));
    }

    /// Close every substitution opened after the first `len`.
    pub(crate) fn close_from(&self, len: usize) {
        let mut table = self.table();
        if table.len() > len {
            table.truncate(len);
        }
    }

    fn slot(&self, path: &Path) -> Slot {
        let Some(fd) = proc_sub_fd(path) else {
            return Slot::Vfs;
        };
        match self.table().iter().rev().find(|(n, _)| *n == fd) {
            Some((_, data)) => Slot::Open(fd, data.clone()),
            None => Slot::Closed,
        }
    }

    /// Error for an operation that a pipe end does not support.
    fn refuse(&self, path: &Path) -> Option<crate::Error> {
        match self.slot(path) {
            Slot::Vfs => None,
            Slot::Open(..) => Some(not_permitted()),
            Slot::Closed => Some(closed_error()),
        }
    }

    fn pipe_metadata() -> Metadata {
        Metadata {
            file_type: FileType::Fifo,
            size: 0,
            mode: 0o600,
            modified: UNIX_EPOCH,
            created: UNIX_EPOCH,
        }
    }

    fn write_pipe(&self, path: &Path, content: &[u8]) -> Option<Result<()>> {
        match self.slot(path) {
            Slot::Vfs => None,
            Slot::Open(_, ProcSubData::Output(buf)) => {
                buf.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend_from_slice(content);
                Some(Ok(()))
            }
            Slot::Open(_, ProcSubData::Input(_)) => Some(Err(not_permitted())),
            Slot::Closed => Some(Err(closed_error())),
        }
    }
}

#[async_trait]
impl FileSystemExt for ProcSubFs {
    fn usage(&self) -> FsUsage {
        self.inner.usage()
    }
    async fn mkfifo(&self, path: &Path, mode: u32) -> Result<()> {
        if let Some(e) = self.refuse(path) {
            return Err(e);
        }
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
impl FileSystem for ProcSubFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        match self.slot(path) {
            Slot::Vfs => self.inner.read_file(path).await,
            Slot::Open(_, ProcSubData::Input(data)) => Ok(data.to_vec()),
            // Reading the write end of `>(cmd)` sees no data.
            Slot::Open(_, ProcSubData::Output(_)) => Ok(Vec::new()),
            Slot::Closed => Err(closed_error()),
        }
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        match self.write_pipe(path, content) {
            Some(r) => r,
            None => self.inner.write_file(path, content).await,
        }
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        match self.write_pipe(path, content) {
            Some(r) => r,
            None => self.inner.append_file(path, content).await,
        }
    }
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        if let Some(e) = self.refuse(path) {
            return Err(e);
        }
        self.inner.mkdir(path, recursive).await
    }
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        if let Some(e) = self.refuse(path) {
            return Err(e);
        }
        self.inner.remove(path, recursive).await
    }
    async fn stat(&self, path: &Path) -> Result<Metadata> {
        match self.slot(path) {
            Slot::Vfs => self.inner.stat(path).await,
            Slot::Open(..) => Ok(Self::pipe_metadata()),
            Slot::Closed => Err(closed_error()),
        }
    }
    async fn lstat(&self, path: &Path) -> Result<Metadata> {
        match self.slot(path) {
            Slot::Vfs => self.inner.lstat(path).await,
            // Linux: `/dev/fd/N` is a symlink to `pipe:[inode]`.
            Slot::Open(..) => Ok(Metadata {
                file_type: FileType::Symlink,
                mode: 0o700,
                ..Self::pipe_metadata()
            }),
            Slot::Closed => Err(closed_error()),
        }
    }
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.inner.read_dir(path).await
    }
    async fn exists(&self, path: &Path) -> Result<bool> {
        match self.slot(path) {
            Slot::Vfs => self.inner.exists(path).await,
            Slot::Open(..) => Ok(true),
            Slot::Closed => Ok(false),
        }
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        if let Some(e) = self.refuse(from).or_else(|| self.refuse(to)) {
            return Err(e);
        }
        self.inner.rename(from, to).await
    }
    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        if proc_sub_fd(from).is_none() && proc_sub_fd(to).is_none() {
            return self.inner.copy(from, to).await;
        }
        let data = self.read_file(from).await?;
        self.write_file(to, &data).await
    }
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        if let Some(e) = self.refuse(link) {
            return Err(e);
        }
        self.inner.symlink(target, link).await
    }
    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        match self.slot(path) {
            Slot::Vfs => self.inner.read_link(path).await,
            // Linux names the pipe's inode; there is none here.
            Slot::Open(fd, _) => Ok(PathBuf::from(format!("pipe:[{fd}]"))),
            Slot::Closed => Err(closed_error()),
        }
    }
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        if let Some(e) = self.refuse(path) {
            return Err(e);
        }
        self.inner.chmod(path, mode).await
    }
    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        if let Some(e) = self.refuse(path) {
            return Err(e);
        }
        self.inner.set_modified_time(path, time).await
    }
    fn as_search_capable(&self) -> Option<&dyn super::SearchCapable> {
        // Indexed search would read the VFS, not the open substitutions.
        if self.open_count() == 0 {
            self.inner.as_search_capable()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;

    #[test]
    fn recognizes_dev_fd_numbers() {
        assert_eq!(proc_sub_fd(Path::new("/dev/fd/63")), Some(63));
        assert_eq!(proc_sub_fd(Path::new("/dev/../dev/fd/./62")), Some(62));
        assert_eq!(proc_sub_fd(Path::new("/dev/fd/1")), None);
        assert_eq!(proc_sub_fd(Path::new("/dev/fd/x")), None);
        assert_eq!(proc_sub_fd(Path::new("/tmp/fd/63")), None);
        assert_eq!(proc_sub_fd(Path::new("/dev/fd")), None);
    }

    #[tokio::test]
    async fn open_fds_are_private_to_the_wrapper() {
        let shared: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let a = ProcSubFs::new(Arc::clone(&shared));
        let b = ProcSubFs::new(Arc::clone(&shared));
        let p = Path::new("/dev/fd/63");
        a.open(63, ProcSubData::Input(Arc::from(&b"a-data"[..])));
        assert_eq!(a.read_file(p).await.unwrap(), b"a-data");
        assert!(a.exists(p).await.unwrap());
        assert_eq!(a.stat(p).await.unwrap().file_type, FileType::Fifo);
        // Other shell, same VFS: not open there, and never written through.
        assert!(!b.exists(p).await.unwrap());
        assert!(b.read_file(p).await.is_err());
        assert!(b.write_file(p, b"x").await.is_err());
        assert!(!shared.exists(p).await.unwrap());
        a.close_from(0);
        assert!(a.read_file(p).await.is_err());
    }

    #[tokio::test]
    async fn output_fd_collects_writes() {
        let fs = ProcSubFs::new(Arc::new(InMemoryFs::new()));
        let buf = Arc::new(Mutex::new(Vec::new()));
        fs.open(62, ProcSubData::Output(Arc::clone(&buf)));
        let p = Path::new("/dev/fd/62");
        fs.write_file(p, b"a").await.unwrap();
        fs.append_file(p, b"b").await.unwrap();
        fs.copy(Path::new("/dev/fd/62"), Path::new("/dev/fd/62"))
            .await
            .unwrap();
        assert_eq!(buf.lock().unwrap().as_slice(), b"ab");
        let forked = fs.fork();
        assert!(forked.is_open(62));
        fs.close_from(0);
        assert!(forked.is_open(62));
        assert!(!fs.is_open(62));
    }
}
