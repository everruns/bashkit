//! WASI preview1 host for the embedded CPython guest, backed by bashkit's VFS.
//!
//! Decisions:
//! - Only `wasi_snapshot_preview1` is implemented, and only against bashkit
//!   state: the VFS, the captured stdin bytes, in-memory stdout/stderr, a
//!   host RNG and host clocks. No host filesystem, socket or process API is
//!   reachable; socket calls return `ENOTSUP`.
//! - One preopen, `/`, at fd 3 (the snapshot cached that exact table).
//! - Files are opened as whole in-memory buffers and written back to the VFS on
//!   close/sync/exit. Buffers are capped by the VFS `max_file_size` and the
//!   per-call output budget, so a guest cannot grow host memory without bound.
//! - The stdlib zip is a read-only overlay at a fixed path; the guest cannot
//!   modify or shadow it.
//! - Paths are normalized lexically and clamped at `/`; `..` cannot escape the
//!   VFS root (which is itself the whole sandbox).

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wasmtime::{Caller, Extern, Linker, Memory, StoreLimits};

use crate::fs::{FileSystem, FileType};

const MODULE: &str = "wasi_snapshot_preview1";

// --- WASI preview1 constants -------------------------------------------------

pub(crate) type Errno = u16;
pub(crate) const SUCCESS: Errno = 0;
const E2BIG: Errno = 1;
const EACCES: Errno = 2;
const EBADF: Errno = 8;
const EEXIST: Errno = 20;
const EFAULT: Errno = 21;
const EFBIG: Errno = 22;
const EILSEQ: Errno = 25;
const EINVAL: Errno = 28;
const EIO: Errno = 29;
const EISDIR: Errno = 31;
const EMFILE: Errno = 33;
const ENAMETOOLONG: Errno = 37;
const ENOENT: Errno = 44;
const ENOSPC: Errno = 51;
const ENOTDIR: Errno = 54;
const ENOTEMPTY: Errno = 55;
const ENOTSUP: Errno = 58;
const EROFS: Errno = 69;
const ESPIPE: Errno = 70;

const FILETYPE_UNKNOWN: u8 = 0;
const FILETYPE_DIRECTORY: u8 = 3;
const FILETYPE_REGULAR_FILE: u8 = 4;
const FILETYPE_SYMBOLIC_LINK: u8 = 7;

const OFLAGS_CREAT: u16 = 1;
const OFLAGS_DIRECTORY: u16 = 2;
const OFLAGS_EXCL: u16 = 4;
const OFLAGS_TRUNC: u16 = 8;

const FDFLAGS_APPEND: u16 = 1;

const RIGHTS_FD_READ: u64 = 1 << 1;
const RIGHTS_FD_WRITE: u64 = 1 << 6;
const RIGHTS_FD_READDIR: u64 = 1 << 14;
const RIGHTS_ALL: u64 = (1 << 30) - 1;

const FSTFLAGS_MTIM: u16 = 1 << 2;
const FSTFLAGS_MTIM_NOW: u16 = 1 << 3;

const CLOCK_REALTIME: u32 = 0;
const CLOCK_MONOTONIC: u32 = 1;

const EVENTTYPE_CLOCK: u8 = 0;
const EVENTTYPE_FD_READ: u8 = 1;
const EVENTTYPE_FD_WRITE: u8 = 2;
const SUBCLOCKFLAGS_ABSTIME: u16 = 1;

const PREOPEN_FD: u32 = 3;
const MAX_FDS: usize = 1024;
const MAX_PATH_LEN: usize = 4096;
/// Longest single `poll_oneoff` sleep; longer sleeps are cut into slices so
/// the call deadline is re-checked.
const MAX_SLEEP_SLICE: Duration = Duration::from_millis(250);

// --- guest state -------------------------------------------------------------

/// Captured guest output with a shared byte cap.
#[derive(Default)]
pub(crate) struct Output {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) truncated: bool,
    cap: usize,
}

impl Output {
    fn write(&mut self, fd: u32, data: &[u8]) {
        let used = self.stdout.len() + self.stderr.len();
        let room = self.cap.saturating_sub(used);
        let take = data.len().min(room);
        if take < data.len() {
            self.truncated = true;
        }
        let target = if fd == 1 {
            &mut self.stdout
        } else {
            &mut self.stderr
        };
        target.extend_from_slice(&data[..take]);
    }
}

struct OpenFile {
    path: PathBuf,
    data: Cow<'static, [u8]>,
    pos: u64,
    read: bool,
    write: bool,
    append: bool,
    dirty: bool,
    modified: SystemTime,
}

struct OpenDir {
    path: PathBuf,
    /// (name, filetype, inode); computed on first `fd_readdir`.
    entries: Option<Vec<(String, u8, u64)>>,
}

enum Fd {
    Stdin,
    Stdout,
    Stderr,
    Root,
    File(Box<OpenFile>),
    Dir(OpenDir),
}

/// Per-call configuration handed to [`GuestState::new`].
pub(crate) struct GuestConfig {
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) stdin: Vec<u8>,
    pub(crate) output_cap: usize,
    pub(crate) max_memory: usize,
    pub(crate) stdlib_path: &'static str,
    pub(crate) stdlib: &'static [u8],
    pub(crate) deadline: crate::time_compat::Instant,
}

/// Everything one `python3` call's wasm instance can reach.
pub(crate) struct GuestState {
    fs: Arc<dyn FileSystem>,
    args: Vec<Vec<u8>>,
    env: Vec<Vec<u8>>,
    stdin: Vec<u8>,
    stdin_pos: usize,
    pub(crate) output: Output,
    fds: Vec<Option<Fd>>,
    pub(crate) limits: StoreLimits,
    stdlib_path: &'static Path,
    stdlib: &'static [u8],
    started: crate::time_compat::Instant,
    deadline: crate::time_compat::Instant,
    /// Bytes held in open file buffers, bounded by `max_file_size` each and
    /// `buffer_cap` in total.
    buffered: usize,
    buffer_cap: usize,
    max_file_size: usize,
    /// Errno of the first failed write-back at exit, if any.
    pub(crate) flush_error: Option<Errno>,
}

/// Exit requested through `proc_exit`.
#[derive(Debug)]
pub(crate) struct ProcExit(pub(crate) i32);

impl std::fmt::Display for ProcExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "proc_exit({})", self.0)
    }
}

impl std::error::Error for ProcExit {}

impl GuestState {
    pub(crate) fn new(fs: Arc<dyn FileSystem>, config: GuestConfig) -> Self {
        let mut args: Vec<Vec<u8>> = config.args.into_iter().map(String::into_bytes).collect();
        for arg in &mut args {
            arg.retain(|b| *b != 0);
        }
        let env = config
            .env
            .into_iter()
            .filter(|(k, v)| !k.is_empty() && !k.contains(['=', '\0']) && !v.contains('\0'))
            .map(|(k, v)| format!("{k}={v}").into_bytes())
            .collect();
        let fs_limits = fs.limits();
        let max_file_size = usize::try_from(fs_limits.max_file_size).unwrap_or(usize::MAX);
        let limits = wasmtime::StoreLimitsBuilder::new()
            .memory_size(config.max_memory)
            .instances(1)
            .tables(4)
            .memories(1)
            .build();
        Self {
            fs,
            args,
            env,
            stdin: config.stdin,
            stdin_pos: 0,
            output: Output {
                cap: config.output_cap,
                ..Output::default()
            },
            fds: vec![
                Some(Fd::Stdin),
                Some(Fd::Stdout),
                Some(Fd::Stderr),
                Some(Fd::Root),
            ],
            limits,
            stdlib_path: Path::new(config.stdlib_path),
            stdlib: config.stdlib,
            started: crate::time_compat::Instant::now(),
            deadline: config.deadline,
            buffered: 0,
            // Open-file buffers share the guest memory budget: a script can
            // keep at most as much file data in flight as it may allocate.
            buffer_cap: config.max_memory,
            max_file_size,
            flush_error: None,
        }
    }

    fn is_stdlib(&self, path: &Path) -> bool {
        path == self.stdlib_path
    }

    fn is_stdlib_ancestor(&self, path: &Path) -> bool {
        path != self.stdlib_path && self.stdlib_path.starts_with(path)
    }

    fn alloc_fd(&mut self, fd: Fd) -> Result<u32, Errno> {
        // Fds 0-2 are stdio and PREOPEN_FD is the root; never reuse them.
        let first = PREOPEN_FD as usize + 1;
        if let Some(slot) = self.fds.iter().skip(first).position(Option::is_none) {
            let idx = slot + first;
            self.fds[idx] = Some(fd);
            return u32::try_from(idx).map_err(|_| EMFILE);
        }
        if self.fds.len() >= MAX_FDS {
            return Err(EMFILE);
        }
        self.fds.push(Some(fd));
        u32::try_from(self.fds.len() - 1).map_err(|_| EMFILE)
    }

    fn fd(&self, fd: u32) -> Result<&Fd, Errno> {
        self.fds
            .get(fd as usize)
            .and_then(Option::as_ref)
            .ok_or(EBADF)
    }

    fn fd_mut(&mut self, fd: u32) -> Result<&mut Fd, Errno> {
        self.fds
            .get_mut(fd as usize)
            .and_then(Option::as_mut)
            .ok_or(EBADF)
    }

    fn dir_base(&self, fd: u32) -> Result<PathBuf, Errno> {
        match self.fd(fd)? {
            Fd::Root => Ok(PathBuf::from("/")),
            Fd::Dir(d) => Ok(d.path.clone()),
            _ => Err(ENOTDIR),
        }
    }

    /// Resolve a guest path relative to directory fd `fd`.
    fn resolve(&self, fd: u32, raw: &[u8]) -> Result<PathBuf, Errno> {
        if raw.len() > MAX_PATH_LEN {
            return Err(ENAMETOOLONG);
        }
        if raw.contains(&0) {
            return Err(EINVAL);
        }
        let rel = std::str::from_utf8(raw).map_err(|_| EILSEQ)?;
        let base = self.dir_base(fd)?;
        Ok(normalize(&base, rel))
    }

    /// Reserve `extra` bytes of open-file buffer space.
    fn reserve(&mut self, extra: usize) -> Result<(), Errno> {
        let next = self.buffered.checked_add(extra).ok_or(ENOSPC)?;
        if next > self.buffer_cap {
            return Err(ENOSPC);
        }
        self.buffered = next;
        Ok(())
    }

    fn release(&mut self, bytes: usize) {
        self.buffered = self.buffered.saturating_sub(bytes);
    }

    /// Write every dirty file buffer back to the VFS (exit-time flush).
    pub(crate) async fn flush_all(&mut self) {
        let fs = self.fs.clone();
        for slot in &mut self.fds {
            if let Some(Fd::File(file)) = slot
                && file.dirty
            {
                if let Err(e) = fs.write_file(&file.path, &file.data).await {
                    self.flush_error.get_or_insert(map_fs_error(&e));
                }
                file.dirty = false;
            }
        }
    }
}

/// Lexically normalize `rel` against `base`, clamping `..` at `/`.
fn normalize(base: &Path, rel: &str) -> PathBuf {
    let mut parts: Vec<&str> = Vec::new();
    let start: &str = if rel.starts_with('/') {
        ""
    } else {
        base.to_str().unwrap_or("/")
    };
    for comp in start.split('/').chain(rel.split('/')) {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = String::from("/");
    out.push_str(&parts.join("/"));
    PathBuf::from(out)
}

fn inode(path: &Path) -> u64 {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    h.finish() | 1
}

fn filetype_of(t: FileType) -> u8 {
    match t {
        FileType::File => FILETYPE_REGULAR_FILE,
        FileType::Directory => FILETYPE_DIRECTORY,
        FileType::Symlink => FILETYPE_SYMBOLIC_LINK,
        FileType::Fifo => FILETYPE_UNKNOWN,
    }
}

fn nanos(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Map a bashkit VFS error to a WASI errno.
fn map_fs_error(e: &crate::Error) -> Errno {
    use std::io::ErrorKind;
    if let crate::Error::Io(io) = e {
        match io.kind() {
            ErrorKind::NotFound => return ENOENT,
            ErrorKind::AlreadyExists => return EEXIST,
            ErrorKind::PermissionDenied => return EACCES,
            ErrorKind::InvalidInput => return EINVAL,
            _ => {}
        }
    }
    let msg = e.to_string().to_ascii_lowercase();
    if msg.contains("not found") || msg.contains("no such file") {
        ENOENT
    } else if msg.contains("is a directory") {
        EISDIR
    } else if msg.contains("not a directory") {
        ENOTDIR
    } else if msg.contains("already exists") {
        EEXIST
    } else if msg.contains("not empty") {
        ENOTEMPTY
    } else if msg.contains("limit") || msg.contains("too large") || msg.contains("exceed") {
        ENOSPC
    } else if msg.contains("read-only") || msg.contains("readonly") {
        EROFS
    } else if msg.contains("permission") || msg.contains("denied") {
        EACCES
    } else {
        EIO
    }
}

// --- guest memory helpers ----------------------------------------------------

fn memory<T>(caller: &mut Caller<'_, T>) -> Result<Memory, Errno> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Ok(m),
        _ => Err(EFAULT),
    }
}

fn range(ptr: i32, len: u32, mem_len: usize) -> Result<std::ops::Range<usize>, Errno> {
    let start = ptr as u32 as usize;
    let end = start.checked_add(len as usize).ok_or(EFAULT)?;
    if end > mem_len {
        return Err(EFAULT);
    }
    Ok(start..end)
}

fn read_bytes(caller: &mut Caller<'_, GuestState>, ptr: i32, len: i32) -> Result<Vec<u8>, Errno> {
    let mem = memory(caller)?;
    let data = mem.data(&*caller);
    let r = range(ptr, len as u32, data.len())?;
    Ok(data[r].to_vec())
}

fn write_bytes(caller: &mut Caller<'_, GuestState>, ptr: i32, bytes: &[u8]) -> Result<(), Errno> {
    let mem = memory(caller)?;
    let data = mem.data_mut(&mut *caller);
    let r = range(
        ptr,
        u32::try_from(bytes.len()).map_err(|_| EFAULT)?,
        data.len(),
    )?;
    data[r].copy_from_slice(bytes);
    Ok(())
}

fn write_u32(caller: &mut Caller<'_, GuestState>, ptr: i32, v: u32) -> Result<(), Errno> {
    write_bytes(caller, ptr, &v.to_le_bytes())
}

fn write_u64(caller: &mut Caller<'_, GuestState>, ptr: i32, v: u64) -> Result<(), Errno> {
    write_bytes(caller, ptr, &v.to_le_bytes())
}

/// Read an iovec array: `(buf_ptr, buf_len)` pairs.
fn read_iovs(
    caller: &mut Caller<'_, GuestState>,
    iovs: i32,
    count: i32,
) -> Result<Vec<(i32, u32)>, Errno> {
    let count = u32::try_from(count).map_err(|_| EINVAL)?;
    if count > 1024 {
        return Err(EINVAL);
    }
    let raw = read_bytes(caller, iovs, i32::try_from(count * 8).map_err(|_| EINVAL)?)?;
    Ok(raw
        .chunks_exact(8)
        .map(|c| {
            let ptr = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            let len = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
            (ptr, len)
        })
        .collect())
}

fn gather(caller: &mut Caller<'_, GuestState>, iovs: &[(i32, u32)]) -> Result<Vec<u8>, Errno> {
    let mem = memory(caller)?;
    let data = mem.data(&*caller);
    let mut out = Vec::new();
    for &(ptr, len) in iovs {
        let r = range(ptr, len, data.len())?;
        out.extend_from_slice(&data[r]);
    }
    Ok(out)
}

fn scatter(
    caller: &mut Caller<'_, GuestState>,
    iovs: &[(i32, u32)],
    src: &[u8],
) -> Result<usize, Errno> {
    let mem = memory(caller)?;
    let data = mem.data_mut(&mut *caller);
    let mut done = 0;
    for &(ptr, len) in iovs {
        if done >= src.len() {
            break;
        }
        let r = range(ptr, len, data.len())?;
        let n = (src.len() - done).min(r.len());
        data[r.start..r.start + n].copy_from_slice(&src[done..done + n]);
        done += n;
    }
    Ok(done)
}

fn total_len(iovs: &[(i32, u32)]) -> usize {
    iovs.iter().map(|&(_, l)| l as usize).sum()
}

fn ret(r: Result<(), Errno>) -> i32 {
    i32::from(match r {
        Ok(()) => SUCCESS,
        Err(e) => e,
    })
}

// --- filestat ----------------------------------------------------------------

struct Stat {
    filetype: u8,
    size: u64,
    mtime: u64,
    ino: u64,
}

fn encode_filestat(s: &Stat) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0..8].copy_from_slice(&1u64.to_le_bytes()); // dev
    b[8..16].copy_from_slice(&s.ino.to_le_bytes());
    b[16] = s.filetype;
    b[24..32].copy_from_slice(&1u64.to_le_bytes()); // nlink
    b[32..40].copy_from_slice(&s.size.to_le_bytes());
    b[40..48].copy_from_slice(&s.mtime.to_le_bytes()); // atim
    b[48..56].copy_from_slice(&s.mtime.to_le_bytes()); // mtim
    b[56..64].copy_from_slice(&s.mtime.to_le_bytes()); // ctim
    b
}

async fn stat_path(state: &GuestState, path: &Path) -> Result<Stat, Errno> {
    if state.is_stdlib(path) {
        return Ok(Stat {
            filetype: FILETYPE_REGULAR_FILE,
            size: state.stdlib.len() as u64,
            mtime: 0,
            ino: inode(path),
        });
    }
    match state.fs.stat(path).await {
        Ok(m) => Ok(Stat {
            filetype: filetype_of(m.file_type),
            size: m.size,
            mtime: nanos(m.modified),
            ino: inode(path),
        }),
        Err(e) if state.is_stdlib_ancestor(path) => {
            let _ = e;
            Ok(Stat {
                filetype: FILETYPE_DIRECTORY,
                size: 0,
                mtime: 0,
                ino: inode(path),
            })
        }
        Err(e) => Err(map_fs_error(&e)),
    }
}

// --- linker ------------------------------------------------------------------

/// Register every `wasi_snapshot_preview1` import the guest uses.
pub(crate) fn add_to_linker(linker: &mut Linker<GuestState>) -> wasmtime::Result<()> {
    // args / environ
    linker.func_wrap(
        MODULE,
        "args_sizes_get",
        |mut c: Caller<'_, GuestState>, argc: i32, size: i32| {
            let n = c.data().args.len() as u32;
            let bytes: usize = c.data().args.iter().map(|a| a.len() + 1).sum();
            ret(write_u32(&mut c, argc, n).and_then(|()| write_u32(&mut c, size, bytes as u32)))
        },
    )?;
    linker.func_wrap(
        MODULE,
        "args_get",
        |mut c: Caller<'_, GuestState>, argv: i32, buf: i32| {
            let items = c.data().args.clone();
            ret(write_string_table(&mut c, &items, argv, buf))
        },
    )?;
    linker.func_wrap(
        MODULE,
        "environ_sizes_get",
        |mut c: Caller<'_, GuestState>, count: i32, size: i32| {
            let n = c.data().env.len() as u32;
            let bytes: usize = c.data().env.iter().map(|a| a.len() + 1).sum();
            ret(write_u32(&mut c, count, n).and_then(|()| write_u32(&mut c, size, bytes as u32)))
        },
    )?;
    linker.func_wrap(
        MODULE,
        "environ_get",
        |mut c: Caller<'_, GuestState>, environ: i32, buf: i32| {
            let items = c.data().env.clone();
            ret(write_string_table(&mut c, &items, environ, buf))
        },
    )?;

    // clocks / random / scheduling
    linker.func_wrap(
        MODULE,
        "clock_res_get",
        |mut c: Caller<'_, GuestState>, _id: i32, out: i32| ret(write_u64(&mut c, out, 1_000)),
    )?;
    linker.func_wrap(
        MODULE,
        "clock_time_get",
        |mut c: Caller<'_, GuestState>, id: i32, _precision: i64, out: i32| {
            let now = match id as u32 {
                CLOCK_REALTIME => nanos(SystemTime::now()),
                CLOCK_MONOTONIC => monotonic_nanos(),
                // Process/thread CPU clocks: time since this call started.
                _ => u64::try_from(c.data().started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            };
            ret(write_u64(&mut c, out, now))
        },
    )?;
    linker.func_wrap(
        MODULE,
        "random_get",
        |mut c: Caller<'_, GuestState>, buf: i32, len: i32| {
            let r = (|| {
                let len = usize::try_from(len).map_err(|_| EINVAL)?;
                if len > 1 << 20 {
                    return Err(EINVAL);
                }
                let mut bytes = vec![0u8; len];
                getrandom::fill(&mut bytes).map_err(|_| EIO)?;
                write_bytes(&mut c, buf, &bytes)
            })();
            ret(r)
        },
    )?;
    linker.func_wrap(MODULE, "sched_yield", |_c: Caller<'_, GuestState>| -> i32 {
        0
    })?;
    linker.func_wrap(
        MODULE,
        "proc_exit",
        |_c: Caller<'_, GuestState>, code: i32| -> wasmtime::Result<()> {
            Err(wasmtime::Error::new(ProcExit(code)))
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "poll_oneoff",
        |mut c: Caller<'_, GuestState>, (subs, events, n, nevents): (i32, i32, i32, i32)| {
            Box::new(async move { ret(poll_oneoff(&mut c, subs, events, n, nevents).await) })
        },
    )?;

    // sockets: never available
    for (name, arity) in [
        ("sock_accept", 3usize),
        ("sock_recv", 6),
        ("sock_send", 5),
        ("sock_shutdown", 2),
    ] {
        match arity {
            2 => linker.func_wrap(
                MODULE,
                name,
                |_c: Caller<'_, GuestState>, _: i32, _: i32| -> i32 { i32::from(ENOTSUP) },
            )?,
            3 => linker.func_wrap(
                MODULE,
                name,
                |_c: Caller<'_, GuestState>, _: i32, _: i32, _: i32| -> i32 { i32::from(ENOTSUP) },
            )?,
            5 => linker.func_wrap(
                MODULE,
                name,
                |_c: Caller<'_, GuestState>, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 {
                    i32::from(ENOTSUP)
                },
            )?,
            _ => linker.func_wrap(
                MODULE,
                name,
                |_c: Caller<'_, GuestState>,
                 _: i32,
                 _: i32,
                 _: i32,
                 _: i32,
                 _: i32,
                 _: i32|
                 -> i32 { i32::from(ENOTSUP) },
            )?,
        };
    }

    // fd: metadata
    linker.func_wrap(
        MODULE,
        "fd_prestat_get",
        |mut c: Caller<'_, GuestState>, fd: i32, buf: i32| {
            let r = match c.data().fd(fd as u32) {
                Ok(Fd::Root) => {
                    let mut b = [0u8; 8];
                    b[4..8].copy_from_slice(&1u32.to_le_bytes());
                    write_bytes(&mut c, buf, &b)
                }
                Ok(_) => Err(EBADF),
                Err(e) => Err(e),
            };
            ret(r)
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_prestat_dir_name",
        |mut c: Caller<'_, GuestState>, fd: i32, buf: i32, len: i32| {
            let r = match c.data().fd(fd as u32) {
                Ok(Fd::Root) if len >= 1 => write_bytes(&mut c, buf, b"/"),
                Ok(Fd::Root) => Err(EINVAL),
                Ok(_) => Err(EBADF),
                Err(e) => Err(e),
            };
            ret(r)
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_fdstat_get",
        |mut c: Caller<'_, GuestState>, fd: i32, buf: i32| {
            let r = (|| {
                let (filetype, flags, rights) = match c.data().fd(fd as u32)? {
                    // Pipes, not ttys: `isatty()` is false, like stdio in a bash pipeline.
                    Fd::Stdin => (FILETYPE_UNKNOWN, 0u16, RIGHTS_FD_READ),
                    Fd::Stdout | Fd::Stderr => (FILETYPE_UNKNOWN, 0, RIGHTS_FD_WRITE),
                    Fd::Root | Fd::Dir(_) => (FILETYPE_DIRECTORY, 0, RIGHTS_ALL),
                    Fd::File(f) => (
                        FILETYPE_REGULAR_FILE,
                        if f.append { FDFLAGS_APPEND } else { 0 },
                        RIGHTS_ALL,
                    ),
                };
                let mut b = [0u8; 24];
                b[0] = filetype;
                b[2..4].copy_from_slice(&flags.to_le_bytes());
                b[8..16].copy_from_slice(&rights.to_le_bytes());
                b[16..24].copy_from_slice(&rights.to_le_bytes());
                write_bytes(&mut c, buf, &b)
            })();
            ret(r)
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_fdstat_set_flags",
        |mut c: Caller<'_, GuestState>, fd: i32, flags: i32| {
            let r = match c.data_mut().fd_mut(fd as u32) {
                Ok(Fd::File(f)) => {
                    f.append = (flags as u16) & FDFLAGS_APPEND != 0;
                    Ok(())
                }
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };
            ret(r)
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "fd_filestat_get",
        |mut c: Caller<'_, GuestState>, (fd, buf): (i32, i32)| {
            Box::new(async move {
                let r = async {
                    let stat = match c.data().fd(fd as u32)? {
                        Fd::File(f) => Stat {
                            filetype: FILETYPE_REGULAR_FILE,
                            size: f.data.len() as u64,
                            mtime: nanos(f.modified),
                            ino: inode(&f.path),
                        },
                        Fd::Root => Stat {
                            filetype: FILETYPE_DIRECTORY,
                            size: 0,
                            mtime: 0,
                            ino: inode(Path::new("/")),
                        },
                        Fd::Dir(d) => {
                            let path = d.path.clone();
                            stat_path(c.data(), &path).await?
                        }
                        Fd::Stdin | Fd::Stdout | Fd::Stderr => Stat {
                            filetype: FILETYPE_UNKNOWN,
                            size: 0,
                            mtime: 0,
                            ino: u64::from(fd as u32) + 1,
                        },
                    };
                    write_bytes(&mut c, buf, &encode_filestat(&stat))
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_filestat_set_size",
        |mut c: Caller<'_, GuestState>, fd: i32, size: i64| {
            let r = (|| {
                let size = usize::try_from(size).map_err(|_| EINVAL)?;
                let state = c.data_mut();
                let max = state.max_file_size;
                let current = match state.fd(fd as u32)? {
                    Fd::File(f) if f.write => f.data.len(),
                    Fd::File(_) => return Err(EBADF),
                    _ => return Err(EINVAL),
                };
                if size > max {
                    return Err(EFBIG);
                }
                if size > current {
                    state.reserve(size - current)?;
                } else {
                    state.release(current - size);
                }
                if let Fd::File(f) = state.fd_mut(fd as u32)? {
                    f.data.to_mut().resize(size, 0);
                    f.dirty = true;
                }
                Ok(())
            })();
            ret(r)
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_filestat_set_times",
        |_c: Caller<'_, GuestState>, _fd: i32, _atim: i64, _mtim: i64, _flags: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_advise",
        |c: Caller<'_, GuestState>, fd: i32, _off: i64, _len: i64, _advice: i32| -> i32 {
            ret(c.data().fd(fd as u32).map(|_| ()))
        },
    )?;

    // fd: data
    linker.func_wrap(
        MODULE,
        "fd_read",
        |mut c: Caller<'_, GuestState>, fd: i32, iovs: i32, n: i32, nread: i32| {
            ret(fd_read(&mut c, fd as u32, iovs, n, None)
                .and_then(|k| write_u32(&mut c, nread, k as u32)))
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_pread",
        |mut c: Caller<'_, GuestState>, fd: i32, iovs: i32, n: i32, offset: i64, nread: i32| {
            let r = u64::try_from(offset)
                .map_err(|_| EINVAL)
                .and_then(|off| fd_read(&mut c, fd as u32, iovs, n, Some(off)))
                .and_then(|k| write_u32(&mut c, nread, k as u32));
            ret(r)
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_write",
        |mut c: Caller<'_, GuestState>, fd: i32, iovs: i32, n: i32, nwritten: i32| {
            ret(fd_write(&mut c, fd as u32, iovs, n, None)
                .and_then(|k| write_u32(&mut c, nwritten, k as u32)))
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_pwrite",
        |mut c: Caller<'_, GuestState>, fd: i32, iovs: i32, n: i32, offset: i64, nwritten: i32| {
            let r = u64::try_from(offset)
                .map_err(|_| EINVAL)
                .and_then(|off| fd_write(&mut c, fd as u32, iovs, n, Some(off)))
                .and_then(|k| write_u32(&mut c, nwritten, k as u32));
            ret(r)
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_seek",
        |mut c: Caller<'_, GuestState>, fd: i32, offset: i64, whence: i32, out: i32| {
            let r = (|| {
                let file = match c.data_mut().fd_mut(fd as u32)? {
                    Fd::File(f) => f,
                    Fd::Stdin | Fd::Stdout | Fd::Stderr => return Err(ESPIPE),
                    _ => return Err(EBADF),
                };
                let base: i128 = match whence {
                    0 => 0,
                    1 => i128::from(file.pos),
                    2 => file.data.len() as i128,
                    _ => return Err(EINVAL),
                };
                let next = base + i128::from(offset);
                if next < 0 {
                    return Err(EINVAL);
                }
                file.pos = u64::try_from(next).map_err(|_| EINVAL)?;
                let pos = file.pos;
                write_u64(&mut c, out, pos)
            })();
            ret(r)
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_tell",
        |mut c: Caller<'_, GuestState>, fd: i32, out: i32| {
            let r = match c.data().fd(fd as u32) {
                Ok(Fd::File(f)) => {
                    let pos = f.pos;
                    write_u64(&mut c, out, pos)
                }
                Ok(Fd::Stdin | Fd::Stdout | Fd::Stderr) => Err(ESPIPE),
                Ok(_) => Err(EBADF),
                Err(e) => Err(e),
            };
            ret(r)
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "fd_close",
        |mut c: Caller<'_, GuestState>, (fd,): (i32,)| {
            Box::new(async move { ret(fd_close(&mut c, fd as u32).await) })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "fd_sync",
        |mut c: Caller<'_, GuestState>, (fd,): (i32,)| {
            Box::new(async move { ret(fd_sync(&mut c, fd as u32).await) })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "fd_datasync",
        |mut c: Caller<'_, GuestState>, (fd,): (i32,)| {
            Box::new(async move { ret(fd_sync(&mut c, fd as u32).await) })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "fd_readdir",
        |mut c: Caller<'_, GuestState>, (fd, buf, len, cookie, used): (i32, i32, i32, i64, i32)| {
            Box::new(
                async move { ret(fd_readdir(&mut c, fd as u32, buf, len, cookie, used).await) },
            )
        },
    )?;

    // paths
    linker.func_wrap_async(
        MODULE,
        "path_open",
        |mut c: Caller<'_, GuestState>,
         (dirfd, _dirflags, path, path_len, oflags, rights, _inherit, fdflags, out): (
            i32,
            i32,
            i32,
            i32,
            i32,
            i64,
            i64,
            i32,
            i32,
        )| {
            Box::new(async move {
                let r = async {
                    let raw = read_bytes(&mut c, path, path_len)?;
                    let fd = path_open(
                        &mut c,
                        dirfd as u32,
                        &raw,
                        oflags as u16,
                        rights as u64,
                        fdflags as u16,
                    )
                    .await?;
                    write_u32(&mut c, out, fd)
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_filestat_get",
        |mut c: Caller<'_, GuestState>,
         (dirfd, _flags, path, path_len, buf): (i32, i32, i32, i32, i32)| {
            Box::new(async move {
                let r = async {
                    let raw = read_bytes(&mut c, path, path_len)?;
                    let p = c.data().resolve(dirfd as u32, &raw)?;
                    let stat = stat_path(c.data(), &p).await?;
                    write_bytes(&mut c, buf, &encode_filestat(&stat))
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_filestat_set_times",
        |mut c: Caller<'_, GuestState>,
         (dirfd, _flags, path, path_len, _atim, mtim, fst): (i32, i32, i32, i32, i64, i64, i32)| {
            Box::new(async move {
                let r = async {
                    let raw = read_bytes(&mut c, path, path_len)?;
                    let p = c.data().resolve(dirfd as u32, &raw)?;
                    let fst = fst as u16;
                    if c.data().is_stdlib(&p) {
                        return Err(EROFS);
                    }
                    let when = if fst & FSTFLAGS_MTIM_NOW != 0 {
                        Some(SystemTime::now())
                    } else if fst & FSTFLAGS_MTIM != 0 {
                        Some(UNIX_EPOCH + Duration::from_nanos(mtim as u64))
                    } else {
                        None
                    };
                    let fs = c.data().fs.clone();
                    fs.stat(&p).await.map_err(|e| map_fs_error(&e))?;
                    if let Some(t) = when {
                        fs.set_modified_time(&p, t).await.map_err(|e| map_fs_error(&e))?;
                    }
                    Ok(())
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_create_directory",
        |mut c: Caller<'_, GuestState>, (dirfd, path, path_len): (i32, i32, i32)| {
            Box::new(async move {
                let r = async {
                    let raw = read_bytes(&mut c, path, path_len)?;
                    let p = c.data().resolve(dirfd as u32, &raw)?;
                    if c.data().is_stdlib(&p) || c.data().is_stdlib_ancestor(&p) {
                        let fs = c.data().fs.clone();
                        if fs.stat(&p).await.is_ok() || c.data().is_stdlib(&p) {
                            return Err(EEXIST);
                        }
                    }
                    let fs = c.data().fs.clone();
                    if fs.stat(&p).await.is_ok() {
                        return Err(EEXIST);
                    }
                    fs.mkdir(&p, false).await.map_err(|e| map_fs_error(&e))
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_remove_directory",
        |mut c: Caller<'_, GuestState>, (dirfd, path, path_len): (i32, i32, i32)| {
            Box::new(async move {
                let r = async {
                    let raw = read_bytes(&mut c, path, path_len)?;
                    let p = c.data().resolve(dirfd as u32, &raw)?;
                    if p == Path::new("/") || c.data().is_stdlib_ancestor(&p) {
                        return Err(EACCES);
                    }
                    let fs = c.data().fs.clone();
                    let meta = fs.stat(&p).await.map_err(|e| map_fs_error(&e))?;
                    if !meta.file_type.is_dir() {
                        return Err(ENOTDIR);
                    }
                    let entries = fs.read_dir(&p).await.map_err(|e| map_fs_error(&e))?;
                    if !entries.is_empty() {
                        return Err(ENOTEMPTY);
                    }
                    fs.remove(&p, false).await.map_err(|e| map_fs_error(&e))
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_unlink_file",
        |mut c: Caller<'_, GuestState>, (dirfd, path, path_len): (i32, i32, i32)| {
            Box::new(async move {
                let r = async {
                    let raw = read_bytes(&mut c, path, path_len)?;
                    let p = c.data().resolve(dirfd as u32, &raw)?;
                    if c.data().is_stdlib(&p) {
                        return Err(EROFS);
                    }
                    let fs = c.data().fs.clone();
                    let meta = fs.stat(&p).await.map_err(|e| map_fs_error(&e))?;
                    if meta.file_type.is_dir() {
                        return Err(EISDIR);
                    }
                    fs.remove(&p, false).await.map_err(|e| map_fs_error(&e))
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_rename",
        |mut c: Caller<'_, GuestState>,
         (fd1, old, old_len, fd2, new, new_len): (i32, i32, i32, i32, i32, i32)| {
            Box::new(async move {
                let r = async {
                    let old_raw = read_bytes(&mut c, old, old_len)?;
                    let new_raw = read_bytes(&mut c, new, new_len)?;
                    let from = c.data().resolve(fd1 as u32, &old_raw)?;
                    let to = c.data().resolve(fd2 as u32, &new_raw)?;
                    if c.data().is_stdlib(&from) || c.data().is_stdlib(&to) {
                        return Err(EROFS);
                    }
                    let fs = c.data().fs.clone();
                    fs.rename(&from, &to).await.map_err(|e| map_fs_error(&e))
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_symlink",
        |mut c: Caller<'_, GuestState>,
         (old, old_len, dirfd, new, new_len): (i32, i32, i32, i32, i32)| {
            Box::new(async move {
                let r = async {
                    let target = read_bytes(&mut c, old, old_len)?;
                    let target = String::from_utf8(target).map_err(|_| EILSEQ)?;
                    let new_raw = read_bytes(&mut c, new, new_len)?;
                    let link = c.data().resolve(dirfd as u32, &new_raw)?;
                    if c.data().is_stdlib(&link) {
                        return Err(EROFS);
                    }
                    let fs = c.data().fs.clone();
                    fs.symlink(Path::new(&target), &link)
                        .await
                        .map_err(|e| map_fs_error(&e))
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap_async(
        MODULE,
        "path_readlink",
        |mut c: Caller<'_, GuestState>,
         (dirfd, path, path_len, buf, buf_len, used): (i32, i32, i32, i32, i32, i32)| {
            Box::new(async move {
                let r = async {
                    let raw = read_bytes(&mut c, path, path_len)?;
                    let p = c.data().resolve(dirfd as u32, &raw)?;
                    let fs = c.data().fs.clone();
                    let target = fs.read_link(&p).await.map_err(|e| map_fs_error(&e))?;
                    let bytes = target.to_string_lossy().into_owned().into_bytes();
                    let n = bytes.len().min(buf_len.max(0) as usize);
                    write_bytes(&mut c, buf, &bytes[..n])?;
                    write_u32(&mut c, used, n as u32)
                }
                .await;
                ret(r)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_link",
        |_c: Caller<'_, GuestState>,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32|
         -> i32 { i32::from(ENOTSUP) },
    )?;
    Ok(())
}

fn monotonic_nanos() -> u64 {
    static BASE: std::sync::OnceLock<crate::time_compat::Instant> = std::sync::OnceLock::new();
    let base = BASE.get_or_init(crate::time_compat::Instant::now);
    u64::try_from(base.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn write_string_table(
    c: &mut Caller<'_, GuestState>,
    items: &[Vec<u8>],
    ptrs: i32,
    buf: i32,
) -> Result<(), Errno> {
    let mut offset = buf as u32;
    let mut pointers = Vec::with_capacity(items.len() * 4);
    let mut blob = Vec::new();
    for item in items {
        pointers.extend_from_slice(&offset.to_le_bytes());
        blob.extend_from_slice(item);
        blob.push(0);
        offset = offset.checked_add(item.len() as u32 + 1).ok_or(E2BIG)?;
    }
    write_bytes(c, ptrs, &pointers)?;
    write_bytes(c, buf, &blob)
}

fn fd_read(
    c: &mut Caller<'_, GuestState>,
    fd: u32,
    iovs: i32,
    n: i32,
    offset: Option<u64>,
) -> Result<usize, Errno> {
    let iovs = read_iovs(c, iovs, n)?;
    let want = total_len(&iovs);
    let chunk: Vec<u8> = {
        let state = c.data_mut();
        match state.fd_mut(fd)? {
            Fd::Stdin => {
                if offset.is_some() {
                    return Err(ESPIPE);
                }
                let start = state.stdin_pos.min(state.stdin.len());
                let end = start.saturating_add(want).min(state.stdin.len());
                state.stdin_pos = end;
                state.stdin[start..end].to_vec()
            }
            Fd::File(f) => {
                if !f.read {
                    return Err(EBADF);
                }
                let pos = offset.unwrap_or(f.pos);
                let start = usize::try_from(pos).unwrap_or(usize::MAX).min(f.data.len());
                let end = start.saturating_add(want).min(f.data.len());
                if offset.is_none() {
                    f.pos = end as u64;
                }
                f.data[start..end].to_vec()
            }
            Fd::Root | Fd::Dir(_) => return Err(EISDIR),
            Fd::Stdout | Fd::Stderr => return Err(EBADF),
        }
    };
    scatter(c, &iovs, &chunk)
}

fn fd_write(
    c: &mut Caller<'_, GuestState>,
    fd: u32,
    iovs: i32,
    n: i32,
    offset: Option<u64>,
) -> Result<usize, Errno> {
    let iovs = read_iovs(c, iovs, n)?;
    let data = gather(c, &iovs)?;
    let state = c.data_mut();
    let max = state.max_file_size;
    match state.fd(fd)? {
        Fd::Stdout | Fd::Stderr => {
            if offset.is_some() {
                return Err(ESPIPE);
            }
            state.output.write(fd, &data);
            return Ok(data.len());
        }
        Fd::File(f) if f.write => {
            let pos = if f.append {
                f.data.len() as u64
            } else {
                offset.unwrap_or(f.pos)
            };
            let start = usize::try_from(pos).map_err(|_| EFBIG)?;
            let end = start.checked_add(data.len()).ok_or(EFBIG)?;
            if end > max {
                return Err(EFBIG);
            }
            let grow = end.saturating_sub(f.data.len());
            state.reserve(grow)?;
        }
        Fd::File(_) => return Err(EBADF),
        Fd::Stdin => return Err(EBADF),
        Fd::Root | Fd::Dir(_) => return Err(EISDIR),
    }
    let Fd::File(f) = state.fd_mut(fd)? else {
        return Err(EBADF);
    };
    let pos = if f.append {
        f.data.len()
    } else {
        usize::try_from(offset.unwrap_or(f.pos)).map_err(|_| EFBIG)?
    };
    let end = pos + data.len();
    let buf = f.data.to_mut();
    if buf.len() < end {
        buf.resize(end, 0);
    }
    buf[pos..end].copy_from_slice(&data);
    f.dirty = true;
    f.modified = SystemTime::now();
    if offset.is_none() {
        f.pos = end as u64;
    }
    Ok(data.len())
}

async fn fd_close(c: &mut Caller<'_, GuestState>, fd: u32) -> Result<(), Errno> {
    let state = c.data_mut();
    let slot = state.fds.get_mut(fd as usize).ok_or(EBADF)?;
    let entry = slot.take().ok_or(EBADF)?;
    if let Fd::File(file) = entry {
        let len = match &file.data {
            Cow::Owned(v) => v.len(),
            Cow::Borrowed(_) => 0,
        };
        state.release(len);
        if file.dirty {
            let fs = state.fs.clone();
            fs.write_file(&file.path, &file.data)
                .await
                .map_err(|e| map_fs_error(&e))?;
        }
    }
    Ok(())
}

async fn fd_sync(c: &mut Caller<'_, GuestState>, fd: u32) -> Result<(), Errno> {
    let state = c.data_mut();
    let fs = state.fs.clone();
    if let Fd::File(f) = state.fd_mut(fd)?
        && f.dirty
    {
        fs.write_file(&f.path, &f.data)
            .await
            .map_err(|e| map_fs_error(&e))?;
        f.dirty = false;
    }
    Ok(())
}

async fn fd_readdir(
    c: &mut Caller<'_, GuestState>,
    fd: u32,
    buf: i32,
    len: i32,
    cookie: i64,
    used: i32,
) -> Result<(), Errno> {
    let (path, cached) = match c.data().fd(fd)? {
        Fd::Root => (PathBuf::from("/"), None),
        Fd::Dir(d) => (d.path.clone(), d.entries.clone()),
        _ => return Err(ENOTDIR),
    };
    let entries = match cached {
        Some(e) => e,
        None => {
            let state = c.data();
            let mut list = vec![
                (".".to_string(), FILETYPE_DIRECTORY, inode(&path)),
                (
                    "..".to_string(),
                    FILETYPE_DIRECTORY,
                    inode(path.parent().unwrap_or(&path)),
                ),
            ];
            let mut names: Vec<(String, u8, u64)> = match state.fs.read_dir(&path).await {
                Ok(items) => items
                    .into_iter()
                    .map(|e| {
                        let child = path.join(&e.name);
                        (e.name, filetype_of(e.metadata.file_type), inode(&child))
                    })
                    .collect(),
                Err(e) if state.is_stdlib_ancestor(&path) => {
                    let _ = e;
                    Vec::new()
                }
                Err(e) => return Err(map_fs_error(&e)),
            };
            // Surface the stdlib overlay in its parent listing.
            if state.stdlib_path.parent() == Some(path.as_path())
                && let Some(name) = state.stdlib_path.file_name().and_then(|n| n.to_str())
                && !names.iter().any(|(n, _, _)| n == name)
            {
                names.push((
                    name.to_string(),
                    FILETYPE_REGULAR_FILE,
                    inode(state.stdlib_path),
                ));
            }
            names.sort_by(|a, b| a.0.cmp(&b.0));
            list.extend(names);
            if let Ok(Fd::Dir(d)) = c.data_mut().fd_mut(fd) {
                d.entries = Some(list.clone());
            }
            list
        }
    };
    let cap = usize::try_from(len).map_err(|_| EINVAL)?;
    let start = usize::try_from(cookie).map_err(|_| EINVAL)?;
    let mut out = Vec::with_capacity(cap.min(64 * 1024));
    for (i, (name, ty, ino)) in entries.iter().enumerate().skip(start) {
        if out.len() >= cap {
            break;
        }
        let mut header = [0u8; 24];
        header[0..8].copy_from_slice(&((i + 1) as u64).to_le_bytes());
        header[8..16].copy_from_slice(&ino.to_le_bytes());
        header[16..20].copy_from_slice(&(name.len() as u32).to_le_bytes());
        header[20] = *ty;
        out.extend_from_slice(&header);
        out.extend_from_slice(name.as_bytes());
    }
    out.truncate(cap);
    write_bytes(c, buf, &out)?;
    write_u32(c, used, out.len() as u32)
}

async fn path_open(
    c: &mut Caller<'_, GuestState>,
    dirfd: u32,
    raw: &[u8],
    oflags: u16,
    rights: u64,
    fdflags: u16,
) -> Result<u32, Errno> {
    let path = c.data().resolve(dirfd, raw)?;
    let want_dir = oflags & OFLAGS_DIRECTORY != 0;
    let creat = oflags & OFLAGS_CREAT != 0;
    let excl = oflags & OFLAGS_EXCL != 0;
    let trunc = oflags & OFLAGS_TRUNC != 0;
    let append = fdflags & FDFLAGS_APPEND != 0;
    let write = rights & RIGHTS_FD_WRITE != 0 || trunc || append;
    let read = rights & (RIGHTS_FD_READ | RIGHTS_FD_READDIR) != 0 || !write;

    if c.data().is_stdlib(&path) {
        if write || creat || trunc {
            return Err(EROFS);
        }
        if want_dir {
            return Err(ENOTDIR);
        }
        if creat && excl {
            return Err(EEXIST);
        }
        let stdlib = c.data().stdlib;
        return c.data_mut().alloc_fd(Fd::File(Box::new(OpenFile {
            path,
            data: Cow::Borrowed(stdlib),
            pos: 0,
            read: true,
            write: false,
            append: false,
            dirty: false,
            modified: UNIX_EPOCH,
        })));
    }

    let fs = c.data().fs.clone();
    let meta = fs.stat(&path).await;
    match meta {
        Ok(m) if m.file_type.is_dir() => {
            if creat && excl {
                return Err(EEXIST);
            }
            if write {
                return Err(EISDIR);
            }
            c.data_mut().alloc_fd(Fd::Dir(OpenDir {
                path,
                entries: None,
            }))
        }
        Ok(m) => {
            if want_dir {
                return Err(ENOTDIR);
            }
            if creat && excl {
                return Err(EEXIST);
            }
            let data = if trunc {
                Vec::new()
            } else {
                let bytes = fs.read_file(&path).await.map_err(|e| map_fs_error(&e))?;
                c.data_mut().reserve(bytes.len())?;
                bytes
            };
            c.data_mut().alloc_fd(Fd::File(Box::new(OpenFile {
                path,
                data: Cow::Owned(data),
                pos: 0,
                read,
                write,
                append,
                dirty: trunc && m.size > 0,
                modified: m.modified,
            })))
        }
        Err(e) => {
            if c.data().is_stdlib_ancestor(&path) {
                if want_dir || !write {
                    return c.data_mut().alloc_fd(Fd::Dir(OpenDir {
                        path,
                        entries: None,
                    }));
                }
                return Err(EISDIR);
            }
            let errno = map_fs_error(&e);
            if errno != ENOENT || !creat || want_dir {
                return Err(errno);
            }
            // O_CREAT: the parent must be an existing directory.
            let parent = path.parent().unwrap_or(Path::new("/"));
            match fs.stat(parent).await {
                Ok(p) if p.file_type.is_dir() => {}
                Ok(_) => return Err(ENOTDIR),
                Err(_) => return Err(ENOENT),
            }
            fs.write_file(&path, b"")
                .await
                .map_err(|e| map_fs_error(&e))?;
            c.data_mut().alloc_fd(Fd::File(Box::new(OpenFile {
                path,
                data: Cow::Owned(Vec::new()),
                pos: 0,
                read,
                write,
                append,
                dirty: false,
                modified: SystemTime::now(),
            })))
        }
    }
}

async fn poll_oneoff(
    c: &mut Caller<'_, GuestState>,
    subs: i32,
    events: i32,
    n: i32,
    nevents: i32,
) -> Result<(), Errno> {
    let n = u32::try_from(n).map_err(|_| EINVAL)?;
    if n == 0 || n > 64 {
        return Err(EINVAL);
    }
    let raw = read_bytes(c, subs, i32::try_from(n * 48).map_err(|_| EINVAL)?)?;
    let mut ready = Vec::new();
    let mut sleep: Option<(Duration, u64)> = None;
    for sub in raw.chunks_exact(48) {
        let userdata = u64::from_le_bytes(sub[0..8].try_into().map_err(|_| EINVAL)?);
        match sub[8] {
            EVENTTYPE_CLOCK => {
                let clock = u32::from_le_bytes(sub[16..20].try_into().map_err(|_| EINVAL)?);
                let timeout = u64::from_le_bytes(sub[24..32].try_into().map_err(|_| EINVAL)?);
                let flags = u16::from_le_bytes(sub[40..42].try_into().map_err(|_| EINVAL)?);
                let wait = if flags & SUBCLOCKFLAGS_ABSTIME != 0 {
                    let now = if clock == CLOCK_REALTIME {
                        nanos(SystemTime::now())
                    } else {
                        monotonic_nanos()
                    };
                    timeout.saturating_sub(now)
                } else {
                    timeout
                };
                let wait = Duration::from_nanos(wait);
                if sleep.is_none_or(|(d, _)| wait < d) {
                    sleep = Some((wait, userdata));
                }
            }
            kind @ (EVENTTYPE_FD_READ | EVENTTYPE_FD_WRITE) => {
                let fd = u32::from_le_bytes(sub[16..20].try_into().map_err(|_| EINVAL)?);
                let (error, nbytes) = match c.data().fd(fd) {
                    Ok(Fd::Stdin) => (SUCCESS, (c.data().stdin.len() - c.data().stdin_pos) as u64),
                    Ok(Fd::File(f)) => {
                        (SUCCESS, f.data.len().saturating_sub(f.pos as usize) as u64)
                    }
                    Ok(_) => (SUCCESS, 0),
                    Err(e) => (e, 0),
                };
                ready.push((userdata, error, kind, nbytes));
            }
            _ => return Err(EINVAL),
        }
    }
    if ready.is_empty()
        && let Some((wait, userdata)) = sleep
    {
        // Sleep in slices, never past the call deadline; the caller's
        // timeout/budget wrapper observes every wake-up.
        let target = crate::time_compat::Instant::now() + wait;
        loop {
            let now = crate::time_compat::Instant::now();
            if now >= target || now >= c.data().deadline {
                break;
            }
            let slice = (target - now).min(MAX_SLEEP_SLICE);
            tokio::time::sleep(slice).await;
        }
        ready.push((userdata, SUCCESS, EVENTTYPE_CLOCK, 0));
    }
    let mut out = Vec::with_capacity(ready.len() * 32);
    for (userdata, error, kind, nbytes) in &ready {
        let mut e = [0u8; 32];
        e[0..8].copy_from_slice(&userdata.to_le_bytes());
        e[8..10].copy_from_slice(&error.to_le_bytes());
        e[10] = *kind;
        e[16..24].copy_from_slice(&nbytes.to_le_bytes());
        out.extend_from_slice(&e);
    }
    write_bytes(c, events, &out)?;
    write_u32(c, nevents, ready.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_clamps_parent_traversal() {
        assert_eq!(
            normalize(Path::new("/"), "../../etc/passwd"),
            PathBuf::from("/etc/passwd")
        );
        assert_eq!(normalize(Path::new("/a/b"), "../c"), PathBuf::from("/a/c"));
        assert_eq!(
            normalize(Path::new("/a"), "./x/./y/.."),
            PathBuf::from("/a/x")
        );
        assert_eq!(normalize(Path::new("/a"), "/abs"), PathBuf::from("/abs"));
        assert_eq!(normalize(Path::new("/"), ""), PathBuf::from("/"));
    }

    #[test]
    fn output_cap_truncates_and_flags() {
        let mut out = Output {
            cap: 4,
            ..Output::default()
        };
        out.write(1, b"abc");
        out.write(2, b"def");
        assert_eq!(out.stdout, b"abc");
        assert_eq!(out.stderr, b"d");
        assert!(out.truncated);
    }

    #[test]
    fn fs_errors_map_to_errno() {
        let nf: crate::Error = std::io::Error::new(std::io::ErrorKind::NotFound, "x").into();
        assert_eq!(map_fs_error(&nf), ENOENT);
        let ex: crate::Error = std::io::Error::new(std::io::ErrorKind::AlreadyExists, "x").into();
        assert_eq!(map_fs_error(&ex), EEXIST);
        let dir: crate::Error = std::io::Error::other("is a directory").into();
        assert_eq!(map_fs_error(&dir), EISDIR);
        let other: crate::Error = std::io::Error::other("boom").into();
        assert_eq!(map_fs_error(&other), EIO);
    }
}
