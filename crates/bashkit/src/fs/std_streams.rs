//! `/dev/stdin`, `/dev/stdout`, `/dev/stderr` as builtin file operands.
//!
//! Decision: redirects already route these paths to descriptors (see
//! `dev_fd_alias` in the interpreter). Builtins that open a path themselves
//! (`cat /dev/stdin`, `tee /dev/stderr`, `grep x /dev/fd/0`) go through the
//! VFS instead, where these paths don't exist. The interpreter wraps the
//! builtin's VFS view in [`StdStreamsFs`] only when an argument names one of
//! these paths: reads of fd 0 return the command's stdin, writes to fd 1/2
//! are captured and appended to the builtin's stdout/stderr afterwards.
//! Only fds 0-2: `/dev/fd/63` etc. stay real VFS files (process substitution).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::{DirEntry, FileSystem, FileSystemExt, FileType, FsLimits, FsUsage, Metadata};
use crate::error::Result;
use crate::time_compat::{SystemTime, UNIX_EPOCH};

/// Captured writes to `/dev/stdout` and `/dev/stderr`.
#[derive(Default)]
pub(crate) struct StdCapture {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

pub(crate) struct StdStreamsFs {
    inner: Arc<dyn FileSystem>,
    stdin: Vec<u8>,
    capture: Arc<Mutex<StdCapture>>,
}

/// Which standard stream a path names, if any.
pub(crate) fn std_stream_fd(path: &Path) -> Option<u8> {
    match super::normalize_path(path).to_str()? {
        "/dev/stdin" | "/dev/fd/0" => Some(0),
        "/dev/stdout" | "/dev/fd/1" => Some(1),
        "/dev/stderr" | "/dev/fd/2" => Some(2),
        _ => None,
    }
}

/// Cheap pre-check on raw arguments, so most builtin calls skip the wrapper.
pub(crate) fn args_name_std_stream(args: &[String]) -> bool {
    args.iter()
        .any(|a| a.contains("/dev/std") || a.contains("/dev/fd/"))
}

impl StdStreamsFs {
    pub(crate) fn wrap(
        inner: Arc<dyn FileSystem>,
        stdin: &[u8],
    ) -> (Arc<dyn FileSystem>, Arc<Mutex<StdCapture>>) {
        let capture = Arc::new(Mutex::new(StdCapture::default()));
        let fs = Arc::new(Self {
            inner,
            stdin: stdin.to_vec(),
            capture: Arc::clone(&capture),
        });
        (fs, capture)
    }

    fn write_stream(&self, fd: u8, content: &[u8]) {
        let mut capture = self.capture.lock().unwrap_or_else(|e| e.into_inner());
        match fd {
            1 => capture.stdout.extend_from_slice(content),
            _ => capture.stderr.extend_from_slice(content),
        }
    }

    fn stream_metadata(&self) -> Metadata {
        Metadata {
            file_type: FileType::Fifo,
            size: 0,
            mode: 0o620,
            modified: UNIX_EPOCH,
            created: UNIX_EPOCH,
        }
    }
}

#[async_trait]
impl FileSystemExt for StdStreamsFs {
    fn usage(&self) -> FsUsage {
        self.inner.usage()
    }
    async fn mkfifo(&self, path: &Path, mode: u32) -> Result<()> {
        self.inner.mkfifo(path, mode).await
    }
    fn limits(&self) -> FsLimits {
        self.inner.limits()
    }
    fn backend_kind(&self) -> &'static str {
        self.inner.backend_kind()
    }
}

#[async_trait]
impl FileSystem for StdStreamsFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        match std_stream_fd(path) {
            Some(0) => Ok(self.stdin.clone()),
            _ => self.inner.read_file(path).await,
        }
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        match std_stream_fd(path) {
            Some(fd @ (1 | 2)) => {
                self.write_stream(fd, content);
                Ok(())
            }
            _ => self.inner.write_file(path, content).await,
        }
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        match std_stream_fd(path) {
            Some(fd @ (1 | 2)) => {
                self.write_stream(fd, content);
                Ok(())
            }
            _ => self.inner.append_file(path, content).await,
        }
    }
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        self.inner.mkdir(path, recursive).await
    }
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.inner.remove(path, recursive).await
    }
    async fn stat(&self, path: &Path) -> Result<Metadata> {
        match std_stream_fd(path) {
            Some(_) => Ok(self.stream_metadata()),
            None => self.inner.stat(path).await,
        }
    }
    async fn lstat(&self, path: &Path) -> Result<Metadata> {
        match std_stream_fd(path) {
            Some(_) => Ok(self.stream_metadata()),
            None => self.inner.lstat(path).await,
        }
    }
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.inner.read_dir(path).await
    }
    async fn exists(&self, path: &Path) -> Result<bool> {
        match std_stream_fd(path) {
            Some(_) => Ok(true),
            None => self.inner.exists(path).await,
        }
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.inner.rename(from, to).await
    }
    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        match (std_stream_fd(from), std_stream_fd(to)) {
            (None, None) => self.inner.copy(from, to).await,
            _ => {
                let data = self.read_file(from).await?;
                self.write_file(to, &data).await
            }
        }
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
    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        self.inner.set_modified_time(path, time).await
    }
    fn as_search_capable(&self) -> Option<&dyn super::SearchCapable> {
        // Indexed search would bypass the stdin override.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;

    #[tokio::test]
    async fn reads_stdin_and_captures_writes() {
        let inner: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let (fs, cap) = StdStreamsFs::wrap(Arc::clone(&inner), b"in\n");
        assert_eq!(
            fs.read_file(Path::new("/dev/stdin")).await.unwrap(),
            b"in\n"
        );
        assert_eq!(fs.read_file(Path::new("/dev/fd/0")).await.unwrap(), b"in\n");
        fs.write_file(Path::new("/dev/stdout"), b"o").await.unwrap();
        fs.append_file(Path::new("/dev/fd/2"), b"e").await.unwrap();
        assert!(fs.exists(Path::new("/dev/stderr")).await.unwrap());
        let (out, err) = {
            let cap = cap.lock().unwrap();
            (cap.stdout.clone(), cap.stderr.clone())
        };
        assert_eq!((out.as_slice(), err.as_slice()), (&b"o"[..], &b"e"[..]));
        // Nothing reached the real VFS.
        assert!(!inner.exists(Path::new("/dev/stdout")).await.unwrap());
    }

    #[tokio::test]
    async fn other_fds_pass_through() {
        let inner: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let (fs, _) = StdStreamsFs::wrap(Arc::clone(&inner), b"");
        assert_eq!(std_stream_fd(Path::new("/dev/fd/63")), None);
        assert_eq!(std_stream_fd(Path::new("/dev/../dev/stdin")), Some(0));
        fs.write_file(Path::new("/tmp/x"), b"1").await.unwrap();
        assert_eq!(inner.read_file(Path::new("/tmp/x")).await.unwrap(), b"1");
    }
}
