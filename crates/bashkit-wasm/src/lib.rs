//! Browser (WebAssembly) bindings for the Bashkit sandboxed bash interpreter.
//!
//! Important decisions (see also the crate-level notes in `Cargo.toml` and
//! `specs/browser-package.md`):
//!
//! - **Single-threaded, no cross-origin isolation.** Target is
//!   `wasm32-unknown-unknown`. There is no `SharedArrayBuffer`, no thread pool,
//!   and therefore no COOP/COEP header requirement. The whole future chain runs
//!   on the browser's one event loop.
//! - **Two execution modes.** `executeSync` drives `Bash::exec` to completion in
//!   a single poll (`now_or_never`) — correct for pure bash + jq that never
//!   yields. `execute` returns a `Promise` via `wasm-bindgen-futures`, so async
//!   JS custom builtins (e.g. a GraphQL binary that awaits a `fetch`/Relay
//!   request) can `await` inside the interpreter.
//! - **`Send` bridging.** `js_sys::Function` and `JsFuture` are `!Send`, but the
//!   `Builtin` trait is `Send + Sync`. On single-threaded wasm we wrap them in
//!   `send_wrapper::SendWrapper`, which only ever touches the value on its origin
//!   thread — sound when there is exactly one thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bashkit::{
    Bash as CoreBash, Builtin, BuiltinContext, ExecResult as CoreExecResult, ExecutionLimits,
    FileSystem as FileSystemTrait, async_trait,
};
use futures_util::future::FutureExt;
use send_wrapper::SendWrapper;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

/// Install a panic hook that forwards Rust panics to `console.error` with a
/// readable message and stack, instead of the default unhelpful
/// `RuntimeError: unreachable`.
#[wasm_bindgen(start)]
pub fn __start() {
    console_error_panic_hook::set_once();
}

// ---------------------------------------------------------------------------
// ExecResult
// ---------------------------------------------------------------------------

/// Result of a bash execution, mirrored from the Rust `ExecResult`.
#[wasm_bindgen(getter_with_clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    #[wasm_bindgen(js_name = exitCode)]
    pub exit_code: i32,
    pub success: bool,
    #[wasm_bindgen(js_name = stdoutTruncated)]
    pub stdout_truncated: bool,
    #[wasm_bindgen(js_name = stderrTruncated)]
    pub stderr_truncated: bool,
}

impl From<CoreExecResult> for ExecResult {
    fn from(r: CoreExecResult) -> Self {
        ExecResult {
            success: r.exit_code == 0,
            stdout: r.stdout,
            stderr: r.stderr,
            exit_code: r.exit_code,
            stdout_truncated: r.stdout_truncated,
            stderr_truncated: r.stderr_truncated,
        }
    }
}

// ---------------------------------------------------------------------------
// FileSystem handle
// ---------------------------------------------------------------------------

/// Drive a `FileSystem` future to completion synchronously.
///
/// Sound because the browser VFS (`InMemoryFs`) is backed by synchronous
/// interior mutability, so every op is `Ready` on the first poll. If a future
/// ever suspends (it does not today) we surface an error instead of blocking.
fn now<T>(fut: impl std::future::Future<Output = bashkit::Result<T>>) -> Result<T, JsError> {
    fut.now_or_never()
        .ok_or_else(|| JsError::new("filesystem operation did not complete synchronously"))?
        .map_err(|e| JsError::new(&e.to_string()))
}

/// A live handle to a `Bash` instance's virtual filesystem.
///
/// Reads observe earlier script writes and writes are visible to subsequent
/// commands — it is the same VFS the executing script sees. Exposed both as
/// `bash.fs()` and as `ctx.fs` inside custom-builtin callbacks.
#[wasm_bindgen]
pub struct FileSystem {
    inner: Arc<dyn FileSystemTrait>,
}

impl FileSystem {
    fn new(inner: Arc<dyn FileSystemTrait>) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
impl FileSystem {
    /// Read a file as UTF-8.
    #[wasm_bindgen(js_name = readFile)]
    pub fn read_file(&self, path: String) -> Result<String, JsError> {
        let bytes = now(self.inner.read_file(Path::new(&path)))?;
        String::from_utf8(bytes).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Write a UTF-8 file, replacing any existing content.
    #[wasm_bindgen(js_name = writeFile)]
    pub fn write_file(&self, path: String, content: String) -> Result<(), JsError> {
        now(self.inner.write_file(Path::new(&path), content.as_bytes()))
    }

    /// Append to a file (creating it if absent).
    #[wasm_bindgen(js_name = appendFile)]
    pub fn append_file(&self, path: String, content: String) -> Result<(), JsError> {
        now(self.inner.append_file(Path::new(&path), content.as_bytes()))
    }

    /// Whether a path exists.
    pub fn exists(&self, path: String) -> Result<bool, JsError> {
        now(self.inner.exists(Path::new(&path)))
    }

    /// Create a directory (and parents).
    pub fn mkdir(&self, path: String) -> Result<(), JsError> {
        now(self.inner.mkdir(Path::new(&path), true))
    }

    /// Remove a file or directory (recursively).
    pub fn remove(&self, path: String) -> Result<(), JsError> {
        now(self.inner.remove(Path::new(&path), true))
    }

    /// List entry names in a directory.
    pub fn ls(&self, path: String) -> Result<Vec<String>, JsError> {
        let entries = now(self.inner.read_dir(Path::new(&path)))?;
        Ok(entries.into_iter().map(|e| e.name).collect())
    }
}

// ---------------------------------------------------------------------------
// Custom builtin adapter (async JS callbacks)
// ---------------------------------------------------------------------------

/// Payload handed to a JS custom-builtin callback (minus the live `fs` handle,
/// which is attached separately because it is not serializable). Matches the
/// `BuiltinContext` shape of the napi bindings.
#[derive(Serialize)]
struct BuiltinRequest<'a> {
    name: &'a str,
    argv: &'a [String],
    stdin: Option<&'a str>,
    env: &'a HashMap<String, String>,
    cwd: String,
}

/// Adapts a JS callback into a bash `Builtin`.
///
/// The callback receives one argument (a `BuiltinRequest` object) and returns
/// either a `string` or a `Promise<string>` resolving to the builtin's stdout.
/// Throwing / rejecting becomes stderr with exit code 1.
struct JsBuiltin {
    name: String,
    callback: SendWrapper<js_sys::Function>,
    is_async_function: bool,
    /// Set while an `executeSync` call is in flight. A JS `Promise` can never
    /// settle without yielding to the event loop, which `executeSync` does not
    /// do, so we fail fast instead of returning `Pending` forever.
    in_sync: Arc<AtomicBool>,
}

#[async_trait]
impl Builtin for JsBuiltin {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<CoreExecResult> {
        let request = BuiltinRequest {
            name: &self.name,
            argv: ctx.args,
            stdin: ctx.stdin,
            env: ctx.env,
            cwd: ctx.cwd.to_string_lossy().into_owned(),
        };
        // `json_compatible` serializes `env` (a HashMap) as a plain JS object
        // rather than a `Map`, matching the `Record<string, string>` TS type.
        let serializer = serde_wasm_bindgen::Serializer::json_compatible();
        let arg = match request.serialize(&serializer) {
            Ok(v) => v,
            Err(e) => return Ok(CoreExecResult::err(format!("{}: {}\n", self.name, e), 1)),
        };

        // Attach the live VFS as `ctx.fs` (a `FileSystem` handle over the same
        // Arc the interpreter uses). Not part of the serialized payload because
        // it is a native object, not JSON.
        let fs_handle = JsValue::from(FileSystem::new(ctx.fs.clone()));
        if js_sys::Reflect::set(&arg, &JsValue::from_str("fs"), &fs_handle).is_err() {
            return Ok(CoreExecResult::err(
                format!("{}: failed to attach fs handle\n", self.name),
                1,
            ));
        }

        // Reject declared async functions before invocation in executeSync.
        // Calling them first would start their body and side effects before the
        // returned Promise can be detected.
        if self.in_sync.load(Ordering::SeqCst) && self.is_async_function {
            return Ok(async_builtin_in_sync_error(&self.name));
        }

        // Call the JS function. Everything up to the first `.await` is `!Send`,
        // but stays on this thread; we only cross an await point through the
        // `SendWrapper<JsFuture>` below.
        let ret = match self.callback.call1(&JsValue::NULL, &arg) {
            Ok(v) => v,
            Err(e) => {
                return Ok(CoreExecResult::err(
                    format!("{}: {}\n", self.name, js_to_string(&e)),
                    1,
                ));
            }
        };

        // A sync callback returns its stdout string directly and works in both
        // modes. An async callback returns a Promise; we can only await it from
        // execute(), never from executeSync (which never yields to the event
        // loop) — so fail fast with a clear message in that case.
        let resolved = match ret.dyn_into::<js_sys::Promise>() {
            Ok(promise) => {
                if self.in_sync.load(Ordering::SeqCst) {
                    return Ok(async_builtin_in_sync_error(&self.name));
                }
                match SendWrapper::new(JsFuture::from(promise)).await {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(CoreExecResult::err(
                            format!("{}: {}\n", self.name, js_to_string(&e)),
                            1,
                        ));
                    }
                }
            }
            Err(v) => v,
        };

        Ok(CoreExecResult::ok(resolved.as_string().unwrap_or_default()))
    }
}

fn async_builtin_in_sync_error(name: &str) -> CoreExecResult {
    CoreExecResult::err(
        format!(
            "{}: async custom builtins require execute() (async) in the browser \
             build; executeSync() cannot await a JS Promise\n",
            name
        ),
        1,
    )
}

/// Best-effort string for a `JsValue` error without leaking Rust `Debug` shapes.
fn js_to_string(v: &JsValue) -> String {
    if let Some(s) = v.as_string() {
        return s;
    }
    // Error objects: prefer `.message`.
    if let Some(obj) = v.dyn_ref::<js_sys::Object>()
        && let Ok(msg) = js_sys::Reflect::get(obj, &JsValue::from_str("message"))
        && let Some(s) = msg.as_string()
    {
        return s;
    }
    js_sys::JSON::stringify(v)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_else(|| "unknown error".to_string())
}

// ---------------------------------------------------------------------------
// Config + Bash
// ---------------------------------------------------------------------------

/// Parsed, `Send`-free construction config. Kept so `reset()` can rebuild an
/// equivalent interpreter.
struct Config {
    username: Option<String>,
    hostname: Option<String>,
    cwd: Option<String>,
    env: Vec<(String, String)>,
    max_commands: Option<usize>,
    max_loop_iterations: Option<usize>,
    max_memory: Option<usize>,
    files: Vec<(String, String)>,
    builtins: Vec<CustomBuiltinConfig>,
}

struct CustomBuiltinConfig {
    name: String,
    callback: SendWrapper<js_sys::Function>,
    is_async_function: bool,
}

/// Sandboxed bash interpreter running entirely in the browser.
#[wasm_bindgen]
pub struct Bash {
    inner: Rc<RefCell<CoreBash>>,
    config: Rc<Config>,
    sync_flag: Arc<AtomicBool>,
}

#[wasm_bindgen]
impl Bash {
    /// Create a new interpreter. `options` is an optional plain object; see the
    /// TypeScript `BashOptions` for the supported fields.
    #[wasm_bindgen(constructor)]
    pub fn new(options: JsValue) -> Result<Bash, JsError> {
        let config = Rc::new(parse_options(&options)?);
        let sync_flag = Arc::new(AtomicBool::new(false));
        let core = build_core(&config, &sync_flag)?;
        Ok(Bash {
            inner: Rc::new(RefCell::new(core)),
            config,
            sync_flag,
        })
    }

    /// Execute `commands` synchronously and return the result.
    ///
    /// Only valid for scripts that complete without yielding — i.e. plain bash
    /// and `jq`. If the script invokes an async custom builtin (or otherwise
    /// suspends), this throws directing you to `execute()`.
    #[wasm_bindgen(js_name = executeSync)]
    pub fn execute_sync(&self, commands: String) -> Result<ExecResult, JsError> {
        let mut guard = self.inner.try_borrow_mut().map_err(|_| {
            JsError::new("bash instance is busy (reentrant execution is not supported)")
        })?;

        self.sync_flag.store(true, Ordering::SeqCst);
        let polled = guard.exec(&commands).now_or_never();
        self.sync_flag.store(false, Ordering::SeqCst);

        match polled {
            Some(Ok(r)) => Ok(r.into()),
            Some(Err(e)) => Err(JsError::new(&e.to_string())),
            None => Err(JsError::new(
                "execution did not complete synchronously (an async builtin, sleep, or \
                 background job suspended it); use execute() instead",
            )),
        }
    }

    /// Execute `commands` asynchronously, returning a `Promise<ExecResult>`.
    ///
    /// This is the path that supports async JS custom builtins.
    // The borrow is intentionally held across the await to serialize execution
    // on the single-threaded event loop. `try_borrow_mut` turns a reentrant call
    // (e.g. execute() invoked from inside a builtin callback) into a clean error
    // instead of a panic, so holding it across the await point is sound here.
    #[allow(clippy::await_holding_refcell_ref)]
    pub fn execute(&self, commands: String) -> js_sys::Promise {
        let inner = self.inner.clone();
        future_to_promise(async move {
            let mut guard = inner.try_borrow_mut().map_err(|_| {
                JsError::new("bash instance is busy (reentrant execution is not supported)")
            })?;
            match guard.exec(&commands).await {
                Ok(r) => Ok(JsValue::from(ExecResult::from(r))),
                Err(e) => Err(JsValue::from(JsError::new(&e.to_string()))),
            }
        })
    }

    /// Reset the interpreter to a fresh state, preserving construction options
    /// and registered custom builtins.
    pub fn reset(&self) -> Result<(), JsError> {
        let core = build_core(&self.config, &self.sync_flag)?;
        *self
            .inner
            .try_borrow_mut()
            .map_err(|_| JsError::new("bash instance is busy"))? = core;
        Ok(())
    }

    // --- VFS helpers ------------------------------------------------------

    /// A live handle to the interpreter's virtual filesystem. The same VFS the
    /// executing script sees, and the same object passed as `ctx.fs` to custom
    /// builtins.
    pub fn fs(&self) -> FileSystem {
        FileSystem::new(self.inner.borrow().fs())
    }

    /// Read a file from the virtual filesystem as UTF-8.
    #[wasm_bindgen(js_name = readFile)]
    pub fn read_file(&self, path: String) -> Result<String, JsError> {
        self.fs().read_file(path)
    }

    /// Write a UTF-8 file to the virtual filesystem.
    #[wasm_bindgen(js_name = writeFile)]
    pub fn write_file(&self, path: String, content: String) -> Result<(), JsError> {
        self.fs().write_file(path, content)
    }

    /// Append to a file in the virtual filesystem (creating it if absent).
    #[wasm_bindgen(js_name = appendFile)]
    pub fn append_file(&self, path: String, content: String) -> Result<(), JsError> {
        self.fs().append_file(path, content)
    }

    /// Whether a path exists in the virtual filesystem.
    pub fn exists(&self, path: String) -> Result<bool, JsError> {
        self.fs().exists(path)
    }

    /// Create a directory (recursively) in the virtual filesystem.
    pub fn mkdir(&self, path: String) -> Result<(), JsError> {
        self.fs().mkdir(path)
    }

    /// Remove a file or directory (recursively) from the virtual filesystem.
    pub fn remove(&self, path: String) -> Result<(), JsError> {
        self.fs().remove(path)
    }

    /// List entry names in a directory.
    pub fn ls(&self, path: String) -> Result<Vec<String>, JsError> {
        self.fs().ls(path)
    }
}

fn build_core(config: &Config, sync_flag: &Arc<AtomicBool>) -> Result<CoreBash, JsError> {
    let mut builder = CoreBash::builder();

    if let Some(u) = &config.username {
        builder = builder.username(u.clone());
    }
    if let Some(h) = &config.hostname {
        builder = builder.hostname(h.clone());
    }
    if let Some(c) = &config.cwd {
        builder = builder.cwd(PathBuf::from(c));
    }
    for (k, v) in &config.env {
        builder = builder.env(k.clone(), v.clone());
    }
    if config.max_commands.is_some() || config.max_loop_iterations.is_some() {
        let mut limits = ExecutionLimits::default();
        if let Some(n) = config.max_commands {
            limits = limits.max_commands(n);
        }
        if let Some(n) = config.max_loop_iterations {
            limits = limits.max_loop_iterations(n);
        }
        builder = builder.limits(limits);
    }
    if let Some(bytes) = config.max_memory {
        builder = builder.max_memory(bytes);
    }
    for builtin in &config.builtins {
        builder = builder.builtin(
            builtin.name.clone(),
            Box::new(JsBuiltin {
                name: builtin.name.clone(),
                callback: builtin.callback.clone(),
                is_async_function: builtin.is_async_function,
                in_sync: sync_flag.clone(),
            }),
        );
    }

    let core = builder.build();

    // Seed pre-created files as normal writable VFS entries.
    for (path, content) in &config.files {
        let fs = core.fs();
        if let Some(parent) = Path::new(path).parent() {
            let _ = fs.mkdir(parent, true).now_or_never();
        }
        fs.write_file(Path::new(path), content.as_bytes())
            .now_or_never()
            .ok_or_else(|| JsError::new("seeding files did not complete synchronously"))?
            .map_err(|e| JsError::new(&e.to_string()))?;
    }

    Ok(core)
}

fn parse_options(options: &JsValue) -> Result<Config, JsError> {
    if options.is_undefined() || options.is_null() {
        return Ok(Config {
            username: None,
            hostname: None,
            cwd: None,
            env: Vec::new(),
            max_commands: None,
            max_loop_iterations: None,
            max_memory: None,
            files: Vec::new(),
            builtins: Vec::new(),
        });
    }
    if !options.is_object() {
        return Err(JsError::new("options must be an object"));
    }

    let get_str = |key: &str| -> Option<String> {
        js_sys::Reflect::get(options, &JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_string())
    };
    let get_usize = |key: &str| -> Option<usize> {
        js_sys::Reflect::get(options, &JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_f64())
            .map(|n| n as usize)
    };

    let env = read_string_map(options, "env")?;
    let files = read_string_map(options, "files")?;

    // customBuiltins: { [name]: (ctx) => string | Promise<string> }
    let mut builtins = Vec::new();
    if let Ok(cb) = js_sys::Reflect::get(options, &JsValue::from_str("customBuiltins"))
        && cb.is_object()
    {
        let obj = js_sys::Object::from(cb);
        for key in js_sys::Object::keys(&obj).iter() {
            let name = key.as_string().unwrap_or_default();
            let value = js_sys::Reflect::get(&obj, &key)
                .map_err(|_| JsError::new("failed to read customBuiltins entry"))?;
            let func = value.dyn_into::<js_sys::Function>().map_err(|_| {
                JsError::new(&format!("customBuiltins['{name}'] must be a function"))
            })?;
            let is_async_function = is_async_function(&func);
            builtins.push(CustomBuiltinConfig {
                name,
                callback: SendWrapper::new(func),
                is_async_function,
            });
        }
    }

    Ok(Config {
        username: get_str("username"),
        hostname: get_str("hostname"),
        cwd: get_str("cwd"),
        env,
        max_commands: get_usize("maxCommands"),
        max_loop_iterations: get_usize("maxLoopIterations"),
        max_memory: get_usize("maxMemory"),
        files,
        builtins,
    })
}

fn is_async_function(func: &js_sys::Function) -> bool {
    js_sys::Reflect::get(func, &JsValue::from_str("constructor"))
        .ok()
        .and_then(|ctor| js_sys::Reflect::get(&ctor, &JsValue::from_str("name")).ok())
        .and_then(|name| name.as_string())
        .is_some_and(|name| name == "AsyncFunction")
}

/// Read a `{ [k: string]: string }` object field into a `Vec<(String, String)>`.
fn read_string_map(options: &JsValue, key: &str) -> Result<Vec<(String, String)>, JsError> {
    let mut out = Vec::new();
    if let Ok(v) = js_sys::Reflect::get(options, &JsValue::from_str(key))
        && v.is_object()
    {
        let obj = js_sys::Object::from(v);
        for k in js_sys::Object::keys(&obj).iter() {
            let name = k.as_string().unwrap_or_default();
            let val = js_sys::Reflect::get(&obj, &k)
                .ok()
                .and_then(|x| x.as_string())
                .ok_or_else(|| JsError::new(&format!("{key}['{name}'] must be a string")))?;
            out.push((name, val));
        }
    }
    Ok(out)
}
