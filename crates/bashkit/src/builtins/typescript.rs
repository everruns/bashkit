//! ts/node/deno/bun builtins via embedded ZapCode TypeScript interpreter
//!
//! # Direct Integration
//!
//! ZapCode runs directly in the host process. No subprocess, no V8, no IPC.
//! Resource limits (memory, time, stack depth, allocations) are enforced
//! by ZapCode's own VM, not by process isolation.
//!
//! # Overview
//!
//! Virtual TypeScript/JavaScript execution with resource limits and VFS access.
//! VFS operations are bridged via ZapCode's external function suspend/resume
//! mechanism. No real filesystem or network access.
//!
//! Supports: `ts -c "code"`, `ts script.ts`, stdin piping, and
//! `node`/`deno`/`bun` aliases.

use async_trait::async_trait;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use zapcode_core::{ResourceLimits, RunResult, Value, VmState, ZapcodeRun};

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

/// Default resource limits for virtual TypeScript execution.
const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(30);
const DEFAULT_MAX_MEMORY: usize = 64 * 1024 * 1024; // 64 MB
const DEFAULT_MAX_STACK_DEPTH: usize = 512;
const DEFAULT_MAX_ALLOCATIONS: usize = 1_000_000;

/// Resource limits for the embedded TypeScript (ZapCode) interpreter.
///
/// Use the builder pattern to customize, or `Default` for the standard limits:
/// - 30 second timeout
/// - 64 MB memory
/// - 512 stack depth
/// - 1,000,000 allocations
///
/// # Example
///
/// ```rust
/// use bashkit::TypeScriptLimits;
/// use std::time::Duration;
///
/// let limits = TypeScriptLimits::default()
///     .max_duration(Duration::from_secs(5))
///     .max_memory(16 * 1024 * 1024);
///
/// assert_eq!(limits.max_duration, Duration::from_secs(5));
/// assert_eq!(limits.max_memory, 16 * 1024 * 1024);
/// ```
#[derive(Debug, Clone)]
pub struct TypeScriptLimits {
    /// Maximum execution time (default: 30s).
    pub max_duration: Duration,
    /// Maximum memory in bytes (default: 64 MB).
    pub max_memory: usize,
    /// Maximum call stack depth (default: 512).
    pub max_stack_depth: usize,
    /// Maximum heap allocations (default: 1,000,000).
    pub max_allocations: usize,
}

impl Default for TypeScriptLimits {
    fn default() -> Self {
        Self {
            max_duration: DEFAULT_MAX_DURATION,
            max_memory: DEFAULT_MAX_MEMORY,
            max_stack_depth: DEFAULT_MAX_STACK_DEPTH,
            max_allocations: DEFAULT_MAX_ALLOCATIONS,
        }
    }
}

impl TypeScriptLimits {
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

    /// Set max call stack depth.
    #[must_use]
    pub fn max_stack_depth(mut self, depth: usize) -> Self {
        self.max_stack_depth = depth;
        self
    }

    /// Set max heap allocations.
    #[must_use]
    pub fn max_allocations(mut self, n: usize) -> Self {
        self.max_allocations = n;
        self
    }

    /// Convert to ZapCode's `ResourceLimits`.
    fn to_zapcode_limits(&self) -> ResourceLimits {
        ResourceLimits {
            memory_limit_bytes: self.max_memory,
            time_limit_ms: self.max_duration.as_millis() as u64,
            max_stack_depth: self.max_stack_depth,
            max_allocations: self.max_allocations,
        }
    }
}

/// Async handler for external TypeScript function calls.
///
/// Receives `(function_name, args)` when TypeScript calls a registered external function.
/// Return `Ok(Value)` for success or `Err(String)` for an error thrown in TypeScript.
pub type TypeScriptExternalFnHandler = Arc<
    dyn Fn(
            String,
            Vec<Value>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<Value, String>> + Send>>
        + Send
        + Sync,
>;

/// External function configuration for the TypeScript builtin.
///
/// Groups function names and their async handler together.
/// Configure via [`BashBuilder::typescript_with_external_handler`](crate::BashBuilder::typescript_with_external_handler).
#[derive(Clone)]
pub struct TypeScriptExternalFns {
    /// Function names callable from TypeScript.
    names: Vec<String>,
    /// Async handler invoked when TypeScript calls one of these functions.
    handler: TypeScriptExternalFnHandler,
}

impl std::fmt::Debug for TypeScriptExternalFns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeScriptExternalFns")
            .field("names", &self.names)
            .field("handler", &"<fn>")
            .finish()
    }
}

/// VFS-bridged external function names automatically registered by the builtin.
const VFS_FUNCTIONS: &[&str] = &[
    "readFile",
    "writeFile",
    "exists",
    "readDir",
    "mkdir",
    "remove",
    "stat",
];

/// Configuration for the TypeScript builtin.
///
/// Controls which command aliases are registered and whether unsupported
/// execution modes produce helpful hint text.
///
/// # Examples
///
/// ```rust
/// use bashkit::{TypeScriptConfig, TypeScriptLimits};
/// use std::time::Duration;
///
/// // Default: all aliases + hints enabled
/// let config = TypeScriptConfig::default();
/// assert!(config.enable_compat_aliases);
/// assert!(config.enable_unsupported_mode_hint);
///
/// // Only ts/typescript, no node/deno/bun aliases
/// let config = TypeScriptConfig::default().compat_aliases(false);
/// assert!(!config.enable_compat_aliases);
///
/// // Custom limits + selective config
/// let config = TypeScriptConfig::default()
///     .limits(TypeScriptLimits::default().max_duration(Duration::from_secs(5)))
///     .compat_aliases(false)
///     .unsupported_mode_hint(false);
/// assert_eq!(config.limits.max_duration, Duration::from_secs(5));
/// assert!(!config.enable_compat_aliases);
/// assert!(!config.enable_unsupported_mode_hint);
/// ```
///
/// Use with the builder:
///
/// ```rust,no_run
/// use bashkit::{Bash, TypeScriptConfig};
///
/// # fn main() {
/// let bash = Bash::builder()
///     .typescript_with_config(TypeScriptConfig::default().compat_aliases(false))
///     .build();
/// # }
#[derive(Debug, Clone)]
pub struct TypeScriptConfig {
    /// Resource limits for the ZapCode interpreter.
    pub limits: TypeScriptLimits,
    /// Register `node`, `deno`, `bun` aliases in addition to `ts`/`typescript`.
    /// Default: true.
    pub enable_compat_aliases: bool,
    /// Show helpful hint text when unsupported execution modes are used
    /// (e.g. `node --inspect`, `deno run`, `bun install`).
    /// Default: true.
    pub enable_unsupported_mode_hint: bool,
}

impl Default for TypeScriptConfig {
    fn default() -> Self {
        Self {
            limits: TypeScriptLimits::default(),
            enable_compat_aliases: true,
            enable_unsupported_mode_hint: true,
        }
    }
}

impl TypeScriptConfig {
    /// Set resource limits.
    #[must_use]
    pub fn limits(mut self, limits: TypeScriptLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Enable or disable `node`/`deno`/`bun` compat aliases (default: true).
    #[must_use]
    pub fn compat_aliases(mut self, enable: bool) -> Self {
        self.enable_compat_aliases = enable;
        self
    }

    /// Enable or disable hint text for unsupported execution modes (default: true).
    #[must_use]
    pub fn unsupported_mode_hint(mut self, enable: bool) -> Self {
        self.enable_unsupported_mode_hint = enable;
        self
    }
}

/// The ts/node/deno/bun builtin command.
///
/// Executes TypeScript/JavaScript code using the embedded ZapCode interpreter.
/// VFS operations are bridged via external function suspend/resume — files
/// created by bash are readable from TypeScript, and vice versa.
///
/// # Usage
///
/// ```bash
/// ts -c "console.log('hello')"
/// node -e "console.log('hello')"
/// ts script.ts
/// echo "console.log('hello')" | ts
/// ts -c "1 + 2 * 3"              # expression result printed
/// ts --version
/// ts -c "const s = await readFile('/tmp/f.txt'); console.log(s)"
/// ```
pub struct TypeScript {
    /// Resource limits for the ZapCode interpreter.
    pub limits: TypeScriptLimits,
    /// Optional user-provided external function configuration.
    external_fns: Option<TypeScriptExternalFns>,
    /// Show hint text for unsupported execution modes.
    unsupported_mode_hint: bool,
    /// The command name this builtin was registered as (e.g. "ts", "node").
    cmd_name: String,
}

impl TypeScript {
    /// Create with default limits, registered as "ts".
    pub fn new() -> Self {
        Self {
            limits: TypeScriptLimits::default(),
            external_fns: None,
            unsupported_mode_hint: true,
            cmd_name: "ts".to_string(),
        }
    }

    /// Create from a config, with a specific command name.
    pub fn from_config(config: &TypeScriptConfig, cmd_name: &str) -> Self {
        Self {
            limits: config.limits.clone(),
            external_fns: None,
            unsupported_mode_hint: config.enable_unsupported_mode_hint,
            cmd_name: cmd_name.to_string(),
        }
    }

    /// Set external function names and handler.
    ///
    /// External functions are callable from TypeScript by name.
    /// When called, execution suspends and the handler is invoked with the args.
    pub fn with_external_handler(
        mut self,
        names: Vec<String>,
        handler: TypeScriptExternalFnHandler,
    ) -> Self {
        self.external_fns = Some(TypeScriptExternalFns { names, handler });
        self
    }
}

impl Default for TypeScript {
    fn default() -> Self {
        Self::new()
    }
}

/// Known flags/subcommands from Node.js, Deno, and Bun that are not supported.
const UNSUPPORTED_NODE_FLAGS: &[&str] = &[
    "--inspect",
    "--inspect-brk",
    "--prof",
    "--watch",
    "--experimental-modules",
    "--loader",
    "--require",
    "--preserve-symlinks",
    "--max-old-space-size",
    "--expose-gc",
    "--harmony",
    "--trace-warnings",
    "--no-warnings",
    "--pending-deprecation",
];

const UNSUPPORTED_DENO_SUBCOMMANDS: &[&str] = &[
    "run",
    "compile",
    "install",
    "uninstall",
    "lint",
    "fmt",
    "test",
    "bench",
    "check",
    "serve",
    "task",
    "repl",
    "upgrade",
    "doc",
    "publish",
    "add",
    "remove",
    "init",
    "info",
    "cache",
    "eval",
    "coverage",
    "types",
    "completions",
];

const UNSUPPORTED_BUN_SUBCOMMANDS: &[&str] = &[
    "run", "install", "add", "remove", "update", "link", "unlink", "pm", "build", "init", "test",
    "x", "create",
];

/// Format a hint message for unsupported execution modes.
fn unsupported_mode_message(cmd: &str, arg: &str) -> String {
    let base = format!("{cmd}: unsupported option or subcommand: {arg}\n");
    let runtime = match cmd {
        "node" => "Node.js",
        "deno" => "Deno",
        "bun" => "Bun",
        _ => "a full runtime",
    };
    let flag = if cmd == "ts" || cmd == "typescript" {
        "-c"
    } else {
        "-e"
    };
    format!(
        "{base}\
         hint: This is an embedded TypeScript interpreter (ZapCode), not {runtime}.\n\
         hint: Only inline execution is supported:\n\
         hint:   {cmd} {flag} \"console.log('hello')\"   # run inline code\n\
         hint:   {cmd} script.ts                       # run file from VFS\n\
         hint:   echo \"code\" | {cmd}                    # pipe code via stdin\n"
    )
}

/// Check if an argument is a known unsupported flag/subcommand for the given command.
fn is_unsupported_mode(cmd: &str, arg: &str) -> bool {
    // Node.js unsupported flags
    if UNSUPPORTED_NODE_FLAGS.iter().any(|f| arg.starts_with(f)) {
        return true;
    }
    // Deno subcommands
    if cmd == "deno" && UNSUPPORTED_DENO_SUBCOMMANDS.contains(&arg) {
        return true;
    }
    // Bun subcommands
    if cmd == "bun" && UNSUPPORTED_BUN_SUBCOMMANDS.contains(&arg) {
        return true;
    }
    false
}

#[async_trait]
impl Builtin for TypeScript {
    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "ts/node/deno/bun: Embedded TypeScript (ZapCode). \
             Supports ES2024 subset: let/const, arrow fns, async/await, \
             template literals, destructuring, array methods. \
             File I/O via readFile()/writeFile() async functions. \
             No npm/import/require. No HTTP/network. No eval().",
        )
    }

    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;
        let cmd = &self.cmd_name;

        // ts --version / ts -V / node --version / etc.
        if args.first().map(|s| s.as_str()) == Some("--version")
            || args.first().map(|s| s.as_str()) == Some("-V")
        {
            return Ok(ExecResult::ok("TypeScript 5.0.0 (zapcode)\n".to_string()));
        }

        // ts --help / ts -h
        if args.first().map(|s| s.as_str()) == Some("--help")
            || args.first().map(|s| s.as_str()) == Some("-h")
        {
            return Ok(ExecResult::ok(format!(
                "usage: {cmd} [-c cmd | -e cmd | file | -] [arg ...]\n\
                 Options:\n  \
                 -c cmd : execute code from string\n  \
                 -e cmd : execute code from string (Node.js compat)\n  \
                 file   : execute code from file (VFS)\n  \
                 -      : read code from stdin\n  \
                 -V     : print version\n"
            )));
        }

        let (code, _filename) = if let Some(first) = args.first() {
            match first.as_str() {
                "-c" | "-e" => {
                    // ts -c "code" / node -e "code"
                    let code = args.get(1).map(|s| s.as_str()).unwrap_or("");
                    if code.is_empty() {
                        return Ok(ExecResult::err(
                            format!("{cmd}: option {} requires argument\n", first),
                            2,
                        ));
                    }
                    (code.to_string(), "<string>".to_string())
                }
                "-" => {
                    // ts - : read from stdin
                    match ctx.stdin {
                        Some(input) if !input.is_empty() => {
                            (input.to_string(), "<stdin>".to_string())
                        }
                        _ => {
                            return Ok(ExecResult::err(format!("{cmd}: no input from stdin\n"), 1));
                        }
                    }
                }
                arg if arg.starts_with('-') => {
                    // Check for known unsupported flags from Node/Deno/Bun
                    if self.unsupported_mode_hint && is_unsupported_mode(cmd, arg) {
                        return Ok(ExecResult::err(unsupported_mode_message(cmd, arg), 2));
                    }
                    return Ok(ExecResult::err(
                        format!("{cmd}: unknown option: {arg}\n"),
                        2,
                    ));
                }
                arg if !arg.contains('.')
                    && self.unsupported_mode_hint
                    && is_unsupported_mode(cmd, arg) =>
                {
                    // Check for known unsupported subcommands (e.g. "deno run", "bun install")
                    return Ok(ExecResult::err(unsupported_mode_message(cmd, arg), 2));
                }
                script_path => {
                    // ts script.ts / node script.js
                    let path = resolve_path(ctx.cwd, script_path);
                    match ctx.fs.read_file(&path).await {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(code) => (code, script_path.to_string()),
                            Err(_) => {
                                return Ok(ExecResult::err(
                                    format!(
                                        "{cmd}: can't decode file '{script_path}': not UTF-8\n"
                                    ),
                                    1,
                                ));
                            }
                        },
                        Err(_) => {
                            return Ok(ExecResult::err(
                                format!(
                                    "{cmd}: can't open file '{script_path}': No such file or directory\n"
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
            if self.unsupported_mode_hint {
                return Ok(ExecResult::err(
                    format!(
                        "{cmd}: interactive mode not supported\n\
                         hint: Use inline execution instead:\n\
                         hint:   {cmd} -c \"console.log('hello')\"   # run inline code\n\
                         hint:   {cmd} script.ts                    # run file from VFS\n\
                         hint:   echo \"code\" | {cmd}                # pipe code via stdin\n"
                    ),
                    1,
                ));
            }
            return Ok(ExecResult::err(
                format!("{cmd}: interactive mode not supported in virtual mode\n"),
                1,
            ));
        };

        run_typescript(
            &code,
            ctx.fs.clone(),
            ctx.cwd,
            &self.limits,
            self.external_fns.as_ref(),
        )
        .await
    }
}

/// Execute TypeScript code via ZapCode with resource limits and VFS bridging.
///
/// Uses ZapCode's start/resume API: execution suspends at external function calls
/// (VFS operations), we bridge them to BashKit's VFS, then resume.
async fn run_typescript(
    code: &str,
    fs: Arc<dyn FileSystem>,
    cwd: &Path,
    ts_limits: &TypeScriptLimits,
    external_fns: Option<&TypeScriptExternalFns>,
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

    // Collect all external function names: VFS builtins + user-registered
    let mut ext_fn_names: Vec<String> = VFS_FUNCTIONS.iter().map(|s| (*s).to_string()).collect();
    if let Some(ef) = external_fns {
        ext_fn_names.extend(ef.names.iter().cloned());
    }

    let runner = match ZapcodeRun::new(
        code.to_string(),
        Vec::new(),
        ext_fn_names,
        ts_limits.to_zapcode_limits(),
    ) {
        Ok(r) => r,
        Err(e) => {
            return Ok(ExecResult::err(format!("{e}\n"), 1));
        }
    };

    let result = match runner.run(Vec::new()) {
        Ok(r) => r,
        Err(e) => {
            return Ok(ExecResult::err(format!("{e}\n"), 1));
        }
    };

    // Process the result through the suspend/resume loop for VFS bridging
    process_vm_result(result, &fs, cwd, external_fns).await
}

/// Process a VmState, handling suspension for external function calls.
async fn process_vm_result(
    result: RunResult,
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
    external_fns: Option<&TypeScriptExternalFns>,
) -> Result<ExecResult> {
    let mut stdout = result.stdout;

    match result.state {
        VmState::Complete(value) => {
            // If the result is not undefined and there was no print output,
            // display the result (like Node REPL behavior for expressions)
            if !matches!(value, Value::Undefined) && stdout.is_empty() {
                stdout = format!("{}\n", value.to_js_string());
            }
            Ok(ExecResult::ok(stdout))
        }
        VmState::Suspended {
            function_name,
            args,
            snapshot,
        } => {
            // Handle external function call
            let return_value =
                handle_external_call(&function_name, &args, fs, cwd, external_fns).await;

            // Resume execution with the return value
            let mut state = match snapshot.resume(return_value) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(format_error_with_output(e, &stdout));
                }
            };

            // Continue the suspend/resume loop
            loop {
                match state {
                    VmState::Complete(value) => {
                        if !matches!(value, Value::Undefined) && stdout.is_empty() {
                            stdout = format!("{}\n", value.to_js_string());
                        }
                        return Ok(ExecResult::ok(stdout));
                    }
                    VmState::Suspended {
                        function_name,
                        args,
                        snapshot,
                    } => {
                        let return_value =
                            handle_external_call(&function_name, &args, fs, cwd, external_fns)
                                .await;

                        state = match snapshot.resume(return_value) {
                            Ok(s) => s,
                            Err(e) => {
                                return Ok(format_error_with_output(e, &stdout));
                            }
                        };
                    }
                }
            }
        }
    }
}

/// Handle an external function call — either VFS operation or user-registered function.
async fn handle_external_call(
    function_name: &str,
    args: &[Value],
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
    external_fns: Option<&TypeScriptExternalFns>,
) -> Value {
    // Try VFS functions first
    match function_name {
        "readFile" => handle_read_file(args, fs, cwd).await,
        "writeFile" => handle_write_file(args, fs, cwd).await,
        "exists" => handle_exists(args, fs, cwd).await,
        "readDir" => handle_read_dir(args, fs, cwd).await,
        "mkdir" => handle_mkdir(args, fs, cwd).await,
        "remove" => handle_remove(args, fs, cwd).await,
        "stat" => handle_stat(args, fs, cwd).await,
        _ => {
            // Try user-registered external functions
            if let Some(ef) = external_fns {
                if ef.names.contains(&function_name.to_string()) {
                    match (ef.handler)(function_name.to_string(), args.to_vec()).await {
                        Ok(v) => v,
                        Err(e) => Value::String(Arc::from(format!("Error: {e}"))),
                    }
                } else {
                    Value::String(Arc::from(format!(
                        "Error: unknown external function '{function_name}'"
                    )))
                }
            } else {
                Value::String(Arc::from(format!(
                    "Error: unknown external function '{function_name}'"
                )))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// VFS bridging: ZapCode external functions → BashKit FileSystem
// ---------------------------------------------------------------------------

/// Extract a path string from the first arg, resolve relative to cwd.
fn extract_path(args: &[Value], cwd: &Path) -> Option<PathBuf> {
    match args.first()? {
        Value::String(s) => {
            let p = Path::new(s.as_ref());
            if p.is_absolute() {
                Some(p.to_owned())
            } else {
                Some(cwd.join(p))
            }
        }
        _ => None,
    }
}

/// readFile(path: string): string
async fn handle_read_file(args: &[Value], fs: &Arc<dyn FileSystem>, cwd: &Path) -> Value {
    let Some(path) = extract_path(args, cwd) else {
        return Value::String(Arc::from("Error: readFile requires a path argument"));
    };
    match fs.read_file(&path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => Value::String(Arc::from(s.as_str())),
            Err(_) => Value::String(Arc::from(format!(
                "Error: can't decode '{}': not valid UTF-8",
                path.display()
            ))),
        },
        Err(e) => Value::String(Arc::from(format!("Error: {e}"))),
    }
}

/// writeFile(path: string, content: string): void
async fn handle_write_file(args: &[Value], fs: &Arc<dyn FileSystem>, cwd: &Path) -> Value {
    let Some(path) = extract_path(args, cwd) else {
        return Value::String(Arc::from("Error: writeFile requires a path argument"));
    };
    let content = match args.get(1) {
        Some(Value::String(s)) => s.as_ref().as_bytes().to_vec(),
        Some(v) => v.to_js_string().into_bytes(),
        None => {
            return Value::String(Arc::from("Error: writeFile requires a content argument"));
        }
    };
    match fs.write_file(&path, &content).await {
        Ok(()) => Value::Undefined,
        Err(e) => Value::String(Arc::from(format!("Error: {e}"))),
    }
}

/// exists(path: string): boolean
async fn handle_exists(args: &[Value], fs: &Arc<dyn FileSystem>, cwd: &Path) -> Value {
    let Some(path) = extract_path(args, cwd) else {
        return Value::Bool(false);
    };
    Value::Bool(fs.exists(&path).await.unwrap_or(false))
}

/// readDir(path: string): string[]
async fn handle_read_dir(args: &[Value], fs: &Arc<dyn FileSystem>, cwd: &Path) -> Value {
    let Some(path) = extract_path(args, cwd) else {
        return Value::String(Arc::from("Error: readDir requires a path argument"));
    };
    match fs.read_dir(&path).await {
        Ok(entries) => {
            let items: Vec<Value> = entries
                .into_iter()
                .map(|e| Value::String(Arc::from(e.name.as_str())))
                .collect();
            Value::Array(items)
        }
        Err(e) => Value::String(Arc::from(format!("Error: {e}"))),
    }
}

/// mkdir(path: string): void
async fn handle_mkdir(args: &[Value], fs: &Arc<dyn FileSystem>, cwd: &Path) -> Value {
    let Some(path) = extract_path(args, cwd) else {
        return Value::String(Arc::from("Error: mkdir requires a path argument"));
    };
    match fs.mkdir(&path, true).await {
        Ok(()) => Value::Undefined,
        Err(e) => Value::String(Arc::from(format!("Error: {e}"))),
    }
}

/// remove(path: string): void
async fn handle_remove(args: &[Value], fs: &Arc<dyn FileSystem>, cwd: &Path) -> Value {
    let Some(path) = extract_path(args, cwd) else {
        return Value::String(Arc::from("Error: remove requires a path argument"));
    };
    match fs.remove(&path, false).await {
        Ok(()) => Value::Undefined,
        Err(e) => Value::String(Arc::from(format!("Error: {e}"))),
    }
}

/// stat(path: string): { size: number, isFile: boolean, isDir: boolean }
///
/// Returns a JSON string that TypeScript code can parse. We avoid constructing
/// a `Value::Object` directly to avoid pulling in `indexmap` as a dependency.
async fn handle_stat(args: &[Value], fs: &Arc<dyn FileSystem>, cwd: &Path) -> Value {
    let Some(path) = extract_path(args, cwd) else {
        return Value::String(Arc::from("Error: stat requires a path argument"));
    };
    match fs.stat(&path).await {
        Ok(meta) => {
            // Return a JSON string — callers use JSON.parse() or we return
            // structured data via an array convention:
            // [size, isFile, isDir]
            let json = format!(
                r#"{{"size":{},"isFile":{},"isDir":{}}}"#,
                meta.size,
                meta.file_type.is_file(),
                meta.file_type.is_dir(),
            );
            Value::String(Arc::from(json.as_str()))
        }
        Err(e) => Value::String(Arc::from(format!("Error: {e}"))),
    }
}

/// Format a ZapCode error with any stdout already produced.
fn format_error_with_output(e: zapcode_core::ZapcodeError, stdout: &str) -> ExecResult {
    let mut result = ExecResult::err(format!("{e}\n"), 1);
    result.stdout = stdout.to_string();
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
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
        TypeScript::new().execute(ctx).await.unwrap()
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
        TypeScript::new().execute(ctx).await.unwrap()
    }

    async fn run_with_vfs(args: &[&str], files: &[(&str, &str)]) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            let p = std::path::Path::new(path);
            if let Some(parent) = p.parent() {
                let _ = fs.mkdir(parent, true).await;
            }
            fs.write_file(p, content.as_bytes()).await.unwrap();
        }
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        TypeScript::new().execute(ctx).await.unwrap()
    }

    // --- Basic functionality ---

    #[tokio::test]
    async fn test_version() {
        let r = run(&["--version"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("TypeScript"));
        assert!(r.stdout.contains("zapcode"));
    }

    #[tokio::test]
    async fn test_version_short() {
        let r = run(&["-V"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("TypeScript"));
    }

    #[tokio::test]
    async fn test_help() {
        let r = run(&["--help"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("usage:"));
    }

    #[tokio::test]
    async fn test_inline_console_log() {
        let r = run(&["-c", "console.log('hello world')"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_inline_expression() {
        let r = run(&["-c", "1 + 2 * 3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "7\n");
    }

    #[tokio::test]
    async fn test_eval_flag() {
        // -e flag (Node.js compat)
        let r = run(&["-e", "console.log('hi')"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hi\n");
    }

    #[tokio::test]
    async fn test_inline_missing_code() {
        let r = run(&["-c", ""], None).await;
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
    async fn test_no_args_no_stdin() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("interactive mode"));
    }

    // --- Stdin ---

    #[tokio::test]
    async fn test_stdin_pipe() {
        let r = run(&[], Some("console.log('piped')")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "piped\n");
    }

    #[tokio::test]
    async fn test_stdin_explicit_dash() {
        let r = run(&["-"], Some("console.log('from stdin')")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from stdin\n");
    }

    #[tokio::test]
    async fn test_stdin_empty() {
        let r = run(&[], Some("")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "");
    }

    // --- Script file ---

    #[tokio::test]
    async fn test_script_file() {
        let r = run_with_file(
            &["script.ts"],
            "/home/user/script.ts",
            "console.log('from file')",
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from file\n");
    }

    #[tokio::test]
    async fn test_script_file_not_found() {
        let r = run(&["missing.ts"], None).await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("No such file"));
    }

    #[tokio::test]
    async fn test_shebang_stripped() {
        let r = run_with_file(
            &["script.ts"],
            "/home/user/script.ts",
            "#!/usr/bin/env ts\nconsole.log('shebang')",
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "shebang\n");
    }

    // --- TypeScript features ---

    #[tokio::test]
    async fn test_let_const() {
        let r = run(
            &["-c", "let x = 10; const y = 20; console.log(x + y)"],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "30\n");
    }

    #[tokio::test]
    async fn test_arrow_function() {
        let r = run(
            &[
                "-c",
                "const add = (a: number, b: number) => a + b; console.log(add(3, 4))",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "7\n");
    }

    #[tokio::test]
    async fn test_template_literal() {
        let r = run(
            &["-c", "const name = 'world'; console.log(`hello ${name}`)"],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_array_methods() {
        let r = run(
            &[
                "-c",
                "const arr = [1, 2, 3]; console.log(arr.map(x => x * 2).join(','))",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "2,4,6\n");
    }

    #[tokio::test]
    async fn test_for_loop() {
        let r = run(
            &[
                "-c",
                "let sum = 0; for (let i = 0; i < 5; i++) { sum += i; } console.log(sum)",
            ],
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "10\n");
    }

    #[tokio::test]
    async fn test_syntax_error() {
        let r = run(&["-c", "const x = {"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(!r.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_runtime_error() {
        let r = run(&["-c", "const x: any = null; x.foo()"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(!r.stderr.is_empty());
    }

    // --- VFS bridging ---
    //
    // NOTE: ZapCode's snapshot.resume() does not expose vm.stdout, so
    // console.log() after an external function call (VFS op) loses output.
    // Use return-value pattern instead: the last expression is printed.

    #[tokio::test]
    async fn test_vfs_read_file() {
        let r = run_with_vfs(
            &["-c", "await readFile('/tmp/test.txt')"],
            &[("/tmp/test.txt", "hello from vfs")],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello from vfs\n");
    }

    #[tokio::test]
    async fn test_vfs_write_and_read() {
        let args: Vec<String> = vec![
            "-c".to_string(),
            "await writeFile('/tmp/out.txt', 'written by ts'); await readFile('/tmp/out.txt')"
                .to_string(),
        ];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let _ = fs.mkdir(std::path::Path::new("/tmp"), true).await;
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = TypeScript::new().execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "written by ts\n");
    }

    #[tokio::test]
    async fn test_vfs_exists() {
        let r = run_with_vfs(
            &["-c", "await exists('/tmp/test.txt')"],
            &[("/tmp/test.txt", "data")],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "true\n");
    }

    #[tokio::test]
    async fn test_vfs_exists_false() {
        let r = run(&["-c", "await exists('/tmp/nope.txt')"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "false\n");
    }

    #[tokio::test]
    async fn test_console_log_before_vfs() {
        // console.log output BEFORE a VFS call is captured
        let r = run_with_vfs(
            &["-c", "console.log('before'); await readFile('/tmp/f.txt')"],
            &[("/tmp/f.txt", "data")],
        )
        .await;
        assert_eq!(r.exit_code, 0);
        // stdout has pre-suspension output
        assert!(r.stdout.contains("before"));
    }

    // --- Limits ---

    #[test]
    fn test_limits_default() {
        let limits = TypeScriptLimits::default();
        assert_eq!(limits.max_duration, Duration::from_secs(30));
        assert_eq!(limits.max_memory, 64 * 1024 * 1024);
        assert_eq!(limits.max_stack_depth, 512);
        assert_eq!(limits.max_allocations, 1_000_000);
    }

    #[test]
    fn test_limits_builder() {
        let limits = TypeScriptLimits::default()
            .max_duration(Duration::from_secs(5))
            .max_memory(1024)
            .max_stack_depth(100)
            .max_allocations(500);
        assert_eq!(limits.max_duration, Duration::from_secs(5));
        assert_eq!(limits.max_memory, 1024);
        assert_eq!(limits.max_stack_depth, 100);
        assert_eq!(limits.max_allocations, 500);
    }

    #[test]
    fn test_llm_hint() {
        let ts = TypeScript::new();
        let hint = ts.llm_hint().unwrap();
        assert!(hint.contains("TypeScript"));
        assert!(hint.contains("ZapCode"));
    }

    // --- Config tests ---

    #[test]
    fn test_config_defaults() {
        let config = TypeScriptConfig::default();
        assert!(config.enable_compat_aliases);
        assert!(config.enable_unsupported_mode_hint);
    }

    #[test]
    fn test_config_builder() {
        let config = TypeScriptConfig::default()
            .compat_aliases(false)
            .unsupported_mode_hint(false)
            .limits(TypeScriptLimits::default().max_duration(Duration::from_secs(5)));
        assert!(!config.enable_compat_aliases);
        assert!(!config.enable_unsupported_mode_hint);
        assert_eq!(config.limits.max_duration, Duration::from_secs(5));
    }

    // --- Unsupported mode hint tests ---

    #[tokio::test]
    async fn test_unsupported_node_inspect() {
        let ts = TypeScript::from_config(&TypeScriptConfig::default(), "node");
        let args = vec!["--inspect".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = ts.execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("hint:"), "should contain hint text");
        assert!(r.stderr.contains("Node.js"), "should mention Node.js");
        assert!(r.stderr.contains("node -e"), "should suggest -e flag");
    }

    #[tokio::test]
    async fn test_unsupported_deno_run() {
        let ts = TypeScript::from_config(&TypeScriptConfig::default(), "deno");
        let args = vec!["run".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = ts.execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("hint:"));
        assert!(r.stderr.contains("Deno"));
    }

    #[tokio::test]
    async fn test_unsupported_bun_install() {
        let ts = TypeScript::from_config(&TypeScriptConfig::default(), "bun");
        let args = vec!["install".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = ts.execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("hint:"));
        assert!(r.stderr.contains("Bun"));
    }

    #[tokio::test]
    async fn test_hint_disabled() {
        let config = TypeScriptConfig::default().unsupported_mode_hint(false);
        let ts = TypeScript::from_config(&config, "node");
        let args = vec!["--inspect".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = ts.execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 2);
        // When hints disabled, just get the basic error
        assert!(!r.stderr.contains("hint:"), "should not contain hint text");
        assert!(r.stderr.contains("unknown option"));
    }

    #[tokio::test]
    async fn test_interactive_mode_hint() {
        let ts = TypeScript::from_config(&TypeScriptConfig::default(), "ts");
        let args: Vec<String> = vec![];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = ts.execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("hint:"), "should contain hint text");
        assert!(r.stderr.contains("ts -c"), "should suggest -c flag");
    }

    #[tokio::test]
    async fn test_interactive_mode_hint_disabled() {
        let config = TypeScriptConfig::default().unsupported_mode_hint(false);
        let ts = TypeScript::from_config(&config, "ts");
        let args: Vec<String> = vec![];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        let r = ts.execute(ctx).await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(!r.stderr.contains("hint:"), "should not contain hint text");
    }
}
