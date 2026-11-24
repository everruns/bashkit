//! Lazy, JS-callback-backed filesystem (`FileSystem.lazy()`).
//!
//! The host describes a tree with two callbacks: `readDir(path)` lists one
//! directory and `readFile(path)` returns one file's bytes. Nothing is fetched
//! up front. A directory is listed the first time a script lists, stats or
//! resolves something inside it; a file's content is fetched the first time a
//! script reads it. Both results are cached for the lifetime of the handle, so
//! each path costs the host at most one call per kind.
//!
//! The backend is read-only. `FileSystem.lazy(..., { writable: true })` wraps
//! it in an `OverlayFs`, so writes land in memory and never reach the host.
//!
//! Decision: callbacks dispatch through a threadsafe function onto the Node
//! event loop. Sync entry points (`executeSync()`, `bash.readFile()`, ...)
//! block that loop, and the blocked future may run on a runtime worker rather
//! than the JS thread itself, so a thread-id check alone is not enough. Every
//! sync entry point holds a [`JsThreadBlocked`] guard for the JS thread it
//! blocks; the backend records its owning JS thread at construction and fails
//! fast while that thread is blocked instead of deadlocking (same contract as
//! custom builtins).
//!
//! Callback output is host data, but it is still validated: entry names must
//! be single path components, entry counts and file sizes are bounded, and
//! malformed responses surface as I/O errors rather than panics.

use std::collections::{HashMap, HashSet};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::thread::ThreadId;

use base64::Engine;
use bashkit::time::SystemTime;
use bashkit::{DirEntry, FileType, FsBackend, FsLimits, FsUsage, Metadata, async_trait};
use napi::bindgen_prelude::Promise;
use serde::Deserialize;

/// JS threads currently blocked inside a synchronous native call, with depth.
fn blocked_js_threads() -> &'static StdMutex<HashMap<ThreadId, usize>> {
    static BLOCKED: OnceLock<StdMutex<HashMap<ThreadId, usize>>> = OnceLock::new();
    BLOCKED.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn is_js_thread_blocked(id: ThreadId) -> bool {
    blocked_js_threads()
        .lock()
        .map(|m| m.contains_key(&id))
        .unwrap_or(false)
}

/// Marks the current (JS) thread as blocked for the guard's lifetime.
pub(crate) struct JsThreadBlocked(ThreadId);

impl JsThreadBlocked {
    pub(crate) fn enter() -> Self {
        let id = std::thread::current().id();
        if let Ok(mut m) = blocked_js_threads().lock() {
            *m.entry(id).or_insert(0) += 1;
        }
        Self(id)
    }
}

impl Drop for JsThreadBlocked {
    fn drop(&mut self) {
        if let Ok(mut m) = blocked_js_threads().lock()
            && let Some(depth) = m.get_mut(&self.0)
        {
            *depth -= 1;
            if *depth == 0 {
                m.remove(&self.0);
            }
        }
    }
}

/// Upper bound on entries returned by one `readDir` call (TM-DOS style cap).
const MAX_DIR_ENTRIES: usize = 100_000;

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
    size: Option<u64>,
    mode: Option<u32>,
    modified: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireResponse {
    #[serde(default)]
    missing: bool,
    entries: Option<Vec<WireEntry>>,
    text: Option<String>,
    base64: Option<String>,
}

#[derive(Default)]
struct Cache {
    /// Directory path -> listing (sorted by name).
    dirs: HashMap<PathBuf, Arc<Vec<DirEntry>>>,
    /// Directories the host reported as missing.
    missing_dirs: HashSet<PathBuf>,
    /// File path -> content.
    files: HashMap<PathBuf, Arc<Vec<u8>>>,
}

pub(crate) struct JsLazyBackend {
    callback: Arc<LazyFsTsfn>,
    js_thread: ThreadId,
    limits: FsLimits,
    cache: StdMutex<Cache>,
}

fn io_err(kind: ErrorKind, msg: impl Into<String>) -> bashkit::Error {
    IoError::new(kind, msg.into()).into()
}

fn read_only() -> bashkit::Error {
    io_err(ErrorKind::PermissionDenied, "filesystem is read-only")
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

impl JsLazyBackend {
    pub(crate) fn new(callback: LazyFsTsfn) -> Self {
        Self {
            callback: Arc::new(callback),
            js_thread: std::thread::current().id(),
            limits: FsLimits::default(),
            cache: StdMutex::new(Cache::default()),
        }
    }

    async fn call(&self, op: &str, path: &Path) -> bashkit::Result<WireResponse> {
        if std::thread::current().id() == self.js_thread || is_js_thread_blocked(self.js_thread) {
            return Err(io_err(ErrorKind::WouldBlock, LAZY_FS_SYNC_ERROR));
        }
        let request = serde_json::json!({ "op": op, "path": path_str(path) }).to_string();
        let promise = self
            .callback
            .call_async(request)
            .await
            .map_err(|e| io_err(ErrorKind::Other, format!("lazy fs {op}: {e}")))?;
        let raw = promise.await.map_err(|e| {
            io_err(
                ErrorKind::Other,
                format!("lazy fs {op} {}: {e}", path_str(path)),
            )
        })?;
        serde_json::from_str(&raw).map_err(|e| {
            io_err(
                ErrorKind::InvalidData,
                format!("lazy fs {op}: bad response: {e}"),
            )
        })
    }

    fn cached_file_size(&self, path: &Path) -> Option<u64> {
        let cache = self.cache.lock().expect("lazy fs cache poisoned");
        cache.files.get(path).map(|c| c.len() as u64)
    }

    /// List `dir` (normalized), fetching and caching on first use.
    async fn list(&self, dir: &Path) -> bashkit::Result<Arc<Vec<DirEntry>>> {
        {
            let cache = self.cache.lock().expect("lazy fs cache poisoned");
            if let Some(hit) = cache.dirs.get(dir) {
                return Ok(hit.clone());
            }
            if cache.missing_dirs.contains(dir) {
                return Err(io_err(ErrorKind::NotFound, "no such directory"));
            }
        }
        // Only list paths whose parent says they are directories, so scripts
        // cannot make the host enumerate arbitrary invented paths.
        if dir != Path::new("/") {
            let meta = self.stat_inner(dir).await?;
            if !meta.file_type.is_dir() {
                return Err(io_err(ErrorKind::Other, "not a directory"));
            }
        }
        let resp = self.call("readDir", dir).await?;
        if resp.missing {
            let mut cache = self.cache.lock().expect("lazy fs cache poisoned");
            cache.missing_dirs.insert(dir.to_path_buf());
            return Err(io_err(ErrorKind::NotFound, "no such directory"));
        }
        let wire = resp.entries.ok_or_else(|| {
            io_err(
                ErrorKind::InvalidData,
                "lazy fs readDir: response has no entries",
            )
        })?;
        if wire.len() > MAX_DIR_ENTRIES {
            return Err(io_err(
                ErrorKind::InvalidData,
                format!("lazy fs readDir: more than {MAX_DIR_ENTRIES} entries"),
            ));
        }
        let mut seen = HashSet::new();
        let mut entries = Vec::with_capacity(wire.len());
        for e in wire {
            if e.name.is_empty()
                || e.name == "."
                || e.name == ".."
                || e.name.contains('/')
                || e.name.contains('\0')
            {
                return Err(io_err(
                    ErrorKind::InvalidData,
                    format!("lazy fs readDir: invalid entry name {:?}", e.name),
                ));
            }
            if !seen.insert(e.name.clone()) {
                continue;
            }
            let file_type = match e.kind.as_str() {
                "file" => FileType::File,
                "dir" | "directory" => FileType::Directory,
                other => {
                    return Err(io_err(
                        ErrorKind::InvalidData,
                        format!("lazy fs readDir: unsupported entry type {other:?}"),
                    ));
                }
            };
            let modified = e
                .modified
                .filter(|ms| ms.is_finite() && *ms >= 0.0)
                .map(|ms| bashkit::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64))
                .unwrap_or_else(SystemTime::now);
            let is_dir = file_type.is_dir();
            entries.push(DirEntry {
                name: e.name,
                metadata: Metadata {
                    file_type,
                    size: if is_dir { 0 } else { e.size.unwrap_or(0) },
                    mode: e.mode.unwrap_or(if is_dir { 0o755 } else { 0o644 }) & 0o7777,
                    modified,
                    created: modified,
                    ..Metadata::default()
                },
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let entries = Arc::new(entries);
        let mut cache = self.cache.lock().expect("lazy fs cache poisoned");
        Ok(cache
            .dirs
            .entry(dir.to_path_buf())
            .or_insert(entries)
            .clone())
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
            .ok_or_else(|| io_err(ErrorKind::NotFound, "no such file or directory"))?;
        let mut meta = entry.metadata.clone();
        if meta.file_type.is_file()
            && let Some(len) = self.cached_file_size(path)
        {
            meta.size = len;
        }
        Ok(meta)
    }
}

#[async_trait]
impl FsBackend for JsLazyBackend {
    async fn read(&self, path: &Path) -> bashkit::Result<Vec<u8>> {
        let path = normalize(path);
        {
            let cache = self.cache.lock().expect("lazy fs cache poisoned");
            if let Some(hit) = cache.files.get(&path) {
                return Ok(hit.as_ref().clone());
            }
        }
        let meta = self.stat_inner(&path).await?;
        if meta.file_type.is_dir() {
            return Err(io_err(ErrorKind::Other, "is a directory"));
        }
        let resp = self.call("readFile", &path).await?;
        if resp.missing {
            return Err(io_err(ErrorKind::NotFound, "no such file or directory"));
        }
        let bytes = match (resp.text, resp.base64) {
            (Some(text), _) => text.into_bytes(),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| io_err(ErrorKind::InvalidData, format!("lazy fs readFile: {e}")))?,
            (None, None) => {
                return Err(io_err(
                    ErrorKind::InvalidData,
                    "lazy fs readFile: response has no content",
                ));
            }
        };
        if bytes.len() as u64 > self.limits.max_file_size {
            return Err(io_err(
                ErrorKind::InvalidData,
                format!(
                    "lazy fs readFile: {} exceeds max file size ({} bytes)",
                    path_str(&path),
                    self.limits.max_file_size
                ),
            ));
        }
        let bytes = Arc::new(bytes);
        let mut cache = self.cache.lock().expect("lazy fs cache poisoned");
        Ok(cache.files.entry(path).or_insert(bytes).as_ref().clone())
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
        Ok(listing
            .iter()
            .map(|e| {
                let mut e = e.clone();
                if e.metadata.file_type.is_file()
                    && let Some(len) = self.cached_file_size(&dir.join(&e.name))
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
            Err(bashkit::Error::Io(e)) if e.kind() == ErrorKind::NotFound => Ok(false),
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
        Err(io_err(ErrorKind::InvalidInput, "not a symbolic link"))
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> bashkit::Result<()> {
        Err(read_only())
    }

    fn usage(&self) -> FsUsage {
        let cache = self.cache.lock().expect("lazy fs cache poisoned");
        let bytes = cache.files.values().map(|c| c.len() as u64).sum();
        FsUsage::new(bytes, cache.files.len() as u64, cache.dirs.len() as u64)
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
}
