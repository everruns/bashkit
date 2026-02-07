//! python/python3 builtin via embedded Monty interpreter (pydantic/monty)
//!
//! Sandboxed Python execution with resource limits and VFS access.
//! Python `pathlib.Path` operations are bridged to BashKit's virtual filesystem
//! via Monty's OsCall pause/resume mechanism. No real filesystem or network access.
//!
//! Supports: `python -c "code"`, `python script.py`, stdin piping.

use async_trait::async_trait;
use monty::{
    dir_stat, file_stat, CollectStringPrint, ExcType, ExternalResult, LimitedTracker,
    MontyException, MontyObject, MontyRun, OsFunction, ResourceLimits, RunProgress,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::{resolve_path, Builtin, Context};
use crate::error::Result;
use crate::fs::{FileSystem, FileType};
use crate::interpreter::ExecResult;

/// Default resource limits for sandboxed Python execution.
const DEFAULT_MAX_ALLOCATIONS: usize = 1_000_000;
const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(30);
const DEFAULT_MAX_MEMORY: usize = 64 * 1024 * 1024; // 64 MB
const DEFAULT_MAX_RECURSION: usize = 200;

/// Resource limits for the embedded Python (Monty) interpreter.
///
/// Use the builder pattern to customize, or `Default` for the standard sandbox limits:
/// - 1,000,000 allocations
/// - 30 second timeout
/// - 64 MB memory
/// - 200 recursion depth
///
/// # Example
///
/// ```rust,ignore
/// use bashkit::PythonLimits;
///
/// let limits = PythonLimits::default()
///     .max_duration(Duration::from_secs(5))
///     .max_memory(16 * 1024 * 1024);
///
/// let bash = Bash::builder().python_with_limits(limits).build();
/// ```
#[derive(Debug, Clone)]
pub struct PythonLimits {
    /// Maximum heap allocations (default: 1,000,000).
    pub max_allocations: usize,
    /// Maximum execution time (default: 30s).
    pub max_duration: Duration,
    /// Maximum memory in bytes (default: 64 MB).
    pub max_memory: usize,
    /// Maximum recursion depth (default: 200).
    pub max_recursion: usize,
}

impl Default for PythonLimits {
    fn default() -> Self {
        Self {
            max_allocations: DEFAULT_MAX_ALLOCATIONS,
            max_duration: DEFAULT_MAX_DURATION,
            max_memory: DEFAULT_MAX_MEMORY,
            max_recursion: DEFAULT_MAX_RECURSION,
        }
    }
}

impl PythonLimits {
    /// Set max heap allocations.
    #[must_use]
    pub fn max_allocations(mut self, n: usize) -> Self {
        self.max_allocations = n;
        self
    }

    /// Set max execution duration.
    #[must_use]
    pub fn max_duration(mut self, d: Duration) -> Self {
        self.max_duration = d;
        self
    }

    /// Set max memory in bytes.
    #[must_use]
    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.max_memory = bytes;
        self
    }

    /// Set max recursion depth.
    #[must_use]
    pub fn max_recursion(mut self, depth: usize) -> Self {
        self.max_recursion = depth;
        self
    }
}

/// The python/python3 builtin command.
///
/// Executes Python code using the embedded Monty interpreter (pydantic/monty).
/// Python `pathlib.Path` operations are bridged to BashKit's VFS — files
/// created by bash (`cat > file`) are readable from Python, and vice versa.
///
/// # Usage
///
/// ```bash
/// python3 -c "print('hello')"
/// python3 script.py
/// echo "print('hello')" | python3
/// python3 -c "2 + 2"              # expression result printed
/// python3 --version
/// python3 -c "from pathlib import Path; print(Path('/tmp/f.txt').read_text())"
/// ```
pub struct Python {
    /// Resource limits for the Monty interpreter.
    pub limits: PythonLimits,
}

impl Python {
    /// Create with default limits.
    pub fn new() -> Self {
        Self {
            limits: PythonLimits::default(),
        }
    }

    /// Create with custom limits.
    pub fn with_limits(limits: PythonLimits) -> Self {
        Self { limits }
    }
}

impl Default for Python {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Builtin for Python {
    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "python/python3: Embedded Python (Monty). \
             File I/O via pathlib.Path only (no open()). \
             No HTTP/network. No classes. No third-party imports.",
        )
    }

    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;

        // python --version / python -V
        if args.first().map(|s| s.as_str()) == Some("--version")
            || args.first().map(|s| s.as_str()) == Some("-V")
        {
            return Ok(ExecResult::ok("Python 3.12.0 (monty)\n".to_string()));
        }

        // python --help / python -h
        if args.first().map(|s| s.as_str()) == Some("--help")
            || args.first().map(|s| s.as_str()) == Some("-h")
        {
            return Ok(ExecResult::ok(
                "usage: python3 [-c cmd | file | -] [arg ...]\n\
                 Options:\n  \
                 -c cmd : execute code from string\n  \
                 file   : execute code from file (VFS)\n  \
                 -      : read code from stdin\n  \
                 -V     : print version\n"
                    .to_string(),
            ));
        }

        let (code, filename) = if let Some(first) = args.first() {
            match first.as_str() {
                "-c" => {
                    // python -c "code"
                    let code = args.get(1).map(|s| s.as_str()).unwrap_or("");
                    if code.is_empty() {
                        return Ok(ExecResult::err(
                            "python3: option -c requires argument\n".to_string(),
                            2,
                        ));
                    }
                    (code.to_string(), "<string>".to_string())
                }
                "-" => {
                    // python - : read from stdin
                    match ctx.stdin {
                        Some(input) if !input.is_empty() => {
                            (input.to_string(), "<stdin>".to_string())
                        }
                        _ => {
                            return Ok(ExecResult::err(
                                "python3: no input from stdin\n".to_string(),
                                1,
                            ));
                        }
                    }
                }
                arg if arg.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!("python3: unknown option: {arg}\n"),
                        2,
                    ));
                }
                script_path => {
                    // python script.py
                    let path = resolve_path(ctx.cwd, script_path);
                    match ctx.fs.read_file(&path).await {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(code) => (code, script_path.to_string()),
                            Err(_) => {
                                return Ok(ExecResult::err(
                                    format!(
                                        "python3: can't decode file '{script_path}': not UTF-8\n"
                                    ),
                                    1,
                                ));
                            }
                        },
                        Err(_) => {
                            return Ok(ExecResult::err(
                                format!(
                                    "python3: can't open file '{script_path}': No such file or directory\n"
                                ),
                                2,
                            ));
                        }
                    }
                }
            }
        } else if let Some(input) = ctx.stdin {
            // Piped input without arguments
            if input.is_empty() {
                return Ok(ExecResult::ok(String::new()));
            }
            (input.to_string(), "<stdin>".to_string())
        } else {
            // No args, no stdin — interactive mode not supported
            return Ok(ExecResult::err(
                "python3: interactive mode not supported in sandbox\n".to_string(),
                1,
            ));
        };

        run_python(
            &code,
            &filename,
            ctx.fs.clone(),
            ctx.cwd,
            ctx.env,
            &self.limits,
        )
        .await
    }
}

/// Execute Python code via Monty with resource limits and VFS bridging.
///
/// Uses Monty's start/resume API: execution pauses at filesystem operations
/// (OsCall), we bridge them to BashKit's VFS, then resume.
async fn run_python(
    code: &str,
    filename: &str,
    fs: Arc<dyn FileSystem>,
    cwd: &Path,
    env: &HashMap<String, String>,
    py_limits: &PythonLimits,
) -> Result<ExecResult> {
    // Strip shebang if present
    let code = if code.starts_with("#!") {
        match code.find('\n') {
            Some(pos) => &code[pos + 1..],
            None => "",
        }
    } else {
        code
    };

    let runner = match MontyRun::new(code.to_owned(), filename, vec![], vec![]) {
        Ok(r) => r,
        Err(e) => return Ok(format_exception(e)),
    };

    let limits = ResourceLimits::new()
        .max_allocations(py_limits.max_allocations)
        .max_duration(py_limits.max_duration)
        .max_memory(py_limits.max_memory)
        .max_recursion_depth(Some(py_limits.max_recursion));

    let tracker = LimitedTracker::new(limits);
    let mut printer = CollectStringPrint::new();

    // Use start() instead of run() to handle OsCall pauses for VFS bridging
    let mut progress = match runner.start(vec![], tracker, &mut printer) {
        Ok(p) => p,
        Err(e) => {
            let printed = printer.into_output();
            return Ok(format_exception_with_output(e, &printed));
        }
    };

    loop {
        match progress {
            RunProgress::OsCall {
                function,
                args,
                kwargs,
                state,
                ..
            } => {
                let result = handle_os_call(function, &args, &kwargs, &fs, cwd, env).await;
                match state.run(result, &mut printer) {
                    Ok(next) => progress = next,
                    Err(e) => {
                        let printed = printer.into_output();
                        return Ok(format_exception_with_output(e, &printed));
                    }
                }
            }
            RunProgress::FunctionCall { state, .. } => {
                // No external functions registered; return error
                let err = MontyException::new(
                    ExcType::RuntimeError,
                    Some("external function not available in sandbox".into()),
                );
                match state.run(ExternalResult::Error(err), &mut printer) {
                    Ok(next) => progress = next,
                    Err(e) => {
                        let printed = printer.into_output();
                        return Ok(format_exception_with_output(e, &printed));
                    }
                }
            }
            RunProgress::ResolveFutures(_) => {
                // Async futures not supported in sandbox
                let printed = printer.into_output();
                let err = MontyException::new(
                    ExcType::RuntimeError,
                    Some("async operations not supported in sandbox".into()),
                );
                return Ok(format_exception_with_output(err, &printed));
            }
            RunProgress::Complete(result) => {
                let mut output = printer.into_output();

                // If the result is not None and there was no print output,
                // display the result (like Python REPL behavior for expressions)
                if !matches!(result, MontyObject::None) && output.is_empty() {
                    output = format!("{}\n", result.py_repr());
                }

                return Ok(ExecResult::ok(output));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// VFS bridging: Monty OsCall → BashKit FileSystem
// ---------------------------------------------------------------------------

/// Dispatch a Monty OsCall to the appropriate VFS operation.
async fn handle_os_call(
    function: OsFunction,
    args: &[MontyObject],
    kwargs: &[(MontyObject, MontyObject)],
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> ExternalResult {
    // Environment access doesn't need a path
    match function {
        OsFunction::Getenv => return handle_getenv(args, env),
        OsFunction::GetEnviron => return handle_get_environ(env),
        _ => {}
    }

    // All other ops need a path as first arg
    let path = match extract_path(args, cwd) {
        Some(p) => p,
        None => {
            return ExternalResult::Error(MontyException::new(
                ExcType::TypeError,
                Some("expected path argument".into()),
            ))
        }
    };

    match function {
        OsFunction::Exists => {
            let exists = fs.exists(&path).await.unwrap_or(false);
            ExternalResult::Return(MontyObject::Bool(exists))
        }
        OsFunction::IsFile => match fs.stat(&path).await {
            Ok(meta) => ExternalResult::Return(MontyObject::Bool(meta.file_type.is_file())),
            Err(_) => ExternalResult::Return(MontyObject::Bool(false)),
        },
        OsFunction::IsDir => match fs.stat(&path).await {
            Ok(meta) => ExternalResult::Return(MontyObject::Bool(meta.file_type.is_dir())),
            Err(_) => ExternalResult::Return(MontyObject::Bool(false)),
        },
        OsFunction::IsSymlink => match fs.stat(&path).await {
            Ok(meta) => ExternalResult::Return(MontyObject::Bool(meta.file_type.is_symlink())),
            Err(_) => ExternalResult::Return(MontyObject::Bool(false)),
        },
        OsFunction::ReadText => match fs.read_file(&path).await {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => ExternalResult::Return(MontyObject::String(s)),
                Err(_) => ExternalResult::Error(MontyException::new(
                    ExcType::OSError,
                    Some(format!(
                        "can't decode '{}': not valid UTF-8",
                        path.display()
                    )),
                )),
            },
            Err(e) => map_vfs_error(e, &path),
        },
        OsFunction::ReadBytes => match fs.read_file(&path).await {
            Ok(bytes) => ExternalResult::Return(MontyObject::Bytes(bytes)),
            Err(e) => map_vfs_error(e, &path),
        },
        OsFunction::WriteText => {
            let content = match args.get(1) {
                Some(MontyObject::String(s)) => s.as_bytes().to_vec(),
                _ => {
                    return ExternalResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("write_text() requires a string argument".into()),
                    ))
                }
            };
            let len = content.len();
            match fs.write_file(&path, &content).await {
                Ok(()) => ExternalResult::Return(MontyObject::Int(len as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        OsFunction::WriteBytes => {
            let content = match args.get(1) {
                Some(MontyObject::Bytes(b)) => b.clone(),
                _ => {
                    return ExternalResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("write_bytes() requires a bytes argument".into()),
                    ))
                }
            };
            let len = content.len();
            match fs.write_file(&path, &content).await {
                Ok(()) => ExternalResult::Return(MontyObject::Int(len as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        OsFunction::Mkdir => {
            let parents = get_bool_kwarg(kwargs, "parents").unwrap_or(false);
            let exist_ok = get_bool_kwarg(kwargs, "exist_ok").unwrap_or(false);
            match fs.mkdir(&path, parents).await {
                Ok(()) => ExternalResult::Return(MontyObject::None),
                Err(e) => {
                    let msg = e.to_string();
                    if exist_ok && msg.contains("already exists") {
                        ExternalResult::Return(MontyObject::None)
                    } else {
                        map_vfs_error(e, &path)
                    }
                }
            }
        }
        OsFunction::Unlink => match fs.remove(&path, false).await {
            Ok(()) => ExternalResult::Return(MontyObject::None),
            Err(e) => map_vfs_error(e, &path),
        },
        OsFunction::Rmdir => match fs.remove(&path, false).await {
            Ok(()) => ExternalResult::Return(MontyObject::None),
            Err(e) => map_vfs_error(e, &path),
        },
        OsFunction::Iterdir => match fs.read_dir(&path).await {
            Ok(entries) => {
                let items: Vec<MontyObject> = entries
                    .into_iter()
                    .map(|e| {
                        let child = path.join(&e.name);
                        MontyObject::Path(child.to_string_lossy().to_string())
                    })
                    .collect();
                ExternalResult::Return(MontyObject::List(items))
            }
            Err(e) => map_vfs_error(e, &path),
        },
        OsFunction::Stat => match fs.stat(&path).await {
            Ok(meta) => {
                let mtime = meta
                    .modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                let stat_obj = match meta.file_type {
                    FileType::Directory => dir_stat(meta.mode as i64, mtime),
                    _ => file_stat(meta.mode as i64, meta.size as i64, mtime),
                };
                ExternalResult::Return(stat_obj)
            }
            Err(e) => map_vfs_error(e, &path),
        },
        OsFunction::Rename => {
            let target = match args.get(1) {
                Some(MontyObject::Path(p)) | Some(MontyObject::String(p)) => {
                    resolve_python_path(p, cwd)
                }
                _ => {
                    return ExternalResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("rename() requires a target path".into()),
                    ))
                }
            };
            match fs.rename(&path, &target).await {
                Ok(()) => {
                    ExternalResult::Return(MontyObject::Path(target.to_string_lossy().to_string()))
                }
                Err(e) => map_vfs_error(e, &path),
            }
        }
        OsFunction::Resolve | OsFunction::Absolute => {
            // No symlink resolution in BashKit VFS; just return absolute path
            ExternalResult::Return(MontyObject::Path(path.to_string_lossy().to_string()))
        }
        // Getenv/GetEnviron handled above
        _ => ExternalResult::Error(MontyException::new(
            ExcType::OSError,
            Some(format!("{function} not supported in sandbox")),
        )),
    }
}

/// Extract a path from the first OsCall arg and resolve relative to cwd.
fn extract_path(args: &[MontyObject], cwd: &Path) -> Option<PathBuf> {
    match args.first()? {
        MontyObject::Path(s) | MontyObject::String(s) => Some(resolve_python_path(s, cwd)),
        _ => None,
    }
}

/// Resolve a Python path string against cwd if relative.
fn resolve_python_path(path_str: &str, cwd: &Path) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_owned()
    } else {
        cwd.join(p)
    }
}

/// Map a BashKit VFS error to a Python exception via ExternalResult.
fn map_vfs_error(e: crate::Error, path: &Path) -> ExternalResult {
    let msg = e.to_string();
    let path_str = path.display().to_string();

    let (exc_type, errno) = if msg.contains("not found") || msg.contains("No such file") {
        (ExcType::FileNotFoundError, 2)
    } else if msg.contains("is a directory") {
        (ExcType::IsADirectoryError, 21)
    } else if msg.contains("not a directory") {
        (ExcType::NotADirectoryError, 20)
    } else if msg.contains("already exists") {
        (ExcType::FileExistsError, 17)
    } else {
        (ExcType::OSError, 0)
    };

    ExternalResult::Error(MontyException::new(
        exc_type,
        Some(format!("[Errno {errno}] {msg}: '{path_str}'")),
    ))
}

/// Extract a bool kwarg by name from the kwargs list.
fn get_bool_kwarg(kwargs: &[(MontyObject, MontyObject)], name: &str) -> Option<bool> {
    for (key, val) in kwargs {
        if let MontyObject::String(k) = key {
            if k == name {
                return match val {
                    MontyObject::Bool(b) => Some(*b),
                    _ => None,
                };
            }
        }
    }
    None
}

/// Handle os.getenv(key, default=None).
fn handle_getenv(args: &[MontyObject], env: &HashMap<String, String>) -> ExternalResult {
    let key = match args.first() {
        Some(MontyObject::String(s)) => s.as_str(),
        _ => {
            return ExternalResult::Error(MontyException::new(
                ExcType::TypeError,
                Some("getenv() requires a string argument".into()),
            ))
        }
    };
    let default = match args.get(1) {
        Some(MontyObject::None) | None => MontyObject::None,
        Some(other) => other.clone(),
    };
    match env.get(key) {
        Some(val) => ExternalResult::Return(MontyObject::String(val.clone())),
        None => ExternalResult::Return(default),
    }
}

/// Handle os.environ → dict of all env vars.
fn handle_get_environ(env: &HashMap<String, String>) -> ExternalResult {
    let pairs: Vec<(MontyObject, MontyObject)> = env
        .iter()
        .map(|(k, v)| {
            (
                MontyObject::String(k.clone()),
                MontyObject::String(v.clone()),
            )
        })
        .collect();
    ExternalResult::Return(MontyObject::dict(pairs))
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

/// Format a MontyException into an ExecResult with exit code 1.
fn format_exception(e: MontyException) -> ExecResult {
    ExecResult::err(format!("{e}\n"), 1)
}

/// Format exception, preserving any output produced before the error.
fn format_exception_with_output(e: MontyException, printed: &str) -> ExecResult {
    let stderr = format!("{e}\n");
    let mut result = ExecResult::err(stderr, 1);
    if !printed.is_empty() {
        result.stdout = printed.to_string();
    }
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::builtins::Context;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, stdin);
        Python::new().execute(ctx).await.unwrap()
    }

    async fn run_with_file(args: &[&str], file_path: &str, content: &str) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new(file_path), content.as_bytes())
            .await
            .unwrap();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Python::new().execute(ctx).await.unwrap()
    }

    /// Helper: run Python with pre-populated VFS files and env vars.
    async fn run_with_vfs(
        args: &[&str],
        files: &[(&str, &str)],
        env_vars: &[(&str, &str)],
    ) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env: HashMap<String, String> = env_vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            // Ensure parent dirs exist
            let p = std::path::Path::new(path);
            if let Some(parent) = p.parent() {
                let _ = fs.mkdir(parent, true).await;
            }
            fs.write_file(p, content.as_bytes()).await.unwrap();
        }
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Python::new().execute(ctx).await.unwrap()
    }

    // --- Basic functionality tests (existing) ---

    #[tokio::test]
    async fn test_version() {
        let r = run(&["--version"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("Python 3.12.0"));
    }

    #[tokio::test]
    async fn test_inline_print() {
        let r = run(&["-c", "print('hello world')"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_inline_expression() {
        let r = run(&["-c", "2 + 3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "5\n");
    }

    #[tokio::test]
    async fn test_inline_multiline() {
        let r = run(&["-c", "x = 10\ny = 20\nprint(x + y)"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "30\n");
    }

    #[tokio::test]
    async fn test_syntax_error() {
        let r = run(&["-c", "def"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("SyntaxError") || r.stderr.contains("Error"));
    }

    #[tokio::test]
    async fn test_runtime_error() {
        let r = run(&["-c", "1/0"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("ZeroDivisionError"));
    }

    #[tokio::test]
    async fn test_stdin_code() {
        let r = run(&["-"], Some("print('from stdin')")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from stdin\n");
    }

    #[tokio::test]
    async fn test_piped_stdin() {
        let r = run(&[], Some("print('piped')")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "piped\n");
    }

    #[tokio::test]
    async fn test_file_execution() {
        let r = run_with_file(&["script.py"], "/home/user/script.py", "print('from file')").await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from file\n");
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let r = run(&["missing.py"], None).await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("can't open file"));
    }

    #[tokio::test]
    async fn test_no_args_no_stdin() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("interactive mode not supported"));
    }

    #[tokio::test]
    async fn test_c_flag_missing_arg() {
        let r = run(&["-c"], None).await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("requires argument"));
    }

    #[tokio::test]
    async fn test_unknown_option() {
        let r = run(&["-x"], None).await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("unknown option"));
    }

    #[tokio::test]
    async fn test_help() {
        let r = run(&["--help"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("usage:"));
    }

    #[tokio::test]
    async fn test_dict_access() {
        let r = run(&["-c", "d = dict()\nd['a'] = 1\nprint(d['a'])"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_list_comprehension() {
        let r = run(&["-c", "[x*2 for x in range(3)]"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[0, 2, 4]\n");
    }

    #[tokio::test]
    async fn test_fstring() {
        let r = run(&["-c", "x = 42\nprint(f'value={x}')"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "value=42\n");
    }

    #[tokio::test]
    async fn test_recursion_limit() {
        let r = run(&["-c", "def r(): r()\nr()"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("RecursionError") || r.stderr.contains("recursion"));
    }

    #[tokio::test]
    async fn test_shebang_stripped() {
        let r = run_with_file(
            &["script.py"],
            "/home/user/script.py",
            "#!/usr/bin/env python3\nprint('shebang ok')",
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "shebang ok\n");
    }

    #[tokio::test]
    async fn test_name_error() {
        let r = run(&["-c", "print(undefined_var)"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("NameError"));
    }

    #[tokio::test]
    async fn test_type_error() {
        let r = run(&["-c", "1 + 'a'"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("TypeError"));
    }

    #[tokio::test]
    async fn test_index_error() {
        let r = run(&["-c", "lst = [1, 2]\nprint(lst[10])"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("IndexError"));
    }

    #[tokio::test]
    async fn test_empty_stdin() {
        let r = run(&["-"], Some("")).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_output_before_error() {
        let r = run(&["-c", "print('before')\n1/0"], None).await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "before\n");
        assert!(r.stderr.contains("ZeroDivisionError"));
    }

    // --- VFS bridging tests ---

    #[tokio::test]
    async fn test_vfs_read_text() {
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\nprint(Path('/tmp/hello.txt').read_text())",
            ],
            &[("/tmp/hello.txt", "hello from vfs")],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello from vfs\n");
    }

    #[tokio::test]
    async fn test_vfs_write_text() {
        // Write via Python, then read via Python to verify
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\nPath('/tmp/out.txt').write_text('written by python')\nprint(Path('/tmp/out.txt').read_text())",
            ],
            &[],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "written by python\n");
    }

    #[tokio::test]
    async fn test_vfs_exists() {
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\nprint(Path('/tmp/hello.txt').exists())\nprint(Path('/tmp/nope.txt').exists())",
            ],
            &[("/tmp/hello.txt", "content")],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nFalse\n");
    }

    #[tokio::test]
    async fn test_vfs_is_file_is_dir() {
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\nprint(Path('/tmp/f.txt').is_file())\nprint(Path('/tmp').is_dir())",
            ],
            &[("/tmp/f.txt", "data")],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nTrue\n");
    }

    #[tokio::test]
    async fn test_vfs_read_not_found() {
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\ntry:\n    Path('/no/such/file').read_text()\nexcept FileNotFoundError as e:\n    print('caught:', e)",
            ],
            &[],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("caught:"));
        assert!(r.stdout.contains("not found") || r.stdout.contains("No such file"));
    }

    #[tokio::test]
    async fn test_vfs_mkdir() {
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\nPath('/tmp/newdir').mkdir()\nprint(Path('/tmp/newdir').is_dir())",
            ],
            &[],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\n");
    }

    #[tokio::test]
    async fn test_vfs_iterdir() {
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\nfor p in Path('/data').iterdir():\n    print(p.name)",
            ],
            &[("/data/a.txt", "a"), ("/data/b.txt", "b")],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        // Order from VFS may vary, check both entries present
        assert!(r.stdout.contains("a.txt"));
        assert!(r.stdout.contains("b.txt"));
    }

    #[tokio::test]
    async fn test_vfs_getenv() {
        let r = run_with_vfs(
            &[
                "-c",
                "import os\nprint(os.getenv('MY_VAR'))\nprint(os.getenv('MISSING', 'default'))",
            ],
            &[],
            &[("MY_VAR", "hello")],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello\ndefault\n");
    }

    #[tokio::test]
    async fn test_vfs_stat() {
        let r = run_with_vfs(
            &[
                "-c",
                "from pathlib import Path\ninfo = Path('/tmp/f.txt').stat()\nprint(info.st_size)",
            ],
            &[("/tmp/f.txt", "12345")],
            &[],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "5\n");
    }

    // --- PythonLimits tests ---

    #[tokio::test]
    async fn test_custom_limits_tight_memory() {
        // Very tight memory limit should cause failure for large allocations
        let limits = PythonLimits::default().max_memory(1024);
        let py = Python::with_limits(limits);
        let args = vec!["-c".to_string(), "x = list(range(100000))".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = py.execute(ctx).await.unwrap();
        assert_ne!(r.exit_code, 0, "Tight memory limit should cause failure");
    }

    #[tokio::test]
    async fn test_custom_limits_generous() {
        // Generous limits should succeed
        let limits = PythonLimits::default()
            .max_allocations(10_000_000)
            .max_memory(128 * 1024 * 1024);
        let py = Python::with_limits(limits);
        let args = vec!["-c".to_string(), "print(sum(range(100)))".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = py.execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "4950\n");
    }

    #[test]
    fn test_python_limits_builder() {
        let limits = PythonLimits::default()
            .max_allocations(500)
            .max_duration(Duration::from_secs(10))
            .max_memory(1024)
            .max_recursion(50);
        assert_eq!(limits.max_allocations, 500);
        assert_eq!(limits.max_duration, Duration::from_secs(10));
        assert_eq!(limits.max_memory, 1024);
        assert_eq!(limits.max_recursion, 50);
    }

    #[test]
    fn test_python_limits_default() {
        let limits = PythonLimits::default();
        assert_eq!(limits.max_allocations, 1_000_000);
        assert_eq!(limits.max_duration, Duration::from_secs(30));
        assert_eq!(limits.max_memory, 64 * 1024 * 1024);
        assert_eq!(limits.max_recursion, 200);
    }
}
