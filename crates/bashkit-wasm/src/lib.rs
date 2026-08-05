//! Browser (WebAssembly) bindings for the Bashkit sandboxed bash interpreter.
//!
//! Important decisions (see also the crate-level notes in `Cargo.toml` and
//! `knowledge/runtimes/browser-package.md`):
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bashkit::{
    Bash as CoreBash, Builtin, BuiltinContext, CheckoutPolicy, CommitOptions,
    ExecResult as CoreExecResult, FileSystem as FileSystemTrait, ObjectId, OutputCallback,
    async_trait,
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
    #[wasm_bindgen(js_name = stdoutBytes)]
    pub stdout_bytes: Vec<u8>,
    pub stderr: String,
    #[wasm_bindgen(js_name = stderrBytes)]
    pub stderr_bytes: Vec<u8>,
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
            stdout: r.stdout.to_string(),
            stdout_bytes: r.stdout.as_bytes().to_vec(),
            stderr: r.stderr.to_string(),
            stderr_bytes: r.stderr.as_bytes().to_vec(),
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

    /// Read a file without interpreting its bytes as UTF-8.
    #[wasm_bindgen(js_name = readFileBytes)]
    pub fn read_file_bytes(&self, path: String) -> Result<js_sys::Uint8Array, JsError> {
        let bytes = now(self.inner.read_file(Path::new(&path)))?;
        Ok(js_sys::Uint8Array::from(bytes.as_slice()))
    }

    /// Write a UTF-8 file, replacing any existing content.
    #[wasm_bindgen(js_name = writeFile)]
    pub fn write_file(&self, path: String, content: String) -> Result<(), JsError> {
        now(self.inner.write_file(Path::new(&path), content.as_bytes()))
    }

    /// Write bytes verbatim, replacing any existing content.
    #[wasm_bindgen(js_name = writeFileBytes)]
    pub fn write_file_bytes(
        &self,
        path: String,
        content: js_sys::Uint8Array,
    ) -> Result<(), JsError> {
        now(self.inner.write_file(Path::new(&path), &content.to_vec()))
    }

    /// Append to a file (creating it if absent).
    #[wasm_bindgen(js_name = appendFile)]
    pub fn append_file(&self, path: String, content: String) -> Result<(), JsError> {
        now(self.inner.append_file(Path::new(&path), content.as_bytes()))
    }

    /// Append bytes verbatim (creating the file if absent).
    #[wasm_bindgen(js_name = appendFileBytes)]
    pub fn append_file_bytes(
        &self,
        path: String,
        content: js_sys::Uint8Array,
    ) -> Result<(), JsError> {
        now(self.inner.append_file(Path::new(&path), &content.to_vec()))
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
            stdin: ctx.stdin.map(|stdin| &**stdin),
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
                    format!("{}: {}\n", self.name, sanitize_js_error(&e)),
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
                            format!("{}: {}\n", self.name, sanitize_js_error(&e)),
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

/// THREAT[TM-INF-028]: JS callback failures cross a sandbox boundary. Surface
/// the Error message only, strip absolute/file URL paths, and cap diagnostics.
fn sanitize_js_error(value: &JsValue) -> String {
    let raw = js_to_string(value);
    let mut sanitized = String::with_capacity(raw.len().min(256));
    let mut offset = 0;
    let mut previous = None;
    while offset < raw.len() && sanitized.chars().count() < 256 {
        let rest = &raw[offset..];
        let is_file_url = rest.starts_with("file://");
        let ch = rest.chars().next().expect("offset is a character boundary");
        let is_absolute_path = ch == '/'
            && previous
                .map(|previous: char| !previous.is_alphanumeric() && previous != '_')
                .unwrap_or(true);
        if is_file_url || is_absolute_path {
            sanitized.push_str("<path>");
            offset += rest.find(char::is_whitespace).unwrap_or(rest.len());
            previous = Some('>');
            continue;
        }
        sanitized.push(ch);
        previous = Some(ch);
        offset += ch.len_utf8();
    }
    let sanitized = sanitized.chars().take(256).collect::<String>();
    if sanitized.is_empty() {
        "callback failed".to_string()
    } else {
        sanitized
    }
}

// ---------------------------------------------------------------------------
// Config + Bash
// ---------------------------------------------------------------------------

/// Parsed, `Send`-free construction config. Kept so `reset()` can rebuild an
/// equivalent interpreter.
struct Config {
    profile: bashkit::ExecutionProfileName,
    username: Option<String>,
    hostname: Option<String>,
    cwd: Option<String>,
    env: Vec<(String, String)>,
    max_commands: Option<usize>,
    max_loop_iterations: Option<usize>,
    timeout_ms: Option<u64>,
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
    cancellation: Rc<RefCell<Arc<AtomicBool>>>,
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
        let cancellation = Rc::new(RefCell::new(core.cancellation_token()));
        Ok(Bash {
            inner: Rc::new(RefCell::new(core)),
            config,
            sync_flag,
            cancellation,
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

    /// Execute asynchronously while forwarding incremental output.
    // Same serialization invariant as `execute`: the single-threaded RefCell
    // borrow intentionally spans the future and rejects reentrant execution.
    #[allow(clippy::await_holding_refcell_ref)]
    #[wasm_bindgen(js_name = executeWithOutput)]
    pub fn execute_with_output(
        &self,
        commands: String,
        on_output: js_sys::Function,
    ) -> js_sys::Promise {
        let inner = self.inner.clone();
        let callback = SendWrapper::new(on_output);
        let callback_error = Arc::new(Mutex::new(None::<String>));
        let callback_error_for_output = callback_error.clone();
        future_to_promise(async move {
            let mut guard = inner.try_borrow_mut().map_err(|_| {
                JsError::new("bash instance is busy (reentrant execution is not supported)")
            })?;
            let output_callback: OutputCallback = Box::new(move |stdout, stderr| {
                if callback_error_for_output
                    .lock()
                    .is_ok_and(|slot| slot.is_some())
                {
                    return;
                }
                if let Err(error) = callback.call2(
                    &JsValue::NULL,
                    &JsValue::from_str(stdout),
                    &JsValue::from_str(stderr),
                ) && let Ok(mut slot) = callback_error_for_output.lock()
                {
                    *slot = Some(sanitize_js_error(&error));
                }
            });
            let result = guard.exec_streaming(&commands, output_callback).await;
            if let Some(error) = callback_error.lock().ok().and_then(|mut slot| slot.take()) {
                return Err(JsValue::from(JsError::new(&format!(
                    "output callback failed: {error}"
                ))));
            }
            match result {
                Ok(r) => Ok(JsValue::from(ExecResult::from(r))),
                Err(e) => Err(JsValue::from(JsError::new(&e.to_string()))),
            }
        })
    }

    /// Analyze a script without executing or mutating it.
    pub fn analyze(&self, script: String) -> Result<JsValue, JsError> {
        let analysis = self
            .inner
            .try_borrow()
            .map_err(|_| JsError::new("bash instance is busy"))?
            .analyze(&script)
            .map_err(|error| JsError::new(&error.to_string()))?;
        let commands = analysis
            .commands
            .iter()
            .map(|command| {
                serde_json::json!({
                    "name": command.name,
                    "args": command.args,
                    "context": command.context.as_str(),
                    "assignments": command.assignments,
                    "isAssignmentOnly": command.is_assignment_only(),
                })
            })
            .collect::<Vec<_>>();
        let redirects = analysis
            .redirects
            .iter()
            .map(|redirect| {
                serde_json::json!({
                    "path": redirect.path,
                    "mode": redirect.mode.as_str(),
                    "isWrite": redirect.mode.is_write(),
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "commands": commands,
            "redirects": redirects,
            "functions": analysis.functions,
            "commandNames": analysis.command_names(),
            "hasDynamicCommands": analysis.has_dynamic_commands,
            "hasCommandSubstitution": analysis.has_command_substitution,
            "hasInterpreterReentry": analysis.has_interpreter_reentry,
            "truncated": analysis.truncated,
            "isOpaque": analysis.is_opaque(),
        });
        value
            .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Set the sticky cancellation token checked at command boundaries.
    pub fn cancel(&self) {
        self.cancellation.borrow().store(true, Ordering::SeqCst);
    }

    /// Clear the sticky cancellation token after an execution has stopped.
    #[wasm_bindgen(js_name = clearCancel)]
    pub fn clear_cancel(&self) {
        self.cancellation.borrow().store(false, Ordering::SeqCst);
    }

    /// Reset the interpreter to a fresh state, preserving construction options
    /// and registered custom builtins.
    pub fn reset(&self) -> Result<(), JsError> {
        let core = build_core(&self.config, &self.sync_flag)?;
        *self.cancellation.borrow_mut() = core.cancellation_token();
        *self
            .inner
            .try_borrow_mut()
            .map_err(|_| JsError::new("bash instance is busy"))? = core;
        Ok(())
    }

    /// Capture content-addressed state for persistence and branching.
    pub fn commit(&self, options: JsValue) -> Result<JsValue, JsError> {
        let mut commit_options = CommitOptions::new();
        if !options.is_undefined() && !options.is_null() {
            if !options.is_object() {
                return Err(JsError::new("commit options must be an object"));
            }
            for parent in read_object_ids(&options, "parents")? {
                commit_options = commit_options.parent(parent);
            }
            if let Ok(meta) = js_sys::Reflect::get(&options, &JsValue::from_str("meta"))
                && meta.is_object()
            {
                let meta = js_sys::Object::from(meta);
                for key in js_sys::Object::keys(&meta).iter() {
                    let name = key.as_string().unwrap_or_default();
                    let value = js_sys::Reflect::get(&meta, &key)
                        .ok()
                        .and_then(|value| value.as_string())
                        .ok_or_else(|| JsError::new("commit metadata values must be strings"))?;
                    commit_options = commit_options.meta(name, value);
                }
            }
            let have = read_object_ids(&options, "have")?;
            commit_options = commit_options.have(have.iter());
            if bool_property(&options, "excludeFilesystem") {
                commit_options = commit_options.exclude_filesystem(true);
            }
            if bool_property(&options, "excludeFunctions") {
                commit_options = commit_options.exclude_functions(true);
            }
        }

        let packed = self
            .inner
            .try_borrow()
            .map_err(|_| JsError::new("bash instance is busy"))?
            .commit(commit_options)
            .map_err(|error| JsError::new(&error.to_string()))?;
        packed_commit_to_js(&packed)
    }

    /// Restore a content-addressed commit. Mutation is atomic on failure.
    pub fn checkout(
        &self,
        commit_id: String,
        objects: JsValue,
        policy: Option<String>,
    ) -> Result<(), JsError> {
        let root = parse_object_id(&commit_id)?;
        let store = object_store_from_js(&objects)?;
        let policy = match policy.as_deref().unwrap_or("superset") {
            "strict" => CheckoutPolicy::Strict,
            "superset" => CheckoutPolicy::Superset,
            "force" => CheckoutPolicy::Force,
            _ => {
                return Err(JsError::new(
                    "checkout policy must be strict, superset, or force",
                ));
            }
        };
        self.inner
            .try_borrow_mut()
            .map_err(|_| JsError::new("bash instance is busy"))?
            .checkout(root, &store, policy)
            .map_err(|error| JsError::new(&error.to_string()))
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

    #[wasm_bindgen(js_name = readFileBytes)]
    pub fn read_file_bytes(&self, path: String) -> Result<js_sys::Uint8Array, JsError> {
        self.fs().read_file_bytes(path)
    }

    /// Write a UTF-8 file to the virtual filesystem.
    #[wasm_bindgen(js_name = writeFile)]
    pub fn write_file(&self, path: String, content: String) -> Result<(), JsError> {
        self.fs().write_file(path, content)
    }

    #[wasm_bindgen(js_name = writeFileBytes)]
    pub fn write_file_bytes(
        &self,
        path: String,
        content: js_sys::Uint8Array,
    ) -> Result<(), JsError> {
        self.fs().write_file_bytes(path, content)
    }

    /// Append to a file in the virtual filesystem (creating it if absent).
    #[wasm_bindgen(js_name = appendFile)]
    pub fn append_file(&self, path: String, content: String) -> Result<(), JsError> {
        self.fs().append_file(path, content)
    }

    #[wasm_bindgen(js_name = appendFileBytes)]
    pub fn append_file_bytes(
        &self,
        path: String,
        content: js_sys::Uint8Array,
    ) -> Result<(), JsError> {
        self.fs().append_file_bytes(path, content)
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

fn parse_object_id(id: &str) -> Result<ObjectId, JsError> {
    ObjectId::from_hex(id).map_err(|error| JsError::new(&error.to_string()))
}

fn bool_property(value: &JsValue, name: &str) -> bool {
    js_sys::Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn read_object_ids(value: &JsValue, name: &str) -> Result<Vec<ObjectId>, JsError> {
    let Ok(ids) = js_sys::Reflect::get(value, &JsValue::from_str(name)) else {
        return Ok(Vec::new());
    };
    if ids.is_undefined() || ids.is_null() {
        return Ok(Vec::new());
    }
    if !js_sys::Array::is_array(&ids) {
        return Err(JsError::new(&format!(
            "{name} must be an array of object ids"
        )));
    }
    js_sys::Array::from(&ids)
        .iter()
        .map(|id| {
            id.as_string()
                .ok_or_else(|| JsError::new(&format!("{name} must contain strings")))
                .and_then(|id| parse_object_id(&id))
        })
        .collect()
}

fn object_store_from_js(objects: &JsValue) -> Result<HashMap<ObjectId, Vec<u8>>, JsError> {
    if !objects.is_object() {
        return Err(JsError::new("checkout objects must be an object"));
    }
    let object = js_sys::Object::from(objects.clone());
    let mut store = HashMap::new();
    for key in js_sys::Object::keys(&object).iter() {
        let id = key.as_string().unwrap_or_default();
        let value = js_sys::Reflect::get(&object, &key)
            .map_err(|_| JsError::new("failed to read checkout object"))?;
        if !value.is_instance_of::<js_sys::Uint8Array>() {
            return Err(JsError::new(&format!(
                "checkout object {id} must be a Uint8Array"
            )));
        }
        store.insert(
            parse_object_id(&id)?,
            js_sys::Uint8Array::new(&value).to_vec(),
        );
    }
    Ok(store)
}

fn packed_commit_to_js(packed: &bashkit::PackedCommit) -> Result<JsValue, JsError> {
    let output = js_sys::Object::new();
    let objects = js_sys::Object::new();
    for (id, bytes) in packed.objects() {
        set_property(
            &objects,
            &id.to_hex(),
            &JsValue::from(js_sys::Uint8Array::from(bytes)),
        )?;
    }
    set_property(&output, "id", &JsValue::from_str(&packed.id().to_hex()))?;
    set_property(
        &output,
        "objectCount",
        &JsValue::from_f64(packed.object_count() as f64),
    )?;
    set_property(
        &output,
        "storedBytes",
        &JsValue::from_f64(packed.stored_bytes() as f64),
    )?;
    set_property(
        &output,
        "selfContained",
        &JsValue::from_bool(packed.is_self_contained()),
    )?;
    set_property(&output, "objects", &objects)?;
    if packed.is_self_contained() {
        let bytes = packed
            .to_bytes()
            .map_err(|error| JsError::new(&error.to_string()))?;
        set_property(
            &output,
            "packed",
            &JsValue::from(js_sys::Uint8Array::from(bytes.as_slice())),
        )?;
    }
    Ok(output.into())
}

fn set_property(object: &js_sys::Object, name: &str, value: &JsValue) -> Result<(), JsError> {
    js_sys::Reflect::set(object, &JsValue::from_str(name), value)
        .map(|_| ())
        .map_err(|_| JsError::new(&format!("failed to create {name} property")))
}

fn build_core(config: &Config, sync_flag: &Arc<AtomicBool>) -> Result<CoreBash, JsError> {
    let profile = bashkit::ExecutionProfile::named(config.profile);
    let mut builder = CoreBash::builder().profile(profile.clone());

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
    if config.max_commands.is_some()
        || config.max_loop_iterations.is_some()
        || config.timeout_ms.is_some()
    {
        let mut limits = profile.execution_limits().clone();
        if let Some(n) = config.max_commands {
            limits = limits.max_commands(n);
        }
        if let Some(n) = config.max_loop_iterations {
            limits = limits.max_loop_iterations(n);
        }
        if let Some(ms) = config.timeout_ms {
            limits = limits.timeout(std::time::Duration::from_millis(ms));
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
            profile: bashkit::ExecutionProfileName::Standard,
            username: None,
            hostname: None,
            cwd: None,
            env: Vec::new(),
            max_commands: None,
            max_loop_iterations: None,
            timeout_ms: None,
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

    let profile = match get_str("profile").as_deref() {
        None | Some("standard") => bashkit::ExecutionProfileName::Standard,
        Some("hardened") => bashkit::ExecutionProfileName::Hardened,
        Some("interactive") => bashkit::ExecutionProfileName::Interactive,
        Some(_) => {
            return Err(JsError::new(
                "profile must be 'hardened', 'standard', or 'interactive'",
            ));
        }
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
        profile,
        username: get_str("username"),
        hostname: get_str("hostname"),
        cwd: get_str("cwd"),
        env,
        max_commands: get_usize("maxCommands"),
        max_loop_iterations: get_usize("maxLoopIterations"),
        timeout_ms: get_usize("timeoutMs").map(|value| value as u64),
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
