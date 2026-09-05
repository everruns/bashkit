// Decision: the public C ABI exposes opaque owned objects and borrowed byte
// views only. Rust layouts, allocators, and async runtime types never cross it.
// ABI v1's deliberately narrow surface is recorded as L-CAPI-001.
// THREAT[TM-INT-008]: every exported operation contains Rust panics at the FFI
// boundary and reports a generic error so unwinding cannot enter the foreign
// caller and the returned error does not include panic details.

use bashkit::{
    Bash, Error as BashError, ExecutionLimits, FileSystem, LimitExceeded, PosixFs, RealFs,
    RealFsMode,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

pub const BASHKIT_ABI_VERSION_1: u32 = 1;
pub const BASHKIT_MAX_CONFIG_BYTES: usize = 10_000_000;
pub const BASHKIT_RESULT_STDOUT_TRUNCATED: u32 = 1 << 0;
pub const BASHKIT_RESULT_STDERR_TRUNCATED: u32 = 1 << 1;

const MAX_ERROR_BYTES: usize = 1024;
const CAPABILITIES_JSON: &[u8] =
    br#"{"abi":1,"features":["git","jq","vfs","realfs-mounts","cancellation"]}"#;
const VERSION: &[u8] = env!("CARGO_PKG_VERSION").as_bytes();

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BashkitStatus {
    Ok = 0,
    InvalidArgument = 1,
    InvalidUtf8 = 2,
    InvalidConfig = 3,
    ExecutionError = 4,
    IoError = 5,
    Unsupported = 6,
    Cancelled = 7,
    InternalError = 255,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BashkitBytes {
    pub ptr: *const u8,
    pub len: usize,
}

impl BashkitBytes {
    const EMPTY: Self = Self {
        ptr: ptr::null(),
        len: 0,
    };

    fn borrowed(value: &[u8]) -> Self {
        if value.is_empty() {
            Self::EMPTY
        } else {
            Self {
                ptr: value.as_ptr(),
                len: value.len(),
            }
        }
    }
}

struct State {
    runtime: Runtime,
    bash: Bash,
    max_input_bytes: usize,
    allowed_mount_paths: Arc<[String]>,
}

pub struct Bashkit {
    state: Mutex<State>,
    // Shared interpreter cancellation flag. Deliberately outside the state
    // lock: bashkit_execute holds that lock while blocked, and cancellation
    // must stay reachable — and lock-free — from any thread.
    cancelled: Arc<AtomicBool>,
}

pub struct BashkitResult {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i32,
    flags: u32,
    final_env_json: Vec<u8>,
}

pub struct BashkitBuffer {
    bytes: Vec<u8>,
}

pub struct BashkitError {
    code: BashkitStatus,
    message: Vec<u8>,
}

struct ApiFailure {
    status: BashkitStatus,
    message: String,
}

impl ApiFailure {
    fn new(status: BashkitStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(BashkitStatus::InvalidArgument, message)
    }

    fn from_bash(error: BashError) -> Self {
        let status = match error {
            BashError::Io(_) => BashkitStatus::IoError,
            BashError::Cancelled => BashkitStatus::Cancelled,
            _ => BashkitStatus::ExecutionError,
        };
        Self::new(status, error.to_string())
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigV1 {
    schema_version: u32,
    #[serde(default)]
    profile: ProfileConfigV1,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    limits: LimitsConfigV1,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    readonly_filesystem: bool,
    #[serde(default)]
    capture_final_env: bool,
    #[serde(default)]
    mounts: Vec<MountConfigV1>,
    #[serde(default)]
    allowed_mount_paths: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct MountConfigV1 {
    path: String,
    root: String,
    #[serde(default)]
    writable: bool,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProfileConfigV1 {
    Hardened,
    #[default]
    Standard,
    Interactive,
}

impl From<ProfileConfigV1> for bashkit::ExecutionProfileName {
    fn from(value: ProfileConfigV1) -> Self {
        match value {
            ProfileConfigV1::Hardened => Self::Hardened,
            ProfileConfigV1::Standard => Self::Standard,
            ProfileConfigV1::Interactive => Self::Interactive,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsConfigV1 {
    timeout_ms: Option<u64>,
    parser_timeout_ms: Option<u64>,
    max_commands: Option<u64>,
    max_input_bytes: Option<u64>,
    max_output_bytes: Option<u64>,
}

fn checked_usize(value: u64, name: &str) -> Result<usize, ApiFailure> {
    usize::try_from(value).map_err(|_| {
        ApiFailure::new(
            BashkitStatus::InvalidConfig,
            format!("{name} exceeds this platform's size limit"),
        )
    })
}

fn make_runtime() -> Result<Runtime, ApiFailure> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| {
            ApiFailure::new(
                BashkitStatus::InternalError,
                "failed to initialize execution runtime",
            )
        })
}

// THREAT[TM-FS-013]: host-directory mounts pierce the sandbox boundary, so
// every mount root must (a) resolve under a configured `allowed_mount_paths`
// prefix and (b) clear the shared sensitive-path denylist
// (`bashkit::is_sensitive_mount_path`). Canonicalization defuses `..` segments
// and symlinks before the checks; comparison is case-folded on Windows
// filesystems. A sensitive root (home trees, `/etc`, `.ssh`, ...) additionally
// requires an allowlist entry that names it exactly: a broad parent entry such
// as the home directory itself is not consent to expose credential stores.
fn fold_path(path: &str) -> String {
    let trimmed = path.trim_end_matches(std::path::MAIN_SEPARATOR);
    let folded = if trimmed.is_empty() { path } else { trimmed };
    if cfg!(windows) {
        folded.to_ascii_lowercase()
    } else {
        folded.to_string()
    }
}

fn validate_mount_root(root: &str, allowed: &[String]) -> Result<PathBuf, ApiFailure> {
    if allowed.is_empty() {
        return Err(ApiFailure::new(
            BashkitStatus::InvalidConfig,
            "mount rejected: session has no allowed_mount_paths".to_string(),
        ));
    }
    let path = PathBuf::from(root);
    let canonical = std::fs::canonicalize(&path).unwrap_or(path);
    let candidate = fold_path(&canonical.to_string_lossy());
    let candidate = candidate.as_bytes();
    let mut covered = false;
    let mut exact = false;
    for prefix in allowed {
        let prefix_path = PathBuf::from(prefix);
        let prefix_canonical = std::fs::canonicalize(&prefix_path).unwrap_or(prefix_path);
        let prefix_folded = fold_path(&prefix_canonical.to_string_lossy());
        let prefix = prefix_folded.as_bytes();
        let exactly = candidate == prefix;
        let under = candidate.len() > prefix.len()
            && candidate.starts_with(prefix)
            && candidate[prefix.len()] == std::path::MAIN_SEPARATOR as u8;
        covered |= exactly || under;
        exact |= exactly;
    }
    if !covered {
        return Err(ApiFailure::new(
            BashkitStatus::InvalidConfig,
            format!("mount root {root:?} is not under any allowed_mount_paths prefix"),
        ));
    }
    if bashkit::is_sensitive_mount_path(&canonical) && !exact {
        return Err(ApiFailure::new(
            BashkitStatus::InvalidConfig,
            format!(
                "mount root {root:?} is a sensitive host path; name it exactly in \
                 allowed_mount_paths to mount it"
            ),
        ));
    }
    Ok(canonical)
}

fn apply_config_mounts(
    bash: &mut Bash,
    mounts: &[MountConfigV1],
    allowed: &[String],
) -> Result<(), ApiFailure> {
    for mount in mounts {
        let root = validate_mount_root(&mount.root, allowed)?;
        let mode = if mount.writable {
            RealFsMode::ReadWrite
        } else {
            RealFsMode::ReadOnly
        };
        #[allow(deprecated)] // The C ABI boundary is synchronous; there is no
        // async context at this call, mirroring `apply_real_mounts`.
        let fs = RealFs::new(&root, mode).map_err(|error| {
            ApiFailure::new(
                BashkitStatus::InvalidConfig,
                format!("failed to open mount root {:?}: {error}", mount.root),
            )
        })?;
        let fs: Arc<dyn FileSystem> = Arc::new(PosixFs::new(fs));
        bash.mount(Path::new(&mount.path), fs)
            .map_err(ApiFailure::from_bash)?;
    }
    Ok(())
}

fn build_from_config(config: ConfigV1) -> Result<(Bash, usize, Arc<[String]>), ApiFailure> {
    if config.schema_version != 1 {
        return Err(ApiFailure::new(
            BashkitStatus::InvalidConfig,
            format!(
                "unsupported configuration schema version {}",
                config.schema_version
            ),
        ));
    }

    let profile = bashkit::ExecutionProfile::named(config.profile.into());
    let mut limits = profile.execution_limits().clone();
    if let Some(value) = config.limits.timeout_ms {
        limits.timeout = Duration::from_millis(value);
    }
    if let Some(value) = config.limits.parser_timeout_ms {
        limits.parser_timeout = Duration::from_millis(value);
    }
    if let Some(value) = config.limits.max_commands {
        limits.max_commands = checked_usize(value, "max_commands")?;
    }
    if let Some(value) = config.limits.max_input_bytes {
        limits.max_input_bytes = checked_usize(value, "max_input_bytes")?;
    }
    if let Some(value) = config.limits.max_output_bytes {
        let value = checked_usize(value, "max_output_bytes")?;
        limits.max_stdout_bytes = value;
        limits.max_stderr_bytes = value;
    }
    limits.capture_final_env = config.capture_final_env;
    let max_input_bytes = limits.max_input_bytes;

    let mut builder = Bash::builder()
        .profile(profile)
        .limits(limits)
        .readonly_filesystem(config.readonly_filesystem);
    if let Some(cwd) = config.cwd {
        builder = builder.cwd(cwd);
    }
    if let Some(username) = config.username {
        builder = builder.username(username);
    }
    if let Some(hostname) = config.hostname {
        builder = builder.hostname(hostname);
    }
    for (key, value) in config.env {
        builder = builder.env(key, value);
    }
    for (path, content) in config.files {
        builder = builder.mount_text(path, content);
    }
    let mut bash = builder.build();
    let allowed: Arc<[String]> = Arc::from(config.allowed_mount_paths);
    apply_config_mounts(&mut bash, &config.mounts, &allowed)?;
    Ok((bash, max_input_bytes, allowed))
}

fn truncate_error(message: String) -> Vec<u8> {
    if message.len() <= MAX_ERROR_BYTES {
        return message.into_bytes();
    }
    let mut end = MAX_ERROR_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.as_bytes()[..end].to_vec()
}

unsafe fn output_slot<'a, T>(out: *mut *mut T, name: &str) -> Result<&'a mut *mut T, ApiFailure> {
    if out.is_null() {
        return Err(ApiFailure::invalid_argument(format!(
            "{name} must not be NULL"
        )));
    }
    let out = unsafe { &mut *out };
    *out = ptr::null_mut();
    Ok(out)
}

unsafe fn input_bytes<'a>(value: BashkitBytes, name: &str) -> Result<&'a [u8], ApiFailure> {
    if value.len == 0 {
        return Ok(&[]);
    }
    if value.ptr.is_null() {
        return Err(ApiFailure::invalid_argument(format!(
            "{name}.ptr must not be NULL when len is non-zero"
        )));
    }
    if value.len > isize::MAX as usize {
        return Err(ApiFailure::invalid_argument(format!(
            "{name}.len exceeds the supported size"
        )));
    }
    Ok(unsafe { slice::from_raw_parts(value.ptr, value.len) })
}

unsafe fn input_str<'a>(value: BashkitBytes, name: &str) -> Result<&'a str, ApiFailure> {
    let bytes = unsafe { input_bytes(value, name)? };
    str::from_utf8(bytes).map_err(|_| {
        ApiFailure::new(
            BashkitStatus::InvalidUtf8,
            format!("{name} must be valid UTF-8"),
        )
    })
}

unsafe fn handle<'a>(bash: *mut Bashkit) -> Result<&'a Bashkit, ApiFailure> {
    if bash.is_null() {
        return Err(ApiFailure::invalid_argument("bash must not be NULL"));
    }
    Ok(unsafe { &*bash })
}

unsafe fn set_error(out_error: *mut *mut BashkitError, failure: ApiFailure) -> BashkitStatus {
    if !out_error.is_null() {
        let error = Box::new(BashkitError {
            code: failure.status,
            message: truncate_error(failure.message),
        });
        unsafe { out_error.write(Box::into_raw(error)) };
    }
    failure.status
}

unsafe fn ffi_boundary(
    out_error: *mut *mut BashkitError,
    operation: impl FnOnce() -> Result<(), ApiFailure>,
) -> BashkitStatus {
    if !out_error.is_null() {
        unsafe { out_error.write(ptr::null_mut()) };
    }
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => BashkitStatus::Ok,
        Ok(Err(failure)) => unsafe { set_error(out_error, failure) },
        Err(_) => unsafe {
            set_error(
                out_error,
                ApiFailure::new(BashkitStatus::InternalError, "internal error"),
            )
        },
    }
}

fn final_env_json(final_env: Option<HashMap<String, String>>) -> Result<Vec<u8>, ApiFailure> {
    let Some(final_env) = final_env else {
        return Ok(Vec::new());
    };
    let sorted: BTreeMap<_, _> = final_env.into_iter().collect();
    serde_json::to_vec(&sorted).map_err(|_| {
        ApiFailure::new(
            BashkitStatus::InternalError,
            "failed to serialize final environment",
        )
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn bashkit_abi_version() -> u32 {
    BASHKIT_ABI_VERSION_1
}

#[unsafe(no_mangle)]
pub extern "C" fn bashkit_version() -> BashkitBytes {
    BashkitBytes::borrowed(VERSION)
}

#[unsafe(no_mangle)]
pub extern "C" fn bashkit_capabilities_json() -> BashkitBytes {
    BashkitBytes::borrowed(CAPABILITIES_JSON)
}

/// # Safety
/// `out_bash` must be writable. `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_create_default(
    out_bash: *mut *mut Bashkit,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let out_bash = output_slot(out_bash, "out_bash")?;
            let engine = Bash::new();
            let cancelled = engine.cancellation_token();
            let bash = Bashkit {
                state: Mutex::new(State {
                    runtime: make_runtime()?,
                    bash: engine,
                    max_input_bytes: ExecutionLimits::default().max_input_bytes,
                    allowed_mount_paths: Arc::from(Vec::<String>::new()),
                }),
                cancelled,
            };
            *out_bash = Box::into_raw(Box::new(bash));
            Ok(())
        })
    }
}

/// # Safety
/// Input bytes must remain readable for the call. `out_bash` must be writable;
/// `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_create_json(
    config_json: BashkitBytes,
    out_bash: *mut *mut Bashkit,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let out_bash = output_slot(out_bash, "out_bash")?;
            if config_json.len > BASHKIT_MAX_CONFIG_BYTES {
                return Err(ApiFailure::new(
                    BashkitStatus::InvalidConfig,
                    "configuration exceeds 10000000 bytes",
                ));
            }
            let config_bytes = input_bytes(config_json, "config_json")?;
            let config: ConfigV1 = serde_json::from_slice(config_bytes).map_err(|error| {
                ApiFailure::new(
                    BashkitStatus::InvalidConfig,
                    format!("invalid configuration: {error}"),
                )
            })?;
            let (engine, max_input_bytes, allowed_mount_paths) = build_from_config(config)?;
            let cancelled = engine.cancellation_token();
            let bash = Bashkit {
                state: Mutex::new(State {
                    runtime: make_runtime()?,
                    bash: engine,
                    max_input_bytes,
                    allowed_mount_paths,
                }),
                cancelled,
            };
            *out_bash = Box::into_raw(Box::new(bash));
            Ok(())
        })
    }
}

/// # Safety
/// `bash` must be null or a live pointer returned by a Bashkit create function,
/// and it must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_free(bash: *mut Bashkit) {
    if bash.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(bash));
    }));
}

/// # Safety
/// `bash` must be live, script bytes readable, and `out_result` writable.
/// `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_execute(
    bash: *mut Bashkit,
    script: BashkitBytes,
    out_result: *mut *mut BashkitResult,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let out_result = output_slot(out_result, "out_result")?;
            let bash = handle(bash)?;
            let mut state = bash.state.lock().map_err(|_| {
                ApiFailure::new(BashkitStatus::InternalError, "bash instance is unavailable")
            })?;
            if script.len > state.max_input_bytes {
                return Err(ApiFailure::from_bash(BashError::ResourceLimit(
                    LimitExceeded::InputTooLarge(script.len, state.max_input_bytes),
                )));
            }
            let script = input_str(script, "script")?;
            let State { runtime, bash, .. } = &mut *state;
            let result = runtime
                .block_on(bash.exec(script))
                .map_err(ApiFailure::from_bash)?;
            let stdout = result.stdout.into_bytes();
            let mut flags = 0;
            if result.stdout_truncated {
                flags |= BASHKIT_RESULT_STDOUT_TRUNCATED;
            }
            if result.stderr_truncated {
                flags |= BASHKIT_RESULT_STDERR_TRUNCATED;
            }
            let result = BashkitResult {
                stdout,
                stderr: result.stderr.into_bytes(),
                exit_code: result.exit_code,
                flags,
                final_env_json: final_env_json(result.final_env)?,
            };
            *out_result = Box::into_raw(Box::new(result));
            Ok(())
        })
    }
}

/// Requests cancellation of the execution currently running on `bash`.
/// Requires the `cancellation` capability. Lock-free — never touches the state
/// mutex — so it is safe to call from any thread while `bashkit_execute` is
/// blocked on the same handle. Execution aborts at the next command boundary
/// and `bashkit_execute` reports `BASHKIT_CANCELLED`.
///
/// The flag is sticky: reset it with `bashkit_clear_cancel` before the next
/// execute, or that call aborts immediately.
///
/// # Safety
/// `bash` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_cancel(bash: *mut Bashkit) -> BashkitStatus {
    unsafe {
        ffi_boundary(ptr::null_mut(), || {
            let bash = handle(bash)?;
            bash.cancelled.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// Clears the flag set by `bashkit_cancel`, restoring normal execution.
/// Lock-free like `bashkit_cancel`.
///
/// # Safety
/// `bash` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_clear_cancel(bash: *mut Bashkit) -> BashkitStatus {
    unsafe {
        ffi_boundary(ptr::null_mut(), || {
            let bash = handle(bash)?;
            bash.cancelled.store(false, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// # Safety
/// `result` must be null or a live result pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_result_exit_code(result: *const BashkitResult) -> i32 {
    unsafe { result.as_ref().map_or(0, |result| result.exit_code) }
}

/// # Safety
/// `result` must be null or live. The returned view expires with `result`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_result_stdout(result: *const BashkitResult) -> BashkitBytes {
    unsafe {
        result.as_ref().map_or(BashkitBytes::EMPTY, |result| {
            BashkitBytes::borrowed(&result.stdout)
        })
    }
}

/// # Safety
/// `result` must be null or live. The returned view expires with `result`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_result_stderr(result: *const BashkitResult) -> BashkitBytes {
    unsafe {
        result.as_ref().map_or(BashkitBytes::EMPTY, |result| {
            BashkitBytes::borrowed(&result.stderr)
        })
    }
}

/// # Safety
/// `result` must be null or a live result pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_result_flags(result: *const BashkitResult) -> u32 {
    unsafe { result.as_ref().map_or(0, |result| result.flags) }
}

/// # Safety
/// `result` must be null or live. The returned view expires with `result`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_result_final_env_json(
    result: *const BashkitResult,
) -> BashkitBytes {
    unsafe {
        result.as_ref().map_or(BashkitBytes::EMPTY, |result| {
            BashkitBytes::borrowed(&result.final_env_json)
        })
    }
}

/// # Safety
/// `result` must be null or a live result pointer and not used afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_result_free(result: *mut BashkitResult) {
    if result.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(result));
    }));
}

unsafe fn with_filesystem(
    bash: *mut Bashkit,
    operation: impl FnOnce(&Runtime, &dyn FileSystem) -> Result<(), BashError>,
) -> Result<(), ApiFailure> {
    let bash = unsafe { handle(bash)? };
    let state = bash.state.lock().map_err(|_| {
        ApiFailure::new(BashkitStatus::InternalError, "bash instance is unavailable")
    })?;
    operation(&state.runtime, state.bash.fs().as_ref()).map_err(ApiFailure::from_bash)
}

/// # Safety
/// `bash` must be live; path and content bytes must remain readable for the call.
/// `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_write_file(
    bash: *mut Bashkit,
    path: BashkitBytes,
    content: BashkitBytes,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let path = input_str(path, "path")?;
            let content = input_bytes(content, "content")?;
            with_filesystem(bash, |runtime, fs| {
                runtime.block_on(fs.write_file(Path::new(path), content))
            })
        })
    }
}

/// # Safety
/// `bash` must be live, path readable, and `out_buffer` writable. `out_error`,
/// when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_read_file(
    bash: *mut Bashkit,
    path: BashkitBytes,
    out_buffer: *mut *mut BashkitBuffer,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let out_buffer = output_slot(out_buffer, "out_buffer")?;
            let path = input_str(path, "path")?;
            let bash = handle(bash)?;
            let state = bash.state.lock().map_err(|_| {
                ApiFailure::new(BashkitStatus::InternalError, "bash instance is unavailable")
            })?;
            let bytes = state
                .runtime
                .block_on(state.bash.fs().read_file(Path::new(path)))
                .map_err(ApiFailure::from_bash)?;
            *out_buffer = Box::into_raw(Box::new(BashkitBuffer { bytes }));
            Ok(())
        })
    }
}

/// # Safety
/// `bash` must be live and path readable. `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_mkdir(
    bash: *mut Bashkit,
    path: BashkitBytes,
    recursive: u32,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let path = input_str(path, "path")?;
            with_filesystem(bash, |runtime, fs| {
                runtime.block_on(fs.mkdir(Path::new(path), recursive != 0))
            })
        })
    }
}

/// # Safety
/// `bash` must be live and path readable. `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_remove(
    bash: *mut Bashkit,
    path: BashkitBytes,
    recursive: u32,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let path = input_str(path, "path")?;
            with_filesystem(bash, |runtime, fs| {
                runtime.block_on(fs.remove(Path::new(path), recursive != 0))
            })
        })
    }
}

/// Mounts a host directory at `vfs_path` for the duration of the session.
/// Requires the `realfs-mounts` capability and a `host_root` that resolves
/// under one of the session's `allowed_mount_paths` prefixes. Sensitive host
/// paths (home trees, `/etc`, `.ssh`, ...) are refused unless the allowlist
/// names the root exactly (TM-FS-013) — the same rule config-time mounts use.
///
/// # Safety
/// `bash` must be live; path bytes must remain readable for the call.
/// `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_mount(
    bash: *mut Bashkit,
    vfs_path: BashkitBytes,
    host_root: BashkitBytes,
    writable: u32,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let bash = handle(bash)?;
            let vfs_path = input_str(vfs_path, "vfs_path")?;
            let host_root = input_str(host_root, "host_root")?;
            let state = bash.state.lock().map_err(|_| {
                ApiFailure::new(BashkitStatus::InternalError, "bash instance is unavailable")
            })?;
            let root = validate_mount_root(host_root, &state.allowed_mount_paths)?;
            let mode = if writable != 0 {
                RealFsMode::ReadWrite
            } else {
                RealFsMode::ReadOnly
            };
            #[allow(deprecated)] // The C ABI boundary is synchronous; there
            // is no async context at this call, mirroring `apply_real_mounts`.
            let fs = RealFs::new(&root, mode).map_err(|error| {
                ApiFailure::new(
                    BashkitStatus::InvalidArgument,
                    format!("failed to open mount root {host_root:?}: {error}"),
                )
            })?;
            let fs: Arc<dyn FileSystem> = Arc::new(PosixFs::new(fs));
            state
                .bash
                .mount(Path::new(vfs_path), fs)
                .map_err(ApiFailure::from_bash)
        })
    }
}

/// Removes the mount at `vfs_path`. Shell state is preserved; paths under
/// `vfs_path` fall back to the underlying filesystem.
///
/// # Safety
/// `bash` must be live; path bytes must remain readable for the call.
/// `out_error`, when non-null, must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_unmount(
    bash: *mut Bashkit,
    vfs_path: BashkitBytes,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(out_error, || {
            let bash = handle(bash)?;
            let vfs_path = input_str(vfs_path, "vfs_path")?;
            let state = bash.state.lock().map_err(|_| {
                ApiFailure::new(BashkitStatus::InternalError, "bash instance is unavailable")
            })?;
            state
                .bash
                .unmount(Path::new(vfs_path))
                .map_err(ApiFailure::from_bash)
        })
    }
}

/// # Safety
/// `buffer` must be null or live. The returned view expires with `buffer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_buffer_bytes(buffer: *const BashkitBuffer) -> BashkitBytes {
    unsafe {
        buffer.as_ref().map_or(BashkitBytes::EMPTY, |buffer| {
            BashkitBytes::borrowed(&buffer.bytes)
        })
    }
}

/// # Safety
/// `buffer` must be null or a live buffer pointer and not used afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_buffer_free(buffer: *mut BashkitBuffer) {
    if buffer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(buffer));
    }));
}

/// # Safety
/// `error` must be null or a live error pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_error_code(error: *const BashkitError) -> u32 {
    unsafe {
        error
            .as_ref()
            .map_or(BashkitStatus::Ok as u32, |error| error.code as u32)
    }
}

/// # Safety
/// `error` must be null or live. The returned view expires with `error`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_error_message(error: *const BashkitError) -> BashkitBytes {
    unsafe {
        error.as_ref().map_or(BashkitBytes::EMPTY, |error| {
            BashkitBytes::borrowed(&error.message)
        })
    }
}

/// # Safety
/// `error` must be null or a live error pointer and not used afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_error_free(error: *mut BashkitError) {
    if error.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(error));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tm_int_008_contains_panics_at_the_foreign_boundary() {
        unsafe {
            let mut error = ptr::null_mut();
            let status = ffi_boundary(&mut error, || -> Result<(), ApiFailure> {
                panic!("injected panic detail");
            });

            assert_eq!(status, BashkitStatus::InternalError);
            assert!(!error.is_null());
            assert_eq!((*error).message, b"internal error");
            bashkit_error_free(error);
        }
    }
}
