// napi macros generate code that triggers some clippy lints
#![allow(clippy::needless_pass_by_value, clippy::trivially_copy_pass_by_ref)]

//! Node.js/TypeScript bindings for the Bashkit sandboxed bash interpreter.
//!
//! Exposes `Bash` (core interpreter), `BashTool` (interpreter + LLM metadata),
//! and `ExecResult` via napi-rs for use from JavaScript/TypeScript.
//!
//! # Safety: `Arc<SharedState>` pattern
//!
//! Both `Bash` and `BashTool` wrap all mutable state in `Arc<SharedState>`.
//! Every `#[napi]` method clones the `Arc` *before* doing any blocking or async
//! work. This prevents CodeQL `rust/access-invalid-pointer` alerts caused by
//! holding a raw-pointer-derived `&self` across `block_on` or `.await` points.

use bashkit::tool::VERSION;
use bashkit::{
    Bash as RustBash, BashTool as RustBashTool, ExecResult as RustExecResult, ExecutionLimits,
    ExtFunctionResult, FileSystem as BashFileSystem, FileType, InMemoryFs, Metadata, MontyObject,
    OutputCallback, PosixFs, PythonExternalFnHandler, PythonLimits, RealFs, RealFsMode,
    ScriptedTool as RustScriptedTool, SnapshotOptions as RustSnapshotOptions, Tool, ToolArgs,
    ToolDef, ToolRequest,
};
use bashkit_fs_interop::{
    BashkitFsAbiHandleV1, BashkitFsAbiOwnedHandleV1, export_filesystem, import_filesystem,
};
use napi::bindgen_prelude::{Buffer, JsObjectValue, Object, sys};
use napi::{Env, JsValue, Unknown};
use napi_derive::napi;
use std::collections::HashMap;
use std::mem::{MaybeUninit, size_of};
use std::path::Path;
use std::pin::Pin;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Shared tokio runtime + concurrency limiter for JS tool callbacks (issue #982).
// A single multi-thread runtime is created lazily and reused for every callback
// invocation, replacing the previous pattern of spawning an unbounded number of
// OS threads each with its own single-threaded runtime. A semaphore caps the
// maximum number of concurrent in-flight callbacks to prevent DoS.
// ---------------------------------------------------------------------------
const MAX_CONCURRENT_TOOL_CALLBACKS: usize = 10;

fn callback_runtime() -> &'static tokio::runtime::Runtime {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to create shared callback runtime")
    })
}

fn callback_semaphore() -> &'static tokio::sync::Semaphore {
    use std::sync::OnceLock;
    static SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(MAX_CONCURRENT_TOOL_CALLBACKS))
}

// Decision: reject same-instance onOutput re-entry at the binding boundary so
// sync paths fail with a JS error instead of deadlocking or panicking.
const ON_OUTPUT_REENTRY_ERROR: &str = "onOutput cannot re-enter the same Bash instance; use collected output or another Bash instance for live access";

struct OnOutputReentryScope {
    depth: Arc<AtomicUsize>,
}

impl OnOutputReentryScope {
    fn enter(depth: Arc<AtomicUsize>) -> Self {
        depth.fetch_add(1, Ordering::SeqCst);
        Self { depth }
    }
}

impl Drop for OnOutputReentryScope {
    fn drop(&mut self) {
        self.depth.fetch_sub(1, Ordering::SeqCst);
    }
}

fn reject_on_output_reentry(state: &Arc<SharedState>) -> napi::Result<()> {
    if state.on_output_reentry_depth.load(Ordering::SeqCst) > 0 {
        return Err(napi::Error::from_reason(ON_OUTPUT_REENTRY_ERROR));
    }
    Ok(())
}

// ============================================================================
// MontyObject <-> JSON conversion
// ============================================================================

#[allow(dead_code)]
fn monty_to_json(obj: &MontyObject) -> serde_json::Value {
    match obj {
        MontyObject::None => serde_json::Value::Null,
        MontyObject::Bool(b) => serde_json::Value::Bool(*b),
        MontyObject::Int(i) => serde_json::json!(*i),
        MontyObject::BigInt(b) => serde_json::Value::String(b.to_string()),
        MontyObject::Float(f) => serde_json::json!(*f),
        MontyObject::String(s) | MontyObject::Path(s) => serde_json::Value::String(s.clone()),
        MontyObject::Bytes(b) => serde_json::Value::String(base64_encode(b)),
        MontyObject::Tuple(items) | MontyObject::List(items) => {
            serde_json::Value::Array(items.iter().map(monty_to_json).collect())
        }
        MontyObject::Set(items) | MontyObject::FrozenSet(items) => {
            serde_json::Value::Array(items.iter().map(monty_to_json).collect())
        }
        MontyObject::Dict(pairs) => {
            let mut map = serde_json::Map::new();
            for (k, v) in pairs.clone() {
                let key = match &k {
                    MontyObject::String(s) => s.clone(),
                    other => format!("{}", monty_to_json(other)),
                };
                map.insert(key, monty_to_json(&v));
            }
            serde_json::Value::Object(map)
        }
        MontyObject::NamedTuple {
            field_names,
            values,
            ..
        } => {
            let mut map = serde_json::Map::new();
            for (name, value) in field_names.iter().zip(values.iter()) {
                map.insert(name.clone(), monty_to_json(value));
            }
            serde_json::Value::Object(map)
        }
        other => serde_json::Value::String(other.py_repr()),
    }
}

#[allow(dead_code)]
fn json_to_monty(val: &serde_json::Value) -> MontyObject {
    match val {
        serde_json::Value::Null => MontyObject::None,
        serde_json::Value::Bool(b) => MontyObject::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                MontyObject::Int(i)
            } else if let Some(f) = n.as_f64() {
                MontyObject::Float(f)
            } else {
                MontyObject::None
            }
        }
        serde_json::Value::String(s) => MontyObject::String(s.clone()),
        serde_json::Value::Array(arr) => MontyObject::List(arr.iter().map(json_to_monty).collect()),
        serde_json::Value::Object(map) => {
            let pairs: Vec<(MontyObject, MontyObject)> = map
                .iter()
                .map(|(k, v)| (MontyObject::String(k.clone()), json_to_monty(v)))
                .collect();
            MontyObject::dict(pairs)
        }
    }
}

#[allow(dead_code)]
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[(n >> 18 & 63) as usize] as char);
        result.push(CHARS[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(n >> 6 & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ============================================================================
// FileMetadata + JsDirEntry + JsFileSystem
// ============================================================================

/// Metadata for a VFS entry, returned by `stat()` and `readDir()`.
#[napi(object)]
pub struct FileMetadata {
    pub file_type: String,
    pub size: f64,
    pub mode: u32,
    pub modified: f64,
    pub created: f64,
}

/// Directory entry with name and metadata.
#[napi(object)]
pub struct JsDirEntry {
    pub name: String,
    pub metadata: FileMetadata,
}

fn metadata_to_js(meta: &Metadata) -> FileMetadata {
    let file_type = match meta.file_type {
        FileType::File => "file",
        FileType::Directory => "directory",
        FileType::Symlink => "symlink",
        FileType::Fifo => "fifo",
    }
    .to_string();
    FileMetadata {
        file_type,
        size: meta.size as f64,
        mode: meta.mode,
        modified: meta
            .modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        created: meta
            .created
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
    }
}

/// Direct VFS accessor — bypasses shell command parsing for file operations.
///
/// Obtained via `bash.fs()` or `bashTool.fs()`. All methods are synchronous
/// and block until the underlying async VFS operation completes.
#[derive(Clone)]
enum FileSystemHandle {
    Static(Arc<dyn BashFileSystem>),
    Live(Arc<SharedState>),
}

#[napi(js_name = "FileSystem")]
pub struct JsFileSystem {
    inner: FileSystemHandle,
}

impl JsFileSystem {
    fn from_static(fs: Arc<dyn BashFileSystem>) -> Self {
        Self {
            inner: FileSystemHandle::Static(fs),
        }
    }

    fn from_live(state: Arc<SharedState>) -> Self {
        Self {
            inner: FileSystemHandle::Live(state),
        }
    }

    fn with_fs<T, Fut>(&self, f: impl FnOnce(Arc<dyn BashFileSystem>) -> Fut) -> napi::Result<T>
    where
        Fut: std::future::Future<Output = napi::Result<T>>,
    {
        match &self.inner {
            FileSystemHandle::Static(fs) => callback_runtime().block_on(f(fs.clone())),
            FileSystemHandle::Live(state) => block_on_with(state, |s| async move {
                let bash = s.inner.lock().await;
                f(bash.fs()).await
            }),
        }
    }

    fn export_fs(&self) -> napi::Result<Arc<dyn BashFileSystem>> {
        self.with_fs(|fs| async move { Ok(fs) })
    }
}

unsafe extern "C" fn finalize_file_system_external(
    _env: sys::napi_env,
    finalize_data: *mut std::ffi::c_void,
    _hint: *mut std::ffi::c_void,
) {
    if finalize_data.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(
            finalize_data.cast::<BashkitFsAbiOwnedHandleV1>(),
        ));
    }
}

fn encode_file_system_handle(handle: &BashkitFsAbiHandleV1) -> Vec<u8> {
    unsafe {
        slice::from_raw_parts(
            (handle as *const BashkitFsAbiHandleV1).cast::<u8>(),
            size_of::<BashkitFsAbiHandleV1>(),
        )
        .to_vec()
    }
}

fn decode_file_system_handle(bytes: &[u8]) -> napi::Result<BashkitFsAbiHandleV1> {
    if bytes.len() != size_of::<BashkitFsAbiHandleV1>() {
        return Err(napi::Error::from_reason(format!(
            "filesystem handle must be {} bytes",
            size_of::<BashkitFsAbiHandleV1>()
        )));
    }
    let mut handle = MaybeUninit::<BashkitFsAbiHandleV1>::uninit();
    unsafe {
        ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            handle.as_mut_ptr().cast::<u8>(),
            bytes.len(),
        );
        Ok(handle.assume_init())
    }
}

fn create_file_system_owner<'env>(
    env: Env,
    handle: BashkitFsAbiOwnedHandleV1,
) -> napi::Result<Unknown<'env>> {
    let raw_handle = Box::into_raw(Box::new(handle));
    let mut raw_value = ptr::null_mut();
    let status = unsafe {
        sys::napi_create_external(
            env.raw(),
            raw_handle.cast(),
            Some(finalize_file_system_external),
            ptr::null_mut(),
            &mut raw_value,
        )
    };
    if status != sys::Status::napi_ok {
        unsafe {
            drop(Box::from_raw(raw_handle));
        }
        return Err(napi::Error::from_reason(
            "failed to create filesystem external".to_string(),
        ));
    }
    Ok(unsafe { Unknown::from_raw_unchecked(env.raw(), raw_value) })
}

fn create_file_system_external(
    env: Env,
    handle: BashkitFsAbiOwnedHandleV1,
) -> napi::Result<Object<'static>> {
    let bytes = encode_file_system_handle(handle.as_handle());
    let owner = create_file_system_owner(env, handle)?;
    let mut external = Object::new(&env)?;
    external.set_named_property("bytes", Buffer::from(bytes))?;
    external.set_named_property("owner", owner)?;
    Ok(external)
}

fn import_external_file_system(external: Unknown<'_>) -> napi::Result<Arc<dyn BashFileSystem>> {
    let external = unsafe { external.cast::<Object<'_>>() }?;
    let bytes: Buffer = external.get_named_property("bytes")?;
    let handle = decode_file_system_handle(bytes.as_ref())?;
    import_filesystem(&handle).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi]
impl JsFileSystem {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self::from_static(Arc::new(InMemoryFs::new()))
    }

    /// Create a filesystem backed by a real host directory.
    #[napi(factory)]
    pub fn real(host_path: String, writable: Option<bool>) -> napi::Result<Self> {
        let mode = if writable.unwrap_or(false) {
            RealFsMode::ReadWrite
        } else {
            RealFsMode::ReadOnly
        };
        let backend =
            RealFs::new(&host_path, mode).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let fs: Arc<dyn BashFileSystem> = Arc::new(PosixFs::new(backend));
        Ok(Self::from_static(fs))
    }

    /// Create a filesystem from an external handle exported by another addon.
    #[napi(factory)]
    pub fn from_external(external: Unknown<'_>) -> napi::Result<Self> {
        let fs = import_external_file_system(external)?;
        Ok(Self::from_static(fs))
    }

    /// Export this filesystem as an external handle for addon interop.
    #[napi]
    pub fn to_external(&self, env: Env) -> napi::Result<Object<'static>> {
        let fs = self.export_fs()?;
        let handle = export_filesystem(fs).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        create_file_system_external(env, handle)
    }

    /// Read a file as UTF-8 string.
    #[napi]
    pub fn read_file(&self, path: String) -> napi::Result<String> {
        self.with_fs(|fs| async move {
            let bytes = fs
                .read_file(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            String::from_utf8(bytes)
                .map_err(|e| napi::Error::from_reason(format!("Invalid UTF-8: {e}")))
        })
    }

    /// Write string content to a file (creates or replaces).
    #[napi]
    pub fn write_file(&self, path: String, content: String) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.write_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Append string content to a file.
    #[napi]
    pub fn append_file(&self, path: String, content: String) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.append_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a directory. If `recursive` is true, creates parent directories.
    #[napi]
    pub fn mkdir(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.mkdir(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Remove a file or directory. If `recursive` is true, removes contents.
    #[napi]
    pub fn remove(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.remove(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Get metadata for a path.
    #[napi]
    pub fn stat(&self, path: String) -> napi::Result<FileMetadata> {
        self.with_fs(|fs| async move {
            let meta = fs
                .stat(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(metadata_to_js(&meta))
        })
    }

    /// Check if a path exists.
    #[napi]
    pub fn exists(&self, path: String) -> napi::Result<bool> {
        self.with_fs(|fs| async move {
            fs.exists(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// List directory entries with metadata.
    #[napi]
    pub fn read_dir(&self, path: String) -> napi::Result<Vec<JsDirEntry>> {
        self.with_fs(|fs| async move {
            let entries = fs
                .read_dir(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(entries
                .iter()
                .map(|e| JsDirEntry {
                    name: e.name.clone(),
                    metadata: metadata_to_js(&e.metadata),
                })
                .collect())
        })
    }

    /// Create a symbolic link.
    #[napi]
    pub fn symlink(&self, target: String, link: String) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.symlink(Path::new(&target), Path::new(&link))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Read the target of a symbolic link.
    #[napi]
    pub fn read_link(&self, path: String) -> napi::Result<String> {
        self.with_fs(|fs| async move {
            let target = fs
                .read_link(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(target.to_string_lossy().to_string())
        })
    }

    /// Change file permissions.
    #[napi]
    pub fn chmod(&self, path: String, mode: u32) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.chmod(Path::new(&path), mode)
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Rename/move a file or directory.
    #[napi]
    pub fn rename(&self, from_path: String, to_path: String) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.rename(Path::new(&from_path), Path::new(&to_path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Copy a file.
    #[napi]
    pub fn copy(&self, from_path: String, to_path: String) -> napi::Result<()> {
        self.with_fs(|fs| async move {
            fs.copy(Path::new(&from_path), Path::new(&to_path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }
}

// ============================================================================
// ExecResult
// ============================================================================

/// Result from executing bash commands.
#[napi(object)]
#[derive(Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub error: Option<String>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub final_env: Option<HashMap<String, String>>,
    /// True if exit_code is 0.
    pub success: bool,
}

// The native JS callback arrives as one `[stdout, stderr]` tuple payload even
// though napi-rs models the args as `(String, String)`. Keep the public TS
// wrapper responsible for adapting that odd FFI shape into its
// object-shaped `{ stdout, stderr }` callback API.
type SyncOutputFn = napi::bindgen_prelude::FunctionRef<(String, String), Option<String>>;
type OutputTsfn = napi::threadsafe_function::ThreadsafeFunction<
    (String, String),
    Option<String>,
    (String, String),
    napi::Status,
    false,
    true,
>;

fn js_exec_result_from_rust(result: RustExecResult) -> ExecResult {
    ExecResult {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        error: None,
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
        final_env: result.final_env,
        success: result.exit_code == 0,
    }
}

fn js_exec_result_from_error(err: impl ToString) -> ExecResult {
    let msg = err.to_string();
    ExecResult {
        stdout: String::new(),
        stderr: msg.clone(),
        exit_code: 1,
        error: Some(msg),
        stdout_truncated: false,
        stderr_truncated: false,
        final_env: None,
        success: false,
    }
}

fn js_exec_result_from_bash_result(result: bashkit::Result<RustExecResult>) -> ExecResult {
    match result {
        Ok(result) => js_exec_result_from_rust(result),
        Err(err) => js_exec_result_from_error(err),
    }
}

fn callback_error_reason(err: impl ToString) -> String {
    format!("onOutput callback failed: {}", err.to_string())
}

fn record_callback_error(
    callback_error: &StdMutex<Option<String>>,
    cancelled: &Arc<AtomicBool>,
    callback_requested_cancel: &Arc<AtomicBool>,
    message: String,
) {
    if let Ok(mut callback_error) = callback_error.lock()
        && callback_error.is_none()
    {
        *callback_error = Some(message);
    }
    if !cancelled.swap(true, Ordering::SeqCst) {
        callback_requested_cancel.store(true, Ordering::SeqCst);
    }
}

fn take_callback_error(callback_error: &StdMutex<Option<String>>) -> Option<napi::Error> {
    callback_error
        .lock()
        .ok()
        .and_then(|mut callback_error| callback_error.take())
        .map(napi::Error::from_reason)
}

fn build_sync_output_callback(
    env_raw: usize,
    on_output: SyncOutputFn,
    cancelled: Arc<AtomicBool>,
    callback_requested_cancel: Arc<AtomicBool>,
    callback_error: Arc<StdMutex<Option<String>>>,
    on_output_reentry_depth: Arc<AtomicUsize>,
) -> OutputCallback {
    Box::new(move |stdout_chunk, stderr_chunk| {
        let has_error = callback_error
            .lock()
            .map(|callback_error| callback_error.is_some())
            .unwrap_or(false);
        if has_error {
            return;
        }

        let env = napi::Env::from_raw(env_raw as napi::sys::napi_env);
        let callback = match on_output.borrow_back(&env) {
            Ok(callback) => callback,
            Err(err) => {
                record_callback_error(
                    &callback_error,
                    &cancelled,
                    &callback_requested_cancel,
                    callback_error_reason(err),
                );
                return;
            }
        };

        let _reentry_scope = OnOutputReentryScope::enter(on_output_reentry_depth.clone());
        match callback.call((stdout_chunk.to_string(), stderr_chunk.to_string())) {
            Ok(Some(err)) => {
                record_callback_error(
                    &callback_error,
                    &cancelled,
                    &callback_requested_cancel,
                    callback_error_reason(err),
                );
            }
            Ok(None) => {}
            Err(err) => {
                record_callback_error(
                    &callback_error,
                    &cancelled,
                    &callback_requested_cancel,
                    callback_error_reason(err),
                );
            }
        }
    })
}

fn build_async_output_callback(
    tsfn: Arc<OutputTsfn>,
    cancelled: Arc<AtomicBool>,
    callback_requested_cancel: Arc<AtomicBool>,
    on_output_reentry_depth: Arc<AtomicUsize>,
) -> (OutputCallback, Arc<StdMutex<Option<String>>>) {
    let callback_error = Arc::new(StdMutex::new(None));
    let callback_error_output = callback_error.clone();
    let cancelled_output = cancelled.clone();
    let callback_requested_cancel_output = callback_requested_cancel.clone();

    let output_callback: OutputCallback = Box::new(move |stdout_chunk, stderr_chunk| {
        let has_error = callback_error_output
            .lock()
            .map(|callback_error| callback_error.is_some())
            .unwrap_or(false);
        if has_error {
            return;
        }

        let stdout = stdout_chunk.to_string();
        let stderr = stderr_chunk.to_string();
        let tsfn = tsfn.clone();
        let on_output_reentry_depth = on_output_reentry_depth.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        // OutputCallback in core bashkit is synchronous. Dispatch onto the
        // shared callback runtime, then block until JS finishes so callback
        // errors abort execution immediately and chunk ordering stays stable.
        callback_runtime().spawn(async move {
            let result: Result<Option<String>, String> = {
                let _reentry_scope = OnOutputReentryScope::enter(on_output_reentry_depth);
                tsfn.call_async((stdout, stderr))
                    .await
                    .map_err(callback_error_reason)
            };
            let _ = tx.send(result);
        });

        match rx.recv() {
            Ok(Ok(Some(err))) => {
                record_callback_error(
                    &callback_error_output,
                    &cancelled_output,
                    &callback_requested_cancel_output,
                    callback_error_reason(err),
                );
            }
            Ok(Ok(None)) => {}
            Ok(Err(err)) => {
                record_callback_error(
                    &callback_error_output,
                    &cancelled_output,
                    &callback_requested_cancel_output,
                    err,
                );
            }
            Err(_) => {
                record_callback_error(
                    &callback_error_output,
                    &cancelled_output,
                    &callback_requested_cancel_output,
                    "onOutput callback failed: callback channel closed".to_string(),
                );
            }
        }
    });

    (output_callback, callback_error)
}

fn create_output_tsfn(
    on_output: napi::bindgen_prelude::Function<'_, (String, String), Option<String>>,
) -> napi::Result<Arc<OutputTsfn>> {
    let tsfn = on_output
        .build_threadsafe_function::<(String, String)>()
        .weak::<true>()
        .build()?;
    Ok(Arc::new(tsfn))
}

async fn execute_rust_bash(
    bash: &mut RustBash,
    commands: &str,
    output_callback: Option<OutputCallback>,
    callback_error: Option<&Arc<StdMutex<Option<String>>>>,
    cancelled: Option<&Arc<AtomicBool>>,
    callback_requested_cancel: Option<&Arc<AtomicBool>>,
) -> napi::Result<ExecResult> {
    let result = if let Some(output_callback) = output_callback {
        bash.exec_streaming(commands, output_callback).await
    } else {
        bash.exec(commands).await
    };

    if let Some(callback_error) = callback_error
        && let Some(err) = take_callback_error(callback_error)
    {
        if let Some((cancelled, callback_requested_cancel)) =
            cancelled.zip(callback_requested_cancel)
            && callback_requested_cancel.load(Ordering::SeqCst)
        {
            cancelled.store(false, Ordering::SeqCst);
        }
        return Err(err);
    }

    Ok(js_exec_result_from_bash_result(result))
}

// ============================================================================
// MountConfig + BashOptions
// ============================================================================

/// Configuration for a real filesystem mount.
#[napi(object)]
#[derive(Clone)]
pub struct MountConfig {
    /// Host filesystem path to mount.
    pub host_path: String,
    /// VFS path where mount appears (defaults to host_path).
    pub vfs_path: Option<String>,
    /// If true, mount is read-write (default: false → read-only).
    pub writable: Option<bool>,
}

/// Options for creating a Bash or BashTool instance.
#[napi(object)]
pub struct BashOptions {
    pub username: Option<String>,
    pub hostname: Option<String>,
    pub max_commands: Option<u32>,
    pub max_loop_iterations: Option<u32>,
    pub max_total_loop_iterations: Option<u32>,
    pub max_function_depth: Option<u32>,
    /// Execution timeout in milliseconds.
    pub timeout_ms: Option<u32>,
    /// Parser timeout in milliseconds.
    pub parser_timeout_ms: Option<u32>,
    pub max_input_bytes: Option<u32>,
    pub max_ast_depth: Option<u32>,
    pub max_parser_operations: Option<u32>,
    pub max_stdout_bytes: Option<u32>,
    pub max_stderr_bytes: Option<u32>,
    /// Maximum interpreter memory in bytes (variables, arrays, functions).
    ///
    /// Caps `max_total_variable_bytes` and clamps `max_function_body_bytes`.
    /// Prevents OOM from untrusted input such as exponential string doubling.
    /// Default (when omitted): 10 MB.
    pub max_memory: Option<f64>,
    /// Whether to capture the final environment state in ExecResult.
    pub capture_final_env: Option<bool>,
    /// Files to mount in the virtual filesystem.
    /// Keys are absolute paths, values are file content strings.
    pub files: Option<HashMap<String, String>>,
    /// Real filesystem mounts. Each entry: { hostPath, vfsPath?, writable? }
    pub mounts: Option<Vec<MountConfig>>,
    /// Enable embedded Python execution (`python`/`python3` builtins).
    pub python: Option<bool>,
    /// Names of external functions callable from embedded Python code.
    pub external_functions: Option<Vec<String>>,
}

#[napi(object)]
pub struct SnapshotOptions {
    pub exclude_filesystem: Option<bool>,
    pub exclude_functions: Option<bool>,
}

fn default_opts() -> BashOptions {
    BashOptions {
        username: None,
        hostname: None,
        max_commands: None,
        max_loop_iterations: None,
        max_total_loop_iterations: None,
        max_function_depth: None,
        timeout_ms: None,
        parser_timeout_ms: None,
        max_input_bytes: None,
        max_ast_depth: None,
        max_parser_operations: None,
        max_stdout_bytes: None,
        max_stderr_bytes: None,
        max_memory: None,
        capture_final_env: None,
        files: None,
        mounts: None,
        python: None,
        external_functions: None,
    }
}

fn to_snapshot_options(options: Option<SnapshotOptions>) -> RustSnapshotOptions {
    RustSnapshotOptions {
        exclude_filesystem: options
            .as_ref()
            .and_then(|options| options.exclude_filesystem)
            .unwrap_or(false),
        exclude_functions: options
            .and_then(|options| options.exclude_functions)
            .unwrap_or(false),
    }
}

// ============================================================================
// SharedState — all mutable state behind Arc to avoid raw pointer issues
// ============================================================================

struct SharedState {
    inner: Mutex<RustBash>,
    rt: Mutex<tokio::runtime::Runtime>,
    cancelled: Arc<AtomicBool>,
    on_output_reentry_depth: Arc<AtomicUsize>,
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
    max_total_loop_iterations: Option<u32>,
    max_function_depth: Option<u32>,
    timeout_ms: Option<u32>,
    parser_timeout_ms: Option<u32>,
    max_input_bytes: Option<u32>,
    max_ast_depth: Option<u32>,
    max_parser_operations: Option<u32>,
    max_stdout_bytes: Option<u32>,
    max_stderr_bytes: Option<u32>,
    max_memory: Option<f64>,
    capture_final_env: Option<bool>,
    mounts: Option<Vec<MountConfig>>,
    python: bool,
    external_functions: Vec<String>,
    external_handler: Option<ExternalHandlerArc>,
}

/// Wrapper for the external handler that can be stored and cloned.
type ExternalHandlerArc = Arc<
    dyn Fn(
            String,
            Vec<MontyObject>,
            Vec<(MontyObject, MontyObject)>,
        ) -> Pin<Box<dyn std::future::Future<Output = ExtFunctionResult> + Send>>
        + Send
        + Sync,
>;

/// Clone `Arc<SharedState>`, then use the runtime to block on a future that
/// captures only the cloned Arc. This avoids holding raw `&self` across
/// `block_on` boundaries.
fn block_on_with<Fut, T>(
    state: &Arc<SharedState>,
    f: impl FnOnce(Arc<SharedState>) -> Fut,
) -> napi::Result<T>
where
    Fut: std::future::Future<Output = napi::Result<T>>,
{
    reject_on_output_reentry(state)?;
    let s = state.clone();
    let rt_guard = s.rt.blocking_lock();
    let s2 = state.clone();
    rt_guard.block_on(f(s2))
}

// ============================================================================
// Bash — core interpreter
// ============================================================================

/// Core bash interpreter with virtual filesystem.
///
/// State persists between calls — files created in one `execute()` are
/// available in subsequent calls.
#[napi]
pub struct Bash {
    state: Arc<SharedState>,
}

#[napi]
impl Bash {
    #[napi(constructor)]
    pub fn new(options: Option<BashOptions>) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);
        let state = shared_state_from_opts(opts, None)?;
        Ok(Self {
            state: Arc::new(state),
        })
    }

    /// Execute bash commands synchronously.
    #[napi(
        ts_args_type = "commands: string, onOutput?: (chunkPair: [string, string]) => string | undefined"
    )]
    pub fn execute_sync(
        &self,
        commands: String,
        on_output: Option<napi::bindgen_prelude::Function<'_, (String, String), Option<String>>>,
    ) -> napi::Result<ExecResult> {
        let env_raw = on_output
            .as_ref()
            .map(|on_output| on_output.value().env as usize);
        let on_output = on_output
            .map(|on_output| on_output.create_ref())
            .transpose()?;
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            if let Some((env_raw, on_output)) = env_raw.zip(on_output) {
                let cancelled = bash.cancellation_token();
                let callback_requested_cancel = Arc::new(AtomicBool::new(false));
                let callback_error = Arc::new(StdMutex::new(None));
                let output_callback = build_sync_output_callback(
                    env_raw,
                    on_output,
                    cancelled.clone(),
                    callback_requested_cancel.clone(),
                    callback_error.clone(),
                    s.on_output_reentry_depth.clone(),
                );
                execute_rust_bash(
                    &mut bash,
                    &commands,
                    Some(output_callback),
                    Some(&callback_error),
                    Some(&cancelled),
                    Some(&callback_requested_cancel),
                )
                .await
            } else {
                execute_rust_bash(&mut bash, &commands, None, None, None, None).await
            }
        })
    }

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        reject_on_output_reentry(&self.state)?;
        let s = self.state.clone();
        let mut bash = s.inner.lock().await;
        execute_rust_bash(&mut bash, &commands, None, None, None, None).await
    }

    #[napi(
        js_name = "executeWithOutput",
        ts_args_type = "commands: string, onOutput: (chunkPair: [string, string]) => string | undefined"
    )]
    pub fn execute_with_output<'env>(
        &self,
        commands: String,
        on_output: napi::bindgen_prelude::Function<'env, (String, String), Option<String>>,
    ) -> napi::Result<napi::bindgen_prelude::PromiseRaw<'env, ExecResult>> {
        reject_on_output_reentry(&self.state)?;
        let raw_env = on_output.value().env;
        let tsfn = create_output_tsfn(on_output)?;
        let state = self.state.clone();
        let promise = napi::bindgen_prelude::execute_tokio_future(
            raw_env,
            async move {
                reject_on_output_reentry(&state)?;
                let mut bash = state.inner.lock().await;
                let cancelled = bash.cancellation_token();
                let callback_requested_cancel = Arc::new(AtomicBool::new(false));
                let (output_callback, callback_error) = build_async_output_callback(
                    tsfn,
                    cancelled.clone(),
                    callback_requested_cancel.clone(),
                    state.on_output_reentry_depth.clone(),
                );
                execute_rust_bash(
                    &mut bash,
                    &commands,
                    Some(output_callback),
                    Some(&callback_error),
                    Some(&cancelled),
                    Some(&callback_requested_cancel),
                )
                .await
            },
            |env, val| unsafe {
                <ExecResult as napi::bindgen_prelude::ToNapiValue>::to_napi_value(env, val)
            },
        )?;
        Ok(napi::bindgen_prelude::PromiseRaw::new(raw_env, promise))
    }

    /// Cancel the currently running execution.
    ///
    /// Safe to call from any thread. Execution will abort at the next
    /// command boundary.
    #[napi]
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::SeqCst);
    }

    /// Clear the cancellation flag so subsequent executions proceed normally.
    ///
    /// Call this after a `cancel()` once the in-flight execution has finished
    /// and you want to reuse the same `Bash` instance without discarding shell
    /// or VFS state.
    #[napi]
    pub fn clear_cancel(&self) {
        self.state.cancelled.store(false, Ordering::SeqCst);
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            *bash = build_bash_from_state(&s, None);
            Ok(())
        })
    }

    // ========================================================================
    // Snapshot / Resume
    // ========================================================================

    /// Serialize interpreter state (shell variables, VFS contents, counters) to bytes.
    ///
    /// Returns a `Buffer` (Uint8Array) that can be persisted and used with
    /// `Bash.fromSnapshot()` to restore the session later.
    #[napi]
    pub fn snapshot(
        &self,
        options: Option<SnapshotOptions>,
    ) -> napi::Result<napi::bindgen_prelude::Buffer> {
        let options = to_snapshot_options(options);
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let bytes = bash
                .snapshot_with_options(options)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(napi::bindgen_prelude::Buffer::from(bytes))
        })
    }

    /// Restore interpreter state from a snapshot previously created with `snapshot()`.
    #[napi]
    pub fn restore_snapshot(&self, data: napi::bindgen_prelude::Buffer) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            bash.restore_snapshot(&data)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a new Bash instance from a snapshot.
    ///
    /// Accepts optional `BashOptions` to re-apply execution limits.
    /// Without options, safe defaults are used (not unlimited).
    #[napi(factory)]
    pub fn from_snapshot(
        data: napi::bindgen_prelude::Buffer,
        options: Option<BashOptions>,
    ) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);
        let mut state = shared_state_from_opts(opts, None)?;

        // restore_snapshot preserves the instance's limits while restoring shell state
        state
            .inner
            .get_mut()
            .restore_snapshot(&data)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(Self {
            state: Arc::new(state),
        })
    }

    // ========================================================================
    // VFS — direct filesystem access
    // ========================================================================

    /// Get metadata for a path in the virtual filesystem.
    #[napi]
    pub fn stat(&self, path: String) -> napi::Result<FileMetadata> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let meta = bash
                .fs()
                .stat(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(metadata_to_js(&meta))
        })
    }

    /// Append content to a file in the virtual filesystem.
    #[napi]
    pub fn append_file(&self, path: String, content: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .append_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Change file permissions in the virtual filesystem.
    #[napi]
    pub fn chmod(&self, path: String, mode: u32) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .chmod(Path::new(&path), mode)
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a symbolic link in the virtual filesystem.
    #[napi]
    pub fn symlink(&self, target: String, link: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .symlink(Path::new(&target), Path::new(&link))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Read the target of a symbolic link.
    #[napi]
    pub fn read_link(&self, path: String) -> napi::Result<String> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let target = bash
                .fs()
                .read_link(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(target.display().to_string())
        })
    }

    /// Read a file from the virtual filesystem. Returns contents as a UTF-8 string.
    #[napi]
    pub fn read_file(&self, path: String) -> napi::Result<String> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let bytes = bash
                .fs()
                .read_file(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            String::from_utf8(bytes)
                .map_err(|e| napi::Error::from_reason(format!("Invalid UTF-8: {e}")))
        })
    }

    /// Write a string to a file in the virtual filesystem.
    /// Creates the file if it doesn't exist, replaces contents if it does.
    #[napi]
    pub fn write_file(&self, path: String, content: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .write_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a directory. If recursive is true, creates parent directories as needed.
    #[napi]
    pub fn mkdir(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .mkdir(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Check if a path exists in the virtual filesystem.
    #[napi]
    pub fn exists(&self, path: String) -> napi::Result<bool> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .exists(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Remove a file or directory. If recursive is true, removes directory contents.
    #[napi]
    pub fn remove(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .remove(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// List entries in a directory with metadata (name, file type, size, etc.).
    #[napi]
    pub fn read_dir(&self, path: String) -> napi::Result<Vec<JsDirEntry>> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let entries = bash
                .fs()
                .read_dir(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(entries
                .into_iter()
                .map(|e| JsDirEntry {
                    name: e.name.clone(),
                    metadata: metadata_to_js(&e.metadata),
                })
                .collect())
        })
    }

    // ========================================================================
    // Mount — real filesystem mounts at runtime
    // ========================================================================

    /// Mount a host directory into the VFS at runtime.
    ///
    /// Read-only by default; pass `writable: true` to enable writes.
    ///
    /// **Security**: Writable mounts log a warning. Consider using
    /// `allowedMountPaths` in `BashOptions` to restrict which host paths
    /// may be mounted.
    #[napi]
    pub fn mount(
        &self,
        host_path: String,
        vfs_path: String,
        writable: Option<bool>,
    ) -> napi::Result<()> {
        let is_writable = writable.unwrap_or(false);
        if is_writable {
            eprintln!(
                "bashkit: warning: writable mount at {} — scripts can modify host files",
                host_path
            );
        }
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let mode = if is_writable {
                RealFsMode::ReadWrite
            } else {
                RealFsMode::ReadOnly
            };
            let real_backend = RealFs::new(&host_path, mode)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let fs: Arc<dyn BashFileSystem> = Arc::new(PosixFs::new(real_backend));
            bash.mount(Path::new(&vfs_path), fs)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Mount a filesystem handle without rebuilding the interpreter.
    #[napi]
    pub fn mount_file_system(&self, vfs_path: String, fs: Unknown<'_>) -> napi::Result<()> {
        let mounted_fs = import_external_file_system(fs)?;
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.mount(Path::new(&vfs_path), mounted_fs)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Unmount a previously mounted filesystem.
    #[napi]
    pub fn unmount(&self, vfs_path: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.unmount(Path::new(&vfs_path))
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Get a `JsFileSystem` handle for direct VFS operations.
    #[napi]
    pub fn fs(&self) -> napi::Result<JsFileSystem> {
        reject_on_output_reentry(&self.state)?;
        Ok(JsFileSystem::from_live(self.state.clone()))
    }
}

// ============================================================================
// BashTool — interpreter + tool-contract metadata
// ============================================================================

/// Bash interpreter with tool-contract metadata (`description`, `help`,
/// `system_prompt`, schemas).
///
/// Use this when integrating with AI frameworks that need tool definitions.
#[napi]
pub struct BashTool {
    state: Arc<SharedState>,
}

impl BashTool {
    fn build_rust_tool(state: &SharedState) -> RustBashTool {
        let mut builder = RustBashTool::builder();

        if let Some(ref username) = state.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = state.hostname {
            builder = builder.hostname(hostname);
        }

        builder.limits(build_limits(state)).build()
    }
}

#[napi]
impl BashTool {
    #[napi(constructor)]
    pub fn new(options: Option<BashOptions>) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);
        let state = shared_state_from_opts(opts, None)?;
        Ok(Self {
            state: Arc::new(state),
        })
    }

    /// Execute bash commands synchronously.
    #[napi(
        ts_args_type = "commands: string, onOutput?: (chunkPair: [string, string]) => string | undefined"
    )]
    pub fn execute_sync(
        &self,
        commands: String,
        on_output: Option<napi::bindgen_prelude::Function<'_, (String, String), Option<String>>>,
    ) -> napi::Result<ExecResult> {
        let env_raw = on_output
            .as_ref()
            .map(|on_output| on_output.value().env as usize);
        let on_output = on_output
            .map(|on_output| on_output.create_ref())
            .transpose()?;
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            if let Some((env_raw, on_output)) = env_raw.zip(on_output) {
                let cancelled = bash.cancellation_token();
                let callback_requested_cancel = Arc::new(AtomicBool::new(false));
                let callback_error = Arc::new(StdMutex::new(None));
                let output_callback = build_sync_output_callback(
                    env_raw,
                    on_output,
                    cancelled.clone(),
                    callback_requested_cancel.clone(),
                    callback_error.clone(),
                    s.on_output_reentry_depth.clone(),
                );
                execute_rust_bash(
                    &mut bash,
                    &commands,
                    Some(output_callback),
                    Some(&callback_error),
                    Some(&cancelled),
                    Some(&callback_requested_cancel),
                )
                .await
            } else {
                execute_rust_bash(&mut bash, &commands, None, None, None, None).await
            }
        })
    }

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        reject_on_output_reentry(&self.state)?;
        let s = self.state.clone();
        let mut bash = s.inner.lock().await;
        execute_rust_bash(&mut bash, &commands, None, None, None, None).await
    }

    #[napi(
        js_name = "executeWithOutput",
        ts_args_type = "commands: string, onOutput: (chunkPair: [string, string]) => string | undefined"
    )]
    pub fn execute_with_output<'env>(
        &self,
        commands: String,
        on_output: napi::bindgen_prelude::Function<'env, (String, String), Option<String>>,
    ) -> napi::Result<napi::bindgen_prelude::PromiseRaw<'env, ExecResult>> {
        reject_on_output_reentry(&self.state)?;
        let raw_env = on_output.value().env;
        let tsfn = create_output_tsfn(on_output)?;
        let state = self.state.clone();
        let promise = napi::bindgen_prelude::execute_tokio_future(
            raw_env,
            async move {
                reject_on_output_reentry(&state)?;
                let mut bash = state.inner.lock().await;
                let cancelled = bash.cancellation_token();
                let callback_requested_cancel = Arc::new(AtomicBool::new(false));
                let (output_callback, callback_error) = build_async_output_callback(
                    tsfn,
                    cancelled.clone(),
                    callback_requested_cancel.clone(),
                    state.on_output_reentry_depth.clone(),
                );
                execute_rust_bash(
                    &mut bash,
                    &commands,
                    Some(output_callback),
                    Some(&callback_error),
                    Some(&cancelled),
                    Some(&callback_requested_cancel),
                )
                .await
            },
            |env, val| unsafe {
                <ExecResult as napi::bindgen_prelude::ToNapiValue>::to_napi_value(env, val)
            },
        )?;
        Ok(napi::bindgen_prelude::PromiseRaw::new(raw_env, promise))
    }

    /// Cancel the currently running execution.
    #[napi]
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::SeqCst);
    }

    /// Clear the cancellation flag so subsequent executions proceed normally.
    ///
    /// Call this after a `cancel()` once the in-flight execution has finished
    /// and you want to reuse the same `BashTool` instance without discarding
    /// shell or VFS state.
    #[napi]
    pub fn clear_cancel(&self) {
        self.state.cancelled.store(false, Ordering::SeqCst);
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            *bash = build_bash_from_state(&s, None);
            Ok(())
        })
    }

    // ========================================================================
    // Snapshot / Resume
    // ========================================================================

    /// Serialize interpreter state (shell variables, VFS contents, counters) to bytes.
    #[napi]
    pub fn snapshot(
        &self,
        options: Option<SnapshotOptions>,
    ) -> napi::Result<napi::bindgen_prelude::Buffer> {
        let options = to_snapshot_options(options);
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let bytes = bash
                .snapshot_with_options(options)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(napi::bindgen_prelude::Buffer::from(bytes))
        })
    }

    /// Restore interpreter state from a snapshot previously created with `snapshot()`.
    #[napi]
    pub fn restore_snapshot(&self, data: napi::bindgen_prelude::Buffer) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            bash.restore_snapshot(&data)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a new BashTool instance from a snapshot.
    ///
    /// Accepts optional `BashOptions` so restored instances preserve caller-provided
    /// execution limits and identity settings.
    #[napi(factory)]
    pub fn from_snapshot(
        data: napi::bindgen_prelude::Buffer,
        options: Option<BashOptions>,
    ) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);
        let mut state = shared_state_from_opts(opts, None)?;

        state
            .inner
            .get_mut()
            .restore_snapshot(&data)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(Self {
            state: Arc::new(state),
        })
    }

    /// Get tool name.
    #[napi(getter)]
    pub fn name(&self) -> &str {
        "bashkit"
    }

    /// Get short description.
    #[napi(getter)]
    pub fn short_description(&self) -> &str {
        "Run bash commands in an isolated virtual filesystem"
    }

    /// Get token-efficient tool description.
    #[napi]
    pub fn description(&self) -> String {
        Self::build_rust_tool(&self.state).description().to_string()
    }

    /// Get help as a Markdown document.
    #[napi]
    pub fn help(&self) -> String {
        Self::build_rust_tool(&self.state).help()
    }

    /// Get compact system-prompt text for orchestration.
    #[napi]
    pub fn system_prompt(&self) -> String {
        Self::build_rust_tool(&self.state).system_prompt()
    }

    /// Get JSON input schema as string.
    #[napi]
    pub fn input_schema(&self) -> napi::Result<String> {
        let schema = Self::build_rust_tool(&self.state).input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get JSON output schema as string.
    #[napi]
    pub fn output_schema(&self) -> napi::Result<String> {
        let schema = Self::build_rust_tool(&self.state).output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get tool version.
    #[napi(getter)]
    pub fn version(&self) -> &str {
        VERSION
    }

    // ========================================================================
    // VFS — direct filesystem access (no shell command composition)
    // ========================================================================

    /// Get metadata for a path in the virtual filesystem.
    #[napi]
    pub fn stat(&self, path: String) -> napi::Result<FileMetadata> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let meta = bash
                .fs()
                .stat(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(metadata_to_js(&meta))
        })
    }

    /// Append content to a file in the virtual filesystem.
    #[napi]
    pub fn append_file(&self, path: String, content: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .append_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Change file permissions in the virtual filesystem.
    #[napi]
    pub fn chmod(&self, path: String, mode: u32) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .chmod(Path::new(&path), mode)
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a symbolic link in the virtual filesystem.
    #[napi]
    pub fn symlink(&self, target: String, link: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .symlink(Path::new(&target), Path::new(&link))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Read the target of a symbolic link.
    #[napi]
    pub fn read_link(&self, path: String) -> napi::Result<String> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let target = bash
                .fs()
                .read_link(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(target.display().to_string())
        })
    }

    /// Read a file from the virtual filesystem. Returns contents as a UTF-8 string.
    #[napi]
    pub fn read_file(&self, path: String) -> napi::Result<String> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let bytes = bash
                .fs()
                .read_file(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            String::from_utf8(bytes)
                .map_err(|e| napi::Error::from_reason(format!("Invalid UTF-8: {e}")))
        })
    }

    /// Write a string to a file in the virtual filesystem.
    #[napi]
    pub fn write_file(&self, path: String, content: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .write_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a directory. If recursive is true, creates parent directories as needed.
    #[napi]
    pub fn mkdir(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .mkdir(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Check if a path exists in the virtual filesystem.
    #[napi]
    pub fn exists(&self, path: String) -> napi::Result<bool> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .exists(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Remove a file or directory. If recursive is true, removes directory contents.
    #[napi]
    pub fn remove(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .remove(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// List entries in a directory with metadata (name, file type, size, etc.).
    #[napi]
    pub fn read_dir(&self, path: String) -> napi::Result<Vec<JsDirEntry>> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let entries = bash
                .fs()
                .read_dir(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(entries
                .into_iter()
                .map(|e| JsDirEntry {
                    name: e.name.clone(),
                    metadata: metadata_to_js(&e.metadata),
                })
                .collect())
        })
    }

    // ========================================================================
    // Mount — real filesystem mounts at runtime
    // ========================================================================

    /// Mount a host directory into the VFS at runtime.
    ///
    /// Read-only by default; pass `writable: true` to enable writes.
    ///
    /// **Security**: Writable mounts log a warning. Consider using
    /// `allowedMountPaths` in `BashOptions` to restrict which host paths
    /// may be mounted.
    #[napi]
    pub fn mount(
        &self,
        host_path: String,
        vfs_path: String,
        writable: Option<bool>,
    ) -> napi::Result<()> {
        let is_writable = writable.unwrap_or(false);
        if is_writable {
            eprintln!(
                "bashkit: warning: writable mount at {} — scripts can modify host files",
                host_path
            );
        }
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let mode = if is_writable {
                RealFsMode::ReadWrite
            } else {
                RealFsMode::ReadOnly
            };
            let real_backend = RealFs::new(&host_path, mode)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let fs: Arc<dyn BashFileSystem> = Arc::new(PosixFs::new(real_backend));
            bash.mount(Path::new(&vfs_path), fs)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Mount a filesystem handle without rebuilding the interpreter.
    #[napi]
    pub fn mount_file_system(&self, vfs_path: String, fs: Unknown<'_>) -> napi::Result<()> {
        let mounted_fs = import_external_file_system(fs)?;
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.mount(Path::new(&vfs_path), mounted_fs)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Unmount a previously mounted filesystem.
    #[napi]
    pub fn unmount(&self, vfs_path: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.unmount(Path::new(&vfs_path))
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Get a `JsFileSystem` handle for direct VFS operations.
    #[napi]
    pub fn fs(&self) -> napi::Result<JsFileSystem> {
        reject_on_output_reentry(&self.state)?;
        Ok(JsFileSystem::from_live(self.state.clone()))
    }
}

// ============================================================================
// ScriptedTool — multi-tool orchestration via bash scripts
// ============================================================================

/// Options for creating a ScriptedTool instance.
#[napi(object)]
pub struct ScriptedToolOptions {
    pub name: String,
    pub short_description: Option<String>,
    pub max_commands: Option<u32>,
    pub max_loop_iterations: Option<u32>,
}

/// Threadsafe callback: data=(String,), return=String, CalleeHandled=false.
/// The tuple matches the JS function signature `(request: string) => string`.
type ToolTsfn = napi::threadsafe_function::ThreadsafeFunction<
    (String,),
    String,
    (String,),
    napi::Status,
    false,
    true,
>;

/// Entry for a registered JS tool callback.
///
/// Stores a threadsafe function that receives a JSON-serialized request string
/// `{"params": {...}, "stdin": "..." | null}` and returns a string result.
struct JsToolEntry {
    name: String,
    description: String,
    schema: serde_json::Value,
    /// Wrapped in Arc so we can share references with Rust ScriptedTool callbacks
    /// (ThreadsafeFunction doesn't implement Clone).
    callback: Arc<ToolTsfn>,
}

/// Compose JS callbacks as bash builtins for multi-tool orchestration.
///
/// Each registered tool becomes a bash builtin command. An LLM (or user) writes
/// a single bash script that pipes, loops, and branches across all tools.
#[napi]
pub struct ScriptedTool {
    name: String,
    short_desc: Option<String>,
    tools: Vec<JsToolEntry>,
    env_vars: Vec<(String, String)>,
    rt: Mutex<tokio::runtime::Runtime>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
}

#[napi]
impl ScriptedTool {
    #[napi(constructor)]
    pub fn new(options: ScriptedToolOptions) -> napi::Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            name: options.name,
            short_desc: options.short_description,
            tools: Vec::new(),
            env_vars: Vec::new(),
            rt: Mutex::new(rt),
            max_commands: options.max_commands,
            max_loop_iterations: options.max_loop_iterations,
        })
    }

    /// Register a tool command.
    ///
    /// The callback receives a JSON string `{"params": {...}, "stdin": "..." | null}`
    /// and must return a string result.
    #[napi(
        ts_args_type = "name: string, description: string, callback: (request: string) => string, schema?: string"
    )]
    pub fn add_tool(
        &mut self,
        name: String,
        description: String,
        callback: napi::bindgen_prelude::Function<(String,), String>,
        schema: Option<String>,
    ) -> napi::Result<()> {
        let tsfn: ToolTsfn = callback
            .build_threadsafe_function::<(String,)>()
            .weak::<true>()
            .build()?;

        let schema_val = match schema {
            Some(s) => serde_json::from_str(&s)
                .map_err(|e| napi::Error::from_reason(format!("Invalid schema JSON: {e}")))?,
            None => serde_json::Value::Object(Default::default()),
        };

        self.tools.push(JsToolEntry {
            name,
            description,
            schema: schema_val,
            callback: Arc::new(tsfn),
        });
        Ok(())
    }

    /// Add an environment variable visible inside scripts.
    #[napi]
    pub fn env(&mut self, key: String, value: String) {
        self.env_vars.push((key, value));
    }

    /// Execute a bash script synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        let tool = self.build_rust_tool();
        let rt_guard = self.rt.blocking_lock();
        let resp = rt_guard.block_on(async move {
            tool.execute(ToolRequest {
                commands,
                timeout_ms: None,
            })
            .await
        });
        Ok(ExecResult {
            stdout: resp.stdout,
            stderr: resp.stderr,
            exit_code: resp.exit_code,
            error: resp.error,
            stdout_truncated: resp.stdout_truncated,
            stderr_truncated: resp.stderr_truncated,
            final_env: resp.final_env,
            success: resp.exit_code == 0,
        })
    }

    /// Execute a bash script asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        let tool = self.build_rust_tool();
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
            success: resp.exit_code == 0,
        })
    }

    /// Get tool name.
    #[napi(getter)]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get short description.
    #[napi(getter)]
    pub fn short_description(&self) -> String {
        self.short_desc
            .clone()
            .unwrap_or_else(|| format!("ScriptedTool: {}", self.name))
    }

    /// Number of registered tools.
    #[napi]
    pub fn tool_count(&self) -> u32 {
        self.tools.len() as u32
    }

    /// Get token-efficient tool description.
    #[napi]
    pub fn description(&self) -> String {
        self.build_rust_tool().description().to_string()
    }

    /// Get help as a Markdown document.
    #[napi]
    pub fn help(&self) -> String {
        self.build_rust_tool().help()
    }

    /// Get compact system-prompt text for orchestration.
    #[napi]
    pub fn system_prompt(&self) -> String {
        self.build_rust_tool().system_prompt()
    }

    /// Get JSON input schema as string.
    #[napi]
    pub fn input_schema(&self) -> napi::Result<String> {
        let tool = self.build_rust_tool();
        let schema = tool.input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get JSON output schema as string.
    #[napi]
    pub fn output_schema(&self) -> napi::Result<String> {
        let tool = self.build_rust_tool();
        let schema = tool.output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get tool version.
    #[napi(getter)]
    pub fn version(&self) -> &str {
        VERSION
    }
}

impl ScriptedTool {
    fn build_rust_tool(&self) -> RustScriptedTool {
        let mut builder = RustScriptedTool::builder(&self.name);

        if let Some(ref desc) = self.short_desc {
            builder = builder.short_description(desc);
        }

        for entry in &self.tools {
            let tsfn = entry.callback.clone();
            let tool_name = entry.name.clone();

            let callback = move |args: &ToolArgs| -> Result<String, String> {
                // Serialize params + stdin as JSON for the JS callback
                let request = serde_json::json!({
                    "params": args.params,
                    "stdin": args.stdin.as_deref(),
                });
                let request_str = serde_json::to_string(&request).map_err(|e| e.to_string())?;

                // Dispatch the TSFN call on the shared callback runtime with a
                // concurrency semaphore to prevent unbounded thread/task creation
                // (see issue #982).
                let tsfn_clone = tsfn.clone();
                let tool_name_clone = tool_name.clone();
                let rt = callback_runtime();
                let sem = callback_semaphore();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    let result = match sem.acquire().await {
                        Ok(_permit) => tsfn_clone
                            .call_async((request_str,))
                            .await
                            .map_err(|e| format!("{}: {}", tool_name_clone, e)),
                        Err(e) => Err(format!("{}: semaphore error: {}", tool_name_clone, e)),
                    };
                    let _ = tx.send(result);
                });
                rx.recv()
                    .map_err(|_| format!("{}: callback channel closed", tool_name))?
            };

            builder = builder.tool_fn(
                ToolDef::new(&entry.name, &entry.description).with_schema(entry.schema.clone()),
                callback,
            );
        }

        for (k, v) in &self.env_vars {
            builder = builder.env(k, v);
        }

        if self.max_commands.is_some() || self.max_loop_iterations.is_some() {
            let mut limits = ExecutionLimits::new();
            if let Some(mc) = self.max_commands {
                limits = limits.max_commands(mc as usize);
            }
            if let Some(mli) = self.max_loop_iterations {
                limits = limits.max_loop_iterations(mli as usize);
            }
            builder = builder.limits(limits);
        }

        builder.build()
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Build `ExecutionLimits` from the limit fields stored in `SharedState`.
fn build_limits(state: &SharedState) -> ExecutionLimits {
    let mut limits = ExecutionLimits::new();
    if let Some(v) = state.max_commands {
        limits = limits.max_commands(v as usize);
    }
    if let Some(v) = state.max_loop_iterations {
        limits = limits.max_loop_iterations(v as usize);
    }
    if let Some(v) = state.max_total_loop_iterations {
        limits = limits.max_total_loop_iterations(v as usize);
    }
    if let Some(v) = state.max_function_depth {
        limits = limits.max_function_depth(v as usize);
    }
    if let Some(v) = state.timeout_ms {
        limits = limits.timeout(std::time::Duration::from_millis(v as u64));
    }
    if let Some(v) = state.parser_timeout_ms {
        limits = limits.parser_timeout(std::time::Duration::from_millis(v as u64));
    }
    if let Some(v) = state.max_input_bytes {
        limits = limits.max_input_bytes(v as usize);
    }
    if let Some(v) = state.max_ast_depth {
        limits = limits.max_ast_depth(v as usize);
    }
    if let Some(v) = state.max_parser_operations {
        limits = limits.max_parser_operations(v as usize);
    }
    if let Some(v) = state.max_stdout_bytes {
        limits = limits.max_stdout_bytes(v as usize);
    }
    if let Some(v) = state.max_stderr_bytes {
        limits = limits.max_stderr_bytes(v as usize);
    }
    if let Some(v) = state.capture_final_env {
        limits = limits.capture_final_env(v);
    }
    limits
}

fn build_bash_from_state(state: &SharedState, files: Option<&HashMap<String, String>>) -> RustBash {
    let mut builder = RustBash::builder();

    if let Some(ref u) = state.username {
        builder = builder.username(u);
    }
    if let Some(ref h) = state.hostname {
        builder = builder.hostname(h);
    }

    builder = builder.limits(build_limits(state));

    if let Some(max_mem) = state.max_memory {
        builder = builder.max_memory(max_mem as usize);
    }

    // Mount files into the virtual filesystem
    if let Some(files) = files {
        for (path, content) in files {
            builder = builder.mount_text(path, content);
        }
    }

    // Apply real filesystem mounts
    if let Some(ref mounts) = state.mounts {
        for m in mounts {
            let writable = m.writable.unwrap_or(false);
            builder = match (writable, &m.vfs_path) {
                (false, None) => builder.mount_real_readonly(&m.host_path),
                (false, Some(vfs)) => builder.mount_real_readonly_at(&m.host_path, vfs),
                (true, None) => builder.mount_real_readwrite(&m.host_path),
                (true, Some(vfs)) => builder.mount_real_readwrite_at(&m.host_path, vfs),
            };
        }
    }

    // Enable Python/Monty
    if state.python {
        if let Some(ref handler) = state.external_handler {
            let h = handler.clone();
            let fn_names = state.external_functions.to_vec();
            let python_handler: PythonExternalFnHandler = Arc::new(move |name, args, kwargs| {
                let h = h.clone();
                Box::pin(async move { h(name, args, kwargs).await })
            });
            builder = builder.python_with_external_handler(
                PythonLimits::default(),
                fn_names,
                python_handler,
            );
        } else {
            builder = builder.python();
        }
    }

    builder.build()
}

/// Build a `SharedState` from `BashOptions`, wiring up all config + interpreter.
fn shared_state_from_opts(
    opts: BashOptions,
    external_handler: Option<ExternalHandlerArc>,
) -> napi::Result<SharedState> {
    let py = opts.python.unwrap_or(false);
    let ext_fns = opts.external_functions.clone().unwrap_or_default();
    let mounts = opts.mounts.clone();

    // Build a temporary SharedState to pass to build_bash_from_state
    let tmp = SharedState {
        inner: Mutex::new(RustBash::new()),
        rt: Mutex::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?,
        ),
        cancelled: Arc::new(AtomicBool::new(false)),
        on_output_reentry_depth: Arc::new(AtomicUsize::new(0)),
        username: opts.username.clone(),
        hostname: opts.hostname.clone(),
        max_commands: opts.max_commands,
        max_loop_iterations: opts.max_loop_iterations,
        max_total_loop_iterations: opts.max_total_loop_iterations,
        max_function_depth: opts.max_function_depth,
        timeout_ms: opts.timeout_ms,
        parser_timeout_ms: opts.parser_timeout_ms,
        max_input_bytes: opts.max_input_bytes,
        max_ast_depth: opts.max_ast_depth,
        max_parser_operations: opts.max_parser_operations,
        max_stdout_bytes: opts.max_stdout_bytes,
        max_stderr_bytes: opts.max_stderr_bytes,
        max_memory: opts.max_memory,
        capture_final_env: opts.capture_final_env,
        mounts: mounts.clone(),
        python: py,
        external_functions: ext_fns.clone(),
        external_handler: external_handler.clone(),
    };

    let bash = build_bash_from_state(&tmp, opts.files.as_ref());
    let cancelled = bash.cancellation_token();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;

    Ok(SharedState {
        inner: Mutex::new(bash),
        rt: Mutex::new(rt),
        cancelled,
        on_output_reentry_depth: tmp.on_output_reentry_depth,
        username: opts.username,
        hostname: opts.hostname,
        max_commands: opts.max_commands,
        max_loop_iterations: opts.max_loop_iterations,
        max_total_loop_iterations: opts.max_total_loop_iterations,
        max_function_depth: opts.max_function_depth,
        timeout_ms: opts.timeout_ms,
        parser_timeout_ms: opts.parser_timeout_ms,
        max_input_bytes: opts.max_input_bytes,
        max_ast_depth: opts.max_ast_depth,
        max_parser_operations: opts.max_parser_operations,
        max_stdout_bytes: opts.max_stdout_bytes,
        max_stderr_bytes: opts.max_stderr_bytes,
        max_memory: opts.max_memory,
        capture_final_env: opts.capture_final_env,
        mounts,
        python: py,
        external_functions: ext_fns,
        external_handler,
    })
}

/// Get the bashkit version string.
#[napi]
pub fn get_version() -> &'static str {
    VERSION
}
