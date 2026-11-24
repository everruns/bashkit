//! Lazy, JS-callback-backed filesystem (`FileSystem.lazy()`).
//!
//! The host describes a tree with two callbacks: `readDir(path)` lists one
//! directory and `readFile(path)` returns one file's bytes. Nothing is fetched
//! up front. A directory is listed the first time a script lists, stats or
//! resolves something inside it; a file's content is fetched the first time a
//! script reads it. Results are cached per handle (bounded, see below), and
//! concurrent first accesses to one path share a single host call.
//!
//! The filesystem is read-only. Writable overlays are deliberately not offered
//! yet: `OverlayFs` pre-creates `/tmp`, `/home` and `/dev` in its upper layer,
//! which would leak into a mount, and it applies no POSIX type checks to the
//! merged view. TODO: add `writable` once an empty-upper overlay exists.
//!
//! Decision: callbacks dispatch through a threadsafe function onto the Node
//! event loop. Sync entry points (`executeSync()`, `bash.readFile()`, ...)
//! block that loop, so a callback they wait on could never run. Before a
//! cache miss takes the per-path lock or dispatches, [`JsLazyBackend::preflight`]
//! decides:
//! - on the owning JS thread itself: fail at once (a sync entry point is
//!   polling this future);
//! - inside an execution's task ([`in_execution`]): proceed. No sync caller
//!   can be waiting on this task, so if the loop is busy the callback simply
//!   runs once it is free;
//! - anywhere else (sqlite's VFS I/O thread, the C ABI export thread, tasks
//!   spawned outside the execution scope) while the owning JS thread is
//!   inside a sync entry point: wait up to [`SYNC_BLOCK_GRACE`], then fail,
//!   because that sync call may be waiting on this very operation. Later
//!   such operations in the same blocked period fail at once.
//!
//! Lazy handles are mounted directly (`mountLazyFileSystem`) and refuse
//! `toExternal()`: the C ABI bridge blocks a runtime worker per operation,
//! which pending promises would starve.
//!
//! `timeoutMs` bounds a pending callback (the interpreter future is dropped).
//! `bash.cancel()` is cooperative and filesystem backends cannot observe it,
//! so cancellation takes effect once pending callbacks settle, as with other
//! slow backends. TODO: expose the execution budget to `FsBackend` operations
//! (like `RuntimeCallContext`) so pending callbacks can race cancellation.
//!
//! THREAT[TM-DOS-148]: callback output is host data, but scripts drive which
//! paths are requested, so responses and caches are bounded. Only paths a
//! parent listing declared reach the host; entry names pass the same
//! `validate_path` rules as in-memory files (TM-DOS-013/015); one listing is
//! capped at 100k entries and all cached listings at 1M entries; declared
//! sizes over `max_file_size` are rejected before fetching; file content is
//! capped by `max_total_bytes`/`max_file_count`; negative entries are capped;
//! concurrent host calls share the process-wide callback semaphore (#982).
//! Residual: the number of host calls is not capped (eviction can make
//! repeated walks re-fetch), and a callback that never settles holds a
//! semaphore permit until `timeoutMs` drops it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use base64::Engine;
use bashkit::time::{SystemTime, UNIX_EPOCH};
use bashkit::{DirEntry, FileType, FsBackend, FsLimits, FsUsage, Metadata, async_trait};
use napi::bindgen_prelude::Promise;
use serde::Deserialize;

/// Upper bound on entries returned by one `readDir` call.
const MAX_DIR_ENTRIES: usize = 100_000;
/// Upper bound on cached directory listings before oldest-first eviction.
const MAX_CACHED_DIRS: usize = 10_000;
/// Upper bound on entries across all cached listings.
const MAX_CACHED_DIR_ENTRIES: usize = 1_000_000;
/// Upper bound on remembered missing paths before the sets are cleared.
const MAX_NEGATIVE_ENTRIES: usize = 100_000;
/// How long an off-thread operation waits for a blocked owning JS thread.
const SYNC_BLOCK_GRACE: Duration = Duration::from_secs(2);

tokio::task_local! {
    static IN_EXECUTION: ();
}

/// Run an execution future so lazy filesystem reads polled by it know that
/// no synchronous caller can be waiting on them.
pub(crate) async fn in_execution<F: std::future::Future>(future: F) -> F::Output {
    IN_EXECUTION.scope((), future).await
}

fn in_execution_scope() -> bool {
    IN_EXECUTION.try_with(|_| ()).is_ok()
}

/// A JS thread inside one or more nested synchronous native calls.
#[derive(Default)]
struct BlockedThread {
    depth: usize,
    /// An off-thread operation already timed out during this blocked period,
    /// so later ones fail at once instead of each waiting the full grace.
    gave_up: bool,
}

/// JS threads currently inside a synchronous native call.
fn blocked_js_threads() -> &'static StdMutex<HashMap<ThreadId, BlockedThread>> {
    static BLOCKED: OnceLock<StdMutex<HashMap<ThreadId, BlockedThread>>> = OnceLock::new();
    BLOCKED.get_or_init(|| StdMutex::new(HashMap::new()))
}

enum JsThreadState {
    Free,
    Blocked,
    GaveUp,
}

fn js_thread_state(id: ThreadId) -> JsThreadState {
    let map = blocked_js_threads()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match map.get(&id) {
        None => JsThreadState::Free,
        Some(b) if b.gave_up => JsThreadState::GaveUp,
        Some(_) => JsThreadState::Blocked,
    }
}

fn mark_gave_up(id: ThreadId) {
    let mut map = blocked_js_threads()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(b) = map.get_mut(&id) {
        b.gave_up = true;
    }
}

/// Marks the current (JS) thread as blocked for the guard's lifetime. Held by
/// every sync entry point that blocks the event loop.
pub(crate) struct JsThreadBlocked(ThreadId);

impl JsThreadBlocked {
    pub(crate) fn enter() -> Self {
        let id = std::thread::current().id();
        let mut map = blocked_js_threads()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        map.entry(id).or_default().depth += 1;
        Self(id)
    }
}

impl Drop for JsThreadBlocked {
    fn drop(&mut self) {
        let mut map = blocked_js_threads()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(b) = map.get_mut(&self.0) {
            b.depth -= 1;
            if b.depth == 0 {
                map.remove(&self.0);
            }
        }
    }
}

pub(crate) const LAZY_FS_SYNC_ERROR: &str = "lazy filesystem callbacks require async access (execute()); synchronous calls such as executeSync() or bash.readFile() would deadlock because the JS event loop is blocked";

/// `(requestJson) => Promise<responseJson>`; the TS wrapper adapts user callbacks.
pub(crate) type LazyFsTsfn = napi::threadsafe_function::ThreadsafeFunction<
    String,
    Promise<String>,
    String,
    napi::Status,
    false,
    true,
>;

#[derive(Deserialize)]
struct WireEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    size: Option<f64>,
    mode: Option<f64>,
    modified: Option<f64>,
}

#[derive(Deserialize)]
struct WireResponse {
    #[serde(default)]
    missing: bool,
    /// Already sanitized and bounded by the TS dispatcher (TM-INF-028).
    error: Option<String>,
    entries: Option<Vec<WireEntry>>,
    text: Option<String>,
    base64: Option<String>,
}

#[derive(Default)]
struct Cache {
    dirs: HashMap<PathBuf, Arc<Vec<DirEntry>>>,
    dir_order: VecDeque<PathBuf>,
    dir_entries: usize,
    files: HashMap<PathBuf, Arc<Vec<u8>>>,
    file_order: VecDeque<PathBuf>,
    file_bytes: u64,
    /// Paths the host reported missing.
    missing_files: HashSet<PathBuf>,
    missing_dirs: HashSet<PathBuf>,
    /// Files whose fetched content exceeded `max_file_size`, with that size.
    oversize: HashMap<PathBuf, u64>,
}

impl Cache {
    fn insert_dir(&mut self, dir: PathBuf, entries: Arc<Vec<DirEntry>>) {
        self.dir_entries += entries.len();
        match self.dirs.insert(dir.clone(), entries) {
            Some(prev) => self.dir_entries -= prev.len(),
            None => self.dir_order.push_back(dir),
        }
        while (self.dirs.len() > MAX_CACHED_DIRS || self.dir_entries > MAX_CACHED_DIR_ENTRIES)
            && self.dirs.len() > 1
        {
            let Some(old) = self.dir_order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.dirs.remove(&old) {
                self.dir_entries -= evicted.len();
            }
        }
    }

    fn remember_missing_file(&mut self, path: PathBuf) {
        if self.missing_files.len() >= MAX_NEGATIVE_ENTRIES {
            self.missing_files.clear();
        }
        self.missing_files.insert(path);
    }

    fn remember_missing_dir(&mut self, dir: PathBuf) {
        if self.missing_dirs.len() >= MAX_NEGATIVE_ENTRIES {
            self.missing_dirs.clear();
        }
        self.missing_dirs.insert(dir);
    }

    /// Size learned by fetching: cached content or a rejected oversize file.
    fn known_size(&self, path: &Path) -> Option<u64> {
        self.files
            .get(path)
            .map(|c| c.len() as u64)
            .or_else(|| self.oversize.get(path).copied())
    }

    fn remember_oversize(&mut self, path: PathBuf, size: u64) {
        if self.oversize.len() >= MAX_NEGATIVE_ENTRIES {
            self.oversize.clear();
        }
        self.oversize.insert(path, size);
    }

    fn insert_file(&mut self, path: PathBuf, bytes: Arc<Vec<u8>>, limits: &FsLimits) {
        let len = bytes.len() as u64;
        if let Some(prev) = self.files.insert(path.clone(), bytes) {
            self.file_bytes -= prev.len() as u64;
        } else {
            self.file_order.push_back(path);
        }
        self.file_bytes += len;
        while (self.file_bytes > limits.max_total_bytes
            || self.files.len() as u64 > limits.max_file_count)
            && self.files.len() > 1
        {
            let Some(old) = self.file_order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.files.remove(&old) {
                self.file_bytes -= evicted.len() as u64;
            }
        }
    }
}

type InflightKey = (&'static str, PathBuf);

/// Holds a per-path in-flight lock entry and removes it from the map when
/// dropped, including when the waiting future is dropped (timeout).
struct InflightEntry<'a> {
    backend: &'a JsLazyBackend,
    key: InflightKey,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl Drop for InflightEntry<'_> {
    fn drop(&mut self) {
        self.backend.release_inflight(&self.key, &self.lock);
    }
}

pub(crate) struct JsLazyBackend {
    callback: Arc<LazyFsTsfn>,
    js_thread: ThreadId,
    limits: FsLimits,
    cache: StdMutex<Cache>,
    /// Per-request locks so concurrent first accesses share one host call.
    inflight: StdMutex<HashMap<InflightKey, Arc<tokio::sync::Mutex<()>>>>,
}

fn not_found() -> bashkit::Error {
    // "file not found" renders as "No such file or directory" (error.rs).
    IoError::new(ErrorKind::NotFound, "file not found").into()
}

fn not_a_directory() -> bashkit::Error {
    IoError::new(ErrorKind::NotADirectory, "not a directory").into()
}

fn is_a_directory() -> bashkit::Error {
    IoError::other("is a directory").into()
}

fn read_only() -> bashkit::Error {
    IoError::new(ErrorKind::PermissionDenied, "filesystem is read-only").into()
}

fn invalid(msg: impl Into<String>) -> bashkit::Error {
    IoError::new(ErrorKind::InvalidData, msg.into()).into()
}

fn callback_failed() -> bashkit::Error {
    IoError::other("lazy filesystem callback failed").into()
}

/// Normalize to an absolute path without `.`/`..` (MountableFs already strips
/// the mount prefix; this guards direct handle use too).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for comp in path.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Non-negative finite number truncated to an integer, else `None`.
fn whole(n: Option<f64>) -> Option<u64> {
    n.filter(|v| v.is_finite() && *v >= 0.0).map(|v| v as u64)
}

impl JsLazyBackend {
    pub(crate) fn new(callback: LazyFsTsfn) -> Self {
        Self {
            callback: Arc::new(callback),
            js_thread: std::thread::current().id(),
            limits: FsLimits::default(),
            cache: StdMutex::new(Cache::default()),
            inflight: StdMutex::new(HashMap::new()),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn inflight_entry(&self, op: &'static str, path: &Path) -> InflightEntry<'_> {
        let key = (op, path.to_path_buf());
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        let lock = map
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        InflightEntry {
            backend: self,
            key,
            lock,
        }
    }

    fn release_inflight(&self, key: &InflightKey, lock: &Arc<tokio::sync::Mutex<()>>) {
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        // Two strong refs: the map's and the caller's. More means waiters
        // still hold the entry; the last one out removes it. Runs under the
        // map mutex, so the count cannot change underneath us.
        if Arc::strong_count(lock) <= 2 {
            map.remove(key);
        }
    }

    /// Decide whether a host call may be made now (see the module docs).
    /// Runs before the per-path lock so a sync caller never queues behind a
    /// call that needs the event loop it is blocking.
    async fn preflight(&self) -> bashkit::Result<()> {
        let sync_error = || IoError::new(ErrorKind::WouldBlock, LAZY_FS_SYNC_ERROR).into();
        if std::thread::current().id() == self.js_thread {
            return Err(sync_error());
        }
        if in_execution_scope() {
            return Ok(());
        }
        // Decision: a plain thread sleep, never a tokio timer. Callers here
        // are helper threads (sqlite's VFS I/O, the C ABI export runtime)
        // whose runtime's timer may be driven by nobody: the JS thread is
        // blocked, or the only other worker is parked in a thread join. Such
        // threads are dedicated, so blocking them for the bounded grace is
        // safe; execution tasks never reach this loop (`in_execution`).
        let started = Instant::now();
        loop {
            match js_thread_state(self.js_thread) {
                JsThreadState::Free => return Ok(()),
                JsThreadState::GaveUp => return Err(sync_error()),
                JsThreadState::Blocked if started.elapsed() >= SYNC_BLOCK_GRACE => {
                    mark_gave_up(self.js_thread);
                    return Err(sync_error());
                }
                JsThreadState::Blocked => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }

    async fn call(&self, op: &str, path: &Path) -> bashkit::Result<WireResponse> {
        self.preflight().await?;
        // Bound concurrent JS callbacks across all instances (issue #982).
        let _permit = crate::callback_semaphore()
            .acquire()
            .await
            .map_err(|_| callback_failed())?;
        let request = serde_json::json!({ "op": op, "path": path_str(path) }).to_string();
        // TSFN/promise failures carry napi status noise; the TS dispatcher
        // already turns user errors into `{error}`, so only a generic message
        // is surfaced here (TM-INF-028).
        let promise = self
            .callback
            .call_async(request)
            .await
            .map_err(|_| callback_failed())?;
        let raw = promise.await.map_err(|_| callback_failed())?;
        let resp: WireResponse = serde_json::from_str(&raw)
            .map_err(|_| invalid("lazy filesystem callback returned a malformed response"))?;
        if let Some(message) = resp.error {
            return Err(IoError::other(message).into());
        }
        Ok(resp)
    }

    fn parse_listing(&self, dir: &Path, wire: Vec<WireEntry>) -> bashkit::Result<Vec<DirEntry>> {
        if wire.len() > MAX_DIR_ENTRIES {
            return Err(invalid(format!(
                "lazy filesystem readDir returned more than {MAX_DIR_ENTRIES} entries"
            )));
        }
        // One timestamp per listing so mtimes stay stable across stats.
        let listed_at = SystemTime::now();
        let mut seen = HashSet::new();
        let mut entries = Vec::with_capacity(wire.len());
        for e in wire {
            let name_ok = !e.name.is_empty()
                && e.name != "."
                && e.name != ".."
                && !e.name.contains('/')
                && !e.name.contains('\0')
                && self.limits.validate_path(&dir.join(&e.name)).is_ok();
            if !name_ok {
                return Err(invalid(format!(
                    "lazy filesystem readDir returned an invalid entry name {:?}",
                    e.name.chars().take(64).collect::<String>()
                )));
            }
            if !seen.insert(e.name.clone()) {
                continue;
            }
            let file_type = match e.kind.as_str() {
                "file" => FileType::File,
                "dir" | "directory" => FileType::Directory,
                other => {
                    return Err(invalid(format!(
                        "lazy filesystem readDir returned unsupported entry type {:?}",
                        other.chars().take(32).collect::<String>()
                    )));
                }
            };
            let is_dir = file_type.is_dir();
            // Out-of-range optional fields fall back to defaults rather than
            // failing the whole listing.
            let modified = whole(e.modified)
                .and_then(|ms| UNIX_EPOCH.checked_add(Duration::from_millis(ms)))
                .unwrap_or(listed_at);
            let mode = whole(e.mode)
                .map(|m| (m as u32) & 0o7777)
                .unwrap_or(if is_dir { 0o755 } else { 0o644 });
            entries.push(DirEntry {
                name: e.name,
                metadata: Metadata {
                    file_type,
                    size: if is_dir {
                        0
                    } else {
                        whole(e.size).unwrap_or(0)
                    },
                    mode,
                    modified,
                    created: modified,
                    ..Metadata::default()
                },
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn cached_listing(&self, dir: &Path) -> Option<bashkit::Result<Arc<Vec<DirEntry>>>> {
        let cache = self.cache();
        if let Some(hit) = cache.dirs.get(dir) {
            return Some(Ok(hit.clone()));
        }
        cache.missing_dirs.contains(dir).then(|| Err(not_found()))
    }

    /// List `dir` (normalized), fetching and caching on first use.
    async fn list(&self, dir: &Path) -> bashkit::Result<Arc<Vec<DirEntry>>> {
        if let Some(hit) = self.cached_listing(dir) {
            return hit;
        }
        // Only list paths whose parent says they are directories, so scripts
        // cannot make the host enumerate arbitrary invented paths.
        if dir != Path::new("/") {
            let meta = self.stat_inner(dir).await?;
            if !meta.file_type.is_dir() {
                return Err(not_a_directory());
            }
        }
        self.preflight().await?;
        let entry = self.inflight_entry("readDir", dir);
        let _guard = entry.lock.lock().await;
        if let Some(hit) = self.cached_listing(dir) {
            return hit;
        }
        let resp = self.call("readDir", dir).await?;
        if resp.missing {
            self.cache().remember_missing_dir(dir.to_path_buf());
            return Err(not_found());
        }
        let wire = resp
            .entries
            .ok_or_else(|| invalid("lazy filesystem readDir returned no entries"))?;
        let entries = Arc::new(self.parse_listing(dir, wire)?);
        self.cache().insert_dir(dir.to_path_buf(), entries.clone());
        Ok(entries)
    }

    async fn stat_inner(&self, path: &Path) -> bashkit::Result<Metadata> {
        let Some(parent) = path.parent() else {
            return Ok(Metadata {
                file_type: FileType::Directory,
                size: 0,
                mode: 0o755,
                ..Metadata::default()
            });
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let listing = Box::pin(self.list(parent)).await?;
        let entry = listing
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(not_found)?;
        let mut meta = entry.metadata.clone();
        if meta.file_type.is_file()
            && let Some(len) = self.cache().known_size(path)
        {
            meta.size = len;
        }
        Ok(meta)
    }

    async fn fetch_file(&self, path: &Path) -> bashkit::Result<Arc<Vec<u8>>> {
        let resp = self.call("readFile", path).await?;
        if resp.missing {
            self.cache().remember_missing_file(path.to_path_buf());
            return Err(not_found());
        }
        let bytes = match (resp.text, resp.base64) {
            (Some(text), _) => text.into_bytes(),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|_| invalid("lazy filesystem readFile returned invalid base64"))?,
            (None, None) => return Err(invalid("lazy filesystem readFile returned no content")),
        };
        if bytes.len() as u64 > self.limits.max_file_size {
            self.cache()
                .remember_oversize(path.to_path_buf(), bytes.len() as u64);
            return Err(self.too_large());
        }
        let bytes = Arc::new(bytes);
        self.cache()
            .insert_file(path.to_path_buf(), bytes.clone(), &self.limits);
        Ok(bytes)
    }

    fn too_large(&self) -> bashkit::Error {
        // EFBIG strerror; the limit itself is documented, not echoed.
        IoError::new(ErrorKind::FileTooLarge, "File too large").into()
    }
}

#[async_trait]
impl FsBackend for JsLazyBackend {
    async fn read(&self, path: &Path) -> bashkit::Result<Vec<u8>> {
        let path = normalize(path);
        if let Some(hit) = self.cache().files.get(&path) {
            return Ok(hit.as_ref().clone());
        }
        let meta = self.stat_inner(&path).await?;
        if meta.file_type.is_dir() {
            return Err(is_a_directory());
        }
        // Reject before transferring anything when the listing already says
        // the file is too large (no script-driven fetch amplification).
        if meta.size > self.limits.max_file_size {
            return Err(self.too_large());
        }
        self.preflight().await?;
        let entry = self.inflight_entry("readFile", &path);
        let _guard = entry.lock.lock().await;
        {
            let cache = self.cache();
            if let Some(hit) = cache.files.get(&path) {
                return Ok(hit.as_ref().clone());
            }
            if cache.missing_files.contains(&path) {
                return Err(not_found());
            }
            if cache.oversize.contains_key(&path) {
                return Err(self.too_large());
            }
        }
        self.fetch_file(&path)
            .await
            .map(|bytes| bytes.as_ref().clone())
    }

    async fn write(&self, _path: &Path, _content: &[u8]) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn append(&self, _path: &Path, _content: &[u8]) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn mkdir(&self, _path: &Path, _recursive: bool) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn remove(&self, _path: &Path, _recursive: bool) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn stat(&self, path: &Path) -> bashkit::Result<Metadata> {
        self.stat_inner(&normalize(path)).await
    }

    async fn read_dir(&self, path: &Path) -> bashkit::Result<Vec<DirEntry>> {
        let dir = normalize(path);
        let listing = self.list(&dir).await?;
        let cache = self.cache();
        Ok(listing
            .iter()
            .map(|e| {
                let mut e = e.clone();
                if e.metadata.file_type.is_file()
                    && let Some(len) = cache.known_size(&dir.join(&e.name))
                {
                    e.metadata.size = len;
                }
                e
            })
            .collect())
    }

    async fn exists(&self, path: &Path) -> bashkit::Result<bool> {
        match self.stat_inner(&normalize(path)).await {
            Ok(_) => Ok(true),
            Err(bashkit::Error::Io(e))
                if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    async fn rename(&self, _from: &Path, _to: &Path) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn copy(&self, _from: &Path, _to: &Path) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn symlink(&self, _target: &Path, _link: &Path) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn read_link(&self, _path: &Path) -> bashkit::Result<PathBuf> {
        Err(IoError::new(ErrorKind::InvalidInput, "not a symbolic link").into())
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> bashkit::Result<()> {
        Err(read_only())
    }

    async fn set_modified_time(&self, _path: &Path, _time: SystemTime) -> bashkit::Result<()> {
        Err(read_only())
    }

    /// Cached host content is not sandbox-written data, so it does not count
    /// toward quotas (like `RealFs`); the cache has its own bound.
    fn usage(&self) -> FsUsage {
        FsUsage::default()
    }

    fn limits(&self) -> FsLimits {
        self.limits.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_dot_segments() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("/../..")), PathBuf::from("/"));
        assert_eq!(normalize(Path::new("rel/x")), PathBuf::from("/rel/x"));
    }

    #[test]
    fn whole_rejects_out_of_range_numbers() {
        assert_eq!(whole(Some(12.9)), Some(12));
        assert_eq!(whole(Some(-1.0)), None);
        assert_eq!(whole(Some(f64::NAN)), None);
        assert_eq!(whole(None), None);
    }

    #[test]
    fn blocked_period_resets_after_last_guard() {
        let id = std::thread::current().id();
        assert!(matches!(js_thread_state(id), JsThreadState::Free));
        let outer = JsThreadBlocked::enter();
        let inner = JsThreadBlocked::enter();
        assert!(matches!(js_thread_state(id), JsThreadState::Blocked));
        mark_gave_up(id);
        drop(inner);
        assert!(matches!(js_thread_state(id), JsThreadState::GaveUp));
        drop(outer);
        assert!(matches!(js_thread_state(id), JsThreadState::Free));
    }

    #[test]
    fn cache_evicts_oldest_file_over_byte_budget() {
        let limits = FsLimits::new().max_total_bytes(10);
        let mut cache = Cache::default();
        cache.insert_file("/a".into(), Arc::new(vec![0; 6]), &limits);
        cache.insert_file("/b".into(), Arc::new(vec![0; 6]), &limits);
        assert!(!cache.files.contains_key(Path::new("/a")));
        assert!(cache.files.contains_key(Path::new("/b")));
        assert_eq!(cache.file_bytes, 6);
    }
}
