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
//!   runs once it is free, and the execution deadline (`timeoutMs`) bounds it;
//! - anywhere else, i.e. helper threads such as sqlite's VFS I/O thread: fail
//!   at once. Such threads block a runtime worker (or the JS thread) until
//!   they finish and cannot be reached by the execution deadline, so waiting
//!   on a promise there can starve or hang the runtime. They are served from
//!   the cache only; scripts load content first (for example `cp` it out).
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
//! sizes over `max_file_size` are rejected before fetching, and undeclared
//! ones before the TS dispatcher encodes them; raw responses are length-checked
//! before parsing; file content is
//! capped by `max_total_bytes`/`max_file_count`; negative entries are capped;
//! concurrent host calls share the process-wide callback semaphore (#982).
//! Residual: the number of host calls is not capped (eviction can make
//! repeated walks re-fetch), and a callback that never settles holds a
//! semaphore permit until `timeoutMs` drops it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::ThreadId;
use std::time::Duration;

use base64::Engine;
use bashkit::time::{SystemTime, UNIX_EPOCH};
use bashkit::{
    DirEntry, FileType, FsBackend, FsLimits, FsUsage, Metadata, async_trait, normalize_path,
    vfs_join,
};
use napi::bindgen_prelude::Promise;
use serde::Deserialize;

/// Upper bound on entries returned by one `readDir` call.
const MAX_DIR_ENTRIES: usize = 100_000;
/// Upper bound on one raw `readDir` response: `MAX_DIR_ENTRIES` maximum-length
/// names, fully escaped, plus their fields, fit well below it.
const MAX_LISTING_RESPONSE_BYTES: u64 = 128 * 1024 * 1024;
/// JSON escapes one content byte to at most six (`\u0001`); base64 needs 4/3.
const MAX_JSON_EXPANSION: u64 = 6;
/// Room for the response envelope and bounded error messages.
const RESPONSE_OVERHEAD_BYTES: u64 = 4096;
/// Upper bound on cached directory listings before oldest-first eviction.
const MAX_CACHED_DIRS: usize = 10_000;
/// Upper bound on entries across all cached listings.
const MAX_CACHED_DIR_ENTRIES: usize = 1_000_000;
/// Upper bound on remembered missing paths before the sets are cleared.
const MAX_NEGATIVE_ENTRIES: usize = 100_000;
const LAZY_EXPORT_ERROR: &str =
    "filesystems of instances with a lazy mount cannot be exported with toExternal()";
const LAZY_AFTER_EXPORT_ERROR: &str = "cannot mount a lazy filesystem into an instance whose filesystem was exported with toExternal()";

/// Per-instance bookkeeping that keeps lazy mounts off the C ABI bridge,
/// whichever happens first: a lazy mount refuses an instance whose
/// filesystem was exported, and an export refuses an instance with a lazy
/// mount. Both flags are sticky (never cleared by `unmount`).
#[derive(Default)]
pub(crate) struct LazyMountGuard {
    has_lazy_mounts: AtomicBool,
    exported: AtomicBool,
}

impl LazyMountGuard {
    pub(crate) fn note_export(&self) -> Result<(), &'static str> {
        if self.has_lazy_mounts.load(Ordering::SeqCst) {
            return Err(LAZY_EXPORT_ERROR);
        }
        self.exported.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Run `mount` unless the instance was exported. The flag is set only
    /// once the mount succeeded; exports run on the same JS thread, so none
    /// can slip in between.
    pub(crate) fn mount_lazy(&self, mount: impl FnOnce() -> napi::Result<()>) -> napi::Result<()> {
        if self.exported.load(Ordering::SeqCst) {
            return Err(napi::Error::from_reason(LAZY_AFTER_EXPORT_ERROR));
        }
        mount()?;
        self.has_lazy_mounts.store(true, Ordering::SeqCst);
        Ok(())
    }
}

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

pub(crate) const LAZY_FS_SYNC_ERROR: &str = "lazy filesystem callbacks require async access (execute()); synchronous calls such as executeSync() or bash.readFile() would deadlock because the JS event loop is blocked";
const LAZY_FS_HELPER_THREAD_ERROR: &str = "lazy file not loaded yet: helper threads (such as sqlite -backend vfs) only see lazy files a script has already read, e.g. with cat FILE > /dev/null";

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
    /// Byte size of content the dispatcher refused to encode.
    oversize: Option<f64>,
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

    /// Whether the host returned `null` for this path, which overrides the
    /// parent listing.
    fn is_missing(&self, path: &Path) -> bool {
        self.missing_files.contains(path) || self.missing_dirs.contains(path)
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

/// A failed host call, kept for the requests that were already waiting on it.
struct FlightError {
    kind: ErrorKind,
    message: String,
}

impl FlightError {
    fn new(error: &bashkit::Error) -> Self {
        let kind = match error {
            bashkit::Error::Io(e) => e.kind(),
            _ => ErrorKind::Other,
        };
        let message = match error {
            bashkit::Error::Io(e) => e.to_string(),
            other => other.to_string(),
        };
        Self { kind, message }
    }

    fn to_error(&self) -> bashkit::Error {
        IoError::new(self.kind, self.message.clone()).into()
    }
}

/// Per-path lock holding the error of a failed call. Successes and missing
/// paths are shared through the cache instead.
type Flight = tokio::sync::Mutex<Option<FlightError>>;

/// Holds a per-path in-flight lock entry and removes it from the map when
/// dropped, including when the waiting future is dropped (timeout).
struct InflightEntry<'a> {
    backend: &'a JsLazyBackend,
    key: InflightKey,
    lock: Arc<Flight>,
}

impl InflightEntry<'_> {
    /// Share `error` with current waiters and retire this flight, so requests
    /// that arrive later start a new call.
    fn fail(&self, outcome: &mut Option<FlightError>, error: &bashkit::Error) {
        *outcome = Some(FlightError::new(error));
        self.backend.retire_inflight(&self.key, &self.lock);
    }
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
    inflight: StdMutex<HashMap<InflightKey, Arc<Flight>>>,
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

/// Dispatch one request to JS under the process-wide callback semaphore and
/// return the raw JSON response.
async fn dispatch(callback: Arc<LazyFsTsfn>, request: String) -> bashkit::Result<String> {
    // Bound concurrent JS callbacks across all instances (issue #982).
    let _permit = crate::callback_semaphore()
        .acquire()
        .await
        .map_err(|_| callback_failed())?;
    // TSFN/promise failures carry napi status noise; the TS dispatcher
    // already turns user errors into `{error}`, so only a generic message is
    // surfaced here (TM-INF-028).
    let promise = callback
        .call_async(request)
        .await
        .map_err(|_| callback_failed())?;
    promise.await.map_err(|_| callback_failed())
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Non-negative finite number truncated to an integer, else `None`.
fn whole(n: Option<f64>) -> Option<u64> {
    n.filter(|v| v.is_finite() && *v >= 0.0).map(|v| v as u64)
}

/// Largest raw response accepted for a `readFile` of up to `max_file_size` bytes.
fn max_file_response_bytes(max_file_size: u64) -> u64 {
    max_file_size
        .saturating_mul(MAX_JSON_EXPANSION)
        .saturating_add(RESPONSE_OVERHEAD_BYTES)
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
            .or_insert_with(|| Arc::new(Flight::new(None)))
            .clone();
        InflightEntry {
            backend: self,
            key,
            lock,
        }
    }

    fn retire_inflight(&self, key: &InflightKey, lock: &Arc<Flight>) {
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        if map
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, lock))
        {
            map.remove(key);
        }
    }

    fn release_inflight(&self, key: &InflightKey, lock: &Arc<Flight>) {
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        // Two strong refs: the map's and the caller's. More means waiters
        // still hold the entry; the last one out removes it. Runs under the
        // map mutex, so the count cannot change underneath us.
        // A retired flight must not remove its successor.
        if Arc::strong_count(lock) <= 2
            && map
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, lock))
        {
            map.remove(key);
        }
    }

    /// Decide whether a host call may be made now (see the module docs).
    /// Runs before the per-path lock so a sync caller never queues behind a
    /// call that needs the event loop it is blocking.
    fn preflight(&self) -> bashkit::Result<()> {
        if std::thread::current().id() == self.js_thread {
            return Err(IoError::new(ErrorKind::WouldBlock, LAZY_FS_SYNC_ERROR).into());
        }
        if in_execution_scope() {
            return Ok(());
        }
        Err(IoError::new(ErrorKind::WouldBlock, LAZY_FS_HELPER_THREAD_ERROR).into())
    }

    async fn call(
        &self,
        op: &str,
        path: &Path,
        limit: u64,
        max_response_bytes: u64,
    ) -> bashkit::Result<WireResponse> {
        self.preflight()?;
        let request =
            serde_json::json!({ "op": op, "path": path_str(path), "limit": limit }).to_string();
        // Inside an execution, so the execution deadline (`timeoutMs`) drops
        // this future if the callback never settles.
        let raw = dispatch(self.callback.clone(), request).await?;
        if raw.len() as u64 > max_response_bytes {
            return Err(invalid("lazy filesystem callback response is too large"));
        }
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
                && self.limits.validate_path(&vfs_join(dir, &e.name)).is_ok();
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
                .filter(|m| *m <= 0o7777)
                .map(|m| m as u32)
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
        self.preflight()?;
        let entry = self.inflight_entry("readDir", dir);
        let mut outcome = entry.lock.lock().await;
        if let Some(hit) = self.cached_listing(dir) {
            return hit;
        }
        if let Some(failed) = outcome.as_ref() {
            return Err(failed.to_error());
        }
        self.fetch_listing(dir)
            .await
            .inspect_err(|e| entry.fail(&mut outcome, e))
    }

    async fn fetch_listing(&self, dir: &Path) -> bashkit::Result<Arc<Vec<DirEntry>>> {
        let resp = self
            .call(
                "readDir",
                dir,
                MAX_DIR_ENTRIES as u64,
                MAX_LISTING_RESPONSE_BYTES,
            )
            .await?;
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
        let cache = self.cache();
        if cache.is_missing(path) {
            return Err(not_found());
        }
        if meta.file_type.is_file()
            && let Some(len) = cache.known_size(path)
        {
            meta.size = len;
        }
        Ok(meta)
    }

    async fn fetch_file(&self, path: &Path) -> bashkit::Result<Arc<Vec<u8>>> {
        let limit = self.limits.max_file_size;
        let resp = self
            .call("readFile", path, limit, max_file_response_bytes(limit))
            .await?;
        if let Some(size) = whole(resp.oversize) {
            self.cache().remember_oversize(path.to_path_buf(), size);
            return Err(self.too_large());
        }
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
        let path = normalize_path(path);
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
        self.preflight()?;
        let entry = self.inflight_entry("readFile", &path);
        let mut outcome = entry.lock.lock().await;
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
        if let Some(failed) = outcome.as_ref() {
            return Err(failed.to_error());
        }
        self.fetch_file(&path)
            .await
            .inspect_err(|e| entry.fail(&mut outcome, e))
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
        self.stat_inner(&normalize_path(path)).await
    }

    async fn read_dir(&self, path: &Path) -> bashkit::Result<Vec<DirEntry>> {
        let dir = normalize_path(path);
        let listing = self.list(&dir).await?;
        let cache = self.cache();
        Ok(listing
            .iter()
            .filter_map(|e| {
                let path = vfs_join(&dir, &e.name);
                if cache.is_missing(&path) {
                    return None;
                }
                let mut e = e.clone();
                if e.metadata.file_type.is_file()
                    && let Some(len) = cache.known_size(&path)
                {
                    e.metadata.size = len;
                }
                Some(e)
            })
            .collect())
    }

    async fn exists(&self, path: &Path) -> bashkit::Result<bool> {
        match self.stat_inner(&normalize_path(path)).await {
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

    /// Compared as strings: `Path` equality hides `\` separators on Windows.
    #[test]
    fn windows_containment_paths_use_forward_slashes() {
        let path = normalize_path(Path::new("/a/../src/./index.ts"));
        assert_eq!(path_str(&path), "/src/index.ts");
        assert_eq!(path_str(&vfs_join(Path::new("/src"), "x")), "/src/x");
    }

    #[test]
    fn whole_rejects_out_of_range_numbers() {
        assert_eq!(whole(Some(12.9)), Some(12));
        assert_eq!(whole(Some(-1.0)), None);
        assert_eq!(whole(Some(f64::NAN)), None);
        assert_eq!(whole(None), None);
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
