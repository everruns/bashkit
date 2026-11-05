//! python/python3 builtin via embedded Monty interpreter (pydantic/monty)
//!
//! # Direct Integration
//!
//! Monty runs directly in the host process. No subprocess, no IPC.
//! Resource limits (memory, time, recursion) are enforced by Monty's own
//! runtime, not by process isolation.
//!
//! # Overview
//!
//! Virtual Python execution with resource limits and VFS access.
//! Python `pathlib.Path` operations and `open()` file handles are bridged to
//! Bashkit's virtual filesystem via Monty's OsCall pause/resume mechanism.
//! No real filesystem or network access.
//!
//! Decision: `open()` performs only VFS open-time effects and returns Monty's
//! virtual file handle. Bashkit never holds host/native Python file handles.
//!
//! Supports: `python -c "code"`, `python script.py`, stdin piping.

use crate::time_compat::Instant;
use async_trait::async_trait;
use chrono::{Datelike, Timelike};
use monty::{MontyRun, RunProgress};
use monty_types::{
    CompileOptions, ExcType, ExtFunctionResult, FileMode, MontyDate, MontyDateTime, MontyException,
    MontyFileHandle, MontyObject, NameLookupResult, OsFunctionCall, PrintWriter, ResourceLimits,
    ResourceTracker, dir_stat, file_stat, symlink_stat,
};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{Builtin, Context, ExecutionDeadline, RuntimeLimits, resolve_path};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::fs::{FileSystem, FileType};
use crate::interpreter::ExecResult;

/// Python's default recursion-depth cap. The other VM limits (duration,
/// memory) default via [`RuntimeLimits::default`].
const DEFAULT_MAX_RECURSION: usize = 200;
// Security decision: virtual Python datetime uses a fixed UTC instant so
// sandboxed code cannot fingerprint host clock/timezone state.
const VIRTUAL_NOW_UNIX_SECS: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z

// TRIAL PORT (monty 1.4): ResourceTracker is a concrete struct; no hooks.
const VIRTUAL_NOW_NANOS: u32 = 123_456_000; // 123456 µs for deterministic microseconds

const PYTHON_INPROCESS_OPT_IN_ENV: &str = "BASHKIT_ALLOW_INPROCESS_PYTHON";

#[derive(Debug, Clone, Copy)]
pub(crate) struct PythonInprocessOptIn(pub bool);

fn python_inprocess_enabled(ctx: &Context<'_>) -> bool {
    ctx.execution_extension::<PythonInprocessOptIn>()
        .is_some_and(|opt_in| opt_in.try_with(|opt_in| opt_in.0).unwrap_or(false))
        // Unit tests in this module build Context::new_for_test without
        // execution extensions. Keep local test ergonomics only under cfg(test).
        || {
            #[cfg(test)]
            {
                let is_enabled = |v: &str| matches!(v, "1" | "true" | "TRUE" | "yes" | "YES");
                ctx.env
                    .get(PYTHON_INPROCESS_OPT_IN_ENV)
                    .is_some_and(|v| is_enabled(v))
            }
            #[cfg(not(test))]
            {
                false
            }
        }
}

/// Resource limits for the embedded Python (Monty) interpreter.
///
/// Use the builder pattern to customize, or `Default` for the standard virtual execution limits:
/// - 30 second timeout
/// - 64 MB memory (also caps collected `print` output)
/// - 200 recursion depth
///
/// There is no allocation-count knob: Monty removed `max_allocations` from its
/// resource limits in 0.0.19, so allocation bombs are contained by `max_memory`
/// and `max_duration` instead.
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
/// The individual limit axes live on [`common`](Self::common) (a shared
/// [`RuntimeLimits`]); read them as e.g. `limits.common.max_memory`. The fluent
/// setters below configure them directly.
#[derive(Debug, Clone)]
pub struct PythonLimits {
    /// Shared VM resource limits (duration, memory, call depth).
    pub common: RuntimeLimits,
}

impl Default for PythonLimits {
    fn default() -> Self {
        Self {
            common: RuntimeLimits {
                // Python defaults to a recursion depth of 200.
                max_call_depth: DEFAULT_MAX_RECURSION,
                ..RuntimeLimits::default()
            },
        }
    }
}

impl PythonLimits {
    /// Set max execution duration.
    #[must_use]
    pub fn max_duration(mut self, d: Duration) -> Self {
        self.common.max_duration = d;
        self
    }

    /// Set max memory in bytes.
    #[must_use]
    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.common.max_memory = bytes;
        self
    }

    /// Set max recursion depth.
    #[must_use]
    pub fn max_recursion(mut self, depth: usize) -> Self {
        self.common.max_call_depth = depth;
        self
    }
}

/// Async handler for external Python function calls.
///
/// Receives `(function_name, positional_args, keyword_args)` directly from monty.
/// Return `ExtFunctionResult::Return(value)` for success or `ExtFunctionResult::Error(exc)` for failure.
///
/// Important security decision: the Python builtin wraps each awaited handler
/// in the remaining [`PythonLimits::max_duration`] budget. Host handlers are
/// trusted code, but should still enforce their own I/O and service limits.
pub type PythonExternalFnHandler = Arc<
    dyn Fn(
            String,
            Vec<MontyObject>,
            Vec<(MontyObject, MontyObject)>,
        ) -> Pin<Box<dyn Future<Output = ExtFunctionResult> + Send>>
        + Send
        + Sync,
>;

/// External function configuration for the Python builtin.
///
/// Groups function names and their async handler together.
/// Configure via [`BashBuilder::python_with_external_handler`](crate::BashBuilder::python_with_external_handler).
#[derive(Clone)]
pub struct PythonExternalFns {
    /// Function names callable from Python (e.g., `"call_tool"`).
    names: Vec<String>,
    /// Async handler invoked when Python calls one of these functions.
    handler: PythonExternalFnHandler,
    prelude: Option<String>,
}

impl std::fmt::Debug for PythonExternalFns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PythonExternalFns")
            .field("names", &self.names)
            .field("handler", &"<fn>")
            .finish()
    }
}

/// The python/python3 builtin command.
///
/// Executes Python code using the embedded Monty interpreter (pydantic/monty).
/// Python `pathlib.Path` and `open()` operations are bridged to Bashkit's VFS
/// — files created by bash (`cat > file`) are readable from Python, and vice
/// versa.
///
/// # Usage
///
/// ```bash
/// python3 -c "print('hello')"
/// python3 script.py
/// echo "print('hello')" | python3
/// python3 -c "print(2 + 2)"       # prints 4 (last expression is not echoed)
/// python3 --version
/// python3 -c "print(open('/tmp/f.txt').read())"
/// ```
pub struct Python {
    /// Resource limits for the Monty interpreter.
    pub limits: PythonLimits,
    /// Optional external function configuration.
    external_fns: Option<PythonExternalFns>,
}

impl Python {
    /// Create with default limits.
    pub fn new() -> Self {
        Self {
            limits: PythonLimits::default(),
            external_fns: None,
        }
    }

    /// Create with custom limits.
    pub fn with_limits(limits: PythonLimits) -> Self {
        Self {
            limits,
            external_fns: None,
        }
    }

    /// Set external function names and handler.
    ///
    /// External functions are callable from Python by name.
    /// When called, execution pauses and the handler is invoked with the raw monty arguments.
    pub fn with_external_handler(
        mut self,
        names: Vec<String>,
        handler: PythonExternalFnHandler,
    ) -> Self {
        self.external_fns = Some(PythonExternalFns {
            names,
            handler,
            prelude: None,
        });
        self
    }

    /// Like [`Self::with_external_handler`], plus a Python prelude evaluated
    /// before user code (e.g. classes that route `tools.get_user(...)` to an
    /// external function). Lets embedders expose namespaced host APIs
    /// without core knowing their shape.
    pub fn with_external_handler_and_prelude(
        mut self,
        names: Vec<String>,
        handler: PythonExternalFnHandler,
        prelude: String,
    ) -> Self {
        self.external_fns = Some(PythonExternalFns {
            names,
            handler,
            prelude: Some(prelude),
        });
        self
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
            "python/python3: Embedded Python (Monty), not CPython. \
             Stdlib: json, math, re, datetime, pathlib, os, sys, typing, csv, collections, \
             itertools, functools, dataclasses, hashlib, base64, random, decimal, unicodedata; \
             no sys.exit/sys.stdin. File I/O via pathlib.Path and open() against the VFS. \
             Simple classes only (no inheritance), no generators (yield). \
             No HTTP/network, subprocess or third-party imports. Output via print().",
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

        if !python_inprocess_enabled(&ctx) {
            return Ok(ExecResult::err(
                format!(
                    "python3: in-process Python disabled by default for security; set {}=1 to enable\n",
                    PYTHON_INPROCESS_OPT_IN_ENV
                ),
                1,
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
                "python3: interactive mode not supported in virtual mode\n".to_string(),
                1,
            ));
        };

        // Charge the source before Monty parses it, so untrusted input is
        // accounted for before any work is done on it.
        ctx.consume_budget_input(code.len())?;
        ctx.consume_budget_work(u64::try_from(code.len().div_ceil(64)).unwrap_or(u64::MAX))?;

        // THREAT[TM-INF]: Python environment access is intentionally scoped
        // to exported variables only (`ctx.env`). Shell-local variables in
        // `ctx.variables` may contain wrapper secrets or internal markers.

        // Clamp Monty's wall-clock budget to the caller's remaining execution
        // deadline (like the TypeScript builtin) so a CPU-bound script cannot
        // overrun a tighter bash timeout — the async timeout cannot preempt
        // Monty's synchronous start/resume sections.
        let mut limits = self.limits.clone();
        if let Some(remaining) = ctx
            .execution_extension::<ExecutionDeadline>()
            .and_then(|deadline| deadline.try_with(ExecutionDeadline::remaining).ok())
        {
            limits.common.max_duration = limits.common.max_duration.min(remaining);
        }
        // Monty's own memory ceiling is per run; heap growth is charged to the
        // shared work budget as it happens (`BudgetTracker::charge_memory_peak`).

        let code = if let Some(prelude) = self
            .external_fns
            .as_ref()
            .and_then(|external| external.prelude.as_ref())
        {
            format!("{prelude}\n{code}")
        } else {
            code
        };
        let policy = PythonExecutionPolicy {
            limits: &limits,
            external_fns: self.external_fns.as_ref(),
            execution_budget: ctx
                .execution_budget()
                .and_then(|budget| budget.try_with(Clone::clone).ok()),
        };
        let execution_budget = ctx
            .execution_budget()
            .and_then(|budget| budget.try_with(Clone::clone).ok());
        let future = run_python(&code, &filename, ctx.fs.clone(), ctx.cwd, ctx.env, policy);
        // Host functions read the request through RuntimeCallContext::current().
        let future = crate::runtime_call::scope(&ctx, future);
        match execution_budget {
            Some(budget) => budget.run(future).await?,
            None => future.await,
        }
    }
}

struct PythonExecutionPolicy<'a> {
    limits: &'a PythonLimits,
    external_fns: Option<&'a PythonExternalFns>,
    execution_budget: Option<crate::limits::ExecutionBudget>,
}

/// Execute Python code via Monty with resource limits and VFS bridging.
///
/// Uses Monty's start/resume API: execution pauses at filesystem operations
/// (OsCall), we bridge them to Bashkit's VFS, then resume.
async fn run_python(
    code: &str,
    filename: &str,
    fs: Arc<dyn FileSystem>,
    cwd: &Path,
    env: &HashMap<String, String>,
    policy: PythonExecutionPolicy<'_>,
) -> Result<ExecResult> {
    let PythonExecutionPolicy {
        limits: py_limits,
        external_fns,
        execution_budget,
    } = policy;
    // Strip shebang if present
    let code = if code.starts_with("#!") {
        match code.find('\n') {
            Some(pos) => &code[pos + 1..],
            None => "",
        }
    } else {
        code
    };

    // Important security decision: end every program with a `None` statement so
    // Monty's result is always `None`. Monty converts the module's final value
    // to a host `MontyObject` outside its memory/time limits; a shared list
    // graph (`x = [x, x]` thirty times) expands to 2^30 leaves there. CPython
    // never echoes the final expression for `-c`, files or stdin, so nothing
    // user-visible is lost. A trailing backslash would join `None` onto the last
    // line, so leave that (already invalid) code alone.
    let code = if code.trim_end().ends_with('\\') {
        code.to_owned()
    } else {
        format!("{code}\nNone\n")
    };
    // Per-VM memory meter + shared-budget checkpoints (python_meter.rs).
    if let Err(msg) = crate::python_meter::ensure_metering() {
        return Ok(ExecResult::err(format!("{msg}\n"), 1));
    }
    let meter = crate::python_meter::VmMeter::new(execution_budget.clone());
    let mut vm_time = Duration::ZERO;
    // Runs one synchronous Monty call inside this VM's metering window.
    macro_rules! in_vm {
        ($e:expr) => {{
            let _window = crate::python_meter::VmWindow::enter(&meter);
            let started = Instant::now();
            let r = $e;
            vm_time += started.elapsed();
            r
        }};
    }
    let runner = match in_vm!(MontyRun::new(
        code,
        filename,
        vec![],
        CompileOptions::default()
    )) {
        Ok(r) => {
            // Security decision: no host clock, no real sleeps (a blocking
            // sleep would also stall the async executor thread).
            let mut r = r.with_os_policy(monty_types::OsPolicy {
                datetime: monty_types::DateTimeSource::Fixed {
                    unix_seconds: VIRTUAL_NOW_UNIX_SECS,
                    microsecond: VIRTUAL_NOW_NANOS / 1000,
                },
                sleep: monty_types::SleepMode::Zero,
                ..monty_types::OsPolicy::default()
            });
            r.set_cwd(&cwd.to_string_lossy());
            r
        }
        Err(e) => return Ok(format_exception(e)),
    };

    let limits = ResourceLimits::default()
        .max_feed_duration(py_limits.common.max_duration)
        .max_memory(py_limits.common.max_memory)
        .max_recursion_depth(py_limits.common.max_call_depth);

    let time_limit_hit = Arc::new(AtomicBool::new(false));
    let tracker = ResourceTracker::new(limits);
    // Important security decision: cap collected print output at the same
    // memory budget as the VM heap. Monty 0.0.19 added a byte cap on
    // `PrintWriter::CollectString` because a `while True: print(...)` loop
    // grows the *host* buffer without touching the VM heap, so an uncapped
    // collector OOMs the host while `max_memory` stays happy. Reusing
    // `max_memory` keeps one number to reason about: tightening the memory
    // limit tightens the output cap with it.
    let print_cap = Some(py_limits.common.max_memory);
    let time_limit = py_limits.common.max_duration;
    // Important security decision: cap awaited host callbacks with the same wall-clock
    // budget as Monty so external functions cannot pin execution between VM steps.
    let python_deadline = Instant::now().checked_add(py_limits.common.max_duration);

    // Run the synchronous start() phase, then extract collected output.
    // PrintWriter::CollectString is not Send, so we scope it to avoid holding across .await.
    let (mut progress, mut buf) = {
        let mut buf = String::new();
        match in_vm!(runner.start(
            vec![],
            tracker,
            PrintWriter::CollectString(&mut buf, print_cap),
        )) {
            Ok(p) => (p, buf),
            Err(e) => {
                if e.exc_type() == ExcType::TimeoutError && vm_time >= time_limit {
                    time_limit_hit.store(true, Ordering::Relaxed);
                }
                return Ok(format_exception_with_output(
                    e,
                    &buf,
                    timed_out(&time_limit_hit, time_limit),
                ));
            }
        }
    };

    loop {
        if let Some(budget) = &execution_budget {
            budget.consume_work(1)?;
        }
        match progress {
            RunProgress::OsCall(os_call) => {
                let function_call = os_call.function_call.clone();
                let result = handle_os_call(function_call, &fs, cwd, env).await;
                match in_vm!(
                    os_call.resume(result, PrintWriter::CollectString(&mut buf, print_cap))
                ) {
                    Ok(next) => {
                        progress = next;
                    }
                    Err(e) => {
                        if e.exc_type() == ExcType::TimeoutError && vm_time >= time_limit {
                            time_limit_hit.store(true, Ordering::Relaxed);
                        }
                        return Ok(format_exception_with_output(
                            e,
                            &buf,
                            timed_out(&time_limit_hit, time_limit),
                        ));
                    }
                }
            }
            RunProgress::FunctionCall(call) => {
                if let Some(budget) = &execution_budget {
                    budget.consume_work(100)?;
                }
                let result = if let Some(ef) = external_fns {
                    call_external_with_deadline(
                        ef,
                        call.function_name.clone(),
                        call.args.args().map(|a| a.to_owned()).collect(),
                        call.args
                            .kwargs()
                            .map(|(k, v)| (k.to_owned(), v.to_owned()))
                            .collect(),
                        python_deadline,
                    )
                    .await
                } else {
                    // No external functions registered; return error
                    ExtFunctionResult::Error(MontyException::new(
                        ExcType::RuntimeError,
                        Some(
                            "no external function handler configured (external functions not enabled)".into(),
                        ),
                    ))
                };

                match in_vm!(call.resume(result, PrintWriter::CollectString(&mut buf, print_cap))) {
                    Ok(next) => {
                        progress = next;
                    }
                    Err(e) => {
                        if e.exc_type() == ExcType::TimeoutError && vm_time >= time_limit {
                            time_limit_hit.store(true, Ordering::Relaxed);
                        }
                        return Ok(format_exception_with_output(
                            e,
                            &buf,
                            timed_out(&time_limit_hit, time_limit),
                        ));
                    }
                }
            }
            RunProgress::NameLookup(lookup) => {
                // External functions are now auto-detected via NameLookup.
                // If the name matches one of our registered external function names,
                // resolve it as a callable; otherwise let Python raise NameError.
                let result = if external_fns
                    .map(|ef| ef.names.contains(&lookup.name))
                    .unwrap_or(false)
                {
                    // Return a callable marker — monty will pause again with
                    // FunctionCall when it's actually invoked.
                    NameLookupResult::Value(MontyObject::function(lookup.name.clone(), None))
                } else {
                    NameLookupResult::Undefined
                };

                match in_vm!(lookup.resume(result, PrintWriter::CollectString(&mut buf, print_cap)))
                {
                    Ok(next) => {
                        progress = next;
                    }
                    Err(e) => {
                        if e.exc_type() == ExcType::TimeoutError && vm_time >= time_limit {
                            time_limit_hit.store(true, Ordering::Relaxed);
                        }
                        return Ok(format_exception_with_output(
                            e,
                            &buf,
                            timed_out(&time_limit_hit, time_limit),
                        ));
                    }
                }
            }
            RunProgress::ResolveFutures(_) => {
                // Async futures not supported in virtual mode
                let err = MontyException::new(
                    ExcType::RuntimeError,
                    Some("async operations not supported in virtual mode".into()),
                );
                return Ok(format_exception_with_output(
                    err,
                    &buf,
                    timed_out(&time_limit_hit, time_limit),
                ));
            }
            RunProgress::Complete(_) => {
                return Ok(ExecResult::ok(buf));
            }
        }
    }
}

fn python_external_timeout_error() -> ExtFunctionResult {
    ExtFunctionResult::Error(MontyException::new(
        ExcType::RuntimeError,
        Some("Python external function exceeded max_duration".into()),
    ))
}

async fn call_external_with_deadline(
    external_fns: &PythonExternalFns,
    function_name: String,
    args: Vec<MontyObject>,
    kwargs: Vec<(MontyObject, MontyObject)>,
    deadline: Option<Instant>,
) -> ExtFunctionResult {
    let Some(deadline) = deadline else {
        // Instant::checked_add overflowed (very large max_duration); run uncapped.
        return (external_fns.handler)(function_name, args, kwargs).await;
    };
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return python_external_timeout_error();
    };
    match crate::time_compat::timeout(
        remaining,
        (external_fns.handler)(function_name, args, kwargs),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => python_external_timeout_error(),
    }
}

// ---------------------------------------------------------------------------
// VFS bridging: Monty OsCall → Bashkit FileSystem
// ---------------------------------------------------------------------------

/// TRIAL PORT (monty 1.4): dispatch on the typed OsFunctionCall enum.
async fn handle_os_call(
    function: OsFunctionCall,
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> ExtFunctionResult {
    use OsFunctionCall as F;
    let rp = |p: &monty_types::MontyPath| resolve_python_path(p.as_str(), cwd);
    let ret = ExtFunctionResult::Return;
    match function {
        F::Getenv(a) => match env.get(&a.key) {
            Some(v) => ret(MontyObject::string(v.clone())),
            None => ret(a.default),
        },
        F::GetEnviron => ret(MontyObject::dict(env.iter().map(|(k, v)| {
            (
                MontyObject::string(k.clone()),
                MontyObject::string(v.clone()),
            )
        }))),
        F::DateToday => handle_date_today(),
        F::DateTimeNow(tz) => handle_datetime_now(tz),
        F::Open(a) => {
            let path = rp(&a.path);
            match open_vfs_file(&path, a.mode, fs).await {
                Ok(()) => ret(MontyObject::file_handle(MontyFileHandle {
                    path: path.to_string_lossy().to_string(),
                    mode: a.mode,
                    position: 0,
                })),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::Exists(p) => ret(MontyObject::bool(fs.exists(&rp(&p)).await.unwrap_or(false))),
        F::IsFile(p) => ret(MontyObject::bool(
            fs.stat(&rp(&p))
                .await
                .map(|m| m.file_type.is_file())
                .unwrap_or(false),
        )),
        F::IsDir(p) => ret(MontyObject::bool(
            fs.stat(&rp(&p))
                .await
                .map(|m| m.file_type.is_dir())
                .unwrap_or(false),
        )),
        F::IsSymlink(p) => ret(MontyObject::bool(
            fs.lstat(&rp(&p))
                .await
                .map(|m| m.file_type.is_symlink())
                .unwrap_or(false),
        )),
        F::ReadText(p) => {
            let path = rp(&p);
            match fs.read_file(&path).await {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => ret(MontyObject::string(s)),
                    Err(_) => ExtFunctionResult::Error(MontyException::new(
                        ExcType::OSError,
                        Some(format!(
                            "can't decode '{}': not valid UTF-8",
                            path.display()
                        )),
                    )),
                },
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::ReadBytes(p) => {
            let path = rp(&p);
            match fs.read_file(&path).await {
                Ok(bytes) => ret(MontyObject::bytes(bytes)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::WriteText(a) => {
            let path = rp(&a.path);
            match fs.write_file(&path, a.data.as_bytes()).await {
                Ok(()) => ret(MontyObject::int(a.data.chars().count() as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::AppendText(a) => {
            let path = rp(&a.path);
            match fs.append_file(&path, a.data.as_bytes()).await {
                Ok(()) => ret(MontyObject::int(a.data.chars().count() as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::WriteBytes(a) => {
            let path = rp(&a.path);
            match fs.write_file(&path, &a.data).await {
                Ok(()) => ret(MontyObject::int(a.data.len() as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::AppendBytes(a) => {
            let path = rp(&a.path);
            match fs.append_file(&path, &a.data).await {
                Ok(()) => ret(MontyObject::int(a.data.len() as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::Mkdir(a) => {
            let path = rp(&a.path);
            match fs.mkdir(&path, a.parents).await {
                Ok(()) => ret(MontyObject::none()),
                Err(e) if a.exist_ok && e.to_string().contains("already exists") => {
                    ret(MontyObject::none())
                }
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::Unlink(p) | F::Rmdir(p) => {
            let path = rp(&p);
            match fs.remove(&path, false).await {
                Ok(()) => ret(MontyObject::none()),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::Iterdir(p) => {
            let path = rp(&p);
            match fs.read_dir(&path).await {
                Ok(entries) => ret(MontyObject::list(entries.into_iter().map(|e| {
                    MontyObject::path(vfs_join(&path, &e.name).to_string_lossy().to_string())
                }))),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::Stat(p) => {
            let path = rp(&p);
            match fs.stat(&path).await {
                Ok(meta) => {
                    let mtime = meta
                        .modified
                        .duration_since(crate::time_compat::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    ret(match meta.file_type {
                        FileType::Directory => dir_stat(meta.mode as i64, mtime),
                        FileType::Symlink => symlink_stat(meta.mode as i64, mtime),
                        _ => file_stat(meta.mode as i64, meta.size as i64, mtime),
                    })
                }
                Err(e) => map_vfs_error(e, &path),
            }
        }
        F::Rename(a) => {
            let (src, dst) = (rp(&a.src), rp(&a.dst));
            match fs.rename(&src, &dst).await {
                Ok(()) => ret(MontyObject::path(dst.to_string_lossy().to_string())),
                Err(e) => map_vfs_error(e, &src),
            }
        }
        F::Resolve(p) | F::Absolute(p) => {
            ret(MontyObject::path(rp(&p).to_string_lossy().to_string()))
        }
        F::Scan(a) => handle_scan(&a, &rp(&a.path), fs).await,
        // Unknown modules: no host modules are offered.
        F::Import(name) => ExtFunctionResult::NotFound(name),
        other => ExtFunctionResult::Error(MontyException::new(
            ExcType::OSError,
            Some(format!("{} not supported in virtual mode", other.name())),
        )),
    }
}

/// Upper bound on entries one `Path.scan` reply may carry (`os.walk`,
/// `glob`, `scandir`). Security decision: the reply is materialised on the host
/// before Monty sees it, so it is capped independently of the VFS size.
const MAX_SCAN_ENTRIES: usize = 100_000;

/// Answer `Path.scan` with the VFS subtree under `root` to `max_depth`.
///
/// Decision: reply with the plain tree and let Monty run the glob selector
/// itself (`ScanReply::Tree`); a superset is always correct. Symlinks are
/// reported but never descended, since the VFS has no symlink resolution here.
async fn handle_scan(
    args: &monty_types::ScanArgs,
    root: &Path,
    fs: &Arc<dyn FileSystem>,
) -> ExtFunctionResult {
    use monty_types::scan::{EntryInfo, ScanEntry, scan_reply};
    let info = |t: FileType| EntryInfo {
        is_dir: t.is_dir(),
        is_file: t.is_file(),
        is_symlink: t.is_symlink(),
    };
    let root_meta = match fs.stat(root).await {
        Ok(m) => m,
        Err(e) => return map_vfs_error(e, root),
    };
    let mut entries = vec![ScanEntry {
        path: String::new(),
        info: info(root_meta.file_type),
    }];
    let mut queue = std::collections::VecDeque::new();
    if root_meta.file_type.is_dir() {
        queue.push_back((root.to_path_buf(), String::new(), 0u32));
    }
    while let Some((dir, rel, depth)) = queue.pop_front() {
        if args.max_depth.is_some_and(|max| depth >= max) {
            continue;
        }
        let children = match fs.read_dir(&dir).await {
            Ok(c) => c,
            Err(e) => return map_vfs_error(e, &dir),
        };
        for child in children {
            if entries.len() >= MAX_SCAN_ENTRIES {
                return ExtFunctionResult::Error(MontyException::new(
                    ExcType::OSError,
                    Some(format!("directory scan exceeds {MAX_SCAN_ENTRIES} entries")),
                ));
            }
            let child_rel = if rel.is_empty() {
                child.name.clone()
            } else {
                format!("{rel}/{}", child.name)
            };
            let kind = child.metadata.file_type;
            if kind.is_dir() {
                queue.push_back((vfs_join(&dir, &child.name), child_rel.clone(), depth + 1));
            }
            entries.push(ScanEntry {
                path: child_rel,
                info: info(kind),
            });
        }
    }
    ExtFunctionResult::Return(scan_reply(entries))
}

async fn open_vfs_file(path: &Path, mode: FileMode, fs: &Arc<dyn FileSystem>) -> Result<()> {
    match mode {
        FileMode::Read(_) | FileMode::ReadUpdate(_) => {
            let meta = fs.stat(path).await?;
            if meta.file_type.is_dir() {
                return Err(std::io::Error::other("is a directory").into());
            }
        }
        FileMode::Write(_) | FileMode::WriteUpdate(_) => {
            fs.write_file(path, b"").await?;
        }
        FileMode::Append(_) | FileMode::AppendUpdate(_) => {
            if fs.exists(path).await.unwrap_or(false) {
                let meta = fs.stat(path).await?;
                if meta.file_type.is_dir() {
                    return Err(std::io::Error::other("is a directory").into());
                }
            } else {
                fs.write_file(path, b"").await?;
            }
        }
    }
    Ok(())
}

/// Resolve a Python path string against cwd if relative.
fn resolve_python_path(path_str: &str, cwd: &Path) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_owned()
    } else {
        vfs_join(cwd, p)
    }
}

/// Map a Bashkit VFS error to a Python exception via ExtFunctionResult.
fn map_vfs_error(e: crate::Error, path: &Path) -> ExtFunctionResult {
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

    ExtFunctionResult::Error(MontyException::new(
        exc_type,
        Some(format!("[Errno {errno}] {msg}: '{path_str}'")),
    ))
}

/// Handle datetime.date.today() using the sandbox virtual clock.
fn handle_date_today() -> ExtFunctionResult {
    let now = virtual_now_utc();
    ExtFunctionResult::Return(MontyObject::date(MontyDate {
        year: now.year(),
        month: now.month() as u8,
        day: now.day() as u8,
    }))
}

/// Handle datetime.datetime.now(tz=None) using the sandbox virtual clock.
///
/// If tz is None, returns a naive datetime (no timezone info).
/// If tz is a TimeZone, returns an aware datetime at that offset.
fn handle_datetime_now(tz: Option<monty_types::MontyTimeZone>) -> ExtFunctionResult {
    let base_utc = virtual_now_utc();

    if let Some(tz) = tz {
        // Aware datetime at the requested offset
        let offset = chrono::FixedOffset::east_opt(tz.offset_seconds)
            .unwrap_or(chrono::FixedOffset::east_opt(0).expect("UTC offset is always valid"));
        let dt = base_utc.with_timezone(&offset);
        ExtFunctionResult::Return(MontyObject::datetime(MontyDateTime {
            year: dt.year(),
            month: dt.month() as u8,
            day: dt.day() as u8,
            hour: dt.hour() as u8,
            minute: dt.minute() as u8,
            second: dt.second() as u8,
            microsecond: dt.nanosecond() / 1000,
            offset_seconds: Some(tz.offset_seconds),
            timezone_name: tz.name,
        }))
    } else {
        // No timezone → naive UTC datetime (no timezone metadata)
        let dt = base_utc.naive_utc();
        ExtFunctionResult::Return(MontyObject::datetime(MontyDateTime {
            year: dt.year(),
            month: dt.month() as u8,
            day: dt.day() as u8,
            hour: dt.hour() as u8,
            minute: dt.minute() as u8,
            second: dt.second() as u8,
            microsecond: dt.nanosecond() / 1000,
            offset_seconds: None,
            timezone_name: None,
        }))
    }
}

fn virtual_now_utc() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(VIRTUAL_NOW_UNIX_SECS, VIRTUAL_NOW_NANOS)
        .expect("virtual timestamp constant must be valid")
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

/// Format a MontyException into an ExecResult with exit code 1.
fn format_exception(e: MontyException) -> ExecResult {
    ExecResult::err(format!("{e}\n"), 1)
}

/// The limit to report when Monty's own time limit fired, else `None`.
fn timed_out(hit: &AtomicBool, limit: Duration) -> Option<Duration> {
    hit.load(Ordering::Relaxed).then_some(limit)
}

/// Format exception, preserving any output produced before the error.
///
/// Decision: Monty's time limit reports like the CPython runtime and coreutils
/// `timeout` (exit 124, `python3: execution timed out after Ns`), so callers can
/// tell a limit from a script error without parsing a traceback. Monty raises
/// the limit as an uncatchable `TimeoutError`; the tracker records that its
/// check fired, because script code can raise a `TimeoutError` with the same
/// message.
fn format_exception_with_output(
    e: MontyException,
    printed: &str,
    time_limit: Option<Duration>,
) -> ExecResult {
    if let Some(time_limit) = time_limit
        && e.exc_type() == ExcType::TimeoutError
    {
        let mut result = ExecResult::with_code(printed.to_string(), 124);
        result.stderr = format!(
            "python3: execution timed out after {:.1}s\n",
            time_limit.as_secs_f64()
        )
        .into();
        return result;
    }
    let stderr = format!("{e}\n");
    let mut result = ExecResult::err(stderr, 1);
    if !printed.is_empty() {
        result.stdout = printed.to_string().into();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::Context;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn opt_in_env() -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert(PYTHON_INPROCESS_OPT_IN_ENV.to_string(), "1".to_string());
        env
    }

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = opt_in_env();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, stdin);
        Python::new().execute(ctx).await.unwrap()
    }

    async fn run_with_file(args: &[&str], file_path: &str, content: &str) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = opt_in_env();
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
        let mut env: HashMap<String, String> = env_vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        env.insert(PYTHON_INPROCESS_OPT_IN_ENV.to_string(), "1".to_string());
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

    // --- Basic functionality tests ---

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
    async fn test_inline_expression_is_not_echoed() {
        // Matches CPython: `python3 -c "2 + 3"` prints nothing. Echoing made
        // Monty export the final value to the host, unbounded by its limits.
        let r = run(&["-c", "2 + 3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "");
    }

    #[tokio::test]
    async fn test_large_shared_final_value_finishes() {
        // Regression: a 2^30-leaf shared list as the last expression expanded
        // to ~500 MiB on the host and ignored the timeout.
        let r = run(
            &["-c", "x = [1]\nfor _ in range(30):\n    x = [x, x]\nx"],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "");
    }

    #[tokio::test]
    async fn test_trailing_backslash_stays_a_syntax_error() {
        let r = run(&["-c", "x = 1 + \\"], None).await;
        assert_ne!(r.exit_code, 0);
        assert!(r.stderr.contains("Error"), "{}", r.stderr);
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
        let r = run(&["-c", "print([x*2 for x in range(3)])"], None).await;
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
        let env = opt_in_env();
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
        let limits = PythonLimits::default().max_memory(128 * 1024 * 1024);
        let py = Python::with_limits(limits);
        let args = vec!["-c".to_string(), "print(sum(range(100)))".to_string()];
        let env = opt_in_env();
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
            .max_duration(Duration::from_secs(10))
            .max_memory(1024)
            .max_recursion(50);
        assert_eq!(limits.common.max_duration, Duration::from_secs(10));
        assert_eq!(limits.common.max_memory, 1024);
        assert_eq!(limits.common.max_call_depth, 50);
    }

    #[test]
    fn test_python_limits_default() {
        let limits = PythonLimits::default();
        assert_eq!(limits.common.max_duration, Duration::from_secs(30));
        assert_eq!(limits.common.max_memory, 64 * 1024 * 1024);
        assert_eq!(limits.common.max_call_depth, 200);
    }

    // --- External function tests ---

    /// Helper: run Python with an external function handler.
    async fn run_with_external(
        code: &str,
        fn_names: &[&str],
        handler: PythonExternalFnHandler,
    ) -> ExecResult {
        run_with_external_and_limits(code, fn_names, handler, PythonLimits::default()).await
    }

    async fn run_with_external_and_limits(
        code: &str,
        fn_names: &[&str],
        handler: PythonExternalFnHandler,
        limits: PythonLimits,
    ) -> ExecResult {
        let args = vec!["-c".to_string(), code.to_string()];
        let env = opt_in_env();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let py = Python::with_limits(limits)
            .with_external_handler(fn_names.iter().map(|s| s.to_string()).collect(), handler);
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        py.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_external_fn_return_value() {
        let handler: PythonExternalFnHandler = Arc::new(|_name, _args, _kwargs| {
            Box::pin(async { ExtFunctionResult::Return(MontyObject::int(42)) })
        });
        let r = run_with_external("print(get_answer())", &["get_answer"], handler).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "42\n");
    }

    #[tokio::test]
    async fn test_external_fn_handler_timeout_uses_python_deadline() {
        let handler: PythonExternalFnHandler = Arc::new(|_name, _args, _kwargs| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                ExtFunctionResult::Return(MontyObject::int(1))
            })
        });
        let started = Instant::now();
        let r = tokio::time::timeout(
            Duration::from_secs(1),
            run_with_external_and_limits(
                "print(slow())",
                &["slow"],
                handler,
                PythonLimits::default().max_duration(Duration::from_millis(25)),
            ),
        )
        .await
        .expect("Python external call should observe max_duration");

        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("RuntimeError"));
        assert!(r.stderr.contains("exceeded max_duration"));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_external_fn_with_args() {
        let handler: PythonExternalFnHandler = Arc::new(|_name, args, _kwargs| {
            Box::pin(async move {
                let a = args[0].as_ref().as_int().unwrap_or(0);
                let b = args[1].as_ref().as_int().unwrap_or(0);
                ExtFunctionResult::Return(MontyObject::int(a + b))
            })
        });
        let r = run_with_external("print(add(3, 4))", &["add"], handler).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "7\n");
    }

    #[tokio::test]
    async fn test_external_fn_with_kwargs() {
        let handler: PythonExternalFnHandler = Arc::new(|_name, _args, kwargs| {
            Box::pin(async move {
                for (k, v) in &kwargs {
                    if let (Some(key), Some(val)) = (k.as_ref().as_str(), v.as_ref().as_str())
                        && key == "name"
                    {
                        return ExtFunctionResult::Return(MontyObject::string(format!(
                            "hello {val}"
                        )));
                    }
                }
                ExtFunctionResult::Return(MontyObject::string("hello unknown"))
            })
        });
        let r = run_with_external("print(greet(name='world'))", &["greet"], handler).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_external_fn_error() {
        let handler: PythonExternalFnHandler = Arc::new(|_name, _args, _kwargs| {
            Box::pin(async {
                ExtFunctionResult::Error(MontyException::new(
                    ExcType::RuntimeError,
                    Some("something went wrong".into()),
                ))
            })
        });
        let r = run_with_external("fail()", &["fail"], handler).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("RuntimeError"));
        assert!(r.stderr.contains("something went wrong"));
    }

    #[tokio::test]
    async fn test_external_fn_caught_error() {
        let handler: PythonExternalFnHandler = Arc::new(|_name, _args, _kwargs| {
            Box::pin(async {
                ExtFunctionResult::Error(MontyException::new(
                    ExcType::ValueError,
                    Some("bad value".into()),
                ))
            })
        });
        let r = run_with_external(
            "try:\n    fail()\nexcept ValueError as e:\n    print(f'caught: {e}')",
            &["fail"],
            handler,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("caught:"));
        assert!(r.stdout.contains("bad value"));
    }

    #[tokio::test]
    async fn test_external_fn_multiple_calls() {
        let counter = Arc::new(std::sync::atomic::AtomicI64::new(0));
        let counter_clone = counter.clone();
        let handler: PythonExternalFnHandler = Arc::new(move |_name, _args, _kwargs| {
            let c = counter_clone.clone();
            Box::pin(async move {
                let val = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                ExtFunctionResult::Return(MontyObject::int(val))
            })
        });
        let r = run_with_external(
            "a = next_id()\nb = next_id()\nc = next_id()\nprint(a, b, c)",
            &["next_id"],
            handler,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "0 1 2\n");
    }

    #[tokio::test]
    async fn test_external_fn_returns_string() {
        let handler: PythonExternalFnHandler = Arc::new(|_name, args, _kwargs| {
            Box::pin(async move {
                let input = args[0].as_ref().as_str().unwrap_or_default().to_owned();
                ExtFunctionResult::Return(MontyObject::string(input.to_uppercase()))
            })
        });
        let r = run_with_external("print(upper('hello'))", &["upper"], handler).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "HELLO\n");
    }

    #[tokio::test]
    async fn test_external_fn_dispatches_by_name() {
        let handler: PythonExternalFnHandler = Arc::new(|name, _args, _kwargs| {
            Box::pin(async move {
                let result = match name.as_str() {
                    "get_x" => MontyObject::int(10),
                    "get_y" => MontyObject::int(20),
                    _ => MontyObject::none(),
                };
                ExtFunctionResult::Return(result)
            })
        });
        let r = run_with_external("print(get_x() + get_y())", &["get_x", "get_y"], handler).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "30\n");
    }

    #[tokio::test]
    async fn test_unregistered_name_reference_raises_name_error() {
        // Referencing a name (not as a call) that is NOT in the registered
        // external function list should raise NameError via NameLookup → Undefined.
        let handler: PythonExternalFnHandler = Arc::new(|_name, _args, _kwargs| {
            Box::pin(async { ExtFunctionResult::Return(MontyObject::int(1)) })
        });
        // Register "registered_fn" but reference "unknown_var" (not a call)
        let r = run_with_external("x = unknown_var", &["registered_fn"], handler).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("NameError"));
    }

    // --- Monty 0.0.8 feature tests ---

    #[tokio::test]
    async fn test_math_module() {
        let r = run(&["-c", "import math; print(math.sqrt(144))"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "12.0");
    }

    #[tokio::test]
    async fn test_llm_hint_matches_runtime() {
        let hint = Python::new().llm_hint().unwrap();
        let stdlib = hint
            .split("Stdlib: ")
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .unwrap();
        for module in stdlib.split(", ") {
            let module = module.split('.').next().unwrap();
            let r = run(&["-c", &format!("import {module}")], None).await;
            assert_eq!(r.exit_code, 0, "hint names {module}: {}", r.stderr);
        }
        assert!(hint.contains("Simple classes only (no inheritance)"));
        let r = run(
            &[
                "-c",
                "class A:\n    def __init__(self):\n        self.v = 1\nprint(A().v)",
            ],
            None,
        )
        .await;
        assert_eq!(r.stdout, "1\n", "{}", r.stderr);
        let r = run(&["-c", "class A: pass\nclass B(A): pass"], None).await;
        assert_ne!(r.exit_code, 0, "inheritance now works; update the hint");
        assert!(hint.contains("no generators (yield)"));
        let r = run(&["-c", "def g():\n    yield 1\nlist(g())"], None).await;
        assert_ne!(r.exit_code, 0, "generators now work; update the hint");
        assert!(hint.contains("no sys.exit/sys.stdin"));
        let r = run(&["-c", "import sys\nsys.exit(0)"], None).await;
        assert_ne!(r.exit_code, 0, "sys.exit now works; update the hint");
    }

    #[tokio::test]
    async fn test_re_module_works() {
        let r = run(
            &[
                "-c",
                "import re; m = re.search(r'(\\d+)', 'abc123def'); print(m.group(1), re.sub(r'a+', 'x', 'caab'))",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "123 cxb\n");
    }

    #[tokio::test]
    async fn test_filter_builtin() {
        let r = run(
            &["-c", "print(list(filter(lambda x: x > 2, [1, 2, 3, 4])))"],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "[3, 4]");
    }

    #[tokio::test]
    async fn test_getattr_builtin() {
        // getattr with default value fallback
        let r = run(
            &["-c", "print(getattr('hello', 'missing_attr', 'default'))"],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "default");
    }

    #[tokio::test]
    async fn test_tuple_comparison() {
        let r = run(&["-c", "print((1, 2) < (1, 3))"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "True");
    }

    #[tokio::test]
    async fn test_pep448_unpacking() {
        let r = run(&["-c", "a = [1, 2]; b = [3, 4]; print([*a, *b])"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "[1, 2, 3, 4]");
    }

    #[tokio::test]
    async fn test_dict_constructor_from_iterable() {
        let r = run(
            &[
                "-c",
                "d = dict([('a', 1), ('b', 2)]); print(d['a'], d['b'])",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "1 2");
    }

    // --- datetime tests (Monty 0.0.11+) ---

    #[tokio::test]
    async fn test_date_today() {
        let r = run(
            &[
                "-c",
                "from datetime import date\nd = date.today()\nprint(f'{d.year}-{d.month}-{d.day}')",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2024-1-1");
    }

    #[tokio::test]
    async fn test_datetime_now_naive() {
        let r = run(
            &[
                "-c",
                "from datetime import datetime\ndt = datetime.now()\nprint(f'{dt.year}-{dt.month}-{dt.day} {dt.hour}:{dt.minute}:{dt.second}.{dt.microsecond}')",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2024-1-1 0:0:0.123456");
    }

    #[tokio::test]
    async fn test_datetime_now_utc() {
        let r = run(
            &[
                "-c",
                "from datetime import datetime, timezone\ndt = datetime.now(timezone.utc)\nprint(f'{dt.year}-{dt.month}-{dt.day} {dt.hour}:{dt.minute}:{dt.second}.{dt.microsecond}')",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2024-1-1 0:0:0.123456");
    }

    #[tokio::test]
    async fn test_datetime_attributes() {
        // Verify datetime components are populated correctly
        let r = run(
            &[
                "-c",
                "from datetime import datetime\ndt = datetime.now()\nassert 1 <= dt.month <= 12\nassert 1 <= dt.day <= 31\nassert 0 <= dt.hour <= 23\nprint('ok')",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "ok");
    }

    // --- json tests (Monty 0.0.9+) ---

    #[tokio::test]
    async fn test_json_dumps_loads() {
        let r = run(
            &[
                "-c",
                "import json\nd = {'a': 1, 'b': [2, 3]}\ns = json.dumps(d)\nprint(json.loads(s) == d)",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "True");
    }
}
