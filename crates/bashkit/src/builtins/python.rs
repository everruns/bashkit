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
    CompileOptions, ExcType, ExtFunctionResult, FileMode, LimitedTracker, MontyDate, MontyDateTime,
    MontyException, MontyFileHandle, MontyObject, NameLookupResult, OsFunctionCall, PrintWriter,
    ResourceError, ResourceLimits, ResourceTracker, dir_stat, file_stat, symlink_stat,
};
use std::cell::Cell;
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
// Security hard-stop: catastrophic regex backtracking can bypass cooperative
// interpreter budget checks, so disable regex stdlib module in untrusted code.
const DISABLED_STDLIB_MODULES: &[&str] = &["re"];
/// Monty heap bytes per shared work unit (see `BudgetTracker::charge_memory_peak`).
const PYTHON_MEMORY_BYTES_PER_WORK_UNIT: usize = 64;
// Security decision: virtual Python datetime uses a fixed UTC instant so
// sandboxed code cannot fingerprint host clock/timezone state.
const VIRTUAL_NOW_UNIX_SECS: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z

/// Bridges Monty's statement/allocation checkpoints into the shared request
/// budget while retaining Monty's own memory/time/recursion ceilings.
#[derive(Debug)]
struct BudgetTracker {
    runtime: LimitedTracker,
    execution: Option<crate::limits::ExecutionBudget>,
    vm_checkpoints: Cell<u64>,
    /// Highest Monty heap size already charged to `execution`, in bytes.
    charged_memory: Cell<usize>,
    /// Set when Monty's own time limit fired. Script code can raise a
    /// look-alike `TimeoutError`, so only this flag marks a real limit hit.
    time_limit_hit: Arc<AtomicBool>,
}

impl BudgetTracker {
    fn budget_error(err: crate::limits::LimitExceeded) -> ResourceError {
        // Monty's tracker error type has no host-defined variant. The shared
        // budget retains the precise poisoned reason; use an uncatchable
        // memory error only to stop the VM at this checkpoint.
        let _ = err;
        ResourceError::Memory { limit: 0, used: 1 }
    }

    fn consume_work(&self) -> std::result::Result<(), ResourceError> {
        if let Some(budget) = &self.execution {
            budget.consume_work(1).map_err(Self::budget_error)?;
        }
        Ok(())
    }

    /// Charge heap growth past the previous high-water mark, one work unit per
    /// [`PYTHON_MEMORY_BYTES_PER_WORK_UNIT`] bytes.
    ///
    /// Important security decision: a run pays for the peak memory it really
    /// uses, so repeated invocations still share one budget (they cannot each
    /// take a fresh `max_memory`), while small runs stay cheap. Reserving
    /// `max_memory / 64` per call up front capped a default script at ~95
    /// `python3` calls even for `pass`.
    fn charge_memory_peak(&self) -> std::result::Result<(), ResourceError> {
        let Some(budget) = &self.execution else {
            return Ok(());
        };
        let current = self.runtime.current_memory();
        let charged = self.charged_memory.get();
        if current <= charged {
            return Ok(());
        }
        let units = current.div_ceil(PYTHON_MEMORY_BYTES_PER_WORK_UNIT)
            - charged.div_ceil(PYTHON_MEMORY_BYTES_PER_WORK_UNIT);
        self.charged_memory.set(current);
        budget
            .consume_work(u64::try_from(units).unwrap_or(u64::MAX))
            .map_err(Self::budget_error)
    }

    fn check_vm(&self) -> std::result::Result<(), ResourceError> {
        if let Some(budget) = &self.execution {
            budget.check().map_err(Self::budget_error)?;
            let checkpoints = self.vm_checkpoints.get().wrapping_add(1);
            self.vm_checkpoints.set(checkpoints);
            if checkpoints.is_multiple_of(64) {
                budget.consume_work(1).map_err(Self::budget_error)?;
            }
        }
        Ok(())
    }
}

impl ResourceTracker for BudgetTracker {
    fn on_free(&self, get_size: impl FnOnce() -> usize) {
        self.runtime.on_free(get_size);
    }

    fn check_time(&self) -> std::result::Result<(), ResourceError> {
        self.check_vm()?;
        let result = self.runtime.check_time();
        if matches!(result, Err(ResourceError::Time { .. })) {
            self.time_limit_hit.store(true, Ordering::Relaxed);
        }
        result
    }

    fn check_recursion_depth(
        &self,
        current_depth: usize,
    ) -> std::result::Result<(), ResourceError> {
        self.runtime.check_recursion_depth(current_depth)
    }

    fn check_large_result(&self, estimated_bytes: usize) -> std::result::Result<(), ResourceError> {
        self.runtime.check_large_result(estimated_bytes)
    }

    fn on_grow(
        &self,
        additional_bytes: impl FnOnce() -> usize,
    ) -> std::result::Result<(), ResourceError> {
        self.consume_work()?;
        self.runtime.on_grow(additional_bytes)?;
        self.charge_memory_peak()
    }

    fn gc_interval(&self) -> Option<usize> {
        self.runtime.gc_interval()
    }

    fn on_execution_start(&self) {
        self.runtime.on_execution_start();
    }

    fn on_execution_stop(&self) {
        self.runtime.on_execution_stop();
    }
}
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

// Important security decision: find imports by parsing, not by scanning lines.
// A line-prefix scan missed `pass; import re`, `if x: import re` and any other
// statement that puts the import after a prefix. Monty 0.0.19 has no import
// hook, so bashkit parses with the same parser Monty runs first
// (`ruff_python_parser::parse_module`) and walks every statement body.
// Monty has no `__import__` or `importlib`, so only `import` / `from` statements
// can load a module. Code that fails to parse is let through: Monty parses it
// the same way, reports the SyntaxError and runs nothing.
fn is_disabled_import(code: &str) -> Option<&'static str> {
    use ruff_python_ast::statement_visitor::{StatementVisitor, walk_stmt};
    use ruff_python_ast::{Stmt, StmtImport, StmtImportFrom};

    fn disabled(module: &str) -> Option<&'static str> {
        DISABLED_STDLIB_MODULES.iter().copied().find(|disabled| {
            module
                .strip_prefix(disabled)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
    }

    struct ImportFinder(Option<&'static str>);

    impl<'a> StatementVisitor<'a> for ImportFinder {
        fn visit_stmt(&mut self, stmt: &'a Stmt) {
            if self.0.is_some() {
                return;
            }
            match stmt {
                Stmt::Import(StmtImport { names, .. }) => {
                    self.0 = names.iter().find_map(|alias| disabled(alias.name.as_str()));
                }
                Stmt::ImportFrom(StmtImportFrom { module, names, .. }) => {
                    self.0 = match module {
                        Some(module) => disabled(module.as_str()),
                        // `from . import re`
                        None => names.iter().find_map(|alias| disabled(alias.name.as_str())),
                    };
                }
                _ => walk_stmt(self, stmt),
            }
        }
    }

    let parsed = ruff_python_parser::parse_module(code).ok()?;
    let mut finder = ImportFinder(None);
    finder.visit_body(&parsed.syntax().body);
    finder.0
}

#[async_trait]
impl Builtin for Python {
    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "python/python3: Embedded Python (Monty), not CPython. \
             Stdlib: json, math, datetime, pathlib, os.getenv, sys, typing, unicodedata; \
             `re` is disabled. File I/O via pathlib.Path and open() against the VFS. \
             Simple classes only (no inheritance), no match statements. \
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

        if let Some(module) = is_disabled_import(&code) {
            return Ok(ExecResult::err(
                format!(
                    "python3: importing module '{module}' is disabled for security reasons (regex DoS risk)\n"
                ),
                1,
            ));
        }
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
    let runner = match MontyRun::new(code, filename, vec![], CompileOptions::default()) {
        Ok(r) => r,
        Err(e) => return Ok(format_exception(e)),
    };

    let limits = ResourceLimits::new()
        .max_duration(py_limits.common.max_duration)
        .max_memory(py_limits.common.max_memory)
        .max_recursion_depth(Some(py_limits.common.max_call_depth));

    let time_limit_hit = Arc::new(AtomicBool::new(false));
    let tracker = BudgetTracker {
        runtime: LimitedTracker::new(limits),
        execution: execution_budget.clone(),
        vm_checkpoints: Cell::new(0),
        charged_memory: Cell::new(0),
        time_limit_hit: time_limit_hit.clone(),
    };
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
        match runner.start(
            vec![],
            tracker,
            PrintWriter::CollectString(&mut buf, print_cap),
        ) {
            Ok(p) => (p, buf),
            Err(e) => {
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
                match os_call.resume(result, PrintWriter::CollectString(&mut buf, print_cap)) {
                    Ok(next) => {
                        progress = next;
                    }
                    Err(e) => {
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
                        call.args.clone(),
                        call.kwargs.clone(),
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

                match call.resume(result, PrintWriter::CollectString(&mut buf, print_cap)) {
                    Ok(next) => {
                        progress = next;
                    }
                    Err(e) => {
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
                    NameLookupResult::Value(MontyObject::Function {
                        name: lookup.name.clone(),
                        docstring: None,
                    })
                } else {
                    NameLookupResult::Undefined
                };

                match lookup.resume(result, PrintWriter::CollectString(&mut buf, print_cap)) {
                    Ok(next) => {
                        progress = next;
                    }
                    Err(e) => {
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

/// Monty's `OsFunctionCall` is a tagged enum carrying typed args. We project it
/// to the generic `(positional, keyword)` `MontyObject` view via the public
/// `to_args()` and dispatch on the stable `name()` string (kept byte-identical
/// across monty releases for snapshot compatibility), keeping the per-op VFS
/// logic unchanged.
async fn handle_os_call(
    function: OsFunctionCall,
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> ExtFunctionResult {
    let name = function.name();
    let (args, kwargs) = function.to_args();
    let args = args.as_slice();
    let kwargs = kwargs.as_slice();

    // Non-filesystem operations: env access, date/time
    match name {
        "os.getenv" => return handle_getenv(args, env),
        "os.environ" => return handle_get_environ(env),
        "date.today" => return handle_date_today(),
        "datetime.now" => return handle_datetime_now(args),
        _ => {}
    }

    // All other ops need a path as first arg
    let path = match extract_path(args, cwd) {
        Some(p) => p,
        None => {
            return ExtFunctionResult::Error(MontyException::new(
                ExcType::TypeError,
                Some("expected path argument".into()),
            ));
        }
    };

    match name {
        "open" => {
            let mode = match parse_open_mode(args) {
                Ok(mode) => mode,
                Err(err) => return err,
            };
            match open_vfs_file(&path, mode, fs).await {
                Ok(()) => ExtFunctionResult::Return(MontyObject::FileHandle(MontyFileHandle {
                    path: path.to_string_lossy().to_string(),
                    mode,
                    position: 0,
                })),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        "Path.exists" => {
            let exists = fs.exists(&path).await.unwrap_or(false);
            ExtFunctionResult::Return(MontyObject::Bool(exists))
        }
        "Path.is_file" => match fs.stat(&path).await {
            Ok(meta) => ExtFunctionResult::Return(MontyObject::Bool(meta.file_type.is_file())),
            Err(_) => ExtFunctionResult::Return(MontyObject::Bool(false)),
        },
        "Path.is_dir" => match fs.stat(&path).await {
            Ok(meta) => ExtFunctionResult::Return(MontyObject::Bool(meta.file_type.is_dir())),
            Err(_) => ExtFunctionResult::Return(MontyObject::Bool(false)),
        },
        "Path.is_symlink" => match fs.lstat(&path).await {
            Ok(meta) => ExtFunctionResult::Return(MontyObject::Bool(meta.file_type.is_symlink())),
            Err(_) => ExtFunctionResult::Return(MontyObject::Bool(false)),
        },
        "Path.read_text" => match fs.read_file(&path).await {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => ExtFunctionResult::Return(MontyObject::String(s)),
                Err(_) => ExtFunctionResult::Error(MontyException::new(
                    ExcType::OSError,
                    Some(format!(
                        "can't decode '{}': not valid UTF-8",
                        path.display()
                    )),
                )),
            },
            Err(e) => map_vfs_error(e, &path),
        },
        "Path.read_bytes" => match fs.read_file(&path).await {
            Ok(bytes) => ExtFunctionResult::Return(MontyObject::Bytes(bytes)),
            Err(e) => map_vfs_error(e, &path),
        },
        "Path.write_text" => {
            let content = match args.get(1) {
                Some(MontyObject::String(s)) => s.as_bytes().to_vec(),
                _ => {
                    return ExtFunctionResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("write_text() requires a string argument".into()),
                    ));
                }
            };
            let len = match args.get(1) {
                Some(MontyObject::String(s)) => s.chars().count(),
                _ => 0,
            };
            match fs.write_file(&path, &content).await {
                Ok(()) => ExtFunctionResult::Return(MontyObject::Int(len as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        "Path.write_bytes" => {
            let content = match args.get(1) {
                Some(MontyObject::Bytes(b)) => b.clone(),
                _ => {
                    return ExtFunctionResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("write_bytes() requires a bytes argument".into()),
                    ));
                }
            };
            let len = content.len();
            match fs.write_file(&path, &content).await {
                Ok(()) => ExtFunctionResult::Return(MontyObject::Int(len as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        "Path.append_text" => {
            let content = match args.get(1) {
                Some(MontyObject::String(s)) => s.as_bytes().to_vec(),
                _ => {
                    return ExtFunctionResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("append_text() requires a string argument".into()),
                    ));
                }
            };
            let len = match args.get(1) {
                Some(MontyObject::String(s)) => s.chars().count(),
                _ => 0,
            };
            match fs.append_file(&path, &content).await {
                Ok(()) => ExtFunctionResult::Return(MontyObject::Int(len as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        "Path.append_bytes" => {
            let content = match args.get(1) {
                Some(MontyObject::Bytes(b)) => b.clone(),
                _ => {
                    return ExtFunctionResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("append_bytes() requires a bytes argument".into()),
                    ));
                }
            };
            let len = content.len();
            match fs.append_file(&path, &content).await {
                Ok(()) => ExtFunctionResult::Return(MontyObject::Int(len as i64)),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        "Path.mkdir" => {
            let parents = get_bool_kwarg(kwargs, "parents").unwrap_or(false);
            let exist_ok = get_bool_kwarg(kwargs, "exist_ok").unwrap_or(false);
            match fs.mkdir(&path, parents).await {
                Ok(()) => ExtFunctionResult::Return(MontyObject::None),
                Err(e) => {
                    let msg = e.to_string();
                    if exist_ok && msg.contains("already exists") {
                        ExtFunctionResult::Return(MontyObject::None)
                    } else {
                        map_vfs_error(e, &path)
                    }
                }
            }
        }
        "Path.unlink" => match fs.remove(&path, false).await {
            Ok(()) => ExtFunctionResult::Return(MontyObject::None),
            Err(e) => map_vfs_error(e, &path),
        },
        "Path.rmdir" => match fs.remove(&path, false).await {
            Ok(()) => ExtFunctionResult::Return(MontyObject::None),
            Err(e) => map_vfs_error(e, &path),
        },
        "Path.iterdir" => match fs.read_dir(&path).await {
            Ok(entries) => {
                let items: Vec<MontyObject> = entries
                    .into_iter()
                    .map(|e| {
                        let child = vfs_join(&path, &e.name);
                        MontyObject::Path(child.to_string_lossy().to_string())
                    })
                    .collect();
                ExtFunctionResult::Return(MontyObject::List(items))
            }
            Err(e) => map_vfs_error(e, &path),
        },
        "Path.stat" => match fs.stat(&path).await {
            Ok(meta) => {
                let mtime = meta
                    .modified
                    .duration_since(crate::time_compat::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                let stat_obj = match meta.file_type {
                    FileType::Directory => dir_stat(meta.mode as i64, mtime),
                    FileType::Symlink => symlink_stat(meta.mode as i64, mtime),
                    _ => file_stat(meta.mode as i64, meta.size as i64, mtime),
                };
                ExtFunctionResult::Return(stat_obj)
            }
            Err(e) => map_vfs_error(e, &path),
        },
        "Path.rename" => {
            let target = match args.get(1) {
                Some(MontyObject::Path(p)) | Some(MontyObject::String(p)) => {
                    resolve_python_path(p, cwd)
                }
                _ => {
                    return ExtFunctionResult::Error(MontyException::new(
                        ExcType::TypeError,
                        Some("rename() requires a target path".into()),
                    ));
                }
            };
            match fs.rename(&path, &target).await {
                Ok(()) => ExtFunctionResult::Return(MontyObject::Path(
                    target.to_string_lossy().to_string(),
                )),
                Err(e) => map_vfs_error(e, &path),
            }
        }
        "Path.resolve" | "Path.absolute" => {
            // No symlink resolution in Bashkit VFS; just return absolute path
            ExtFunctionResult::Return(MontyObject::Path(path.to_string_lossy().to_string()))
        }
        // Getenv/GetEnviron handled above
        _ => ExtFunctionResult::Error(MontyException::new(
            ExcType::OSError,
            Some(format!("{name} not supported in virtual mode")),
        )),
    }
}

/// Extract a path from the first OsCall arg and resolve relative to cwd.
fn extract_path(args: &[MontyObject], cwd: &Path) -> Option<PathBuf> {
    match args.first()? {
        MontyObject::Path(s) | MontyObject::String(s) => Some(resolve_python_path(s, cwd)),
        MontyObject::FileHandle(handle) => Some(resolve_python_path(&handle.path, cwd)),
        _ => None,
    }
}

fn parse_open_mode(args: &[MontyObject]) -> std::result::Result<FileMode, ExtFunctionResult> {
    let Some(MontyObject::String(mode)) = args.get(1) else {
        return Err(ExtFunctionResult::Error(MontyException::new(
            ExcType::TypeError,
            Some("open() missing mode argument".into()),
        )));
    };

    mode.parse::<FileMode>().map_err(|msg| {
        ExtFunctionResult::Error(MontyException::new(
            ExcType::ValueError,
            Some(msg.into_owned()),
        ))
    })
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

/// Extract a bool kwarg by name from the kwargs list.
fn get_bool_kwarg(kwargs: &[(MontyObject, MontyObject)], name: &str) -> Option<bool> {
    for (key, val) in kwargs {
        if let MontyObject::String(k) = key
            && k == name
        {
            return match val {
                MontyObject::Bool(b) => Some(*b),
                _ => None,
            };
        }
    }
    None
}

/// Handle os.getenv(key, default=None).
fn handle_getenv(args: &[MontyObject], env: &HashMap<String, String>) -> ExtFunctionResult {
    let key = match args.first() {
        Some(MontyObject::String(s)) => s.as_str(),
        _ => {
            return ExtFunctionResult::Error(MontyException::new(
                ExcType::TypeError,
                Some("getenv() requires a string argument".into()),
            ));
        }
    };
    let default = match args.get(1) {
        Some(MontyObject::None) | None => MontyObject::None,
        Some(other) => other.clone(),
    };
    match env.get(key) {
        Some(val) => ExtFunctionResult::Return(MontyObject::String(val.clone())),
        None => ExtFunctionResult::Return(default),
    }
}

/// Handle os.environ → dict of all env vars.
fn handle_get_environ(env: &HashMap<String, String>) -> ExtFunctionResult {
    let pairs: Vec<(MontyObject, MontyObject)> = env
        .iter()
        .map(|(k, v)| {
            (
                MontyObject::String(k.clone()),
                MontyObject::String(v.clone()),
            )
        })
        .collect();
    ExtFunctionResult::Return(MontyObject::dict(pairs))
}

/// Handle datetime.date.today() using the sandbox virtual clock.
fn handle_date_today() -> ExtFunctionResult {
    let now = virtual_now_utc();
    ExtFunctionResult::Return(MontyObject::Date(MontyDate {
        year: now.year(),
        month: now.month() as u8,
        day: now.day() as u8,
    }))
}

/// Handle datetime.datetime.now(tz=None) using the sandbox virtual clock.
///
/// If tz is None, returns a naive datetime (no timezone info).
/// If tz is a TimeZone, returns an aware datetime at that offset.
fn handle_datetime_now(args: &[MontyObject]) -> ExtFunctionResult {
    let base_utc = virtual_now_utc();
    let tz = match args.first() {
        Some(MontyObject::TimeZone(tz)) => Some(tz),
        _ => None,
    };

    if let Some(tz) = tz {
        // Aware datetime at the requested offset
        let offset = chrono::FixedOffset::east_opt(tz.offset_seconds)
            .unwrap_or(chrono::FixedOffset::east_opt(0).expect("UTC offset is always valid"));
        let dt = base_utc.with_timezone(&offset);
        ExtFunctionResult::Return(MontyObject::DateTime(MontyDateTime {
            year: dt.year(),
            month: dt.month() as u8,
            day: dt.day() as u8,
            hour: dt.hour() as u8,
            minute: dt.minute() as u8,
            second: dt.second() as u8,
            microsecond: dt.nanosecond() / 1000,
            offset_seconds: Some(tz.offset_seconds),
            timezone_name: tz.name.clone(),
        }))
    } else {
        // No timezone → naive UTC datetime (no timezone metadata)
        let dt = base_utc.naive_utc();
        ExtFunctionResult::Return(MontyObject::DateTime(MontyDateTime {
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
            Box::pin(async { ExtFunctionResult::Return(MontyObject::Int(42)) })
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
                ExtFunctionResult::Return(MontyObject::Int(1))
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
                let a = match &args[0] {
                    MontyObject::Int(i) => *i,
                    _ => 0,
                };
                let b = match &args[1] {
                    MontyObject::Int(i) => *i,
                    _ => 0,
                };
                ExtFunctionResult::Return(MontyObject::Int(a + b))
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
                    if let (MontyObject::String(key), MontyObject::String(val)) = (k, v)
                        && key == "name"
                    {
                        return ExtFunctionResult::Return(MontyObject::String(format!(
                            "hello {val}"
                        )));
                    }
                }
                ExtFunctionResult::Return(MontyObject::String("hello unknown".into()))
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
                ExtFunctionResult::Return(MontyObject::Int(val))
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
                let input = match &args[0] {
                    MontyObject::String(s) => s.clone(),
                    _ => String::new(),
                };
                ExtFunctionResult::Return(MontyObject::String(input.to_uppercase()))
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
                    "get_x" => MontyObject::Int(10),
                    "get_y" => MontyObject::Int(20),
                    _ => MontyObject::None,
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
            Box::pin(async { ExtFunctionResult::Return(MontyObject::Int(1)) })
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
    async fn test_re_module_is_blocked() {
        let r = run(
            &[
                "-c",
                "import re; m = re.search(r'(\\d+)', 'abc123def'); print(m.group(1))",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("importing module 're' is disabled"));
    }

    #[tokio::test]
    async fn test_re_module_dynamic_import_is_unavailable() {
        // Monty has no `__import__` or `importlib`, so import statements are
        // the only way to load a module. Pin that: if Monty adds either, the
        // AST gate in `is_disabled_import` must learn about it.
        let r = run(&["-c", "m = __import__('re')\nprint(m)"], None).await;
        assert_eq!(r.exit_code, 1, "{}", r.stdout);
        assert!(r.stdout.is_empty(), "{}", r.stdout);
        let r = run(&["-c", "import importlib"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(
            r.stderr.contains("No module named 'importlib'"),
            "{}",
            r.stderr
        );
    }

    #[tokio::test]
    async fn test_re_module_hidden_imports_are_blocked() {
        // Regression: the gate used to match only lines starting with
        // `import`/`from`, so any statement prefix bypassed it.
        for code in [
            "pass; import re",
            "x = 1; from re import split",
            "if True: import re",
            "if True:\n    import re",
            "def f():\n    import re\n    return re\nf()",
            "try:\n    import re\nexcept Exception:\n    pass",
            "for _ in [1]: import json, re",
            "with open('/tmp/x', 'w') as f: import re.sub",
            "import json as j, re as r",
            "class C:\n    import re",
        ] {
            let r = run(&["-c", code], None).await;
            assert_eq!(r.exit_code, 1, "{code}: {}", r.stderr);
            assert!(
                r.stderr.contains("importing module 're' is disabled"),
                "{code}: {}",
                r.stderr
            );
        }
    }

    #[tokio::test]
    async fn test_re_named_text_is_not_an_import() {
        // Strings, comments and other modules that merely mention `re` run.
        let r = run(
            &[
                "-c",
                "import json\nre = 'import re'\n# import re\nprint(re, json.dumps('re'))",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "import re \"re\"\n");
    }

    /// The LLM hint must describe what Monty really does: every module it
    /// names imports, simple classes work, and what it rules out fails.
    /// Regression: the hint said "No classes" long after Monty added them.
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
        let r = run(&["-c", "match 1:\n    case 1: pass"], None).await;
        assert_ne!(r.exit_code, 0, "match now works; update the hint");
        assert!(hint.contains("`re` is disabled"));
        let r = run(&["-c", "import re"], None).await;
        assert_ne!(r.exit_code, 0);
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
