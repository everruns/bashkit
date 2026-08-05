//! Host-backed filesystem bridge for the wasm bindings.
//!
//! Important decisions (see `knowledge/runtimes/browser-package.md`):
//!
//! - **The host owns the bytes.** `new Bash({ fs })` replaces the in-memory VFS
//!   with a JS object the embedder supplies, so the interpreter reads and writes
//!   through the embedder's store (a Durable Object, IndexedDB, an OPFS handle)
//!   instead of a copy. No seeding pass, no write-back diff, no size ceiling
//!   beyond the host's own.
//! - **Async only.** Every host call may return a `Promise`, so a script that
//!   touches the filesystem suspends. `executeSync` cannot await and reports the
//!   suspension; embedders with a host fs use `execute()`.
//! - **`Send` bridging.** `js_sys` values and `JsFuture` are `!Send` while
//!   `FsBackend` is `Send + Sync`. Same treatment as `JsBuiltin`: build and call
//!   inside a synchronous scope, then cross the await point through a
//!   `SendWrapper`, which is sound on single-threaded wasm.
//! - **`FsBackend`, not `FileSystem`.** The host implements raw storage; POSIX
//!   semantics (parent checks, type checks, symlink resolution) stay in
//!   `PosixFs` so every host gets them for free and hosts stay small.
//! - **Optional methods degrade, they do not fail silently.** `append`, `copy`,
//!   `rename`, and `chmod` are synthesized from the required primitives when the
//!   host omits them; `symlink` / `readLink` report `Unsupported` because they
//!   cannot be faked.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use bashkit::{DirEntry, FileType, FsBackend, Metadata, Result as BashkitResult, async_trait};
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;
// `Metadata` timestamps are `web_time::SystemTime` on wasm32 (bashkit's
// `time_compat` alias, which is crate-private). Depend on the same crate at the
// same workspace pin so the types unify.
use web_time::{SystemTime, UNIX_EPOCH};

/// Methods a host filesystem must provide. Missing ones make construction fail
/// loudly instead of surfacing as mid-script errors.
const REQUIRED: &[&str] = &[
    "read", "write", "mkdir", "remove", "stat", "readDir", "exists",
];

/// A `FsBackend` that forwards every operation to a JS object.
pub struct HostFs {
    obj: SendWrapper<js_sys::Object>,
}

impl HostFs {
    /// Validate and adopt a JS host filesystem.
    pub fn new(value: JsValue) -> Result<Self, JsError> {
        if !value.is_object() {
            return Err(JsError::new("fs must be an object"));
        }
        let obj = js_sys::Object::from(value);
        for name in REQUIRED {
            if method(&obj, name).is_none() {
                return Err(JsError::new(&format!(
                    "fs is missing the required method '{name}'"
                )));
            }
        }
        Ok(Self {
            obj: SendWrapper::new(obj),
        })
    }

    fn has(&self, name: &str) -> bool {
        method(&self.obj, name).is_some()
    }

    /// Invoke a host method with already-`Send` arguments.
    ///
    /// Arguments are converted to `JsValue` inside this synchronous scope so no
    /// `!Send` value is ever live across the await in [`HostFs::call`].
    fn invoke(&self, name: &str, args: &[Arg]) -> Result<Invoked, JsValue> {
        let func = method(&self.obj, name)
            .ok_or_else(|| JsValue::from(JsError::new(&format!("fs.{name} is not a function"))))?;
        let js_args = js_sys::Array::new();
        for arg in args {
            js_args.push(&arg.to_js());
        }
        let ret = js_sys::Reflect::apply(&func, &self.obj, &js_args)?;
        match ret.dyn_into::<js_sys::Promise>() {
            Ok(promise) => Ok(Invoked::Pending(SendWrapper::new(
                wasm_bindgen_futures::JsFuture::from(promise),
            ))),
            Err(value) => Ok(Invoked::Done(SendWrapper::new(value))),
        }
    }

    /// Call a host method, awaiting the result when it returns a `Promise`.
    async fn call(&self, name: &str, args: &[Arg]) -> BashkitResult<SendWrapper<JsValue>> {
        let invoked = self.invoke(name, args).map_err(|e| host_error(name, &e))?;
        match invoked {
            Invoked::Done(value) => Ok(value),
            Invoked::Pending(future) => match future.await {
                Ok(value) => Ok(SendWrapper::new(value)),
                Err(e) => Err(host_error(name, &e)),
            },
        }
    }

    async fn call_unit(&self, name: &str, args: &[Arg]) -> BashkitResult<()> {
        self.call(name, args).await.map(|_| ())
    }
}

enum Invoked {
    Done(SendWrapper<JsValue>),
    Pending(SendWrapper<wasm_bindgen_futures::JsFuture>),
}

/// A host-call argument. `Send` by construction so it can live in the async
/// method bodies; converted to `JsValue` only inside [`HostFs::invoke`].
enum Arg {
    Str(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Num(f64),
}

impl Arg {
    fn path(path: &Path) -> Self {
        Arg::Str(path.to_string_lossy().into_owned())
    }

    fn to_js(&self) -> JsValue {
        match self {
            Arg::Str(s) => JsValue::from_str(s),
            Arg::Bytes(b) => js_sys::Uint8Array::from(b.as_slice()).into(),
            Arg::Bool(b) => JsValue::from_bool(*b),
            Arg::Num(n) => JsValue::from_f64(*n),
        }
    }
}

#[async_trait]
impl FsBackend for HostFs {
    async fn read(&self, path: &Path) -> BashkitResult<Vec<u8>> {
        let value = self.call("read", &[Arg::path(path)]).await?;
        to_bytes(&value)
            .ok_or_else(|| invalid("fs.read must resolve to a Uint8Array, ArrayBuffer, or string"))
    }

    async fn write(&self, path: &Path, content: &[u8]) -> BashkitResult<()> {
        self.call_unit("write", &[Arg::path(path), Arg::Bytes(content.to_vec())])
            .await
    }

    async fn append(&self, path: &Path, content: &[u8]) -> BashkitResult<()> {
        if self.has("append") {
            return self
                .call_unit("append", &[Arg::path(path), Arg::Bytes(content.to_vec())])
                .await;
        }
        // Synthesized: read-modify-write. A host that cares about concurrent
        // appends implements `append` itself.
        let mut merged = match self.read(path).await {
            Ok(existing) => existing,
            Err(e) if is_not_found(&e) => Vec::new(),
            Err(e) => return Err(e),
        };
        merged.extend_from_slice(content);
        self.write(path, &merged).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> BashkitResult<()> {
        self.call_unit("mkdir", &[Arg::path(path), Arg::Bool(recursive)])
            .await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> BashkitResult<()> {
        self.call_unit("remove", &[Arg::path(path), Arg::Bool(recursive)])
            .await
    }

    async fn stat(&self, path: &Path) -> BashkitResult<Metadata> {
        let value = self.call("stat", &[Arg::path(path)]).await?;
        parse_metadata(&value).ok_or_else(|| invalid("fs.stat must resolve to a stat object"))
    }

    async fn read_dir(&self, path: &Path) -> BashkitResult<Vec<DirEntry>> {
        let value = self.call("readDir", &[Arg::path(path)]).await?;
        parse_dir_entries(&value)
            .ok_or_else(|| invalid("fs.readDir must resolve to an array of dirent objects"))
    }

    async fn exists(&self, path: &Path) -> BashkitResult<bool> {
        let value = self.call("exists", &[Arg::path(path)]).await?;
        Ok(value.as_bool().unwrap_or(false))
    }

    async fn rename(&self, from: &Path, to: &Path) -> BashkitResult<()> {
        if self.has("rename") {
            return self
                .call_unit("rename", &[Arg::path(from), Arg::path(to)])
                .await;
        }
        self.copy(from, to).await?;
        self.remove(from, false).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> BashkitResult<()> {
        if self.has("copy") {
            return self
                .call_unit("copy", &[Arg::path(from), Arg::path(to)])
                .await;
        }
        let content = self.read(from).await?;
        self.write(to, &content).await
    }

    async fn symlink(&self, target: &Path, link: &Path) -> BashkitResult<()> {
        if self.has("symlink") {
            return self
                .call_unit("symlink", &[Arg::path(target), Arg::path(link)])
                .await;
        }
        Err(unsupported("symlink"))
    }

    async fn read_link(&self, path: &Path) -> BashkitResult<PathBuf> {
        if !self.has("readLink") {
            return Err(unsupported("readLink"));
        }
        let value = self.call("readLink", &[Arg::path(path)]).await?;
        value
            .as_string()
            .map(PathBuf::from)
            .ok_or_else(|| invalid("fs.readLink must resolve to a string"))
    }

    async fn chmod(&self, path: &Path, mode: u32) -> BashkitResult<()> {
        if !self.has("chmod") {
            // Hosts without a permission model accept the call and keep the
            // mode they already report from `stat`. Failing here would break
            // ordinary scripts (`chmod +x build.sh`) for no benefit.
            return Ok(());
        }
        self.call_unit("chmod", &[Arg::path(path), Arg::Num(f64::from(mode))])
            .await
    }
}

// ---------------------------------------------------------------------------
// JS <-> Rust conversions
// ---------------------------------------------------------------------------

fn method(obj: &js_sys::Object, name: &str) -> Option<js_sys::Function> {
    js_sys::Reflect::get(obj, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<js_sys::Function>().ok())
}

fn to_bytes(value: &JsValue) -> Option<Vec<u8>> {
    if let Some(text) = value.as_string() {
        return Some(text.into_bytes());
    }
    if let Some(array) = value.dyn_ref::<js_sys::Uint8Array>() {
        return Some(array.to_vec());
    }
    if let Some(buffer) = value.dyn_ref::<js_sys::ArrayBuffer>() {
        return Some(js_sys::Uint8Array::new(buffer).to_vec());
    }
    None
}

fn get(value: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(value, &JsValue::from_str(key)).ok()
}

fn file_type(value: &JsValue) -> Option<FileType> {
    match get(value, "type")?.as_string()?.as_str() {
        "file" => Some(FileType::File),
        "dir" | "directory" => Some(FileType::Directory),
        "symlink" => Some(FileType::Symlink),
        "fifo" => Some(FileType::Fifo),
        _ => None,
    }
}

fn parse_metadata(value: &JsValue) -> Option<Metadata> {
    if !value.is_object() {
        return None;
    }
    let file_type = file_type(value)?;
    let default_mode = if file_type.is_dir() { 0o755 } else { 0o644 };
    Some(Metadata {
        file_type,
        size: get(value, "size").and_then(|v| v.as_f64()).unwrap_or(0.0) as u64,
        mode: get(value, "mode")
            .and_then(|v| v.as_f64())
            .map_or(default_mode, |m| m as u32),
        modified: get(value, "mtimeMs")
            .and_then(|v| v.as_f64())
            .map_or_else(SystemTime::now, from_epoch_ms),
        created: SystemTime::now(),
    })
}

fn from_epoch_ms(ms: f64) -> SystemTime {
    let ms = if ms.is_finite() && ms > 0.0 { ms } else { 0.0 };
    UNIX_EPOCH + std::time::Duration::from_millis(ms as u64)
}

fn parse_dir_entries(value: &JsValue) -> Option<Vec<DirEntry>> {
    let array: &js_sys::Array = value.dyn_ref::<js_sys::Array>()?;
    let mut entries = Vec::with_capacity(array.length() as usize);
    for item in array.iter() {
        let name = get(&item, "name")?.as_string()?;
        entries.push(DirEntry {
            name,
            metadata: parse_metadata(&item)?,
        });
    }
    Some(entries)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Map a thrown/rejected host error onto an `io::ErrorKind` so builtins print
/// the message bash users expect ("No such file or directory", not "host
/// error"). Hosts opt in by setting `error.code`; anything else is `Other`.
fn host_error(method: &str, value: &JsValue) -> bashkit::Error {
    let code = get(value, "code").and_then(|v| v.as_string());
    let message = error_message(value).unwrap_or_else(|| format!("fs.{method} failed"));
    let kind = match code.as_deref() {
        Some("ENOENT") => ErrorKind::NotFound,
        Some("EEXIST") => ErrorKind::AlreadyExists,
        Some("EACCES") | Some("EPERM") => ErrorKind::PermissionDenied,
        Some("EISDIR") => ErrorKind::IsADirectory,
        Some("ENOTDIR") => ErrorKind::NotADirectory,
        Some("ENOTEMPTY") => ErrorKind::DirectoryNotEmpty,
        Some("EXDEV") => ErrorKind::CrossesDevices,
        Some("ENOSYS") => ErrorKind::Unsupported,
        _ => ErrorKind::Other,
    };
    std::io::Error::new(kind, message).into()
}

fn error_message(value: &JsValue) -> Option<String> {
    if let Some(text) = value.as_string() {
        return Some(text);
    }
    get(value, "message")?.as_string()
}

fn invalid(message: &str) -> bashkit::Error {
    std::io::Error::new(ErrorKind::InvalidData, message.to_string()).into()
}

fn unsupported(method: &str) -> bashkit::Error {
    std::io::Error::new(
        ErrorKind::Unsupported,
        format!("the host filesystem does not implement {method}"),
    )
    .into()
}

fn is_not_found(error: &bashkit::Error) -> bool {
    matches!(error, bashkit::Error::Io(e) if e.kind() == ErrorKind::NotFound)
}
