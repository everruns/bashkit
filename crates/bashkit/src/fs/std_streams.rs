//! `/dev/stdin`, `/dev/stdout`, `/dev/stderr` as builtin file operands.
//!
//! Decision: redirects already route these paths to descriptors (see
//! `dev_fd_alias` in the interpreter). Builtins that open a path themselves
//! (`cat /dev/stdin`, `tee /dev/stderr`, `grep x /dev/fd/0`) go through the
//! VFS instead, where these paths don't exist. The interpreter wraps the
//! builtin's VFS view in [`StdStreamsFs`] only when an argument names one of
//! these paths: reads of fd 0 return the command's stdin, writes to fd 1/2
//! are captured and appended to the builtin's stdout/stderr afterwards.
//! Only fds 0-2: `/dev/fd/63` etc. are the shell's process substitutions,
//! answered by `ProcSubFs` underneath.
//!
//! Decision: this interception bypasses filesystem quotas. Capture and stdin
//! copies therefore own request-budgeted capacity; capture ceilings apply
//! before writes and collection, even when an outer redirect discards output.
//! Collected byte/text leases survive redirect and hook consumption. Returned
//! stdin Vecs cannot carry leases through FileSystem, so the wrapper retains
//! their conservative charges until its last owner drops.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::{DirEntry, FileSystem, FileSystemExt, FileType, FsLimits, FsUsage, Metadata};
use crate::error::Result;
use crate::limits::{
    BudgetedBytes, BudgetedVec, ExecutionBudget, ExecutionBudgetLease, ExecutionLimits,
    LimitExceeded,
};
use crate::stream::StreamStorage;
use crate::time_compat::{SystemTime, UNIX_EPOCH};
use crate::{ExecResult, StreamData};

/// Captured writes to `/dev/stdout` and `/dev/stderr`.
pub(crate) struct StdCapture {
    stdout: BudgetedBytes,
    stderr: BudgetedBytes,
}

impl StdCapture {
    fn new(budget: &ExecutionBudget) -> Result<Self> {
        Ok(Self {
            stdout: BudgetedBytes::new(Some(budget))?,
            stderr: BudgetedBytes::new(Some(budget))?,
        })
    }

    pub(crate) fn collect(
        &mut self,
        result: &mut ExecResult,
        budget: &ExecutionBudget,
        limits: &ExecutionLimits,
    ) -> Result<[StreamStorage; 2]> {
        let capture = std::mem::replace(self, Self::new(budget)?);
        Ok([
            collect_stream(
                &mut result.stdout,
                capture.stdout,
                budget,
                limits.max_stdout_bytes,
            )?,
            collect_stream(
                &mut result.stderr,
                capture.stderr,
                budget,
                limits.max_stderr_bytes,
            )?,
        ])
    }
}

fn collect_stream(
    output: &mut StreamData,
    capture: BudgetedBytes,
    budget: &ExecutionBudget,
    limit: usize,
) -> Result<StreamStorage> {
    if capture.is_empty() {
        return Ok([None, None]);
    }
    let bytes = if output.is_empty() {
        capture
    } else {
        // Keep the capture lease alive while constructing the admitted copy.
        budget.consume_work(1 + output.len().div_ceil(1024) as u64)?;
        let mut bytes = BudgetedBytes::new(Some(budget))?;
        bytes.try_extend_from_slice_with_limit(output.as_bytes(), limit)?;
        bytes.try_extend_from_slice_with_limit(&capture, limit)?;
        bytes
    };
    let (stream, storage) = StreamData::from_budgeted_bytes(bytes, budget)?;
    *output = stream;
    Ok(storage)
}

type StdCaptureHandle = Arc<Mutex<StdCapture>>;

pub(crate) struct StdStreamsFs {
    inner: Arc<dyn FileSystem>,
    stdin: BudgetedBytes,
    read_storage: Mutex<BudgetedVec<ExecutionBudgetLease>>,
    capture: StdCaptureHandle,
    budget: ExecutionBudget,
    stdout_limit: usize,
    stderr_limit: usize,
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
        budget: &ExecutionBudget,
        limits: &ExecutionLimits,
    ) -> Result<(Arc<dyn FileSystem>, StdCaptureHandle)> {
        budget.consume_work(1 + stdin.len().div_ceil(1024) as u64)?;
        let mut input = BudgetedBytes::new(Some(budget))?;
        input.try_extend_from_slice(stdin)?;
        let capture = Arc::new(Mutex::new(StdCapture::new(budget)?));
        let fs = Arc::new(Self {
            inner,
            stdin: input,
            read_storage: Mutex::new(BudgetedVec::new(Some(budget))?),
            capture: Arc::clone(&capture),
            budget: budget.clone(),
            stdout_limit: limits.max_stdout_bytes,
            stderr_limit: limits.max_stderr_bytes,
        });
        Ok((fs, capture))
    }

    // THREAT[TM-DOS-103]: reserve retained capacity before any stream growth.
    fn write_stream(&self, fd: u8, content: &[u8]) -> Result<()> {
        self.budget
            .consume_work(1 + content.len().div_ceil(1024) as u64)?;
        let mut capture = self.capture.lock().unwrap_or_else(|e| e.into_inner());
        let (buffer, limit) = match fd {
            1 => (&mut capture.stdout, self.stdout_limit),
            _ => (&mut capture.stderr, self.stderr_limit),
        };
        buffer.try_extend_from_slice_with_limit(content, limit)?;
        Ok(())
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
            Some(0) => {
                self.budget.consume_input(self.stdin.len())?;
                self.budget
                    .consume_work(1 + self.stdin.len().div_ceil(1024) as u64)?;
                let mut data = BudgetedBytes::new(Some(&self.budget))?;
                data.try_extend_from_slice(&self.stdin)?;
                let (data, lease) = data.into_parts();
                if let Some(lease) = lease {
                    self.read_storage
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .try_push(lease)?;
                }
                Ok(data)
            }
            _ => self.inner.read_file(path).await,
        }
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        match std_stream_fd(path) {
            Some(fd @ (1 | 2)) => self.write_stream(fd, content),
            _ => self.inner.write_file(path, content).await,
        }
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        match std_stream_fd(path) {
            Some(fd @ (1 | 2)) => self.write_stream(fd, content),
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
            (Some(0), _) => {
                // Borrow the already admitted stdin instead of making a copy.
                self.budget
                    .consume_work(1 + self.stdin.len().div_ceil(1024) as u64)?;
                self.write_file(to, &self.stdin).await
            }
            _ => {
                // The wrapper owns this temporary read, so admit the backend's
                // reported size before loading it, including discarded sinks.
                let source = self
                    .budget
                    .run(super::canonicalize(&*self.inner, from))
                    .await??;
                let metadata = self.budget.run(self.inner.stat(&source)).await??;
                // System devices report size zero, but materialize bounded data.
                // Resolve aliases and use the backend's canonical read ceilings.
                let reported_size = usize::try_from(metadata.size)
                    .map_err(|_| LimitExceeded::Memory("stream copy size overflow".into()))?;
                let device_size = match source.to_str() {
                    Some("/dev/zero") => super::memory::DEV_ZERO_READ_BYTES,
                    Some("/dev/random" | "/dev/urandom") => super::memory::DEV_RANDOM_READ_BYTES,
                    _ => 0,
                };
                let size = reported_size.max(device_size);
                let limit = match std_stream_fd(to) {
                    Some(1) => self.stdout_limit,
                    Some(2) => self.stderr_limit,
                    _ => usize::MAX,
                };
                if size > limit {
                    return Err(
                        LimitExceeded::Memory("stream copy size limit exceeded".into()).into(),
                    );
                }
                self.budget.consume_input(size)?;
                self.budget.consume_work(1 + size.div_ceil(1024) as u64)?;
                let _read_storage = self.budget.lease_bytes(size)?;
                let data = self.budget.run(self.inner.read_file(&source)).await??;
                let _extra_storage = self
                    .budget
                    .lease_bytes(data.capacity().saturating_sub(size))?;
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

    fn test_budget(limits: &ExecutionLimits) -> ExecutionBudget {
        ExecutionBudget::new(limits, Arc::new(std::sync::atomic::AtomicBool::new(false)))
    }

    #[tokio::test]
    async fn reads_stdin_and_captures_writes() {
        let inner: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let limits = ExecutionLimits::new();
        let budget = test_budget(&limits);
        let (fs, cap) = StdStreamsFs::wrap(Arc::clone(&inner), b"in\n", &budget, &limits).unwrap();
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
            (cap.stdout.to_vec(), cap.stderr.to_vec())
        };
        assert_eq!((out.as_slice(), err.as_slice()), (&b"o"[..], &b"e"[..]));
        // Nothing reached the real VFS.
        assert!(!inner.exists(Path::new("/dev/stdout")).await.unwrap());
    }

    #[tokio::test]
    async fn other_fds_pass_through() {
        let inner: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let limits = ExecutionLimits::new();
        let budget = test_budget(&limits);
        let (fs, _) = StdStreamsFs::wrap(Arc::clone(&inner), b"", &budget, &limits).unwrap();
        assert_eq!(std_stream_fd(Path::new("/dev/fd/63")), None);
        assert_eq!(std_stream_fd(Path::new("/dev/../dev/stdin")), Some(0));
        fs.write_file(Path::new("/tmp/x"), b"1").await.unwrap();
        assert_eq!(inner.read_file(Path::new("/tmp/x")).await.unwrap(), b"1");
    }
    #[tokio::test]
    async fn aliases_append_and_limit_rejection_are_atomic() {
        for (path, alias, stdout) in [
            ("/dev/stdout", "/dev/fd/1", true),
            ("/dev/stderr", "/dev/fd/2", false),
        ] {
            let limits = ExecutionLimits::new()
                .max_stdout_bytes(2)
                .max_stderr_bytes(2);
            let budget = test_budget(&limits);
            let (fs, cap) =
                StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"", &budget, &limits).unwrap();
            fs.write_file(Path::new(path), b"a").await.unwrap();
            fs.append_file(Path::new(alias), b"b").await.unwrap();
            let error = fs.write_file(Path::new(path), b"c").await.unwrap_err();
            assert!(matches!(error, crate::Error::ResourceLimit(_)));
            let cap = cap.lock().unwrap();
            let bytes = if stdout { &cap.stdout } else { &cap.stderr };
            assert_eq!(&**bytes, b"ab");
            assert_eq!(budget.live_bytes_for_test(), 2);
        }
    }

    #[tokio::test]
    async fn shared_capacity_is_admitted_before_growth() {
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(6);
        let budget = test_budget(&limits);
        let (fs, cap) =
            StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"", &budget, &limits).unwrap();
        fs.write_file(Path::new("/dev/stdout"), b"abc")
            .await
            .unwrap();
        fs.write_file(Path::new("/dev/stderr"), b"def")
            .await
            .unwrap();
        assert_eq!(budget.live_bytes_for_test(), 6);
        let error = fs
            .append_file(Path::new("/dev/fd/1"), b"g")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("live intermediate"));
        assert_eq!(&*cap.lock().unwrap().stdout, b"abc");
        assert_eq!(budget.live_bytes_for_test(), 6);
        drop((fs, cap));
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[tokio::test]
    async fn empty_writes_check_work_and_cancellation() {
        let limits = ExecutionLimits::new().max_work_units(2);
        let budget = test_budget(&limits);
        let (fs, _) =
            StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"", &budget, &limits).unwrap();
        fs.write_file(Path::new("/dev/stdout"), b"").await.unwrap();
        let error = fs
            .append_file(Path::new("/dev/stderr"), b"")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("work units"));
        assert_eq!(budget.live_bytes_for_test(), 0);

        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let budget = ExecutionBudget::new(&ExecutionLimits::new(), cancelled.clone());
        let (fs, cap) = StdStreamsFs::wrap(
            Arc::new(InMemoryFs::new()),
            b"",
            &budget,
            &ExecutionLimits::new(),
        )
        .unwrap();
        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(matches!(
            fs.write_file(Path::new("/dev/fd/2"), b"a").await,
            Err(crate::Error::Cancelled)
        ));
        assert!(cap.lock().unwrap().stderr.is_empty());
    }

    #[tokio::test]
    async fn collection_retains_bytes_and_text_leases() {
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(16);
        let budget = test_budget(&limits);
        let (fs, cap) =
            StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"", &budget, &limits).unwrap();
        fs.write_file(Path::new("/dev/stdout"), b"hi")
            .await
            .unwrap();
        let mut result = ExecResult::default();
        let storage = cap
            .lock()
            .unwrap()
            .collect(&mut result, &budget, &limits)
            .unwrap();
        drop((fs, cap));
        assert_eq!(result.stdout, "hi");
        assert_eq!(budget.live_bytes_for_test(), 4);
        drop(result);
        drop(storage);
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[tokio::test]
    async fn collection_admits_lossy_text_and_preserves_utf8_boundaries() {
        let limits = ExecutionLimits::new();
        let budget = test_budget(&limits);
        let (fs, cap) =
            StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"", &budget, &limits).unwrap();
        fs.write_file(Path::new("/dev/stdout"), &[0xa9, 0xff])
            .await
            .unwrap();
        let mut result = ExecResult::ok_bytes(vec![0xc3]);
        let _storage = cap
            .lock()
            .unwrap()
            .collect(&mut result, &budget, &limits)
            .unwrap();
        assert_eq!(result.stdout.as_bytes(), &[0xc3, 0xa9, 0xff]);
        assert_eq!(result.stdout.text_lossy(), "é�");

        let limits = ExecutionLimits::new().max_live_intermediate_bytes(3);
        let budget = test_budget(&limits);
        let (fs, cap) =
            StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"", &budget, &limits).unwrap();
        fs.write_file(Path::new("/dev/stderr"), &[0xff])
            .await
            .unwrap();
        let error = cap
            .lock()
            .unwrap()
            .collect(&mut ExecResult::default(), &budget, &limits)
            .unwrap_err();
        assert!(error.to_string().contains("live intermediate"));
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[tokio::test]
    async fn stdin_reads_retain_copy_and_container_capacity() {
        let limits = ExecutionLimits::new();
        let budget = test_budget(&limits);
        let (fs, cap) =
            StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"hi", &budget, &limits).unwrap();
        assert_eq!(budget.live_bytes_for_test(), 2);
        let first = fs.read_file(Path::new("/dev/stdin")).await.unwrap();
        let second = fs.read_file(Path::new("/dev/fd/0")).await.unwrap();
        assert_eq!(
            (first.as_slice(), second.as_slice()),
            (b"hi".as_slice(), b"hi".as_slice())
        );
        assert_eq!(
            budget.live_bytes_for_test(),
            6 + 2 * std::mem::size_of::<ExecutionBudgetLease>() as u64
        );
        drop((first, second, fs, cap));
        assert_eq!(budget.live_bytes_for_test(), 0);

        let limits = ExecutionLimits::new().max_live_intermediate_bytes(1);
        let budget = test_budget(&limits);
        assert!(StdStreamsFs::wrap(Arc::new(InMemoryFs::new()), b"hi", &budget, &limits).is_err());
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[tokio::test]
    async fn copies_borrow_stdin_and_admit_temporary_file_reads() {
        let limits = ExecutionLimits::new();
        let budget = test_budget(&limits);
        let inner: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        inner.write_file(Path::new("/tmp/in"), b"ok").await.unwrap();
        let (fs, cap) = StdStreamsFs::wrap(inner.clone(), b"hi", &budget, &limits).unwrap();
        fs.copy(Path::new("/dev/stdin"), Path::new("/dev/stdout"))
            .await
            .unwrap();
        assert_eq!(budget.live_bytes_for_test(), 4);
        fs.copy(Path::new("/dev/fd/0"), Path::new("/tmp/out"))
            .await
            .unwrap();
        assert_eq!(inner.read_file(Path::new("/tmp/out")).await.unwrap(), b"hi");
        fs.copy(Path::new("/tmp/in"), Path::new("/dev/stderr"))
            .await
            .unwrap();
        assert_eq!(&*cap.lock().unwrap().stderr, b"ok");
        assert_eq!(budget.live_bytes_for_test(), 6);
        fs.copy(Path::new("/tmp/in"), Path::new("/tmp/pass"))
            .await
            .unwrap();
        assert_eq!(
            inner.read_file(Path::new("/tmp/pass")).await.unwrap(),
            b"ok"
        );

        let limits = ExecutionLimits::new().max_live_intermediate_bytes(1);
        let budget = test_budget(&limits);
        let (fs, cap) = StdStreamsFs::wrap(inner, b"", &budget, &limits).unwrap();
        let error = fs
            .copy(Path::new("/tmp/in"), Path::new("/dev/fd/2"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("live intermediate"));
        assert!(cap.lock().unwrap().stderr.is_empty());
    }
    #[tokio::test]
    async fn windows_containment_stream_copy_admits_device_before_read() {
        let limits = ExecutionLimits::new().max_stdout_bytes(0);
        let budget = test_budget(&limits);
        let inner: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        inner
            .symlink(Path::new("/dev/urandom"), Path::new("/tmp/device-alias"))
            .await
            .unwrap();
        let (fs, cap) = StdStreamsFs::wrap(inner, b"", &budget, &limits).unwrap();
        // Metadata is zero; only numeric device capacity is inspected. No
        // random bytes are read or allocated under zero destination admission.
        for source in ["/dev/urandom", "/tmp/device-alias"] {
            let error = fs
                .copy(Path::new(source), Path::new("/dev/stdout"))
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                crate::Error::ResourceLimit(LimitExceeded::Memory(message))
                    if message == "stream copy size limit exceeded"
            ));
            assert_eq!(budget.live_bytes_for_test(), 0);
            assert!(cap.lock().unwrap().stdout.is_empty());
        }
    }
}
