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
//! event loop. Sync entry points (`executeSync()`, `bash.readFile()`, ...) run
//! their future on the JS thread with `block_on`, so a callback queued from
//! there could never run. The backend records its owning JS thread at
//! construction and fails fast when an operation runs on that thread, instead
//! of deadlocking (same contract as custom builtins). Same-addon handles are
//! mounted directly (`mountNativeFileSystem`), never through the C ABI, whose
//! blocking per-operation bridge would park runtime workers on pending promises.
//!
//! `timeoutMs` bounds a pending callback (the interpreter future is dropped).
//! `bash.cancel()` is cooperative and filesystem backends cannot observe it,
//! so cancellation takes effect once pending callbacks settle, as with other
//! slow backends. TODO: expose the execution budget to `FsBackend` operations
//! (like `RuntimeCallContext`) so pending callbacks can race cancellation.
//!
//! THREAT[TM-DOS-148]: callback output is host data, but scripts drive which
//! paths are requested, so every response is bounded. Only paths a parent
//! listing declared reach the host; entry names pass the same `validate_path`
//! rules as in-memory files (TM-DOS-013/015); listings are capped; declared
//! sizes over `max_file_size` are rejected before fetching; the cache is
//! capped in bytes and entries and evicts oldest-first; concurrent host calls
//! share the process-wide callback semaphore (#982).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::ThreadId;
use std::time::Duration;

use base64::Engine;
use bashkit::time::{SystemTime, UNIX_EPOCH};
use bashkit::{DirEntry, FileType, FsBackend, FsLimits, FsUsage, Metadata, async_trait};
use napi::bindgen_prelude::Promise;
use serde::Deserialize;

/// Upper bound on entries returned by one `readDir` call.
const MAX_DIR_ENTRIES: usize = 100_000;
/// Upper bound on cached directory listings before oldest-first eviction.
const MAX_CACHED_DIRS: usize = 10_000;

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
    files: HashMap<PathBuf, Arc<Vec<u8>>>,
    file_order: VecDeque<PathBuf>,
    file_bytes: u64,
    /// Paths the host reported missing, or that exceed `max_file_size`.
    missing_files: HashSet<PathBuf>,
    missing_dirs: HashSet<PathBuf>,
}

impl Cache {
    fn insert_dir(&mut self, dir: PathBuf, entries: Arc<Vec<DirEntry>>) {
        if self.dirs.insert(dir.clone(), entries).is_none() {
            self.dir_order.push_back(dir);
        }
        while self.dirs.len() > MAX_CACHED_DIRS {
            let Some(old) = self.dir_order.pop_front() else {
                break;
            };
            self.dirs.remove(&old);
        }
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

    fn inflight_lock(&self, op: &'static str, path: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        map.entry((op, path.to_path_buf()))
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn release_inflight(&self, op: &'static str, path: &Path, lock: &Arc<tokio::sync::Mutex<()>>) {
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        // Two strong refs: the map's and the caller's. More means waiters
        // still hold the entry; the last one out removes it.
        if Arc::strong_count(lock) <= 2 {
            map.remove(&(op, path.to_path_buf()));
        }
    }

    async fn call(&self, op: &str, path: &Path) -> bashkit::Result<WireResponse> {
        if std::thread::current().id() == self.js_thread {
            return Err(IoError::new(ErrorKind::WouldBlock, LAZY_FS_SYNC_ERROR).into());
        }
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
        let lock = self.inflight_lock("readDir", dir);
        let result = async {
            let _guard = lock.lock().await;
            if let Some(hit) = self.cached_listing(dir) {
                return hit;
            }
            let resp = self.call("readDir", dir).await?;
            if resp.missing {
                self.cache().missing_dirs.insert(dir.to_path_buf());
                return Err(not_found());
            }
            let wire = resp
                .entries
                .ok_or_else(|| invalid("lazy filesystem readDir returned no entries"))?;
            let entries = Arc::new(self.parse_listing(dir, wire)?);
            self.cache().insert_dir(dir.to_path_buf(), entries.clone());
            Ok(entries)
        }
        .await;
        self.release_inflight("readDir", dir, &lock);
        result
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
            && let Some(hit) = self.cache().files.get(path)
        {
            meta.size = hit.len() as u64;
        }
        Ok(meta)
    }

    async fn fetch_file(&self, path: &Path) -> bashkit::Result<Arc<Vec<u8>>> {
        let resp = self.call("readFile", path).await?;
        if resp.missing {
            self.cache().missing_files.insert(path.to_path_buf());
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
            self.cache().missing_files.insert(path.to_path_buf());
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
        let lock = self.inflight_lock("readFile", &path);
        let result = async {
            let _guard = lock.lock().await;
            {
                let cache = self.cache();
                if let Some(hit) = cache.files.get(&path) {
                    return Ok(hit.clone());
                }
                if cache.missing_files.contains(&path) {
                    return Err(not_found());
                }
            }
            self.fetch_file(&path).await
        }
        .await;
        self.release_inflight("readFile", &path, &lock);
        result.map(|bytes| bytes.as_ref().clone())
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
                    && let Some(hit) = cache.files.get(&dir.join(&e.name))
                {
                    e.metadata.size = hit.len() as u64;
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
