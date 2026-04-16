//! Bashkit Python package
//!
//! Primary interface: `Bash` — the core interpreter with virtual filesystem.
//! Convenience wrapper: `BashTool` — adds contract metadata (`description`,
//! `help`, `system_prompt`, JSON schemas) on top of the core interpreter.
//! Orchestration: `ScriptedTool` — composes Python callbacks as bash builtins.

use bashkit::tool::VERSION;
use bashkit::{
    Bash, BashTool as RustBashTool, Builtin, BuiltinContext, DirEntry as FsDirEntry, ExcType,
    ExecResult as RustExecResult, ExecutionExtensions, ExecutionLimits, ExtFunctionResult,
    FileSystem, FileSystemExt, FileType as FsFileType, InMemoryFs, Metadata as FsMetadata,
    MontyException, MontyObject, OutputCallback as RustOutputCallback, OverlayFs, PosixFs,
    PythonExternalFnHandler, PythonLimits, RealFs, RealFsMode, ScriptedTool as RustScriptedTool,
    ShellState as RustShellState, SnapshotOptions as RustSnapshotOptions, Tool, ToolArgs, ToolDef,
    ToolRequest, async_trait,
};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PyModule, PySet, PyTuple};
use pyo3_async_runtimes::TaskLocals;
use pyo3_async_runtimes::tokio::future_into_py;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, oneshot};

// ============================================================================
// JSON <-> Python helpers
// ============================================================================

/// Convert serde_json::Value → Py<PyAny>
const MAX_NESTING_DEPTH: usize = 64;

fn json_to_py(py: Python<'_>, val: &serde_json::Value) -> PyResult<Py<PyAny>> {
    json_to_py_inner(py, val, 0)
}

fn json_to_py_inner(py: Python<'_>, val: &serde_json::Value, depth: usize) -> PyResult<Py<PyAny>> {
    if depth > MAX_NESTING_DEPTH {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "JSON nesting depth exceeds maximum of 64",
        ));
    }
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any().unbind())
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        serde_json::Value::Array(arr) => {
            let items: Vec<Py<PyAny>> = arr
                .iter()
                .map(|v| json_to_py_inner(py, v, depth + 1))
                .collect::<PyResult<_>>()?;
            Ok(PyList::new(py, &items)?.into_any().unbind())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py_inner(py, v, depth + 1)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

/// Convert Py<PyAny> → serde_json::Value (for schema dicts)
fn py_to_json(py: Python<'_>, obj: &Bound<'_, pyo3::PyAny>) -> PyResult<serde_json::Value> {
    py_to_json_inner(py, obj, 0)
}

#[allow(clippy::only_used_in_recursion)]
fn py_to_json_inner(
    py: Python<'_>,
    obj: &Bound<'_, pyo3::PyAny>,
    depth: usize,
) -> PyResult<serde_json::Value> {
    if depth > MAX_NESTING_DEPTH {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Python object nesting depth exceeds maximum of 64",
        ));
    }
    if obj.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(serde_json::json!(i));
    }
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(serde_json::json!(f));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        let arr: Vec<serde_json::Value> = list
            .iter()
            .map(|item| py_to_json_inner(py, &item, depth + 1))
            .collect::<PyResult<_>>()?;
        return Ok(serde_json::Value::Array(arr));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json_inner(py, &v, depth + 1)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    // Fallback: str()
    let s = obj.str()?.extract::<String>()?;
    Ok(serde_json::Value::String(s))
}

/// Real filesystem mount config (internal, parsed from Python dicts).
#[derive(Clone)]
struct RealMountConfig {
    host_path: String,
    vfs_mount: Option<String>,
    writable: bool,
}

enum PyFileMount {
    Static { path: String, content: String },
    Lazy { path: String, provider: Py<PyAny> },
}

const PY_FILE_PROVIDER_TYPE_ERROR_PREFIX: &str = "__bashkit_py_file_provider_type_error__:";

fn make_runtime() -> PyResult<Arc<Runtime>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map(Arc::new)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))
}

/// Parse `mounts` kwarg (list of dicts) into internal config.
/// Each dict: { "host_path": str, "vfs_path"?: str, "writable"?: bool }.
fn parse_mounts(mounts: Option<&Bound<'_, PyList>>) -> PyResult<Vec<RealMountConfig>> {
    let Some(list) = mounts else {
        return Ok(Vec::new());
    };
    let mut configs = Vec::with_capacity(list.len());
    for item in list.iter() {
        let dict = item
            .cast::<PyDict>()
            .map_err(|_| PyValueError::new_err("each mount must be a dict with 'host_path' key"))?;
        let host_path: String = dict
            .get_item("host_path")?
            .ok_or_else(|| PyValueError::new_err("mount dict missing required 'host_path' key"))?
            .extract()?;
        let vfs_mount: Option<String> = dict
            .get_item("vfs_path")?
            .map(|v| v.extract())
            .transpose()?;
        let writable: bool = dict
            .get_item("writable")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(false);
        configs.push(RealMountConfig {
            host_path,
            vfs_mount,
            writable,
        });
    }
    Ok(configs)
}

fn parse_files(files: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<PyFileMount>> {
    let Some(dict) = files else {
        return Ok(Vec::new());
    };

    let mut mounts = Vec::with_capacity(dict.len());
    for (path_obj, value_obj) in dict.iter() {
        let path: String = path_obj.extract()?;
        if let Ok(content) = value_obj.extract::<String>() {
            mounts.push(PyFileMount::Static { path, content });
            continue;
        }
        if value_obj.is_callable() {
            mounts.push(PyFileMount::Lazy {
                path,
                provider: value_obj.unbind(),
            });
            continue;
        }
        return Err(PyTypeError::new_err(format!(
            "files['{path}'] must be str or callable returning str"
        )));
    }
    Ok(mounts)
}

fn parse_custom_builtins(
    py: Python<'_>,
    custom_builtins: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<PyCustomBuiltinEntry>> {
    let Some(dict) = custom_builtins else {
        return Ok(Vec::new());
    };

    let mut builtins = Vec::with_capacity(dict.len());
    for (name_obj, callback_obj) in dict.iter() {
        let name: String = name_obj.extract()?;
        builtins.push(build_py_custom_builtin_entry(
            py,
            name,
            callback_obj.unbind(),
        )?);
    }
    Ok(builtins)
}

fn clone_file_mounts(py: Python<'_>, mounts: &[PyFileMount]) -> Vec<PyFileMount> {
    mounts
        .iter()
        .map(|mount| match mount {
            PyFileMount::Static { path, content } => PyFileMount::Static {
                path: path.clone(),
                content: content.clone(),
            },
            PyFileMount::Lazy { path, provider } => PyFileMount::Lazy {
                path: path.clone(),
                provider: provider.clone_ref(py),
            },
        })
        .collect()
}

fn map_fs_error_to_py(err: impl ToString) -> PyErr {
    let msg = err.to_string();
    if let Some((_, rest)) = msg.split_once(PY_FILE_PROVIDER_TYPE_ERROR_PREFIX) {
        PyTypeError::new_err(rest.to_string())
    } else {
        PyRuntimeError::new_err(msg)
    }
}

// Lazy Python file providers need read-time Python exceptions, so the binding
// keeps them in a small overlay wrapper instead of bashkit's byte-only lazy loader.
struct PythonLazyFilesFs {
    overlay: Arc<OverlayFs>,
    providers: RwLock<std::collections::HashMap<PathBuf, Py<PyAny>>>,
}

impl PythonLazyFilesFs {
    fn new(lower: Arc<dyn FileSystem>, files: &[PyFileMount]) -> Self {
        let overlay = Arc::new(OverlayFs::new(lower));
        let mut providers = std::collections::HashMap::new();

        for file in files {
            if let PyFileMount::Lazy { path, provider } = file {
                overlay
                    .upper()
                    .add_lazy_file(path, 0, 0o644, Arc::new(Vec::<u8>::new));
                providers.insert(
                    bashkit::normalize_path(Path::new(path)),
                    Python::attach(|py| provider.clone_ref(py)),
                );
            }
        }

        Self {
            overlay,
            providers: RwLock::new(providers),
        }
    }

    fn normalize_path(path: &Path) -> PathBuf {
        bashkit::normalize_path(path)
    }

    async fn materialize_if_needed(&self, path: &Path) -> bashkit::Result<()> {
        let normalized = Self::normalize_path(path);
        let provider = {
            let mut providers = self.providers.write().unwrap();
            providers.remove(&normalized)
        };
        let Some(provider) = provider else {
            return Ok(());
        };

        let loaded = Python::attach(|py| -> std::result::Result<Vec<u8>, String> {
            let value = provider.bind(py).call0().map_err(|e| e.to_string())?;
            let text = value.extract::<String>().map_err(|_| {
                format!(
                    "{PY_FILE_PROVIDER_TYPE_ERROR_PREFIX}file provider for '{}' must return str",
                    normalized.display()
                )
            })?;
            Ok(text.into_bytes())
        });

        match loaded {
            Ok(content) => {
                self.overlay.write_file(&normalized, &content).await?;
                Ok(())
            }
            Err(message) => {
                self.providers.write().unwrap().insert(normalized, provider);
                Err(std::io::Error::other(message).into())
            }
        }
    }

    fn remove_provider_paths(&self, path: &Path, recursive: bool) {
        let normalized = Self::normalize_path(path);
        let mut providers = self.providers.write().unwrap();
        providers.retain(|candidate, _| {
            !(candidate == &normalized || (recursive && candidate.starts_with(&normalized)))
        });
    }

    fn move_provider_paths(&self, from: &Path, to: &Path) {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);
        let mut providers = self.providers.write().unwrap();
        let mut moved = Vec::new();

        for (path, provider) in &*providers {
            if path == &from || path.starts_with(&from) {
                let new_path = if path == &from {
                    to.clone()
                } else {
                    to.join(path.strip_prefix(&from).unwrap())
                };
                moved.push((
                    path.clone(),
                    new_path,
                    Python::attach(|py| provider.clone_ref(py)),
                ));
            }
        }

        for (old_path, _, _) in &moved {
            providers.remove(old_path);
        }
        for (_, new_path, provider) in moved {
            providers.insert(new_path, provider);
        }
    }
}

#[async_trait]
impl FileSystemExt for PythonLazyFilesFs {}

#[async_trait]
impl FileSystem for PythonLazyFilesFs {
    async fn read_file(&self, path: &Path) -> bashkit::Result<Vec<u8>> {
        self.materialize_if_needed(path).await?;
        self.overlay.read_file(path).await
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> bashkit::Result<()> {
        self.remove_provider_paths(path, false);
        self.overlay.write_file(path, content).await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> bashkit::Result<()> {
        self.materialize_if_needed(path).await?;
        self.overlay.append_file(path, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> bashkit::Result<()> {
        self.overlay.mkdir(path, recursive).await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> bashkit::Result<()> {
        self.remove_provider_paths(path, recursive);
        self.overlay.remove(path, recursive).await
    }

    async fn stat(&self, path: &Path) -> bashkit::Result<FsMetadata> {
        self.overlay.stat(path).await
    }

    async fn read_dir(&self, path: &Path) -> bashkit::Result<Vec<FsDirEntry>> {
        self.overlay.read_dir(path).await
    }

    async fn exists(&self, path: &Path) -> bashkit::Result<bool> {
        self.overlay.exists(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> bashkit::Result<()> {
        self.overlay.rename(from, to).await?;
        self.move_provider_paths(from, to);
        Ok(())
    }

    async fn copy(&self, from: &Path, to: &Path) -> bashkit::Result<()> {
        self.materialize_if_needed(from).await?;
        self.overlay.copy(from, to).await
    }

    async fn symlink(&self, target: &Path, link: &Path) -> bashkit::Result<()> {
        self.overlay.symlink(target, link).await
    }

    async fn read_link(&self, path: &Path) -> bashkit::Result<PathBuf> {
        self.overlay.read_link(path).await
    }

    async fn chmod(&self, path: &Path, mode: u32) -> bashkit::Result<()> {
        self.overlay.chmod(path, mode).await
    }
}

/// Apply `files` dict and `mounts` list to a builder.
fn apply_fs_config(
    mut builder: bashkit::BashBuilder,
    files: &[PyFileMount],
    real_mounts: &[RealMountConfig],
) -> PyResult<bashkit::BashBuilder> {
    let lazy_files: Vec<PyFileMount> = files
        .iter()
        .filter_map(|file| match file {
            PyFileMount::Lazy { path, provider } => Some(PyFileMount::Lazy {
                path: path.clone(),
                provider: Python::attach(|py| provider.clone_ref(py)),
            }),
            PyFileMount::Static { .. } => None,
        })
        .collect();

    if !lazy_files.is_empty() {
        builder = builder.fs(Arc::new(PythonLazyFilesFs::new(
            Arc::new(InMemoryFs::new()),
            &lazy_files,
        )));
    }

    for file in files {
        if let PyFileMount::Static { path, content } = file {
            builder = builder.mount_text(path, content.clone());
        }
    }

    for mount in real_mounts {
        builder = match (mount.writable, &mount.vfs_mount) {
            (false, None) => builder.mount_real_readonly(&mount.host_path),
            (false, Some(vfs_mount)) => builder.mount_real_readonly_at(&mount.host_path, vfs_mount),
            (true, None) => builder.mount_real_readwrite(&mount.host_path),
            (true, Some(vfs_mount)) => builder.mount_real_readwrite_at(&mount.host_path, vfs_mount),
        };
    }

    Ok(builder)
}

fn system_time_to_unix_seconds(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn file_type_name(file_type: FsFileType) -> &'static str {
    match file_type {
        FsFileType::File => "file",
        FsFileType::Directory => "directory",
        FsFileType::Symlink => "symlink",
        FsFileType::Fifo => "fifo",
    }
}

fn metadata_to_pydict(py: Python<'_>, metadata: &FsMetadata) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("file_type", file_type_name(metadata.file_type))?;
    dict.set_item("size", metadata.size)?;
    dict.set_item("mode", metadata.mode)?;
    dict.set_item("modified", system_time_to_unix_seconds(metadata.modified))?;
    dict.set_item("created", system_time_to_unix_seconds(metadata.created))?;
    Ok(dict.into_any().unbind())
}

fn dir_entry_to_pydict(py: Python<'_>, entry: &FsDirEntry) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &entry.name)?;
    dict.set_item("metadata", metadata_to_pydict(py, &entry.metadata)?)?;
    Ok(dict.into_any().unbind())
}

#[derive(Clone)]
enum FileSystemHandle {
    Static(Arc<dyn FileSystem>),
    Live(Arc<Mutex<Bash>>),
}

impl FileSystemHandle {
    async fn resolve(&self) -> Arc<dyn FileSystem> {
        match self {
            Self::Static(fs) => Arc::clone(fs),
            Self::Live(inner) => {
                let bash = inner.lock().await;
                bash.fs()
            }
        }
    }
}

fn with_live_fs<T, F, Fut>(rt: &Arc<Runtime>, inner: &Arc<Mutex<Bash>>, f: F) -> PyResult<T>
where
    F: FnOnce(Arc<dyn FileSystem>) -> Fut,
    Fut: Future<Output = PyResult<T>>,
{
    let inner = inner.clone();
    rt.block_on(async move {
        let fs = {
            let bash = inner.lock().await;
            bash.fs()
        };
        f(fs).await
    })
}

fn read_text_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
) -> PyResult<String> {
    with_live_fs(rt, inner, move |fs| async move {
        let bytes = fs
            .read_file(Path::new(&path))
            .await
            .map_err(map_fs_error_to_py)?;
        String::from_utf8(bytes).map_err(|e| PyRuntimeError::new_err(format!("Invalid UTF-8: {e}")))
    })
}

fn write_text_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
    content: String,
) -> PyResult<()> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.write_file(Path::new(&path), content.as_bytes())
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn append_text_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
    content: String,
) -> PyResult<()> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.append_file(Path::new(&path), content.as_bytes())
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn mkdir_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
    recursive: bool,
) -> PyResult<()> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.mkdir(Path::new(&path), recursive)
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn remove_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
    recursive: bool,
) -> PyResult<()> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.remove(Path::new(&path), recursive)
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn exists_via_live_fs(rt: &Arc<Runtime>, inner: &Arc<Mutex<Bash>>, path: String) -> PyResult<bool> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.exists(Path::new(&path))
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn stat_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
) -> PyResult<FsMetadata> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.stat(Path::new(&path)).await.map_err(map_fs_error_to_py)
    })
}

fn chmod_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
    mode: u32,
) -> PyResult<()> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.chmod(Path::new(&path), mode)
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn symlink_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    target: String,
    link: String,
) -> PyResult<()> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.symlink(Path::new(&target), Path::new(&link))
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn read_link_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
) -> PyResult<String> {
    with_live_fs(rt, inner, move |fs| async move {
        let target = fs
            .read_link(Path::new(&path))
            .await
            .map_err(map_fs_error_to_py)?;
        Ok(target.display().to_string())
    })
}

fn read_dir_via_live_fs(
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    path: String,
) -> PyResult<Vec<FsDirEntry>> {
    with_live_fs(rt, inner, move |fs| async move {
        fs.read_dir(Path::new(&path))
            .await
            .map_err(map_fs_error_to_py)
    })
}

fn is_safe_glob_pattern(pattern: &str) -> bool {
    pattern.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(ch, '*' | '?' | '[' | ']' | '.' | '_' | '-' | '/' | ' ')
    })
}

fn glob_match_path(value: &str, pattern: &str) -> bool {
    let mut value_chars = value.chars().peekable();
    let mut pattern_chars = pattern.chars().peekable();

    loop {
        match (pattern_chars.peek(), value_chars.peek()) {
            (None, None) => return true,
            (None, Some(_)) => return false,
            (Some('*'), _) => {
                pattern_chars.next();
                if pattern_chars.peek().is_none() {
                    return true;
                }
                while value_chars.peek().is_some() {
                    let remaining_value: String = value_chars.clone().collect();
                    let remaining_pattern: String = pattern_chars.clone().collect();
                    if glob_match_path(&remaining_value, &remaining_pattern) {
                        return true;
                    }
                    value_chars.next();
                }
                let remaining_pattern: String = pattern_chars.collect();
                return glob_match_path("", &remaining_pattern);
            }
            (Some('?'), Some(_)) => {
                pattern_chars.next();
                value_chars.next();
            }
            (Some('?'), None) => return false,
            (Some(p), Some(v)) => {
                if *p == *v {
                    pattern_chars.next();
                    value_chars.next();
                } else {
                    return false;
                }
            }
            (Some(_), None) => return false,
        }
    }
}

fn normalize_find_path(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("//") {
        format!("/{}", stripped)
    } else {
        path.to_string()
    }
}

fn glob_via_bash(rt: &Arc<Runtime>, inner: &Arc<Mutex<Bash>>, pattern: String) -> Vec<String> {
    if !is_safe_glob_pattern(&pattern) {
        return Vec::new();
    }

    let inner = inner.clone();
    let command = "find / -type f 2>/dev/null".to_string();
    let result = rt.block_on(async move {
        let mut bash = inner.lock().await;
        bash.exec(&command).await
    });

    match result {
        Ok(exec) if exec.exit_code == 0 => exec
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(normalize_find_path)
            .filter(|line| glob_match_path(line, &pattern))
            .collect(),
        _ => Vec::new(),
    }
}

// Decision: snapshot factories build with caller kwargs first, then restore
// bytes into that configured instance so limits and identity settings survive.
fn raise_snapshot_error<E: std::fmt::Display>(err: E) -> PyErr {
    BashError::new_err(err.to_string())
}

fn snapshot_live_bash(
    py: Python<'_>,
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    exclude_filesystem: bool,
) -> PyResult<Vec<u8>> {
    let rt = rt.clone();
    let inner = inner.clone();
    py.detach(|| {
        rt.block_on(async move {
            let bash = inner.lock().await;
            bash.snapshot_with_options(RustSnapshotOptions { exclude_filesystem })
                .map_err(raise_snapshot_error)
        })
    })
}

fn restore_live_bash(
    py: Python<'_>,
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    data: Vec<u8>,
) -> PyResult<()> {
    let rt = rt.clone();
    let inner = inner.clone();
    py.detach(|| {
        rt.block_on(async move {
            let mut bash = inner.lock().await;
            bash.restore_snapshot(&data).map_err(raise_snapshot_error)
        })
    })
}

fn mapping_proxy(py: Python<'_>, dict: Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    Ok(PyModule::import(py, "types")?
        .getattr("MappingProxyType")?
        .call1((dict,))?
        .unbind())
}

fn readonly_string_map(py: Python<'_>, map: &HashMap<String, String>) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    for (key, value) in map {
        dict.set_item(key, value)?;
    }
    mapping_proxy(py, dict)
}

fn readonly_indexed_arrays(
    py: Python<'_>,
    map: &HashMap<String, HashMap<usize, String>>,
) -> PyResult<Py<PyAny>> {
    let outer = PyDict::new(py);
    for (name, values) in map {
        let inner = PyDict::new(py);
        for (index, value) in values {
            inner.set_item(*index, value)?;
        }
        outer.set_item(name, mapping_proxy(py, inner)?)?;
    }
    mapping_proxy(py, outer)
}

fn readonly_assoc_arrays(
    py: Python<'_>,
    map: &HashMap<String, HashMap<String, String>>,
) -> PyResult<Py<PyAny>> {
    let outer = PyDict::new(py);
    for (name, values) in map {
        let inner = PyDict::new(py);
        for (key, value) in values {
            inner.set_item(key, value)?;
        }
        outer.set_item(name, mapping_proxy(py, inner)?)?;
    }
    mapping_proxy(py, outer)
}

fn capture_shell_state(
    py: Python<'_>,
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
) -> PyResult<ShellState> {
    let rt = rt.clone();
    let inner = inner.clone();
    py.detach(|| {
        rt.block_on(async move {
            let bash = inner.lock().await;
            Ok(ShellState::from(bash.shell_state()))
        })
    })
}

#[pyclass(name = "FileSystem")]
struct PyFileSystem {
    inner: FileSystemHandle,
    rt: Arc<Runtime>,
}

impl PyFileSystem {
    fn from_static(inner: Arc<dyn FileSystem>, rt: Arc<Runtime>) -> Self {
        Self {
            inner: FileSystemHandle::Static(inner),
            rt,
        }
    }

    fn from_live(inner: Arc<Mutex<Bash>>, rt: Arc<Runtime>) -> Self {
        Self {
            inner: FileSystemHandle::Live(inner),
            rt,
        }
    }

    fn with_fs<T, F, Fut>(&self, f: F) -> PyResult<T>
    where
        F: FnOnce(Arc<dyn FileSystem>) -> Fut,
        Fut: Future<Output = PyResult<T>>,
    {
        let inner = self.inner.clone();
        self.rt.block_on(async move {
            let fs = inner.resolve().await;
            f(fs).await
        })
    }
}

#[pymethods]
impl PyFileSystem {
    #[new]
    fn new() -> PyResult<Self> {
        let rt = make_runtime()?;
        Ok(Self::from_static(Arc::new(InMemoryFs::new()), rt))
    }

    #[staticmethod]
    #[pyo3(signature = (host_path, writable=false))]
    fn real(host_path: String, writable: bool) -> PyResult<Self> {
        let rt = make_runtime()?;
        let mode = if writable {
            RealFsMode::ReadWrite
        } else {
            RealFsMode::ReadOnly
        };
        let backend =
            RealFs::new(&host_path, mode).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let fs: Arc<dyn FileSystem> = PosixFs::new(backend).into();
        Ok(Self::from_static(fs, rt))
    }

    fn read_file<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyBytes>> {
        let data = py.detach(|| {
            self.with_fs(|fs| async move {
                fs.read_file(Path::new(&path))
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })?;
        Ok(PyBytes::new(py, &data))
    }

    fn write_file(&self, py: Python<'_>, path: String, content: Vec<u8>) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.write_file(Path::new(&path), &content)
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    fn append_file(&self, py: Python<'_>, path: String, content: Vec<u8>) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.append_file(Path::new(&path), &content)
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    #[pyo3(signature = (path, recursive=false))]
    fn mkdir(&self, py: Python<'_>, path: String, recursive: bool) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.mkdir(Path::new(&path), recursive)
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    #[pyo3(signature = (path, recursive=false))]
    fn remove(&self, py: Python<'_>, path: String, recursive: bool) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.remove(Path::new(&path), recursive)
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    fn stat(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let metadata = py.detach(|| {
            self.with_fs(
                |fs| async move { fs.stat(Path::new(&path)).await.map_err(map_fs_error_to_py) },
            )
        })?;
        metadata_to_pydict(py, &metadata)
    }

    fn read_dir(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let entries = py.detach(|| {
            self.with_fs(|fs| async move {
                fs.read_dir(Path::new(&path))
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })?;
        let items: Vec<Py<PyAny>> = entries
            .iter()
            .map(|entry| dir_entry_to_pydict(py, entry))
            .collect::<PyResult<_>>()?;
        Ok(PyList::new(py, &items)?.into_any().unbind())
    }

    fn exists(&self, py: Python<'_>, path: String) -> PyResult<bool> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.exists(Path::new(&path))
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    fn rename(&self, py: Python<'_>, from_path: String, to_path: String) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.rename(Path::new(&from_path), Path::new(&to_path))
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    fn copy(&self, py: Python<'_>, from_path: String, to_path: String) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.copy(Path::new(&from_path), Path::new(&to_path))
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    fn symlink(&self, py: Python<'_>, target: String, link: String) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.symlink(Path::new(&target), Path::new(&link))
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    fn chmod(&self, py: Python<'_>, path: String, mode: u32) -> PyResult<()> {
        py.detach(|| {
            self.with_fs(|fs| async move {
                fs.chmod(Path::new(&path), mode)
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })
    }

    fn read_link(&self, py: Python<'_>, path: String) -> PyResult<String> {
        let target = py.detach(|| {
            self.with_fs(|fs| async move {
                fs.read_link(Path::new(&path))
                    .await
                    .map_err(map_fs_error_to_py)
            })
        })?;
        Ok(target.display().to_string())
    }
}

// ============================================================================
// ExecResult
// ============================================================================

// Decision: Python ShellState is an inspection-focused view, not a full Rust
// ShellState mirror. Copy only Python-friendly fields so Rust-only additions
// like AST-backed functions do not silently widen the Python API surface. Use
// snapshot(exclude_filesystem=True) when callers need shell-only restore bytes.
/// Read-only snapshot of shell state for prompt rendering and inspection.
///
/// Transient fields like `last_exit_code` and `traps` reflect the captured
/// snapshot, but Rust core clears them before each top-level `exec()`.
#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct ShellState {
    env: HashMap<String, String>,
    variables: HashMap<String, String>,
    arrays: HashMap<String, HashMap<usize, String>>,
    assoc_arrays: HashMap<String, HashMap<String, String>>,
    cwd: String,
    last_exit_code: i32,
    aliases: HashMap<String, String>,
    traps: HashMap<String, String>,
}

impl From<RustShellState> for ShellState {
    fn from(inner: RustShellState) -> Self {
        let RustShellState {
            env,
            variables,
            arrays,
            assoc_arrays,
            cwd,
            last_exit_code,
            aliases,
            traps,
            ..
        } = inner;
        Self {
            env,
            variables,
            arrays,
            assoc_arrays,
            cwd: cwd.to_string_lossy().into_owned(),
            last_exit_code,
            aliases,
            traps,
        }
    }
}

#[pymethods]
impl ShellState {
    fn __repr__(&self) -> String {
        format!(
            "ShellState(cwd={:?}, last_exit_code={}, env={}, variables={}, arrays={}, assoc_arrays={}, aliases={}, traps={})",
            self.cwd,
            self.last_exit_code,
            self.env.len(),
            self.variables.len(),
            self.arrays.len(),
            self.assoc_arrays.len(),
            self.aliases.len(),
            self.traps.len(),
        )
    }

    #[getter]
    fn env(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        readonly_string_map(py, &self.env)
    }

    #[getter]
    fn variables(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        readonly_string_map(py, &self.variables)
    }

    #[getter]
    fn arrays(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        readonly_indexed_arrays(py, &self.arrays)
    }

    #[getter]
    fn assoc_arrays(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        readonly_assoc_arrays(py, &self.assoc_arrays)
    }

    #[getter]
    fn cwd(&self) -> String {
        self.cwd.clone()
    }

    #[getter]
    fn last_exit_code(&self) -> i32 {
        self.last_exit_code
    }

    #[getter]
    fn aliases(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        readonly_string_map(py, &self.aliases)
    }

    #[getter]
    fn traps(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        readonly_string_map(py, &self.traps)
    }
}

/// Result from executing bash commands
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct ExecResult {
    #[pyo3(get)]
    pub stdout: String,
    #[pyo3(get)]
    pub stderr: String,
    #[pyo3(get)]
    pub exit_code: i32,
    #[pyo3(get)]
    pub error: Option<String>,
    #[pyo3(get)]
    pub stdout_truncated: bool,
    #[pyo3(get)]
    pub stderr_truncated: bool,
    #[pyo3(get)]
    pub final_env: Option<std::collections::HashMap<String, String>>,
}

#[pymethods]
impl ExecResult {
    fn __repr__(&self) -> String {
        format!(
            "ExecResult(stdout={:?}, stderr={:?}, exit_code={}, error={:?}, stdout_truncated={}, stderr_truncated={}, final_env={:?})",
            self.stdout,
            self.stderr,
            self.exit_code,
            self.error,
            self.stdout_truncated,
            self.stderr_truncated,
            self.final_env
        )
    }

    fn __str__(&self) -> String {
        if self.exit_code == 0 {
            self.stdout.clone()
        } else {
            format!("Error ({}): {}", self.exit_code, self.stderr)
        }
    }

    /// Check if command succeeded
    #[getter]
    fn success(&self) -> bool {
        self.exit_code == 0
    }

    /// Return output as dict
    fn to_dict(&self) -> pyo3::PyResult<pyo3::Py<PyDict>> {
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("stdout", &self.stdout)?;
            dict.set_item("stderr", &self.stderr)?;
            dict.set_item("exit_code", self.exit_code)?;
            dict.set_item("error", &self.error)?;
            dict.set_item("stdout_truncated", self.stdout_truncated)?;
            dict.set_item("stderr_truncated", self.stderr_truncated)?;
            dict.set_item("final_env", &self.final_env)?;
            Ok(dict.into())
        })
    }
}

// ============================================================================
// Bash — core interpreter
// ============================================================================

fn py_exec_result_from_rust(result: RustExecResult) -> ExecResult {
    ExecResult {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        error: None,
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
        final_env: result.final_env,
    }
}

fn py_exec_result_from_error(err: impl ToString) -> ExecResult {
    let msg = err.to_string();
    ExecResult {
        stdout: String::new(),
        stderr: msg.clone(),
        exit_code: 1,
        error: Some(msg),
        stdout_truncated: false,
        stderr_truncated: false,
        final_env: None,
    }
}

fn py_exec_result_from_bash_result(result: bashkit::Result<RustExecResult>) -> ExecResult {
    match result {
        Ok(result) => py_exec_result_from_rust(result),
        Err(err) => py_exec_result_from_error(err),
    }
}

// Snapshot caller-owned ContextVars at execute*-call time so output callbacks
// invoked later from the Rust runtime still see the embedding framework's
// request-scoped state. Keep inspect.isawaitable cached so callbacks that
// accidentally return coroutine/awaitable objects fail synchronously.
struct PyOutputHandler {
    callback: Py<PyAny>,
    context: Py<PyAny>,
    is_awaitable: Py<PyAny>,
}

fn copy_current_context(py: Python<'_>) -> PyResult<Py<PyAny>> {
    py.import("contextvars")?
        .call_method0("copy_context")
        .map(|ctx| ctx.unbind())
}

fn is_coroutine_callable(py: Python<'_>, callable: &Bound<'_, PyAny>) -> PyResult<bool> {
    let inspect = py.import("inspect")?;
    let is_coro_fn = inspect.getattr("iscoroutinefunction")?;
    Ok(is_coro_fn.call1((callable,))?.extract::<bool>()?
        || callable
            .getattr("__call__")
            .ok()
            .and_then(|c| is_coro_fn.call1((c,)).ok())
            .and_then(|r| r.extract::<bool>().ok())
            .unwrap_or(false))
}

fn validate_python_callback(
    py: Python<'_>,
    callable: &Bound<'_, PyAny>,
    callable_error: impl FnOnce() -> String,
) -> PyResult<bool> {
    if !callable.is_callable() {
        return Err(PyTypeError::new_err(callable_error()));
    }
    is_coroutine_callable(py, callable)
}

fn prepare_output_handler(
    py: Python<'_>,
    on_output: Option<Py<PyAny>>,
) -> PyResult<Option<PyOutputHandler>> {
    let Some(on_output) = on_output else {
        return Ok(None);
    };

    let bound = on_output.bind(py);
    if !bound.is_callable() {
        return Err(PyTypeError::new_err("on_output must be callable"));
    }
    if is_coroutine_callable(py, bound)? {
        return Err(PyTypeError::new_err(
            "on_output must be a synchronous callable (async/coroutine handlers are not supported)",
        ));
    }

    let is_awaitable = py.import("inspect")?.getattr("isawaitable")?.unbind();

    Ok(Some(PyOutputHandler {
        callback: on_output,
        context: copy_current_context(py)?,
        is_awaitable,
    }))
}

// Decision: `custom_builtins` mirror Rust builtin semantics with a shell-first
// context object (`argv`, `stdin`, `env`, `cwd`); `ScriptedTool` remains
// schema-first and continues to pass `(params, stdin)`.
//
// Deliberately omitted for now: live `fs`, mutable shell variables, mutable
// `cwd`, and feature-gated network/git/ssh clients from Rust `BuiltinContext`.
// Those are reasonable follow-ups, but this PR keeps the Python surface small
// and shell-first while we validate the core callback shape.
/// Execution context passed to Python-backed custom builtins.
///
/// Fields are snapshots of the shell state at invocation time.
#[pyclass(name = "BuiltinContext", skip_from_py_object)]
struct PyBuiltinContext {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    argv: Vec<String>,
    #[pyo3(get)]
    stdin: Option<String>,
    #[pyo3(get)]
    env: std::collections::HashMap<String, String>,
    #[pyo3(get)]
    cwd: String,
}

#[pymethods]
impl PyBuiltinContext {
    fn __repr__(&self) -> String {
        format!(
            "BuiltinContext(name={:?}, argv={:?}, stdin={:?}, cwd={:?})",
            self.name, self.argv, self.stdin, self.cwd
        )
    }
}

fn make_py_builtin_context(
    py: Python<'_>,
    name: &str,
    ctx: &BuiltinContext<'_>,
) -> Result<Py<PyBuiltinContext>, String> {
    Py::new(
        py,
        PyBuiltinContext {
            name: name.to_string(),
            argv: ctx.args.to_vec(),
            stdin: ctx.stdin.map(str::to_owned),
            env: ctx.env.clone(),
            cwd: ctx.cwd.to_string_lossy().into_owned(),
        },
    )
    .map_err(|e| e.to_string())
}

// Decision: split long-lived callback machinery from per-execution callback
// state. The engine owns reusable sync-fallback resources; each execute*()
// call owns its captured ContextVars, caller loop, and cancellable callback
// tasks via a fresh session.
//
// Decision: sync-fallback loop reuse is surface-specific. Bash/BashTool keep
// one shared private loop because persistent custom builtins may retain
// loop-bound asyncio state across execute_sync() calls. ScriptedTool sessions
// use a per-execution private loop so concurrent execute_sync() calls on one
// tool never race on run_until_complete.
struct PyCallbackSessionState {
    context: Py<PyAny>,
    caller_loop_locals: Option<TaskLocals>,
}

#[derive(Clone, Copy)]
enum PySyncLoopMode {
    SharedAcrossSessions,
    PerSession,
}

struct PyPrivateAsyncLoop {
    event_loop: StdMutex<Option<Py<PyAny>>>,
}

impl PyPrivateAsyncLoop {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            event_loop: StdMutex::new(None),
        })
    }

    fn event_loop(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let mut event_loop = self.event_loop.lock().expect("tool async loop lock");
        if event_loop.is_none() {
            let asyncio = py.import("asyncio")?;
            *event_loop = Some(asyncio.call_method0("new_event_loop")?.unbind());
        }
        Ok(event_loop
            .as_ref()
            .expect("tool async loop prepared")
            .clone_ref(py))
    }

    fn run_awaitable(&self, py: Python<'_>, awaitable: &Py<PyAny>) -> PyResult<Py<PyAny>> {
        self.event_loop(py)?
            .bind(py)
            .call_method1("run_until_complete", (awaitable.bind(py),))
            .map(|value| value.unbind())
    }
}

struct PyCallbackEngine {
    shared_private_async_loop: Arc<PyPrivateAsyncLoop>,
    caller: Py<PyAny>,
}

impl PyCallbackEngine {
    fn new(py: Python<'_>) -> PyResult<Arc<Self>> {
        Ok(Arc::new(Self {
            shared_private_async_loop: PyPrivateAsyncLoop::new(),
            caller: create_context_callback_caller(py)?,
        }))
    }

    fn invoke(
        &self,
        py: Python<'_>,
        context: &Py<PyAny>,
        callback: &Py<PyAny>,
        args: Vec<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = PyTuple::new(py, &args)?;
        self.caller.call1(py, (context, callback, args))
    }
}

struct PyCallbackSession {
    state: StdMutex<PyCallbackSessionState>,
    active_caller_tasks: Arc<StdMutex<Vec<Py<PyAny>>>>,
    private_async_loop: Arc<PyPrivateAsyncLoop>,
    engine: Arc<PyCallbackEngine>,
}

impl PyCallbackSession {
    fn capture(
        py: Python<'_>,
        engine: Arc<PyCallbackEngine>,
        needs_async_callbacks: bool,
        use_caller_loop: bool,
        sync_loop_mode: PySyncLoopMode,
    ) -> PyResult<Arc<Self>> {
        let private_async_loop = match sync_loop_mode {
            PySyncLoopMode::SharedAcrossSessions => engine.shared_private_async_loop.clone(),
            PySyncLoopMode::PerSession => PyPrivateAsyncLoop::new(),
        };
        Ok(Arc::new(Self {
            state: StdMutex::new(capture_callback_state(
                py,
                needs_async_callbacks,
                use_caller_loop,
            )?),
            active_caller_tasks: Arc::new(StdMutex::new(Vec::new())),
            private_async_loop,
            engine,
        }))
    }

    fn current_context(&self, py: Python<'_>) -> Py<PyAny> {
        self.state
            .lock()
            .expect("tool callback session lock")
            .context
            .clone_ref(py)
    }

    fn current_caller_loop_locals(&self) -> Option<TaskLocals> {
        self.state
            .lock()
            .expect("tool callback session lock")
            .caller_loop_locals
            .clone()
    }

    fn active_caller_tasks(&self) -> Arc<StdMutex<Vec<Py<PyAny>>>> {
        self.active_caller_tasks.clone()
    }

    fn cancel_active_caller_tasks(&self, py: Python<'_>) -> PyResult<()> {
        let Some(locals) = self.current_caller_loop_locals() else {
            return Ok(());
        };
        let tasks = self
            .active_caller_tasks
            .lock()
            .expect("tool active caller tasks lock")
            .drain(..)
            .collect::<Vec<_>>();
        for task in tasks {
            cancel_python_task(py, &locals, &task)?;
        }
        Ok(())
    }

    fn invoke(
        &self,
        py: Python<'_>,
        callback: &Py<PyAny>,
        args: Vec<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let context = self.current_context(py);
        self.engine.invoke(py, &context, callback, args)
    }

    fn run_awaitable_on_private_loop(
        &self,
        py: Python<'_>,
        awaitable: &Py<PyAny>,
    ) -> PyResult<Py<PyAny>> {
        self.private_async_loop.run_awaitable(py, awaitable)
    }
}

struct PyCustomBuiltinEntry {
    name: String,
    callback: Py<PyAny>,
    is_async: bool,
}

fn capture_callback_state(
    py: Python<'_>,
    needs_async_callbacks: bool,
    use_caller_loop: bool,
) -> PyResult<PyCallbackSessionState> {
    if needs_async_callbacks && use_caller_loop {
        let locals = pyo3_async_runtimes::tokio::get_current_locals(py)?;
        return Ok(PyCallbackSessionState {
            context: locals.context(py).unbind(),
            caller_loop_locals: Some(locals),
        });
    }

    Ok(PyCallbackSessionState {
        context: copy_current_context(py)?,
        caller_loop_locals: None,
    })
}

fn create_context_callback_caller(py: Python<'_>) -> PyResult<Py<PyAny>> {
    pyo3::types::PyModule::from_code(
        py,
        c"def _call(ctx, fn, args):\n    return ctx.run(fn, *args)",
        c"<bashkit_callback>",
        c"_bashkit_callback",
    )?
    .getattr("_call")
    .map(|caller| caller.unbind())
}

fn asyncio_cancelled_error(py: Python<'_>) -> PyResult<PyErr> {
    Ok(PyErr::from_value(
        py.import("asyncio")?.call_method0("CancelledError")?,
    ))
}

fn call_soon_threadsafe_with_context(
    py: Python<'_>,
    locals: &TaskLocals,
    callback: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let kwargs = PyDict::new(py);
    kwargs.set_item("context", locals.context(py))?;
    locals
        .event_loop(py)
        .call_method("call_soon_threadsafe", (callback,), Some(&kwargs))?;
    Ok(())
}

fn cancel_python_task(py: Python<'_>, locals: &TaskLocals, task: &Py<PyAny>) -> PyResult<()> {
    let cancel = task.bind(py).getattr("cancel")?;
    call_soon_threadsafe_with_context(py, locals, &cancel)
}

fn remove_active_caller_task(
    active_caller_tasks: &Arc<StdMutex<Vec<Py<PyAny>>>>,
    task: &Bound<'_, PyAny>,
) {
    let task_ptr = task.as_ptr();
    if let Ok(mut active_tasks) = active_caller_tasks.lock() {
        active_tasks.retain(|active_task| active_task.bind(task.py()).as_ptr() != task_ptr);
    }
}

#[pyclass]
struct PyCallbackTaskCompleter {
    tx: Option<oneshot::Sender<PyResult<Py<PyAny>>>>,
    active_caller_tasks: Arc<StdMutex<Vec<Py<PyAny>>>>,
}

#[pymethods]
impl PyCallbackTaskCompleter {
    #[pyo3(signature = (task))]
    fn __call__(&mut self, task: &Bound<'_, PyAny>) -> PyResult<()> {
        remove_active_caller_task(&self.active_caller_tasks, task);
        let result = match task.call_method0("result") {
            Ok(value) => Ok(value.unbind()),
            Err(err) => Err(err),
        };
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(result);
        }
        Ok(())
    }
}

#[pyclass]
struct PyScheduleCallbackTask {
    awaitable: Py<PyAny>,
    on_complete: Py<PyAny>,
    shared_task: Arc<StdMutex<Option<Py<PyAny>>>>,
    active_caller_tasks: Arc<StdMutex<Vec<Py<PyAny>>>>,
    cancel_requested: Arc<AtomicBool>,
}

#[pymethods]
impl PyScheduleCallbackTask {
    fn __call__(&self) -> PyResult<()> {
        Python::attach(|py| {
            let task = py
                .import("asyncio")?
                .call_method1("ensure_future", (self.awaitable.bind(py),))?;
            *self.shared_task.lock().expect("tool callback task lock") =
                Some(task.clone().unbind());
            self.active_caller_tasks
                .lock()
                .expect("tool active caller tasks lock")
                .push(task.clone().unbind());
            task.call_method1("add_done_callback", (self.on_complete.bind(py),))?;
            if self.cancel_requested.load(Ordering::Relaxed) {
                let _ = task.call_method0("cancel");
            }
            Ok(())
        })
    }
}

#[pyclass]
struct PyCancelActiveCallbackTasks {
    session: Arc<PyCallbackSession>,
}

#[pymethods]
impl PyCancelActiveCallbackTasks {
    #[pyo3(signature = (future))]
    fn __call__(&self, future: &Bound<'_, PyAny>) -> PyResult<()> {
        if future.call_method0("cancelled")?.extract::<bool>()? {
            self.session.cancel_active_caller_tasks(future.py())?;
        }
        Ok(())
    }
}

fn attach_future_cancellation_callback(
    py: Python<'_>,
    future: &Bound<'_, PyAny>,
    session: Arc<PyCallbackSession>,
) -> PyResult<()> {
    let on_cancel = Py::new(py, PyCancelActiveCallbackTasks { session })?
        .into_bound(py)
        .into_any()
        .unbind();
    future.call_method1("add_done_callback", (on_cancel.bind(py),))?;
    Ok(())
}

#[pyclass]
struct PyCancelActiveCallbackTasksNow {
    session: Arc<PyCallbackSession>,
}

#[pymethods]
impl PyCancelActiveCallbackTasksNow {
    fn __call__(&self, py: Python<'_>) -> PyResult<()> {
        self.session.cancel_active_caller_tasks(py)
    }
}

fn create_execute_cancel_wrapper(py: Python<'_>) -> PyResult<Py<PyAny>> {
    pyo3::types::PyModule::from_code(
        py,
        c"import asyncio\nclass _CancelForwardingAwaitable:\n    def __init__(self, inner, on_cancel):\n        self._inner = inner\n        self._on_cancel = on_cancel\n    def cancel(self):\n        self._on_cancel()\n        return self._inner.cancel()\n    def __await__(self):\n        async def _await_inner():\n            try:\n                return await self._inner\n            except asyncio.CancelledError:\n                self._on_cancel()\n                self._inner.cancel()\n                raise\n        return _await_inner().__await__()\n    def __getattr__(self, name):\n        return getattr(self._inner, name)\ndef wrap(inner, on_cancel):\n    return _CancelForwardingAwaitable(inner, on_cancel)",
        c"<bashkit_cancel_wrapper>",
        c"_bashkit_cancel_wrapper",
    )?
    .getattr("wrap")
    .map(|wrap| wrap.unbind())
}

fn wrap_future_with_cancel<'py>(
    py: Python<'py>,
    future: Bound<'py, PyAny>,
    session: Arc<PyCallbackSession>,
) -> PyResult<Bound<'py, PyAny>> {
    let on_cancel = Py::new(py, PyCancelActiveCallbackTasksNow { session })?
        .into_bound(py)
        .into_any()
        .unbind();
    create_execute_cancel_wrapper(py)?
        .bind(py)
        .call1((future, on_cancel.bind(py)))
}

struct PyCancellableLoopFuture {
    result_rx: Option<oneshot::Receiver<PyResult<Py<PyAny>>>>,
    locals: TaskLocals,
    shared_task: Arc<StdMutex<Option<Py<PyAny>>>>,
    cancel_requested: Arc<AtomicBool>,
    completed: bool,
}

impl PyCancellableLoopFuture {
    fn new(
        py: Python<'_>,
        session: Arc<PyCallbackSession>,
        awaitable: Py<PyAny>,
    ) -> PyResult<Self> {
        let locals = session
            .current_caller_loop_locals()
            .expect("caller loop locals required for async callback session");
        let (tx, rx) = oneshot::channel();
        let shared_task = Arc::new(StdMutex::new(None));
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let active_caller_tasks = session.active_caller_tasks();
        let on_complete = Py::new(
            py,
            PyCallbackTaskCompleter {
                tx: Some(tx),
                active_caller_tasks: active_caller_tasks.clone(),
            },
        )?
        .into_bound(py)
        .into_any()
        .unbind();
        let scheduler = Py::new(
            py,
            PyScheduleCallbackTask {
                awaitable,
                on_complete,
                shared_task: shared_task.clone(),
                active_caller_tasks,
                cancel_requested: cancel_requested.clone(),
            },
        )?
        .into_bound(py)
        .into_any()
        .unbind();
        call_soon_threadsafe_with_context(py, &locals, scheduler.bind(py))?;
        Ok(Self {
            result_rx: Some(rx),
            locals,
            shared_task,
            cancel_requested,
            completed: false,
        })
    }

    async fn wait(mut self) -> PyResult<Py<PyAny>> {
        let result_rx = self.result_rx.take().expect("callback result receiver");
        let result = match result_rx.await {
            Ok(result) => result,
            Err(_) => Err(Python::attach(asyncio_cancelled_error)?),
        };
        self.completed = true;
        result
    }
}

impl Drop for PyCancellableLoopFuture {
    fn drop(&mut self) {
        if self.completed {
            return;
        }

        self.cancel_requested.store(true, Ordering::Relaxed);
        Python::attach(|py| {
            let maybe_task = self
                .shared_task
                .lock()
                .ok()
                .and_then(|shared_task| shared_task.as_ref().map(|task| task.clone_ref(py)));
            if let Some(task) = maybe_task {
                let _ = cancel_python_task(py, &self.locals, &task);
            }
        });
    }
}

fn build_py_custom_builtin_entry(
    py: Python<'_>,
    name: String,
    callback: Py<PyAny>,
) -> PyResult<PyCustomBuiltinEntry> {
    let bound = callback.bind(py);
    let is_async = validate_python_callback(py, bound, || {
        format!("custom_builtins['{name}'] must be callable")
    })?;
    Ok(PyCustomBuiltinEntry {
        name,
        callback,
        is_async,
    })
}

fn build_py_tool_entry(
    py: Python<'_>,
    name: String,
    description: String,
    callback: Py<PyAny>,
    schema: Option<Bound<'_, pyo3::PyAny>>,
) -> PyResult<PyToolEntry> {
    let schema_val = match schema {
        Some(ref schema) => py_to_json(py, schema)?,
        None => serde_json::Value::Object(Default::default()),
    };
    let bound = callback.bind(py);
    let is_async = validate_python_callback(py, bound, || {
        format!("tool '{name}' callback must be callable")
    })?;
    Ok(PyToolEntry {
        name,
        description,
        schema: schema_val,
        callback,
        is_async,
    })
}

fn make_py_tool_callback_args(py: Python<'_>, args: &ToolArgs) -> Result<Vec<Py<PyAny>>, String> {
    let params = json_to_py(py, &args.params).map_err(|e: PyErr| e.to_string())?;
    let stdin_arg = match args.stdin.as_deref() {
        Some(stdin) => stdin
            .into_pyobject(py)
            .expect("str -> Python object")
            .into_any()
            .unbind(),
        None => py.None(),
    };
    Ok(vec![params, stdin_arg])
}

fn call_python_string_callback_sync(
    py: Python<'_>,
    session: &PyCallbackSession,
    callback_name: &str,
    callback: &Py<PyAny>,
    args: Vec<Py<PyAny>>,
) -> Result<String, String> {
    let result = session
        .invoke(py, callback, args)
        .map_err(|e| format!("{callback_name}: {e}"))?;
    result
        .extract::<String>(py)
        .map_err(|e| format!("{callback_name}: callback must return str, got {e}"))
}

async fn call_python_string_callback_async(
    session: Arc<PyCallbackSession>,
    callback_name: &str,
    callback: &Py<PyAny>,
    args: Vec<Py<PyAny>>,
) -> Result<String, String> {
    let awaitable = Python::attach(|py| session.invoke(py, callback, args))
        .map_err(|e| format!("{callback_name}: {e}"))?;

    let result = if session.current_caller_loop_locals().is_some() {
        let future = Python::attach(|py| PyCancellableLoopFuture::new(py, session, awaitable))
            .map_err(|e| format!("{callback_name}: {e}"))?;
        future
            .wait()
            .await
            .map_err(|e| format!("{callback_name}: {e}"))?
    } else {
        Python::attach(|py| session.run_awaitable_on_private_loop(py, &awaitable))
            .map_err(|e| format!("{callback_name}: {e}"))?
    };

    Python::attach(|py| {
        result
            .extract::<String>(py)
            .map_err(|e| format!("{callback_name}: callback must return str, got {e}"))
    })
}

struct PyCustomBuiltinAdapter {
    name: String,
    callback: Py<PyAny>,
    is_async: bool,
}

#[async_trait]
impl Builtin for PyCustomBuiltinAdapter {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<RustExecResult> {
        let session = ctx.execution_extension::<Arc<PyCallbackSession>>().cloned();
        let builtin_arg = Python::attach(|py| -> Result<Py<PyAny>, String> {
            let builtin_arg = make_py_builtin_context(py, &self.name, &ctx)
                .map_err(|e| format!("{}: {}", self.name, e))?
                .into_bound(py)
                .into_any()
                .unbind();
            Ok(builtin_arg)
        });
        let callback_result = match builtin_arg {
            Ok(builtin_arg) if self.is_async => match session {
                Some(session) => {
                    call_python_string_callback_async(
                        session,
                        &self.name,
                        &self.callback,
                        vec![builtin_arg],
                    )
                    .await
                }
                None => Err(format!("{}: missing Python callback session", self.name)),
            },
            Ok(builtin_arg) => match session {
                Some(session) => Python::attach(|py| {
                    call_python_string_callback_sync(
                        py,
                        session.as_ref(),
                        &self.name,
                        &self.callback,
                        vec![builtin_arg],
                    )
                }),
                None => Err(format!("{}: missing Python callback session", self.name)),
            },
            Err(err) => Err(err),
        };

        Ok(match callback_result {
            Ok(stdout) => RustExecResult::ok(stdout),
            Err(msg) => RustExecResult::err(msg, 1),
        })
    }
}

fn build_runtime_custom_builtin_impls(
    py: Python<'_>,
    builtins: &[PyCustomBuiltinEntry],
) -> Vec<PyCustomBuiltinAdapter> {
    builtins
        .iter()
        .map(|entry| PyCustomBuiltinAdapter {
            name: entry.name.clone(),
            callback: entry.callback.clone_ref(py),
            is_async: entry.is_async,
        })
        .collect()
}

fn apply_custom_builtins_to_builder(
    py: Python<'_>,
    mut builder: bashkit::BashBuilder,
    builtins: &[PyCustomBuiltinEntry],
) -> bashkit::BashBuilder {
    for builtin in build_runtime_custom_builtin_impls(py, builtins) {
        let name = builtin.name.clone();
        builder = builder.builtin(name, Box::new(builtin));
    }
    builder
}

fn capture_custom_builtin_session(
    py: Python<'_>,
    builtins: &[PyCustomBuiltinEntry],
    engine: &Arc<PyCallbackEngine>,
    use_caller_loop: bool,
) -> PyResult<Option<Arc<PyCallbackSession>>> {
    if builtins.is_empty() {
        return Ok(None);
    }
    PyCallbackSession::capture(
        py,
        engine.clone(),
        builtins.iter().any(|entry| entry.is_async),
        use_caller_loop,
        PySyncLoopMode::SharedAcrossSessions,
    )
    .map(Some)
}

fn replace_live_bash_with_builder(
    py: Python<'_>,
    rt: &Arc<Runtime>,
    inner: &Arc<Mutex<Bash>>,
    cancelled: &Arc<RwLock<Arc<AtomicBool>>>,
    builder: bashkit::BashBuilder,
) -> PyResult<()> {
    let rt = rt.clone();
    let inner = inner.clone();
    let cancelled = cancelled.clone();
    py.detach(|| {
        rt.block_on(async move {
            let mut bash = inner.lock().await;
            let rebuilt = builder.build();
            let token = rebuilt.cancellation_token();
            *bash = rebuilt;
            if let Ok(mut current) = cancelled.write() {
                *current = token;
            }
            Ok(())
        })
    })
}

fn take_output_handler_error(callback_error: &StdMutex<Option<PyErr>>) -> Option<PyErr> {
    callback_error
        .lock()
        .ok()
        .and_then(|mut callback_error| callback_error.take())
}

fn close_awaitable_if_possible(awaitable: &Bound<'_, PyAny>) {
    if let Ok(close) = awaitable.getattr("close") {
        let _ = close.call0();
    }
}

fn build_python_output_callback(
    on_output: PyOutputHandler,
    cancelled: Arc<AtomicBool>,
    callback_requested_cancel: Arc<AtomicBool>,
    callback_error: Arc<StdMutex<Option<PyErr>>>,
) -> RustOutputCallback {
    Box::new(move |stdout_chunk, stderr_chunk| {
        let has_error = callback_error
            .lock()
            .map(|callback_error| callback_error.is_some())
            .unwrap_or(false);
        if has_error {
            return;
        }

        let callback_result = Python::attach(|py| {
            // Re-enter the caller's copied ContextVar snapshot for each chunk.
            let result = on_output.context.bind(py).call_method1(
                "run",
                (on_output.callback.bind(py), stdout_chunk, stderr_chunk),
            )?;
            let is_awaitable = on_output
                .is_awaitable
                .bind(py)
                .call1((&result,))?
                .extract::<bool>()?;
            if is_awaitable {
                close_awaitable_if_possible(&result);
                return Err(PyTypeError::new_err(
                    "on_output must be synchronous and must not return an awaitable",
                ));
            }
            Ok(())
        });

        if let Err(err) = callback_result {
            if let Ok(mut callback_error) = callback_error.lock()
                && callback_error.is_none()
            {
                *callback_error = Some(err);
            }
            if !cancelled.swap(true, Ordering::Relaxed) {
                callback_requested_cancel.store(true, Ordering::Relaxed);
            }
        }
    })
}

async fn exec_bash_with_optional_output(
    bash: &mut Bash,
    commands: &str,
    on_output: Option<PyOutputHandler>,
    builtin_session: Option<Arc<PyCallbackSession>>,
) -> PyResult<ExecResult> {
    let mut execution_extensions = ExecutionExtensions::new();
    if let Some(session) = builtin_session {
        let _ = execution_extensions.insert(session);
    }

    let result = if let Some(on_output) = on_output {
        // Preserve explicit cancel() calls across execute* entry. Only clear
        // cancellation if an on_output failure introduced it for this call.
        let cancelled = bash.cancellation_token();
        let callback_requested_cancel = Arc::new(AtomicBool::new(false));
        let callback_error = Arc::new(StdMutex::new(None));
        let output_callback = build_python_output_callback(
            on_output,
            cancelled.clone(),
            callback_requested_cancel.clone(),
            callback_error.clone(),
        );
        let result = bash
            .exec_streaming_with_extensions(commands, output_callback, execution_extensions)
            .await;
        if let Some(err) = take_output_handler_error(&callback_error) {
            if callback_requested_cancel.load(Ordering::Relaxed) {
                cancelled.store(false, Ordering::Relaxed);
            }
            return Err(err);
        }
        result
    } else {
        bash.exec_with_extensions(commands, execution_extensions)
            .await
    };

    Ok(py_exec_result_from_bash_result(result))
}

/// Build a `PythonExternalFnHandler` from a Python async callable.
///
/// The handler converts MontyObject args/kwargs to Python objects, calls the
/// async handler coroutine, awaits it, and converts the result back.
fn make_external_handler(py_handler: Py<PyAny>) -> PythonExternalFnHandler {
    Arc::new(move |fn_name, args, kwargs| {
        let py_handler = Python::attach(|py| py_handler.clone_ref(py));
        Box::pin(async move {
            let fut = Python::attach(|py| {
                let py_args = args
                    .iter()
                    .map(|o| monty_to_py(py, o))
                    .collect::<PyResult<Vec<_>>>()?;
                let py_args_list = PyList::new(py, &py_args)?;
                let py_kwargs = PyDict::new(py);
                for (k, v) in &kwargs {
                    py_kwargs.set_item(monty_to_py(py, k)?, monty_to_py(py, v)?)?;
                }
                let coro = py_handler.call1(py, (fn_name, py_args_list, py_kwargs))?;
                pyo3_async_runtimes::tokio::into_future(coro.into_bound(py))
            });
            match fut {
                Err(e) => ExtFunctionResult::Error(MontyException::new(
                    ExcType::RuntimeError,
                    Some(e.to_string()),
                )),
                Ok(awaitable) => match awaitable.await {
                    Err(e) => ExtFunctionResult::Error(MontyException::new(
                        ExcType::RuntimeError,
                        Some(e.to_string()),
                    )),
                    Ok(py_result) => {
                        Python::attach(|py| match py_to_monty(py, py_result.bind(py)) {
                            Ok(v) => ExtFunctionResult::Return(v),
                            Err(e) => ExtFunctionResult::Error(MontyException::new(
                                ExcType::RuntimeError,
                                Some(e.to_string()),
                            )),
                        })
                    }
                },
            }
        })
    })
}

/// Apply python/external_handler configuration to a `BashBuilder`.
///
/// Centralises the logic shared between `new()` and `reset()`.
fn apply_python_config(
    mut builder: bashkit::BashBuilder,
    python: bool,
    fn_names: Vec<String>,
    handler: Option<Py<PyAny>>,
) -> bashkit::BashBuilder {
    // By construction, handler.is_some() implies python=true (validated in new()).
    match (python, handler) {
        (true, Some(h)) => {
            builder = builder.python_with_external_handler(
                PythonLimits::default(),
                fn_names,
                make_external_handler(h),
            );
        }
        (true, None) => {
            builder = builder.python();
        }
        (false, _) => {}
    }
    builder
}

/// Core bash interpreter with virtual filesystem.
///
/// State persists between calls — files created in one `execute()` are
/// available in subsequent calls. This is the primary interface.
///
/// Example:
///     ```python
///     from bashkit import Bash
///
///     bash = Bash()
///     result = await bash.execute("echo 'Hello, World!'")
///     print(result.stdout)  # Hello, World!
///     ```
#[pyclass(name = "Bash")]
#[allow(dead_code)]
pub struct PyBash {
    inner: Arc<Mutex<Bash>>,
    /// Shared tokio runtime — reused across all sync calls to avoid
    /// per-call OS thread/fd exhaustion (issue #414).
    rt: Arc<Runtime>,
    /// Cancellation token. Wrapped in RwLock so reset() can swap it to
    /// the new interpreter's token without requiring &mut self.
    cancelled: Arc<RwLock<Arc<AtomicBool>>>,
    username: Option<String>,
    hostname: Option<String>,
    /// Whether Monty Python execution is enabled (`python`/`python3` builtins).
    python: bool,
    /// External function names callable from Monty code via the handler.
    external_functions: Vec<String>,
    /// Async Python callable invoked when Monty calls an external function.
    external_handler: Option<Py<PyAny>>,
    custom_builtins: Vec<PyCustomBuiltinEntry>,
    builtin_engine: Arc<PyCallbackEngine>,
    files: Vec<PyFileMount>,
    real_mounts: Vec<RealMountConfig>,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
    max_memory: Option<u64>,
    timeout_seconds: Option<f64>,
}

impl PyBash {
    fn build_live_builder(&self, py: Python<'_>) -> PyResult<bashkit::BashBuilder> {
        let mut builder = Bash::builder();

        if let Some(ref username) = self.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = self.hostname {
            builder = builder.hostname(hostname);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(max_commands) = self.max_commands {
            limits = limits.max_commands(usize::try_from(max_commands).unwrap_or(usize::MAX));
        }
        if let Some(max_loop_iterations) = self.max_loop_iterations {
            limits = limits
                .max_loop_iterations(usize::try_from(max_loop_iterations).unwrap_or(usize::MAX));
        }
        if let Some(timeout_seconds) = self.timeout_seconds {
            limits = limits.timeout(std::time::Duration::from_secs_f64(timeout_seconds));
        }
        builder = builder.limits(limits);

        if let Some(max_memory) = self.max_memory {
            builder = builder.max_memory(usize::try_from(max_memory).unwrap_or(usize::MAX));
        }

        let handler_clone = self.external_handler.as_ref().map(|h| h.clone_ref(py));
        builder = apply_python_config(
            builder,
            self.python,
            self.external_functions.clone(),
            handler_clone,
        );
        let files = clone_file_mounts(py, &self.files);
        let builder = apply_fs_config(builder, &files, &self.real_mounts)?;
        Ok(apply_custom_builtins_to_builder(
            py,
            builder,
            &self.custom_builtins,
        ))
    }
}

#[pymethods]
impl PyBash {
    #[new]
    #[pyo3(signature = (
        username=None,
        hostname=None,
        max_commands=None,
        max_loop_iterations=None,
        max_memory=None,
        timeout_seconds=None,
        python=false,
        external_functions=None,
        external_handler=None,
        files=None,
        mounts=None,
        custom_builtins=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        username: Option<String>,
        hostname: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
        max_memory: Option<u64>,
        timeout_seconds: Option<f64>,
        python: bool,
        external_functions: Option<Vec<String>>,
        external_handler: Option<Py<PyAny>>,
        files: Option<&Bound<'_, PyDict>>,
        mounts: Option<&Bound<'_, PyList>>,
        custom_builtins: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let mut builder = Bash::builder();

        if let Some(ref u) = username {
            builder = builder.username(u);
        }
        if let Some(ref h) = hostname {
            builder = builder.hostname(h);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(mc) = max_commands {
            limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
        }
        if let Some(mli) = max_loop_iterations {
            limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
        }
        if let Some(ts) = timeout_seconds {
            limits = limits.timeout(std::time::Duration::from_secs_f64(ts));
        }
        builder = builder.limits(limits);

        if let Some(mm) = max_memory {
            builder = builder.max_memory(usize::try_from(mm).unwrap_or(usize::MAX));
        }

        let files = parse_files(files)?;
        let real_mounts = parse_mounts(mounts)?;
        let custom_builtins = parse_custom_builtins(py, custom_builtins)?;

        let fn_names = external_functions.clone().unwrap_or_default();
        if !fn_names.is_empty() && external_handler.is_none() {
            return Err(PyValueError::new_err(
                "external_functions requires external_handler — the list has no effect without a handler",
            ));
        }
        if external_handler.is_some() && !python {
            return Err(PyValueError::new_err(
                "external_handler requires python=True",
            ));
        }
        if external_handler
            .as_ref()
            .is_some_and(|h| !h.bind(py).is_callable())
        {
            return Err(PyValueError::new_err("external_handler must be callable"));
        }
        if let Some(ref handler) = external_handler {
            let bound = handler.bind(py);
            if !is_coroutine_callable(py, bound)? {
                return Err(PyValueError::new_err(
                    "external_handler must be an async callable (coroutine function)",
                ));
            }
        }
        let handler_for_build = external_handler.as_ref().map(|h| h.clone_ref(py));
        builder = apply_python_config(builder, python, fn_names, handler_for_build);
        builder = apply_fs_config(builder, &files, &real_mounts)?;
        let builtin_engine = PyCallbackEngine::new(py)?;
        builder = apply_custom_builtins_to_builder(py, builder, &custom_builtins);

        let bash = builder.build();
        let cancelled = Arc::new(RwLock::new(bash.cancellation_token()));

        let rt = make_runtime()?;

        Ok(Self {
            inner: Arc::new(Mutex::new(bash)),
            rt,
            cancelled,
            username,
            hostname,
            python,
            external_functions: external_functions.unwrap_or_default(),
            external_handler,
            custom_builtins,
            builtin_engine,
            files,
            real_mounts,
            max_commands,
            max_loop_iterations,
            max_memory,
            timeout_seconds,
        })
    }

    /// Cancel the currently running execution.
    ///
    /// Safe to call from any thread. Execution will abort at the next
    /// command boundary.
    fn cancel(&self) {
        if let Ok(token) = self.cancelled.read() {
            token.store(true, Ordering::Relaxed);
        }
    }

    /// Clear the cancellation flag so subsequent executions proceed normally.
    ///
    /// Call this after a `cancel()` once the in-flight execution has finished
    /// and you want to reuse the same `Bash` instance (preserving VFS state).
    /// Without this, every future `execute()` will immediately fail with
    /// ``"execution cancelled"``.
    ///
    /// **Note:** Calling this while an execution is still in-flight may
    /// allow that execution to continue past the cancellation point.
    /// Wait for the cancelled execution to finish before clearing
    /// (await the async call or let `execute_sync` return).
    fn clear_cancel(&self) {
        if let Ok(token) = self.cancelled.read() {
            token.store(false, Ordering::Relaxed);
        }
    }

    /// Execute commands asynchronously.
    #[pyo3(signature = (commands, on_output=None))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let builtin_session =
            capture_custom_builtin_session(py, &self.custom_builtins, &self.builtin_engine, true)?;
        let on_output = prepare_output_handler(py, on_output)?;
        let inner = self.inner.clone();
        let cancel_session = builtin_session.clone();
        let future = future_into_py(py, async move {
            let mut bash = inner.lock().await;
            exec_bash_with_optional_output(&mut bash, &commands, on_output, builtin_session).await
        })?;
        if let Some(cancel_session) = cancel_session {
            attach_future_cancellation_callback(py, &future, cancel_session.clone())?;
            return wrap_future_with_cancel(py, future, cancel_session);
        }
        Ok(future)
    }

    /// Execute commands synchronously (blocking).
    ///
    /// Not supported when `external_handler` is configured: the handler is an async
    /// Python coroutine that requires a running event loop, which is unavailable in
    /// sync context. Use `execute()` (async) instead.
    ///
    /// Releases GIL before blocking on tokio to prevent deadlock with callbacks.
    ///
    /// # Thread safety
    ///
    /// This method acquires an async mutex with a 30-second timeout to prevent
    /// deadlocks when multiple threads call `execute_sync()` concurrently on the
    /// same `Bash` instance. If the lock cannot be acquired within the timeout,
    /// a `RuntimeError` is raised. For concurrent workloads, use separate `Bash`
    /// instances per thread or use the async `execute()` method.
    #[pyo3(signature = (commands, on_output=None))]
    fn execute_sync(
        &self,
        py: Python<'_>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<ExecResult> {
        if self.external_handler.is_some() {
            return Err(PyRuntimeError::new_err(
                "execute_sync is not supported when external_handler is configured — use execute() (async) instead, e.g. asyncio.run(bash.execute(...))",
            ));
        }
        let builtin_session =
            capture_custom_builtin_session(py, &self.custom_builtins, &self.builtin_engine, false)?;
        let on_output = prepare_output_handler(py, on_output)?;
        let inner = self.inner.clone();

        py.detach(|| {
            self.rt.block_on(async move {
                // THREAT[TM-DOS-FFI]: Use timeout on mutex acquisition to prevent
                // deadlocks when multiple Python threads call execute_sync concurrently.
                let mut bash =
                    match tokio::time::timeout(std::time::Duration::from_secs(30), inner.lock())
                        .await
                    {
                        Ok(guard) => guard,
                        Err(_) => {
                            return Err(PyRuntimeError::new_err(
                                "execute_sync: timed out waiting for lock (30s). \
                             Another thread may be holding the interpreter. \
                             Use separate Bash instances for concurrent access.",
                            ));
                        }
                    };
                exec_bash_with_optional_output(&mut bash, &commands, on_output, builtin_session)
                    .await
            })
        })
    }

    /// Execute commands synchronously. Raises `BashError` on non-zero exit.
    ///
    /// Not supported when `external_handler` is configured.
    #[pyo3(signature = (commands, on_output=None))]
    fn execute_sync_or_throw(
        &self,
        py: Python<'_>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<ExecResult> {
        let result = self.execute_sync(py, commands, on_output)?;
        if result.exit_code != 0 {
            return Err(raise_bash_error(&result));
        }
        Ok(result)
    }

    /// Execute commands asynchronously. Raises `BashError` on non-zero exit.
    #[pyo3(signature = (commands, on_output=None))]
    fn execute_or_throw<'py>(
        &self,
        py: Python<'py>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let builtin_session =
            capture_custom_builtin_session(py, &self.custom_builtins, &self.builtin_engine, true)?;
        let on_output = prepare_output_handler(py, on_output)?;
        let inner = self.inner.clone();
        let cancel_session = builtin_session.clone();
        let future = future_into_py(py, async move {
            let mut bash = inner.lock().await;
            let result =
                exec_bash_with_optional_output(&mut bash, &commands, on_output, builtin_session)
                    .await?;
            if result.exit_code != 0 {
                return Err(raise_bash_error(&result));
            }
            Ok(result)
        })?;
        if let Some(cancel_session) = cancel_session {
            attach_future_cancellation_callback(py, &future, cancel_session.clone())?;
            return wrap_future_with_cancel(py, future, cancel_session);
        }
        Ok(future)
    }

    /// Reset interpreter to fresh state, preserving all configuration including
    /// python mode, external function handler, and custom builtins.
    /// Releases GIL before blocking on tokio to prevent deadlock.
    fn reset(&self, py: Python<'_>) -> PyResult<()> {
        replace_live_bash_with_builder(
            py,
            &self.rt,
            &self.inner,
            &self.cancelled,
            self.build_live_builder(py)?,
        )
    }

    /// Serialize interpreter state to bytes for checkpoint/restore flows.
    #[pyo3(signature = (exclude_filesystem=false))]
    fn snapshot<'py>(
        &self,
        py: Python<'py>,
        exclude_filesystem: bool,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = snapshot_live_bash(py, &self.rt, &self.inner, exclude_filesystem)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Capture a read-only shell-state snapshot for prompt rendering and inspection.
    fn shell_state(&self, py: Python<'_>) -> PyResult<ShellState> {
        capture_shell_state(py, &self.rt, &self.inner)
    }

    /// Restore interpreter state from bytes previously produced by `snapshot()`.
    fn restore_snapshot(&self, py: Python<'_>, data: Vec<u8>) -> PyResult<()> {
        restore_live_bash(py, &self.rt, &self.inner, data)
    }

    /// Create a new Bash instance from a snapshot and optional constructor kwargs.
    #[staticmethod]
    #[pyo3(signature = (
        data,
        username=None,
        hostname=None,
        max_commands=None,
        max_loop_iterations=None,
        max_memory=None,
        timeout_seconds=None,
        python=false,
        external_functions=None,
        external_handler=None,
        files=None,
        mounts=None,
        custom_builtins=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_snapshot(
        py: Python<'_>,
        data: Vec<u8>,
        username: Option<String>,
        hostname: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
        max_memory: Option<u64>,
        timeout_seconds: Option<f64>,
        python: bool,
        external_functions: Option<Vec<String>>,
        external_handler: Option<Py<PyAny>>,
        files: Option<&Bound<'_, PyDict>>,
        mounts: Option<&Bound<'_, PyList>>,
        custom_builtins: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let bash = Self::new(
            py,
            username,
            hostname,
            max_commands,
            max_loop_iterations,
            max_memory,
            timeout_seconds,
            python,
            external_functions,
            external_handler,
            files,
            mounts,
            custom_builtins,
        )?;
        bash.restore_snapshot(py, data)?;
        Ok(bash)
    }

    fn read_file(&self, py: Python<'_>, path: String) -> PyResult<String> {
        py.detach(|| read_text_via_live_fs(&self.rt, &self.inner, path))
    }

    fn write_file(&self, py: Python<'_>, path: String, content: String) -> PyResult<()> {
        py.detach(|| write_text_via_live_fs(&self.rt, &self.inner, path, content))
    }

    fn append_file(&self, py: Python<'_>, path: String, content: String) -> PyResult<()> {
        py.detach(|| append_text_via_live_fs(&self.rt, &self.inner, path, content))
    }

    #[pyo3(signature = (path, recursive=false))]
    fn mkdir(&self, py: Python<'_>, path: String, recursive: bool) -> PyResult<()> {
        py.detach(|| mkdir_via_live_fs(&self.rt, &self.inner, path, recursive))
    }

    fn exists(&self, py: Python<'_>, path: String) -> PyResult<bool> {
        py.detach(|| exists_via_live_fs(&self.rt, &self.inner, path))
    }

    #[pyo3(signature = (path, recursive=false))]
    fn remove(&self, py: Python<'_>, path: String, recursive: bool) -> PyResult<()> {
        py.detach(|| remove_via_live_fs(&self.rt, &self.inner, path, recursive))
    }

    fn stat(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let metadata = py.detach(|| stat_via_live_fs(&self.rt, &self.inner, path))?;
        metadata_to_pydict(py, &metadata)
    }

    fn chmod(&self, py: Python<'_>, path: String, mode: u32) -> PyResult<()> {
        py.detach(|| chmod_via_live_fs(&self.rt, &self.inner, path, mode))
    }

    fn symlink(&self, py: Python<'_>, target: String, link: String) -> PyResult<()> {
        py.detach(|| symlink_via_live_fs(&self.rt, &self.inner, target, link))
    }

    fn read_link(&self, py: Python<'_>, path: String) -> PyResult<String> {
        py.detach(|| read_link_via_live_fs(&self.rt, &self.inner, path))
    }

    fn read_dir(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let entries = py.detach(|| read_dir_via_live_fs(&self.rt, &self.inner, path))?;
        let items: Vec<Py<PyAny>> = entries
            .iter()
            .map(|entry| dir_entry_to_pydict(py, entry))
            .collect::<PyResult<_>>()?;
        Ok(PyList::new(py, &items)?.into_any().unbind())
    }

    #[pyo3(signature = (path=".".to_string()))]
    fn ls(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let names = match py.detach(|| read_dir_via_live_fs(&self.rt, &self.inner, path)) {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        Ok(PyList::new(py, names)?.into_any().unbind())
    }

    fn glob(&self, py: Python<'_>, pattern: String) -> PyResult<Py<PyAny>> {
        let matches = py.detach(|| -> PyResult<Vec<String>> {
            Ok(glob_via_bash(&self.rt, &self.inner, pattern))
        })?;
        Ok(PyList::new(py, matches)?.into_any().unbind())
    }

    /// Return a live filesystem handle backed by the current interpreter.
    ///
    /// Each operation on the returned handle acquires the interpreter lock,
    /// so it always reflects the latest state (including post-reset). For
    /// batch reads where consistency isn't needed, prefer reading files via
    /// `execute_sync("cat ...")`.
    fn fs(&self, py: Python<'_>) -> PyResult<Py<PyFileSystem>> {
        Py::new(
            py,
            PyFileSystem::from_live(self.inner.clone(), self.rt.clone()),
        )
    }

    /// Mount a filesystem at `vfs_path` without rebuilding the interpreter.
    fn mount(&self, py: Python<'_>, vfs_path: String, fs: PyRef<'_, PyFileSystem>) -> PyResult<()> {
        let inner = self.inner.clone();
        let source = fs.inner.clone();
        py.detach(|| {
            self.rt.block_on(async move {
                let mounted_fs = source.resolve().await;
                let bash = inner.lock().await;
                bash.mount(Path::new(&vfs_path), mounted_fs)
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))
            })
        })
    }

    /// Unmount a live filesystem without rebuilding the interpreter.
    fn unmount(&self, py: Python<'_>, vfs_path: String) -> PyResult<()> {
        let inner = self.inner.clone();
        py.detach(|| {
            self.rt.block_on(async move {
                let bash = inner.lock().await;
                bash.unmount(Path::new(&vfs_path))
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))
            })
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "Bash(username={:?}, hostname={:?})",
            self.username.as_deref().unwrap_or("user"),
            self.hostname.as_deref().unwrap_or("sandbox")
        )
    }
}

// ============================================================================
// BashTool — interpreter + tool-contract metadata
// ============================================================================

/// Bash interpreter with tool-contract metadata (`description`, `help`,
/// `system_prompt`, schemas).
///
/// Extends `Bash` with methods required by LLM tool-use protocols.
/// Use this when integrating with LangChain, PydanticAI, or similar frameworks.
///
/// Example:
///     ```python
///     from bashkit import BashTool
///
///     tool = BashTool()
///     print(tool.input_schema())  # JSON schema for LLM
///     result = await tool.execute("echo 'Hello!'")
///     ```
/// with a virtual filesystem. State persists between calls - files created
/// in one call are available in subsequent calls.
///
/// Example:
///     ```python
///     from bashkit import BashTool
///
///     tool = BashTool()
///     result = await tool.execute("echo 'Hello, World!'")
///     print(result.stdout)  # Hello, World!
///     ```
#[pyclass]
#[allow(dead_code)]
pub struct BashTool {
    inner: Arc<Mutex<Bash>>,
    /// Shared tokio runtime — reused across all sync calls to avoid
    /// per-call OS thread/fd exhaustion (issue #414).
    rt: Arc<Runtime>,
    /// Cancellation token. Wrapped in RwLock so reset() can swap it to
    /// the new interpreter's token without requiring &mut self.
    cancelled: Arc<RwLock<Arc<AtomicBool>>>,
    username: Option<String>,
    hostname: Option<String>,
    custom_builtins: Vec<PyCustomBuiltinEntry>,
    builtin_engine: Arc<PyCallbackEngine>,
    files: Vec<PyFileMount>,
    real_mounts: Vec<RealMountConfig>,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
    max_memory: Option<u64>,
    timeout_seconds: Option<f64>,
}

impl BashTool {
    fn build_live_builder(&self, py: Python<'_>) -> PyResult<bashkit::BashBuilder> {
        let mut builder = Bash::builder();

        if let Some(ref username) = self.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = self.hostname {
            builder = builder.hostname(hostname);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(max_commands) = self.max_commands {
            limits = limits.max_commands(usize::try_from(max_commands).unwrap_or(usize::MAX));
        }
        if let Some(max_loop_iterations) = self.max_loop_iterations {
            limits = limits
                .max_loop_iterations(usize::try_from(max_loop_iterations).unwrap_or(usize::MAX));
        }
        if let Some(timeout_seconds) = self.timeout_seconds {
            limits = limits.timeout(std::time::Duration::from_secs_f64(timeout_seconds));
        }
        builder = builder.limits(limits);

        if let Some(max_memory) = self.max_memory {
            builder = builder.max_memory(usize::try_from(max_memory).unwrap_or(usize::MAX));
        }

        let files = clone_file_mounts(py, &self.files);
        let builder = apply_fs_config(builder, &files, &self.real_mounts)?;
        Ok(apply_custom_builtins_to_builder(
            py,
            builder,
            &self.custom_builtins,
        ))
    }

    fn build_rust_tool(&self) -> RustBashTool {
        let mut builder = RustBashTool::builder();

        if let Some(ref username) = self.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = self.hostname {
            builder = builder.hostname(hostname);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(mc) = self.max_commands {
            limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
        }
        if let Some(mli) = self.max_loop_iterations {
            limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
        }
        if let Some(ts) = self.timeout_seconds {
            limits = limits.timeout(std::time::Duration::from_secs_f64(ts));
        }

        if !self.custom_builtins.is_empty() {
            let builtins =
                Python::attach(|py| build_runtime_custom_builtin_impls(py, &self.custom_builtins));
            for builtin in builtins {
                let name = builtin.name.clone();
                builder = builder.builtin(name, Box::new(builtin));
            }
        }

        builder.limits(limits).build()
    }
}

#[pymethods]
impl BashTool {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (
        username=None,
        hostname=None,
        max_commands=None,
        max_loop_iterations=None,
        max_memory=None,
        timeout_seconds=None,
        files=None,
        mounts=None,
        custom_builtins=None,
    ))]
    fn new(
        py: Python<'_>,
        username: Option<String>,
        hostname: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
        max_memory: Option<u64>,
        timeout_seconds: Option<f64>,
        files: Option<&Bound<'_, PyDict>>,
        mounts: Option<&Bound<'_, PyList>>,
        custom_builtins: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let mut builder = Bash::builder();

        if let Some(ref u) = username {
            builder = builder.username(u);
        }
        if let Some(ref h) = hostname {
            builder = builder.hostname(h);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(mc) = max_commands {
            limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
        }
        if let Some(mli) = max_loop_iterations {
            limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
        }
        if let Some(ts) = timeout_seconds {
            limits = limits.timeout(std::time::Duration::from_secs_f64(ts));
        }
        builder = builder.limits(limits);

        if let Some(mm) = max_memory {
            builder = builder.max_memory(usize::try_from(mm).unwrap_or(usize::MAX));
        }

        let files = parse_files(files)?;
        let real_mounts = parse_mounts(mounts)?;
        let custom_builtins = parse_custom_builtins(py, custom_builtins)?;
        builder = apply_fs_config(builder, &files, &real_mounts)?;
        let builtin_engine = PyCallbackEngine::new(py)?;
        builder = apply_custom_builtins_to_builder(py, builder, &custom_builtins);

        let bash = builder.build();
        let cancelled = Arc::new(RwLock::new(bash.cancellation_token()));

        let rt = make_runtime()?;

        Ok(Self {
            inner: Arc::new(Mutex::new(bash)),
            rt,
            cancelled,
            username,
            hostname,
            custom_builtins,
            builtin_engine,
            files,
            real_mounts,
            max_commands,
            max_loop_iterations,
            max_memory,
            timeout_seconds,
        })
    }

    /// Cancel the currently running execution.
    fn cancel(&self) {
        if let Ok(token) = self.cancelled.read() {
            token.store(true, Ordering::Relaxed);
        }
    }

    /// Clear the cancellation flag so subsequent executions proceed normally.
    ///
    /// Call this after a `cancel()` once the in-flight execution has finished
    /// and you want to reuse the same `BashTool` instance (preserving VFS state).
    /// Without this, every future `execute()` will immediately fail with
    /// ``"execution cancelled"``.
    ///
    /// **Note:** Calling this while an execution is still in-flight may
    /// allow that execution to continue past the cancellation point.
    /// Wait for the cancelled execution to finish before clearing
    /// (await the async call or let `execute_sync` return).
    fn clear_cancel(&self) {
        if let Ok(token) = self.cancelled.read() {
            token.store(false, Ordering::Relaxed);
        }
    }

    #[pyo3(signature = (commands, on_output=None))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let builtin_session =
            capture_custom_builtin_session(py, &self.custom_builtins, &self.builtin_engine, true)?;
        let on_output = prepare_output_handler(py, on_output)?;
        let inner = self.inner.clone();
        let cancel_session = builtin_session.clone();
        let future = future_into_py(py, async move {
            let mut bash = inner.lock().await;
            exec_bash_with_optional_output(&mut bash, &commands, on_output, builtin_session).await
        })?;
        if let Some(cancel_session) = cancel_session {
            attach_future_cancellation_callback(py, &future, cancel_session.clone())?;
            return wrap_future_with_cancel(py, future, cancel_session);
        }
        Ok(future)
    }

    /// Releases GIL before blocking on tokio to prevent deadlock with callbacks.
    ///
    /// # Thread safety
    ///
    /// Acquires async mutex with 30-second timeout. For concurrent workloads,
    /// use separate `BashTool` instances per thread or the async `execute()`.
    #[pyo3(signature = (commands, on_output=None))]
    fn execute_sync(
        &self,
        py: Python<'_>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<ExecResult> {
        let builtin_session =
            capture_custom_builtin_session(py, &self.custom_builtins, &self.builtin_engine, false)?;
        let on_output = prepare_output_handler(py, on_output)?;
        let inner = self.inner.clone();

        py.detach(|| {
            self.rt.block_on(async move {
                // THREAT[TM-DOS-FFI]: Timeout on mutex to prevent deadlock.
                let mut bash =
                    match tokio::time::timeout(std::time::Duration::from_secs(30), inner.lock())
                        .await
                    {
                        Ok(guard) => guard,
                        Err(_) => {
                            return Err(PyRuntimeError::new_err(
                                "execute_sync: timed out waiting for lock (30s). \
                             Another thread may be holding the interpreter. \
                             Use separate BashTool instances for concurrent access.",
                            ));
                        }
                    };
                exec_bash_with_optional_output(&mut bash, &commands, on_output, builtin_session)
                    .await
            })
        })
    }

    /// Execute commands synchronously. Raises `BashError` on non-zero exit.
    #[pyo3(signature = (commands, on_output=None))]
    fn execute_sync_or_throw(
        &self,
        py: Python<'_>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<ExecResult> {
        let result = self.execute_sync(py, commands, on_output)?;
        if result.exit_code != 0 {
            return Err(raise_bash_error(&result));
        }
        Ok(result)
    }

    /// Execute commands asynchronously. Raises `BashError` on non-zero exit.
    #[pyo3(signature = (commands, on_output=None))]
    fn execute_or_throw<'py>(
        &self,
        py: Python<'py>,
        commands: String,
        on_output: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let builtin_session =
            capture_custom_builtin_session(py, &self.custom_builtins, &self.builtin_engine, true)?;
        let on_output = prepare_output_handler(py, on_output)?;
        let inner = self.inner.clone();
        let cancel_session = builtin_session.clone();
        let future = future_into_py(py, async move {
            let mut bash = inner.lock().await;
            let result =
                exec_bash_with_optional_output(&mut bash, &commands, on_output, builtin_session)
                    .await?;
            if result.exit_code != 0 {
                return Err(raise_bash_error(&result));
            }
            Ok(result)
        })?;
        if let Some(cancel_session) = cancel_session {
            attach_future_cancellation_callback(py, &future, cancel_session.clone())?;
            return wrap_future_with_cancel(py, future, cancel_session);
        }
        Ok(future)
    }

    /// Releases GIL before blocking on tokio to prevent deadlock.
    /// THREAT[TM-PY-028]: Rebuild with same config to preserve security limits
    /// and registered custom builtins.
    fn reset(&self, py: Python<'_>) -> PyResult<()> {
        replace_live_bash_with_builder(
            py,
            &self.rt,
            &self.inner,
            &self.cancelled,
            self.build_live_builder(py)?,
        )
    }

    /// Serialize interpreter state to bytes for checkpoint/restore flows.
    #[pyo3(signature = (exclude_filesystem=false))]
    fn snapshot<'py>(
        &self,
        py: Python<'py>,
        exclude_filesystem: bool,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = snapshot_live_bash(py, &self.rt, &self.inner, exclude_filesystem)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Capture a read-only shell-state snapshot for prompt rendering and inspection.
    fn shell_state(&self, py: Python<'_>) -> PyResult<ShellState> {
        capture_shell_state(py, &self.rt, &self.inner)
    }

    /// Restore interpreter state from bytes previously produced by `snapshot()`.
    fn restore_snapshot(&self, py: Python<'_>, data: Vec<u8>) -> PyResult<()> {
        restore_live_bash(py, &self.rt, &self.inner, data)
    }

    /// Create a new BashTool instance from a snapshot and optional constructor kwargs.
    #[staticmethod]
    #[pyo3(signature = (
        data,
        username=None,
        hostname=None,
        max_commands=None,
        max_loop_iterations=None,
        max_memory=None,
        timeout_seconds=None,
        files=None,
        mounts=None,
        custom_builtins=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_snapshot(
        py: Python<'_>,
        data: Vec<u8>,
        username: Option<String>,
        hostname: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
        max_memory: Option<u64>,
        timeout_seconds: Option<f64>,
        files: Option<&Bound<'_, PyDict>>,
        mounts: Option<&Bound<'_, PyList>>,
        custom_builtins: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let tool = Self::new(
            py,
            username,
            hostname,
            max_commands,
            max_loop_iterations,
            max_memory,
            timeout_seconds,
            files,
            mounts,
            custom_builtins,
        )?;
        tool.restore_snapshot(py, data)?;
        Ok(tool)
    }

    fn read_file(&self, py: Python<'_>, path: String) -> PyResult<String> {
        py.detach(|| read_text_via_live_fs(&self.rt, &self.inner, path))
    }

    fn write_file(&self, py: Python<'_>, path: String, content: String) -> PyResult<()> {
        py.detach(|| write_text_via_live_fs(&self.rt, &self.inner, path, content))
    }

    fn append_file(&self, py: Python<'_>, path: String, content: String) -> PyResult<()> {
        py.detach(|| append_text_via_live_fs(&self.rt, &self.inner, path, content))
    }

    #[pyo3(signature = (path, recursive=false))]
    fn mkdir(&self, py: Python<'_>, path: String, recursive: bool) -> PyResult<()> {
        py.detach(|| mkdir_via_live_fs(&self.rt, &self.inner, path, recursive))
    }

    fn exists(&self, py: Python<'_>, path: String) -> PyResult<bool> {
        py.detach(|| exists_via_live_fs(&self.rt, &self.inner, path))
    }

    #[pyo3(signature = (path, recursive=false))]
    fn remove(&self, py: Python<'_>, path: String, recursive: bool) -> PyResult<()> {
        py.detach(|| remove_via_live_fs(&self.rt, &self.inner, path, recursive))
    }

    fn stat(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let metadata = py.detach(|| stat_via_live_fs(&self.rt, &self.inner, path))?;
        metadata_to_pydict(py, &metadata)
    }

    fn chmod(&self, py: Python<'_>, path: String, mode: u32) -> PyResult<()> {
        py.detach(|| chmod_via_live_fs(&self.rt, &self.inner, path, mode))
    }

    fn symlink(&self, py: Python<'_>, target: String, link: String) -> PyResult<()> {
        py.detach(|| symlink_via_live_fs(&self.rt, &self.inner, target, link))
    }

    fn read_link(&self, py: Python<'_>, path: String) -> PyResult<String> {
        py.detach(|| read_link_via_live_fs(&self.rt, &self.inner, path))
    }

    fn read_dir(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let entries = py.detach(|| read_dir_via_live_fs(&self.rt, &self.inner, path))?;
        let items: Vec<Py<PyAny>> = entries
            .iter()
            .map(|entry| dir_entry_to_pydict(py, entry))
            .collect::<PyResult<_>>()?;
        Ok(PyList::new(py, &items)?.into_any().unbind())
    }

    #[pyo3(signature = (path=".".to_string()))]
    fn ls(&self, py: Python<'_>, path: String) -> PyResult<Py<PyAny>> {
        let names = match py.detach(|| read_dir_via_live_fs(&self.rt, &self.inner, path)) {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        Ok(PyList::new(py, names)?.into_any().unbind())
    }

    fn glob(&self, py: Python<'_>, pattern: String) -> PyResult<Py<PyAny>> {
        let matches = py.detach(|| -> PyResult<Vec<String>> {
            Ok(glob_via_bash(&self.rt, &self.inner, pattern))
        })?;
        Ok(PyList::new(py, matches)?.into_any().unbind())
    }

    /// Return a live filesystem handle backed by the current interpreter.
    ///
    /// Each operation on the returned handle acquires the interpreter lock,
    /// so it always reflects the latest state (including post-reset). For
    /// batch reads where consistency isn't needed, prefer reading files via
    /// `execute_sync("cat ...")`.
    fn fs(&self, py: Python<'_>) -> PyResult<Py<PyFileSystem>> {
        Py::new(
            py,
            PyFileSystem::from_live(self.inner.clone(), self.rt.clone()),
        )
    }

    /// Mount a filesystem at `vfs_path` without rebuilding the interpreter.
    fn mount(&self, py: Python<'_>, vfs_path: String, fs: PyRef<'_, PyFileSystem>) -> PyResult<()> {
        let inner = self.inner.clone();
        let source = fs.inner.clone();
        py.detach(|| {
            self.rt.block_on(async move {
                let mounted_fs = source.resolve().await;
                let bash = inner.lock().await;
                bash.mount(Path::new(&vfs_path), mounted_fs)
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
                Ok(())
            })
        })
    }

    /// Unmount a live filesystem without rebuilding the interpreter.
    fn unmount(&self, py: Python<'_>, vfs_path: String) -> PyResult<()> {
        let inner = self.inner.clone();
        py.detach(|| {
            self.rt.block_on(async move {
                let bash = inner.lock().await;
                bash.unmount(Path::new(&vfs_path))
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
                Ok(())
            })
        })
    }

    #[getter]
    fn name(&self) -> &str {
        "bashkit"
    }

    #[getter]
    fn short_description(&self) -> &str {
        "Run bash commands in an isolated virtual filesystem"
    }

    fn description(&self) -> PyResult<String> {
        Ok(self.build_rust_tool().description().to_string())
    }

    fn help(&self) -> PyResult<String> {
        Ok(self.build_rust_tool().help())
    }

    fn system_prompt(&self) -> PyResult<String> {
        Ok(self.build_rust_tool().system_prompt())
    }

    fn input_schema(&self) -> PyResult<String> {
        let schema = self.build_rust_tool().input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    fn output_schema(&self) -> PyResult<String> {
        let schema = self.build_rust_tool().output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    #[getter]
    fn version(&self) -> &str {
        VERSION
    }

    fn __repr__(&self) -> String {
        format!(
            "BashTool(username={:?}, hostname={:?})",
            self.username.as_deref().unwrap_or("user"),
            self.hostname.as_deref().unwrap_or("sandbox")
        )
    }
}

// ============================================================================
// ScriptedTool — multi-tool orchestration via bash scripts
// ============================================================================

/// Entry for a registered Python tool callback
struct PyToolEntry {
    name: String,
    description: String,
    schema: serde_json::Value,
    callback: Py<PyAny>,
    /// True when callback is `async def` (coroutine function).
    is_async: bool,
}

/// Compose Python callbacks as bash builtins for multi-tool orchestration.
///
/// Each registered tool becomes a bash builtin command. An LLM (or user) writes
/// a single bash script that pipes, loops, and branches across all tools.
///
/// Python callbacks receive `(params: dict, stdin: str | None)` and return a
/// string. Raise an exception to signal failure.
///
/// Example:
///     ```python
///     from bashkit import ScriptedTool
///
///     def get_user(params, stdin=None):
///         return '{"id": 1, "name": "Alice"}'
///
///     tool = ScriptedTool("api")
///     tool.add_tool("get_user", "Fetch user by ID",
///         callback=get_user,
///         schema={"type": "object", "properties": {"id": {"type": "integer"}}})
///
///     result = tool.execute_sync("get_user --id 1 | jq -r '.name'")
///     print(result.stdout)  # Alice
///     ```
#[pyclass]
pub struct ScriptedTool {
    name: String,
    short_desc: Option<String>,
    tools: Vec<PyToolEntry>,
    env_vars: Vec<(String, String)>,
    /// Shared tokio runtime — reused across all sync calls to avoid
    /// per-call OS thread/fd exhaustion (issue #414).
    rt: Arc<Runtime>,
    callback_engine: Arc<PyCallbackEngine>,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
}

impl ScriptedTool {
    /// Build a Rust ScriptedTool from stored Python config.
    ///
    /// The supplied session captures the caller's `ContextVar` state and, for
    /// async execute(), the caller's active asyncio loop. Each Python callback
    /// is invoked via `ctx.run()` to restore those vars.
    fn build_rust_tool_with_session(
        &self,
        py: Python<'_>,
        session: Arc<PyCallbackSession>,
    ) -> RustScriptedTool {
        let mut builder = RustScriptedTool::builder(&self.name);

        if let Some(ref desc) = self.short_desc {
            builder = builder.short_description(desc);
        }

        for entry in &self.tools {
            let py_callback = entry.callback.clone_ref(py);
            let def =
                ToolDef::new(&entry.name, &entry.description).with_schema(entry.schema.clone());
            let tool_name = entry.name.clone();

            if entry.is_async {
                let session = session.clone();
                builder = builder.async_tool_fn(def, move |args: ToolArgs| {
                    let session = session.clone();
                    let tool_name = tool_name.clone();
                    let py_callback = Python::attach(|py| py_callback.clone_ref(py));
                    async move {
                        let py_args = Python::attach(|py| make_py_tool_callback_args(py, &args))?;
                        call_python_string_callback_async(
                            session,
                            &tool_name,
                            &py_callback,
                            py_args,
                        )
                        .await
                    }
                });
            } else {
                let session = session.clone();
                builder = builder.tool_fn(def, move |args: &ToolArgs| {
                    Python::attach(|py| {
                        call_python_string_callback_sync(
                            py,
                            session.as_ref(),
                            &tool_name,
                            &py_callback,
                            make_py_tool_callback_args(py, args)?,
                        )
                    })
                });
            }
        }

        for (k, v) in &self.env_vars {
            builder = builder.env(k, v);
        }

        if self.max_commands.is_some() || self.max_loop_iterations.is_some() {
            let mut limits = ExecutionLimits::new();
            if let Some(mc) = self.max_commands {
                limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
            }
            if let Some(mli) = self.max_loop_iterations {
                limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
            }
            builder = builder.limits(limits);
        }

        builder.build()
    }
}

#[pymethods]
impl ScriptedTool {
    /// Create a new ScriptedTool.
    ///
    /// Args:
    ///     name: Tool name (used in system prompt and docs)
    ///     short_description: One-line description
    ///     max_commands: Max commands per execute call
    ///     max_loop_iterations: Max loop iterations per execute call
    #[new]
    #[pyo3(signature = (name, short_description=None, max_commands=None, max_loop_iterations=None))]
    fn new(
        name: String,
        short_description: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
    ) -> PyResult<Self> {
        let rt = make_runtime()?;
        let callback_engine = Python::attach(PyCallbackEngine::new)?;

        Ok(Self {
            name,
            short_desc: short_description,
            tools: Vec::new(),
            env_vars: Vec::new(),
            rt,
            callback_engine,
            max_commands,
            max_loop_iterations,
        })
    }

    /// Register a tool command.
    ///
    /// The callback can be synchronous or ``async def``:
    ///
    /// - sync:  ``callback(params: dict, stdin: str | None) -> str``
    /// - async: ``async def callback(params: dict, stdin: str | None) -> str``
    ///
    /// ``contextvars.ContextVar`` values active at ``execute()`` / ``execute_sync()``
    /// call time are automatically propagated into callbacks.
    ///
    /// Args:
    ///     name: Command name (becomes a bash builtin)
    ///     description: Human-readable description
    ///     callback: Python callable ``(params, stdin) -> str``
    ///     schema: Optional JSON Schema dict for input parameters
    #[pyo3(signature = (name, description, callback, schema=None))]
    fn add_tool(
        &mut self,
        py: Python<'_>,
        name: String,
        description: String,
        callback: Py<PyAny>,
        schema: Option<Bound<'_, pyo3::PyAny>>,
    ) -> PyResult<()> {
        self.tools.push(build_py_tool_entry(
            py,
            name,
            description,
            callback,
            schema,
        )?);
        Ok(())
    }

    /// Add an environment variable visible inside scripts.
    fn env(&mut self, key: String, value: String) {
        self.env_vars.push((key, value));
    }

    /// Execute a bash script asynchronously.
    fn execute<'py>(&self, py: Python<'py>, commands: String) -> PyResult<Bound<'py, PyAny>> {
        let session = PyCallbackSession::capture(
            py,
            self.callback_engine.clone(),
            self.tools.iter().any(|entry| entry.is_async),
            true,
            PySyncLoopMode::PerSession,
        )?;
        let tool = self.build_rust_tool_with_session(py, session.clone());
        let future = future_into_py(py, async move {
            let resp = tool
                .execute(ToolRequest {
                    commands,
                    timeout_ms: None,
                })
                .await;
            Ok(ExecResult {
                stdout: resp.stdout,
                stderr: resp.stderr,
                exit_code: resp.exit_code,
                error: resp.error,
                stdout_truncated: resp.stdout_truncated,
                stderr_truncated: resp.stderr_truncated,
                final_env: resp.final_env,
            })
        })?;
        attach_future_cancellation_callback(py, &future, session.clone())?;
        wrap_future_with_cancel(py, future, session)
    }

    /// Execute a bash script synchronously (blocking).
    /// Releases GIL before blocking on tokio to prevent deadlock with callbacks.
    fn execute_sync(&self, py: Python<'_>, commands: String) -> PyResult<ExecResult> {
        let session = PyCallbackSession::capture(
            py,
            self.callback_engine.clone(),
            self.tools.iter().any(|entry| entry.is_async),
            false,
            PySyncLoopMode::PerSession,
        )?;
        let tool = self.build_rust_tool_with_session(py, session);

        let resp = py.detach(|| {
            self.rt.block_on(async move {
                tool.execute(ToolRequest {
                    commands,
                    timeout_ms: None,
                })
                .await
            })
        });
        Ok(ExecResult {
            stdout: resp.stdout,
            stderr: resp.stderr,
            exit_code: resp.exit_code,
            error: resp.error,
            stdout_truncated: resp.stdout_truncated,
            stderr_truncated: resp.stderr_truncated,
            final_env: resp.final_env,
        })
    }

    /// Get the tool name.
    #[getter(name)]
    fn name_prop(&self) -> &str {
        &self.name
    }

    /// Get the short description.
    #[getter]
    fn short_description(&self) -> String {
        self.short_desc
            .clone()
            .unwrap_or_else(|| format!("ScriptedTool: {}", self.name))
    }

    /// Number of registered tools.
    fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Get the token-efficient description.
    fn description(&self) -> String {
        Python::attach(|py| {
            let session = PyCallbackSession::capture(
                py,
                self.callback_engine.clone(),
                false,
                false,
                PySyncLoopMode::PerSession,
            )
            .expect("callback session");
            self.build_rust_tool_with_session(py, session)
                .description()
                .to_string()
        })
    }

    /// Get help as a Markdown document.
    fn help(&self) -> String {
        Python::attach(|py| {
            let session = PyCallbackSession::capture(
                py,
                self.callback_engine.clone(),
                false,
                false,
                PySyncLoopMode::PerSession,
            )
            .expect("callback session");
            self.build_rust_tool_with_session(py, session).help()
        })
    }

    /// Get compact system-prompt text for orchestration.
    fn system_prompt(&self) -> String {
        Python::attach(|py| {
            let session = PyCallbackSession::capture(
                py,
                self.callback_engine.clone(),
                false,
                false,
                PySyncLoopMode::PerSession,
            )
            .expect("callback session");
            self.build_rust_tool_with_session(py, session)
                .system_prompt()
        })
    }

    /// Get JSON input schema.
    fn input_schema(&self, py: Python<'_>) -> PyResult<String> {
        let session = PyCallbackSession::capture(
            py,
            self.callback_engine.clone(),
            false,
            false,
            PySyncLoopMode::PerSession,
        )?;
        let tool = self.build_rust_tool_with_session(py, session);
        let schema = tool.input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    /// Get JSON output schema.
    fn output_schema(&self, py: Python<'_>) -> PyResult<String> {
        let session = PyCallbackSession::capture(
            py,
            self.callback_engine.clone(),
            false,
            false,
            PySyncLoopMode::PerSession,
        )?;
        let tool = self.build_rust_tool_with_session(py, session);
        let schema = tool.output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    /// Get tool version.
    #[getter]
    fn version(&self) -> &str {
        VERSION
    }

    fn __repr__(&self) -> String {
        format!(
            "ScriptedTool(name={:?}, tools={})",
            self.name,
            self.tools.len()
        )
    }
}

// ============================================================================
// BashError — exception for non-zero exit codes
// ============================================================================

pyo3::create_exception!(bashkit, BashError, pyo3::exceptions::PyException);

/// Raise a `BashError` from an `ExecResult` with non-zero exit code.
fn raise_bash_error(result: &ExecResult) -> PyErr {
    let message = result
        .error
        .clone()
        .unwrap_or_else(|| result.stderr.clone());
    let msg = if message.is_empty() {
        format!("Exit code {}", result.exit_code)
    } else {
        message
    };
    Python::attach(|py| {
        let err = BashError::new_err(msg);
        // Attach structured fields to the exception instance.
        let val = err.value(py);
        let _ = val.setattr("exit_code", result.exit_code);
        let _ = val.setattr("stderr", &result.stderr);
        let _ = val.setattr("stdout", &result.stdout);
        err
    })
}

// ============================================================================
// Module-level functions
// ============================================================================

/// Get the bashkit version string.
#[pyfunction]
fn get_version() -> &'static str {
    VERSION
}

/// Create a LangChain-compatible tool spec from BashTool.
#[pyfunction]
fn create_langchain_tool_spec() -> PyResult<pyo3::Py<PyDict>> {
    let tool = RustBashTool::default();

    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item("name", tool.name())?;
        dict.set_item("description", tool.description())?;

        let schema = tool.input_schema();
        let schema_str = serde_json::to_string(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema error: {}", e)))?;
        dict.set_item("args_schema", schema_str)?;

        Ok(dict.into())
    })
}

// ============================================================================
// Python module
// ============================================================================

#[pymodule]
fn _bashkit(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyBash>()?;
    m.add_class::<BashTool>()?;
    m.add_class::<ScriptedTool>()?;
    m.add_class::<ShellState>()?;
    m.add_class::<ExecResult>()?;
    m.add_class::<PyBuiltinContext>()?;
    m.add_class::<PyFileSystem>()?;
    m.add("BashError", m.py().get_type::<BashError>())?;
    m.add_function(wrap_pyfunction!(create_langchain_tool_spec, m)?)?;
    m.add_function(wrap_pyfunction!(get_version, m)?)?;
    Ok(())
}

// ============================================================================
// MontyObject <-> Python conversion helpers
// ============================================================================

fn monty_to_py(py: Python<'_>, obj: &MontyObject) -> PyResult<Py<PyAny>> {
    monty_to_py_inner(py, obj, 0)
}

fn monty_to_py_inner(py: Python<'_>, obj: &MontyObject, depth: usize) -> PyResult<Py<PyAny>> {
    if depth > MAX_NESTING_DEPTH {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "MontyObject nesting depth exceeds maximum of 64",
        ));
    }
    match obj {
        MontyObject::None => Ok(py.None()),
        MontyObject::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        MontyObject::Int(i) => Ok(i.into_pyobject(py)?.into_any().unbind()),
        // BigInt: convert to Python int via its decimal string representation.
        MontyObject::BigInt(b) => {
            let int_str = b.to_string();
            let py_int = py.import("builtins")?.getattr("int")?.call1((int_str,))?;
            Ok(py_int.into_any().unbind())
        }
        MontyObject::Float(f) => Ok(f.into_pyobject(py)?.into_any().unbind()),
        MontyObject::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        // Known limitation: Path becomes a plain Python str, not pathlib.Path.
        MontyObject::Path(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        MontyObject::Bytes(b) => Ok(b.as_slice().into_pyobject(py)?.into_any().unbind()),
        MontyObject::Tuple(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py_inner(py, v, depth + 1))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyTuple::new(py, &py_items)?.into_any().unbind())
        }
        MontyObject::List(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py_inner(py, v, depth + 1))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyList::new(py, &py_items)?.into_any().unbind())
        }
        MontyObject::Set(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py_inner(py, v, depth + 1))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PySet::new(py, &py_items)?.into_any().unbind())
        }
        MontyObject::FrozenSet(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py_inner(py, v, depth + 1))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyFrozenSet::new(py, &py_items)?.into_any().unbind())
        }
        // NamedTuple: convert to dict mapping field names to values, preserving field names.
        MontyObject::NamedTuple {
            field_names,
            values,
            ..
        } => {
            let dict = PyDict::new(py);
            for (name, value) in field_names.iter().zip(values.iter()) {
                dict.set_item(name, monty_to_py_inner(py, value, depth + 1)?)?;
            }
            Ok(dict.into_any().unbind())
        }
        MontyObject::Dict(dict_pairs) => {
            let dict = PyDict::new(py);
            // DictPairs only implements IntoIterator (consuming), so clone is required
            // to iterate without moving out of the match guard.
            for (k, v) in dict_pairs.clone() {
                dict.set_item(
                    monty_to_py_inner(py, &k, depth + 1)?,
                    monty_to_py_inner(py, &v, depth + 1)?,
                )?;
            }
            Ok(dict.into_any().unbind())
        }
        // All other variants (Exception, Type, Function, etc.) — repr as string.
        other => Ok(other.py_repr().into_pyobject(py)?.into_any().unbind()),
    }
}

fn py_to_monty(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<MontyObject> {
    py_to_monty_inner(py, obj, 0)
}

// `py` is used directly in `is_instance_of`, `import`, and `cast` calls — not only
// forwarded in recursive calls — so clippy's "only used in recursion" is a false positive.
#[allow(clippy::only_used_in_recursion)]
fn py_to_monty_inner(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
    depth: usize,
) -> PyResult<MontyObject> {
    if depth > MAX_NESTING_DEPTH {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Python object nesting depth exceeds maximum of 64",
        ));
    }
    if obj.is_none() {
        return Ok(MontyObject::None);
    }
    // bool must come before int — bool is a subtype of int in Python
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(MontyObject::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(MontyObject::Int(i));
    }
    // Large Python int that overflows i64: convert via decimal string → BigInt.
    if obj.is_instance_of::<PyInt>() {
        let s = obj.str()?.extract::<String>()?;
        let b = s.parse::<num_bigint::BigInt>().map_err(|e| {
            PyValueError::new_err(format!("failed to parse Python int as BigInt: {e}"))
        })?;
        return Ok(MontyObject::BigInt(b));
    }
    // Guard f64 with an isinstance check so large Python ints (which widen to f64)
    // are not incorrectly classified as floats.
    if obj.is_instance_of::<PyFloat>()
        && let Ok(f) = obj.extract::<f64>()
    {
        return Ok(MontyObject::Float(f));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(MontyObject::String(s));
    }
    // Guard bytes with isinstance to avoid ambiguity with str-like objects.
    if obj.is_instance_of::<PyBytes>()
        && let Ok(b) = obj.extract::<Vec<u8>>()
    {
        return Ok(MontyObject::Bytes(b));
    }
    if let Ok(tuple) = obj.cast::<PyTuple>() {
        let items = tuple
            .iter()
            .map(|v| py_to_monty_inner(py, &v, depth + 1))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::Tuple(items));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        let items = list
            .iter()
            .map(|v| py_to_monty_inner(py, &v, depth + 1))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::List(items));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let pairs: Vec<(MontyObject, MontyObject)> = dict
            .iter()
            .map(|(k, v)| {
                Ok((
                    py_to_monty_inner(py, &k, depth + 1)?,
                    py_to_monty_inner(py, &v, depth + 1)?,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::dict(pairs));
    }
    if let Ok(set) = obj.cast::<PySet>() {
        let items = set
            .iter()
            .map(|v| py_to_monty_inner(py, &v, depth + 1))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::Set(items));
    }
    if let Ok(fset) = obj.cast::<PyFrozenSet>() {
        let items = fset
            .iter()
            .map(|v| py_to_monty_inner(py, &v, depth + 1))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::FrozenSet(items));
    }
    // Fallback: convert to string via __str__
    Ok(MontyObject::String(obj.str()?.extract::<String>()?))
}
