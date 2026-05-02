//! Phase 2 IO adapter — bridges turso's sync `IO` trait to bashkit's async
//! `FileSystem`.
//!
//! Turso's [`IO`] / [`File`] traits are synchronous and completion-based;
//! bashkit's [`FileSystem`] is `async`. To bridge:
//!
//! 1. On `open_file`, we eagerly load the file's contents into a
//!    `Mutex<Vec<u8>>` using `tokio::task::block_in_place` + the current
//!    runtime handle. After that, all `pread`/`pwrite`/`size`/`truncate`
//!    operations run purely in memory (no `.await`).
//! 2. Each [`VfsFile`] tracks a dirty flag. After SQL execution finishes,
//!    the builtin layer calls [`BashkitVfsIO::flush_dirty`] from async
//!    context to write modified buffers back to the VFS.
//!
//! Trade-offs: large databases live entirely in memory while a connection is
//! open. Practical for the kinds of databases bashkit users operate on
//! (config, metadata, eval results) and far simpler than implementing
//! page-streaming async IO.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::runtime::Handle;
use turso_core::{
    Buffer, Completion, File, IO, OpenFlags,
    io::FileSyncType,
    io::clock::{Clock, DefaultClock, MonotonicInstant, WallClockInstant},
};

use crate::fs::FileSystem;

use super::engine::EngineResult;

/// Tracks one open file. The bytes vector is the canonical state; we read
/// from it on `pread` and mutate it on `pwrite`/`truncate`.
pub(super) struct VfsFile {
    path: PathBuf,
    bytes: Mutex<Vec<u8>>,
    dirty: AtomicBool,
}

impl VfsFile {
    fn new(path: PathBuf, bytes: Vec<u8>) -> Self {
        Self {
            path,
            bytes: Mutex::new(bytes),
            dirty: AtomicBool::new(false),
        }
    }
}

fn lock_bytes<'a>(m: &'a Mutex<Vec<u8>>) -> std::sync::MutexGuard<'a, Vec<u8>> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl File for VfsFile {
    fn lock_file(&self, _exclusive: bool) -> turso_core::Result<()> {
        // Bashkit is single-writer per Bash instance — no inter-process locks.
        Ok(())
    }

    fn unlock_file(&self) -> turso_core::Result<()> {
        Ok(())
    }

    fn pread(&self, pos: u64, c: Completion) -> turso_core::Result<Completion> {
        let buf = lock_bytes(&self.bytes);
        let r = c.as_read();
        let read_buf = r.buf();
        let read_len = read_buf.len();
        if read_len == 0 {
            c.complete(0);
            return Ok(c);
        }
        let pos_usize = pos as usize;
        if pos_usize >= buf.len() {
            c.complete(0);
            return Ok(c);
        }
        let take = read_len.min(buf.len() - pos_usize);
        read_buf.as_mut_slice()[..take].copy_from_slice(&buf[pos_usize..pos_usize + take]);
        for byte in &mut read_buf.as_mut_slice()[take..] {
            *byte = 0;
        }
        c.complete(take as i32);
        Ok(c)
    }

    fn pwrite(
        &self,
        pos: u64,
        buffer: Arc<Buffer>,
        c: Completion,
    ) -> turso_core::Result<Completion> {
        let mut buf = lock_bytes(&self.bytes);
        let pos_usize = pos as usize;
        let needed = pos_usize + buffer.len();
        if needed > buf.len() {
            buf.resize(needed, 0);
        }
        if !buffer.is_empty() {
            buf[pos_usize..pos_usize + buffer.len()].copy_from_slice(buffer.as_slice());
        }
        self.dirty.store(true, Ordering::Release);
        c.complete(buffer.len() as i32);
        Ok(c)
    }

    fn sync(&self, c: Completion, _sync_type: FileSyncType) -> turso_core::Result<Completion> {
        // Defer real persistence to flush_dirty() on the IO. Marking the
        // completion done here is correct because durability for the VFS is
        // a no-op (it's already in the bashkit address space).
        c.complete(0);
        Ok(c)
    }

    fn size(&self) -> turso_core::Result<u64> {
        Ok(lock_bytes(&self.bytes).len() as u64)
    }

    fn truncate(&self, len: u64, c: Completion) -> turso_core::Result<Completion> {
        let mut buf = lock_bytes(&self.bytes);
        buf.resize(len as usize, 0);
        self.dirty.store(true, Ordering::Release);
        c.complete(0);
        Ok(c)
    }
}

/// IO adapter exposing bashkit's [`FileSystem`] to turso.
pub(super) struct BashkitVfsIO {
    fs: Arc<dyn FileSystem>,
    /// All currently-open files keyed by path string. Used to flush dirty
    /// buffers back to the VFS after SQL execution.
    open_files: Mutex<HashMap<String, Arc<VfsFile>>>,
    /// Tokio runtime handle captured at construction. We use this from the
    /// sync `open_file` path to bridge back into async VFS reads.
    handle: Handle,
    /// Soft cap on a single file's in-memory buffer. Reading a VFS file
    /// larger than this aborts the open with an error. Defaults to 256 MB.
    max_file_bytes: usize,
}

impl BashkitVfsIO {
    pub(super) fn new(fs: Arc<dyn FileSystem>, handle: Handle) -> Arc<Self> {
        Arc::new(Self {
            fs,
            open_files: Mutex::new(HashMap::new()),
            handle,
            max_file_bytes: 256 * 1024 * 1024,
        })
    }

    /// Test/admin hook to override the file-size cap.
    ///
    /// Currently unused outside of the public crate API but retained as a
    /// test seam: integration tests that want to exercise the VFS-cap path
    /// without going through the full builtin can construct an IO with a
    /// tighter limit here.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(super) fn new_with_cap(
        fs: Arc<dyn FileSystem>,
        handle: Handle,
        max_file_bytes: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            fs,
            open_files: Mutex::new(HashMap::new()),
            handle,
            max_file_bytes,
        })
    }

    fn open_files_lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<VfsFile>>> {
        self.open_files.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Persist any dirty in-memory buffers back to the underlying `FileSystem`.
    pub(super) async fn flush_dirty(&self) -> EngineResult<usize> {
        let to_flush: Vec<Arc<VfsFile>> = {
            let map = self.open_files_lock();
            map.values()
                .filter(|f| f.dirty.load(Ordering::Acquire))
                .cloned()
                .collect()
        };
        let mut count = 0usize;
        for file in &to_flush {
            let bytes = lock_bytes(&file.bytes).clone();
            if let Some(parent) = file.path.parent()
                && !parent.as_os_str().is_empty()
                && !self.fs.exists(parent).await.unwrap_or(false)
            {
                return Err(format!(
                    "parent directory does not exist: {}",
                    parent.display()
                ));
            }
            self.fs
                .write_file(&file.path, &bytes)
                .await
                .map_err(|e| format!("flush failed for {}: {e}", file.path.display()))?;
            file.dirty.store(false, Ordering::Release);
            count += 1;
        }
        Ok(count)
    }
}

impl Clock for BashkitVfsIO {
    fn current_time_monotonic(&self) -> MonotonicInstant {
        DefaultClock.current_time_monotonic()
    }

    fn current_time_wall_clock(&self) -> WallClockInstant {
        DefaultClock.current_time_wall_clock()
    }
}

impl IO for BashkitVfsIO {
    fn open_file(
        &self,
        path: &str,
        flags: OpenFlags,
        _direct: bool,
    ) -> turso_core::Result<Arc<dyn File>> {
        if let Some(existing) = self.open_files_lock().get(path).cloned() {
            return Ok(existing as Arc<dyn File>);
        }
        let pb = PathBuf::from(path);
        let cap = self.max_file_bytes;
        let bytes_opt = run_async(&self.handle, {
            let fs = self.fs.clone();
            let pb = pb.clone();
            move || async move { fs.read_file(&pb).await.ok() }
        });
        let bytes = match bytes_opt {
            Some(b) => {
                if b.len() > cap {
                    return Err(turso_core::LimboError::InternalError(format!(
                        "sqlite: VFS file exceeds {} bytes cap",
                        cap
                    )));
                }
                b
            }
            None => {
                if !flags.contains(OpenFlags::Create) {
                    return Err(turso_core::LimboError::InternalError(format!(
                        "sqlite: file not found: {path}"
                    )));
                }
                Vec::new()
            }
        };
        let file = Arc::new(VfsFile::new(pb, bytes));
        self.open_files_lock()
            .insert(path.to_string(), file.clone());
        Ok(file as Arc<dyn File>)
    }

    fn remove_file(&self, path: &str) -> turso_core::Result<()> {
        self.open_files_lock().remove(path);
        run_async(&self.handle, {
            let fs = self.fs.clone();
            let pb = PathBuf::from(path);
            move || async move {
                let _ = fs.remove(&pb, false).await;
            }
        });
        Ok(())
    }
}

/// Run an async closure synchronously, regardless of whether we are already
/// inside the same tokio runtime. Direct `Handle::block_on` panics when
/// invoked from inside a current-thread runtime; we sidestep that by
/// spawning an OS thread, running `block_on` on it, and joining.
fn run_async<F, Fut, R>(handle: &tokio::runtime::Handle, make_fut: F) -> R
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = R> + Send,
    R: Send + 'static,
{
    let handle = handle.clone();
    std::thread::scope(|scope| {
        scope
            .spawn(move || handle.block_on(make_fut()))
            .join()
            .expect("vfs_io thread panicked")
    })
}

/// Best-effort runtime handle resolver. Inside `Builtin::execute` the current
/// runtime handle is always available; outside (e.g. some integration tests
/// constructing the IO directly) we fall back to a process-wide single-thread
/// runtime so the IO is still usable.
pub(super) fn current_handle_or_default() -> Handle {
    if let Ok(h) = Handle::try_current() {
        return h;
    }
    use std::sync::OnceLock;
    static FALLBACK: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    FALLBACK
        .get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("fallback runtime")
        })
        .handle()
        .clone()
}
