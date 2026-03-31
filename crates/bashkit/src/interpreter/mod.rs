//! Interpreter for executing bash scripts
//!
//! # Fail Points (enabled with `failpoints` feature)
//!
//! - `interp::execute_command` - Inject failures in command execution
//! - `interp::expand_variable` - Inject failures in variable expansion
//! - `interp::execute_function` - Inject failures in function calls

// Interpreter uses chars().last().unwrap() and chars().next().unwrap() after
// validating string contents. This is safe because we check for non-empty strings.
#![allow(clippy::unwrap_used)]

mod glob;
mod jobs;
mod state;

#[allow(unused_imports)]
pub use jobs::{JobTable, SharedJobTable};
pub use state::{BuiltinSideEffect, ControlFlow, ExecResult};
// Re-export snapshot type for public API

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Monotonic counter for unique process substitution file paths
static PROC_SUB_COUNTER: AtomicU64 = AtomicU64::new(0);

use futures::FutureExt;

use crate::builtins::{self, Builtin};
#[cfg(feature = "failpoints")]
use crate::error::Error;
use crate::error::Result;
use crate::fs::FileSystem;
use crate::limits::{ExecutionCounters, ExecutionLimits, SessionLimits};

/// A single command history entry.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The command line as entered
    pub command: String,
    /// Unix timestamp when the command was executed
    pub timestamp: i64,
    /// Working directory at execution time
    pub cwd: String,
    /// Exit code of the command
    pub exit_code: i32,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Callback for streaming output chunks as they are produced.
///
/// Arguments: `(stdout_chunk, stderr_chunk)`. Called after each loop iteration
/// and each top-level command completes. Only non-empty chunks trigger a call.
///
/// Requires `Send + Sync` because the interpreter holds this across `.await` points.
/// Closures capturing `Arc<Mutex<_>>` satisfy both bounds automatically.
pub type OutputCallback = Box<dyn FnMut(&str, &str) + Send + Sync>;
use crate::parser::{
    ArithmeticForCommand, Assignment, AssignmentValue, CaseCommand, Command, CommandList,
    CompoundCommand, CoprocCommand, ForCommand, FunctionDef, IfCommand, ListOperator, ParameterOp,
    Parser, Pipeline, Redirect, RedirectKind, Script, SelectCommand, SimpleCommand, Span,
    TimeCommand, UntilCommand, WhileCommand, Word, WordPart,
};

#[cfg(feature = "failpoints")]
use fail::fail_point;

/// The canonical /dev/null path.
/// This is handled at the interpreter level to prevent custom filesystems from bypassing it.
const DEV_NULL: &str = "/dev/null";

/// Check if a name is a shell keyword (for `command -v`/`command -V`).
fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "then"
            | "else"
            | "elif"
            | "fi"
            | "for"
            | "while"
            | "until"
            | "do"
            | "done"
            | "case"
            | "esac"
            | "in"
            | "function"
            | "select"
            | "time"
            | "{"
            | "}"
            | "[["
            | "]]"
            | "!"
    )
}

/// Borrowed reference to interpreter shell state for builtins.
///
/// Provides:
/// - **Direct mutable access** to aliases and traps (simple HashMaps, no invariants)
/// - **Read-only access** to functions, builtins, call stack, history, jobs
///
/// Design rationale: aliases and traps are directly mutable because they're
/// simple HashMap state with no invariants to enforce. Arrays use
/// [`BuiltinSideEffect`] because they need memory budget checking.
/// History uses side effects for VFS persistence.
///
/// All fields are disjoint from `Context`'s mutable borrows (variables, cwd),
/// enabling safe split borrowing in `dispatch_command`.
pub(crate) struct ShellRef<'a> {
    /// Direct mutable access to shell aliases.
    pub(crate) aliases: &'a mut HashMap<String, String>,
    /// Direct mutable access to trap handlers.
    pub(crate) traps: &'a mut HashMap<String, String>,
    /// Registered builtin commands (read-only, accessed via `has_builtin`).
    pub(crate) builtins: &'a HashMap<String, Box<dyn Builtin>>,
    /// Defined shell functions (read-only, accessed via `has_function`).
    pub(crate) functions: &'a HashMap<String, FunctionDef>,
    /// Call stack frames (read-only, accessed via `call_stack_depth`/`call_stack_frame_name`).
    call_stack: &'a [CallFrame],
    /// Command history (read-only, accessed via `history_entries`).
    pub(crate) history: &'a [HistoryEntry],
    /// Shared job table (read-only, accessed via `jobs`).
    pub(crate) jobs: &'a SharedJobTable,
}

impl ShellRef<'_> {
    /// Check if a name is a registered builtin command.
    pub(crate) fn has_builtin(&self, name: &str) -> bool {
        self.builtins.contains_key(name)
    }

    /// Check if a name is a defined shell function.
    pub(crate) fn has_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Check if a name is a shell keyword.
    pub(crate) fn is_keyword(&self, name: &str) -> bool {
        is_keyword(name)
    }

    /// Get call stack depth (number of active function frames).
    pub(crate) fn call_stack_depth(&self) -> usize {
        self.call_stack.len()
    }

    /// Get function name at a given frame index (0 = most recent).
    pub(crate) fn call_stack_frame_name(&self, idx: usize) -> Option<&str> {
        if self.call_stack.is_empty() {
            return None;
        }
        // idx 0 = most recent frame (last in vec)
        let vec_idx = self.call_stack.len().checked_sub(1 + idx)?;
        Some(self.call_stack[vec_idx].name.as_str())
    }

    /// Get command history entries.
    pub(crate) fn history_entries(&self) -> &[HistoryEntry] {
        self.history
    }

    /// Get the shared job table for wait operations.
    pub(crate) fn jobs(&self) -> &SharedJobTable {
        self.jobs
    }
}

/// Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = b.len();
    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Hint for common commands that are unavailable in the sandbox.
fn unavailable_command_hint(name: &str) -> Option<&'static str> {
    match name {
        "pip" | "pip3" | "pip2" => Some("Package managers are not available in the sandbox."),
        "apt" | "apt-get" | "yum" | "dnf" | "pacman" | "brew" | "apk" => {
            Some("Package managers are not available in the sandbox.")
        }
        "npm" | "yarn" | "pnpm" | "bun" => {
            Some("Package managers are not available in the sandbox.")
        }
        "sudo" | "su" | "doas" => Some("All commands run without privilege restrictions."),
        "ssh" | "scp" | "sftp" | "rsync" => Some("Network access is limited to curl/wget."),
        "docker" | "podman" | "kubectl" | "systemctl" | "service" => {
            Some("Container and service management is not available in the sandbox.")
        }
        "make" | "cmake" | "gcc" | "g++" | "clang" | "rustc" | "cargo" | "go" | "javac"
        | "node" => Some("Compilers and build tools are not available in the sandbox."),
        "vi" | "vim" | "nano" | "emacs" => {
            Some("Interactive editors are not available. Use echo/printf/cat to write files.")
        }
        "man" | "info" => Some("Manual pages are not available in the sandbox."),
        _ => None,
    }
}

/// Build a "command not found" error with optional suggestions.
fn command_not_found_message(name: &str, known_commands: &[&str]) -> String {
    let mut msg = format!("bash: {}: command not found", name);

    // Check for unavailable command hints first
    if let Some(hint) = unavailable_command_hint(name) {
        msg.push_str(&format!(". {}", hint));
        return msg;
    }

    // Find close matches via Levenshtein distance
    let max_dist = if name.len() <= 3 { 1 } else { 2 };
    let mut suggestions: Vec<(&str, usize)> = known_commands
        .iter()
        .filter_map(|cmd| {
            let d = levenshtein(name, cmd);
            if d > 0 && d <= max_dist {
                Some((*cmd, d))
            } else {
                None
            }
        })
        .collect();
    suggestions.sort_unstable_by(|(left_name, left_dist), (right_name, right_dist)| {
        left_dist
            .cmp(right_dist)
            .then_with(|| left_name.cmp(right_name))
    });
    suggestions.truncate(3);

    if !suggestions.is_empty() {
        let names: Vec<&str> = suggestions.iter().map(|(s, _)| *s).collect();
        msg.push_str(&format!(". Did you mean: {}?", names.join(", ")));
    }

    msg
}

/// Check if a path refers to /dev/null after normalization.
/// Handles attempts to bypass via paths like `/dev/../dev/null`.
/// Convert bytes to string preserving all byte values (Latin-1/ISO 8859-1 mapping).
/// Each byte 0x00-0xFF maps to the corresponding Unicode code point.
/// This avoids the lossy UTF-8 conversion that replaces bytes > 0x7F with U+FFFD.
fn bytes_to_latin1_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn is_dev_null(path: &Path) -> bool {
    // Normalize the path to handle .. and . components
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir => normalized.push("/"),
            std::path::Component::Normal(name) => normalized.push(name),
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            std::path::Component::Prefix(_) => {}
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push("/");
    }
    normalized == Path::new(DEV_NULL)
}

/// THREAT[TM-INJ-009,TM-INJ-016]: Check if a variable name is an internal marker.
/// Used by builtins and interpreter to block user assignment to internal prefixes.
pub(crate) fn is_internal_variable(name: &str) -> bool {
    name.starts_with("_NAMEREF_")
        || name.starts_with("_READONLY_")
        || name.starts_with("_UPPER_")
        || name.starts_with("_LOWER_")
        || name.starts_with("_INTEGER_")
        || name.starts_with("_ARRAY_READ_")
        || name == "_EVAL_CMD"
        || name == "_SHIFT_COUNT"
        || name == "_SET_POSITIONAL"
}

/// Check if a string is a valid shell variable name: `[a-zA-Z_][a-zA-Z0-9_]*`.
///
/// Single canonical copy used by interpreter and builtins.
pub(crate) fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Flags shared between `declare` and `local` builtins.
#[derive(Default)]
struct DeclareFlags {
    nameref: bool,
    array: bool,
    assoc: bool,
    integer: bool,
}

impl DeclareFlags {
    /// Parse common declare/local flags from a flag argument like "-naAi".
    fn parse_flag_chars(&mut self, flag_arg: &str) {
        for c in flag_arg[1..].chars() {
            match c {
                'n' => self.nameref = true,
                'a' => self.array = true,
                'A' => self.assoc = true,
                'i' => self.integer = true,
                _ => {}
            }
        }
    }
}

/// Reconstruct compound assignments that were split across arguments.
///
/// Shell compound assignments like `arr=(1 2 3)` get split into
/// `["arr=(1", "2", "3)"]` by the parser. This merges them back.
fn merge_compound_assignments<S: AsRef<str>>(args: &[S]) -> Vec<String> {
    let mut merged = Vec::new();
    let mut pending: Option<String> = None;
    for arg in args {
        let s = arg.as_ref();
        if let Some(ref mut p) = pending {
            p.push(' ');
            p.push_str(s);
            if s.ends_with(')') {
                merged.push(p.clone());
                pending = None;
            }
        } else if s.contains("=(") && !s.ends_with(')') {
            pending = Some(s.to_string());
        } else {
            merged.push(s.to_string());
        }
    }
    if let Some(p) = pending {
        merged.push(p);
    }
    merged
}

/// A frame in the call stack for local variable scoping
#[derive(Debug, Clone)]
struct CallFrame {
    /// Function name
    name: String,
    /// Local variables in this scope
    locals: HashMap<String, String>,
    /// Positional parameters ($1, $2, etc.)
    positional: Vec<String>,
}

/// A snapshot of shell state (variables, env, cwd, options).
///
/// Captures the serializable portions of the interpreter state.
/// Combined with [`VfsSnapshot`](crate::VfsSnapshot) this provides
/// full session snapshot/restore.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShellState {
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Shell variables
    pub variables: HashMap<String, String>,
    /// Indexed arrays
    pub arrays: HashMap<String, HashMap<usize, String>>,
    /// Associative arrays
    pub assoc_arrays: HashMap<String, HashMap<String, String>>,
    /// Current working directory
    pub cwd: PathBuf,
    /// Last exit code
    pub last_exit_code: i32,
    /// Shell aliases
    pub aliases: HashMap<String, String>,
    /// Trap handlers
    pub traps: HashMap<String, String>,
}

/// Interpreter state.
pub struct Interpreter {
    fs: Arc<dyn FileSystem>,
    env: HashMap<String, String>,
    variables: HashMap<String, String>,
    /// Arrays - stored as name -> index -> value
    arrays: HashMap<String, HashMap<usize, String>>,
    /// Associative arrays (declare -A) - stored as name -> key -> value
    assoc_arrays: HashMap<String, HashMap<String, String>>,
    cwd: PathBuf,
    last_exit_code: i32,
    /// Built-in commands (default + custom)
    builtins: HashMap<String, Box<dyn Builtin>>,
    /// Defined functions
    functions: HashMap<String, FunctionDef>,
    /// Call stack for local variable scoping
    call_stack: Vec<CallFrame>,
    /// Source file stack for BASH_SOURCE array
    bash_source_stack: Vec<String>,
    /// Resource limits
    limits: ExecutionLimits,
    /// Session-level resource limits (persist across exec() calls)
    session_limits: SessionLimits,
    /// Per-instance memory limits
    memory_limits: crate::limits::MemoryLimits,
    /// Memory budget tracker
    memory_budget: crate::limits::MemoryBudget,
    /// Trace event collector
    trace: crate::trace::TraceCollector,
    /// Execution counters for resource tracking
    counters: ExecutionCounters,
    /// Job table for background execution (shared for wait builtin access)
    jobs: SharedJobTable,
    /// Current line number for $LINENO
    current_line: usize,
    /// HTTP client for network builtins (curl, wget)
    #[cfg(feature = "http_client")]
    http_client: Option<crate::network::HttpClient>,
    /// Git client for git builtins
    #[cfg(feature = "git")]
    git_client: Option<crate::git::GitClient>,
    /// Stdin inherited from pipeline for compound commands (while read, etc.)
    /// Each read operation consumes one line, advancing through the data.
    pipeline_stdin: Option<String>,
    /// Optional callback for streaming output chunks during execution.
    /// When set, output is emitted incrementally via this callback in addition
    /// to being accumulated in the returned ExecResult.
    output_callback: Option<OutputCallback>,
    /// Monotonic counter incremented each time output is emitted via callback.
    /// Used to detect whether sub-calls already emitted output, preventing duplicates.
    output_emit_count: u64,
    /// Pending nounset (set -u) error message, consumed by execute_command.
    nounset_error: Option<String>,
    /// Trap handlers: signal/event name -> command string
    traps: HashMap<String, String>,
    /// PIPESTATUS: exit codes of the last pipeline's commands
    pipestatus: Vec<i32>,
    /// Shell aliases: name -> expansion value
    aliases: HashMap<String, String>,
    /// Aliases currently being expanded (prevents infinite recursion).
    /// When alias `foo` expands to `foo bar`, the inner `foo` is not re-expanded.
    expanding_aliases: HashSet<String>,
    /// Command history entries for the current session.
    history: Vec<HistoryEntry>,
    /// Optional VFS path for persisting history between sessions.
    history_file: Option<PathBuf>,
    /// Whether history has been loaded from VFS (to avoid re-loading on each exec).
    history_loaded: bool,
    /// Monotonic counter incremented on each command substitution execution.
    /// Used to detect whether assignment value expansion ran a command substitution
    /// (for correct exit code: plain assignment → 0, assignment with subst → subst's exit code).
    subst_generation: u64,
    /// Coprocess read buffers: maps virtual FD number to remaining lines.
    /// When a coproc runs, its stdout is split into lines and stored here
    /// so `read -u FD` or `read <&FD` can consume them one at a time.
    coproc_buffers: HashMap<i32, Vec<String>>,
    /// Next virtual FD to assign for coproc read ends (starts at 63, like bash).
    coproc_next_fd: i32,
    /// Cancellation token: when set to `true`, execution aborts at the next
    /// command boundary with `Error::Cancelled`.
    cancelled: Arc<AtomicBool>,
    /// Deferred output process substitutions: after a command writes to the
    /// virtual file path, run these commands with the file content as stdin.
    /// Each entry is (virtual_path, commands_to_run).
    deferred_proc_subs: Vec<(String, Vec<Command>)>,
}

impl Interpreter {
    const MAX_GLOB_DEPTH: usize = 50;

    /// Create a new interpreter with the given filesystem.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self::with_config(fs, None, None, None, HashMap::new())
    }

    /// Create a new interpreter with custom username, hostname, and builtins.
    ///
    /// # Arguments
    ///
    /// * `fs` - The virtual filesystem to use
    /// * `username` - Optional custom username for virtual identity
    /// * `hostname` - Optional custom hostname for virtual identity
    /// * `custom_builtins` - Custom builtins to register (override defaults if same name)
    pub fn with_config(
        fs: Arc<dyn FileSystem>,
        username: Option<String>,
        hostname: Option<String>,
        fixed_epoch: Option<i64>,
        custom_builtins: HashMap<String, Box<dyn Builtin>>,
    ) -> Self {
        // Macro to reduce boilerplate for simple zero-arg builtin registration.
        // Custom-construction builtins (date, source, hostname, etc.) are registered below.
        macro_rules! register_builtins {
            ($map:ident, $( $name:literal => $type:ident ),+ $(,)?) => {
                $( $map.insert($name.to_string(), Box::new(builtins::$type) as Box<dyn Builtin>); )+
            };
        }

        let mut builtins: HashMap<String, Box<dyn Builtin>> = HashMap::new();

        register_builtins!(builtins,
            // Core shell builtins
            "echo" => Echo,
            "true" => True,
            "false" => False,
            "exit" => Exit,
            "cd" => Cd,
            "pwd" => Pwd,
            "cat" => Cat,
            "break" => Break,
            "continue" => Continue,
            "return" => Return,
            "test" => Test,
            "[" => Bracket,
            "printf" => Printf,
            "export" => Export,
            "read" => Read,
            "set" => Set,
            "unset" => Unset,
            "shift" => Shift,
            "local" => Local,
            // POSIX special built-ins
            ":" => Colon,
            "readonly" => Readonly,
            "times" => Times,
            "eval" => Eval,
            // Text processing
            "jq" => Jq,
            "grep" => Grep,
            "sed" => Sed,
            "awk" => Awk,
            "head" => Head,
            "tail" => Tail,
            "sort" => Sort,
            "uniq" => Uniq,
            "cut" => Cut,
            "tr" => Tr,
            "wc" => Wc,
            "nl" => Nl,
            "paste" => Paste,
            "column" => Column,
            "comm" => Comm,
            "diff" => Diff,
            "strings" => Strings,
            "tac" => Tac,
            "rev" => Rev,
            "fold" => Fold,
            "expand" => Expand,
            "unexpand" => Unexpand,
            "join" => Join,
            "split" => Split,
            // File operations
            "basename" => Basename,
            "dirname" => Dirname,
            "realpath" => Realpath,
            "readlink" => Readlink,
            "mkdir" => Mkdir,
            "mktemp" => Mktemp,
            "mkfifo" => Mkfifo,
            "rm" => Rm,
            "cp" => Cp,
            "mv" => Mv,
            "touch" => Touch,
            "chmod" => Chmod,
            "ln" => Ln,
            "chown" => Chown,
            "rmdir" => Rmdir,
            // Directory listing and search
            "ls" => Ls,
            "find" => Find,
            "tree" => Tree,
            // File inspection
            "less" => Less,
            "file" => File,
            "stat" => Stat,
            // Binary / encoding
            "od" => Od,
            "xxd" => Xxd,
            "hexdump" => Hexdump,
            "base64" => Base64,
            "md5sum" => Md5sum,
            "sha1sum" => Sha1sum,
            "sha256sum" => Sha256sum,
            // Archive operations
            "tar" => Tar,
            "gzip" => Gzip,
            "gunzip" => Gunzip,
            "zip" => Zip,
            "unzip" => Unzip,
            // Numeric / math
            "seq" => Seq,
            "expr" => Expr,
            "bc" => Bc,
            "numfmt" => Numfmt,
            // Misc utilities
            "yes" => Yes,
            "sleep" => Sleep,
            "kill" => Kill,
            "wait" => Wait,
            "timeout" => Timeout,
            // Navigation
            "pushd" => Pushd,
            "popd" => Popd,
            "dirs" => Dirs,
            // Disk usage
            "du" => Du,
            "df" => Df,
            // Environment
            "env" => Env,
            "printenv" => Printenv,
            "history" => History,
            // Network
            "curl" => Curl,
            "wget" => Wget,
            "http" => Http,
            // Pipeline control
            "xargs" => Xargs,
            "tee" => Tee,
            "watch" => Watch,
            // Shell introspection (moved from interpreter if-chain)
            "type" => Type,
            "which" => Which,
            "hash" => Hash,
            "alias" => Alias,
            "unalias" => Unalias,
            "trap" => Trap,
            "caller" => Caller,
            "mapfile" => Mapfile,
            "readarray" => Mapfile,
            // Shell options
            "shopt" => Shopt,
            "clear" => Clear,
            // Extended builtins
            "envsubst" => Envsubst,
            "assert" => Assert,
            "dotenv" => Dotenv,
            "glob" => GlobCmd,
            "log" => Log,
            "retry" => Retry,
            "semver" => Semver,
            "verify" => Verify,
            "compgen" => Compgen,
            "csv" => Csv,
            "fc" => Fc,
            "help" => Help,
            "iconv" => Iconv,
            "json" => Json,
            "parallel" => Parallel,
            "patch" => Patch,
            "rg" => Rg,
            "template" => Template,
            "tomlq" => Tomlq,
            "yaml" => Yaml,
        );

        // Custom-construction builtins that need parameters

        // source/. requires filesystem access
        builtins.insert(
            "source".to_string(),
            Box::new(builtins::Source::new(fs.clone())),
        );
        builtins.insert(".".to_string(), Box::new(builtins::Source::new(fs.clone())));

        // THREAT[TM-INF-018]: Use fixed epoch if configured, else real clock
        builtins.insert(
            "date".to_string(),
            Box::new(if let Some(epoch) = fixed_epoch {
                use chrono::DateTime;
                builtins::Date::with_fixed_epoch(
                    DateTime::from_timestamp(epoch, 0).unwrap_or_default(),
                )
            } else {
                builtins::Date::new()
            }),
        );

        // System info builtins (configurable virtual values)
        let hostname_val = hostname.unwrap_or_else(|| builtins::DEFAULT_HOSTNAME.to_string());
        let username_val = username.unwrap_or_else(|| builtins::DEFAULT_USERNAME.to_string());
        builtins.insert(
            "hostname".to_string(),
            Box::new(builtins::Hostname::with_hostname(&hostname_val)),
        );
        builtins.insert(
            "uname".to_string(),
            Box::new(builtins::Uname::with_hostname(&hostname_val)),
        );
        builtins.insert(
            "whoami".to_string(),
            Box::new(builtins::Whoami::with_username(&username_val)),
        );
        builtins.insert(
            "id".to_string(),
            Box::new(builtins::Id::with_username(&username_val)),
        );

        // Git builtin (requires git feature and configuration at runtime)
        #[cfg(feature = "git")]
        builtins.insert("git".to_string(), Box::new(builtins::Git));

        // Merge custom builtins (override defaults if same name)
        for (name, builtin) in custom_builtins {
            builtins.insert(name, builtin);
        }

        // Initialize default shell variables
        let mut variables = HashMap::new();
        variables.insert("HOME".to_string(), format!("/home/{}", &username_val));
        variables.insert("USER".to_string(), username_val.clone());
        variables.insert("UID".to_string(), "1000".to_string());
        variables.insert("EUID".to_string(), "1000".to_string());
        variables.insert("HOSTNAME".to_string(), hostname_val.clone());

        // BASH_VERSINFO array: (major minor patch build status machine)
        let version = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = version.split('.').collect();
        let mut bash_versinfo = HashMap::new();
        bash_versinfo.insert(0, parts.first().unwrap_or(&"0").to_string());
        bash_versinfo.insert(1, parts.get(1).unwrap_or(&"0").to_string());
        bash_versinfo.insert(2, parts.get(2).unwrap_or(&"0").to_string());
        bash_versinfo.insert(3, "0".to_string());
        bash_versinfo.insert(4, "release".to_string());
        bash_versinfo.insert(5, "virtual".to_string());

        let mut arrays = HashMap::new();
        arrays.insert("BASH_VERSINFO".to_string(), bash_versinfo);

        Self {
            fs,
            env: HashMap::new(),
            variables,
            arrays,
            assoc_arrays: HashMap::new(),
            cwd: PathBuf::from("/home/user"),
            last_exit_code: 0,
            builtins,
            functions: HashMap::new(),
            call_stack: Vec::new(),
            bash_source_stack: Vec::new(),
            limits: ExecutionLimits::default(),
            session_limits: SessionLimits::default(),
            memory_limits: crate::limits::MemoryLimits::default(),
            memory_budget: crate::limits::MemoryBudget::default(),
            trace: crate::trace::TraceCollector::default(),
            counters: ExecutionCounters::new(),
            jobs: jobs::new_shared_job_table(),
            current_line: 1,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            pipeline_stdin: None,
            output_callback: None,
            output_emit_count: 0,
            nounset_error: None,
            traps: HashMap::new(),
            pipestatus: Vec::new(),
            aliases: HashMap::new(),
            expanding_aliases: HashSet::new(),
            history: Vec::new(),
            history_file: None,
            history_loaded: false,
            subst_generation: 0,
            coproc_buffers: HashMap::new(),
            coproc_next_fd: 63,
            cancelled: Arc::new(AtomicBool::new(false)),
            deferred_proc_subs: Vec::new(),
        }
    }

    /// Return a shared cancellation token. Set it to `true` from any thread
    /// to abort execution at the next command boundary.
    pub fn cancellation_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Check if cancellation has been requested.
    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(crate::error::Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Check if errexit (set -e) is enabled.
    /// Sync the internal bash_source_stack to the BASH_SOURCE indexed array.
    fn update_bash_source(&mut self) {
        let arr: HashMap<usize, String> = self
            .bash_source_stack
            .iter()
            .rev()
            .enumerate()
            .map(|(i, s)| (i, s.clone()))
            .collect();
        self.arrays.insert("BASH_SOURCE".to_string(), arr);
    }

    fn is_errexit_enabled(&self) -> bool {
        self.variables
            .get("SHOPT_e")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if xtrace (set -x) is enabled.
    fn is_xtrace_enabled(&self) -> bool {
        self.variables
            .get("SHOPT_x")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Set execution limits.
    pub fn set_limits(&mut self, limits: ExecutionLimits) {
        self.limits = limits;
    }

    /// Set session-level limits.
    pub fn set_session_limits(&mut self, limits: SessionLimits) {
        self.session_limits = limits;
    }

    /// Set per-instance memory limits.
    pub fn set_memory_limits(&mut self, limits: crate::limits::MemoryLimits) {
        self.memory_limits = limits;
    }

    /// Set the trace collector.
    pub fn set_trace(&mut self, trace: crate::trace::TraceCollector) {
        self.trace = trace;
    }

    /// Get execution limits.
    pub fn limits(&self) -> &ExecutionLimits {
        &self.limits
    }

    /// THREAT[TM-ISO-005/006/007]: Reset per-exec transient state.
    /// Called by Bash::exec() before each top-level execution to prevent
    /// traps, exit code, and shell options from leaking across calls.
    pub fn reset_transient_state(&mut self) {
        self.traps.clear();
        self.last_exit_code = 0;
        self.variables.retain(|k, _| !k.starts_with("SHOPT_"));
    }

    /// Set an environment variable.
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_string(), value.to_string());
    }

    /// Set a shell variable (public API for builder).
    pub fn set_var(&mut self, key: &str, value: &str) {
        self.variables.insert(key.to_string(), value.to_string());
    }

    /// Set the current working directory.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    /// Get the current working directory.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Record a history entry for the current session.
    pub fn record_history(
        &mut self,
        command: String,
        timestamp: i64,
        cwd: String,
        exit_code: i32,
        duration_ms: u64,
    ) {
        self.history.push(HistoryEntry {
            command,
            timestamp,
            cwd,
            exit_code,
            duration_ms,
        });
    }

    /// Set the VFS path for persisting history.
    pub fn set_history_file(&mut self, path: PathBuf) {
        self.history_file = Some(path);
    }

    /// Get a reference to the history entries.
    #[allow(dead_code)]
    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// Clear all history entries.
    #[allow(dead_code)]
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Load history from the VFS history file (if configured). No-op after first call.
    pub async fn load_history(&mut self) {
        if self.history_loaded {
            return;
        }
        self.history_loaded = true;
        let path = match &self.history_file {
            Some(p) => p.clone(),
            None => return,
        };
        let bytes = match self.fs.read_file(&path).await {
            Ok(b) => b,
            Err(_) => return, // File doesn't exist yet
        };
        let content = String::from_utf8_lossy(&bytes);
        for line in content.lines() {
            // Format: timestamp|exit_code|duration_ms|cwd|command
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() == 5
                && let (Ok(ts), Ok(ec), Ok(dur)) = (
                    parts[0].parse::<i64>(),
                    parts[1].parse::<i32>(),
                    parts[2].parse::<u64>(),
                )
            {
                self.history.push(HistoryEntry {
                    timestamp: ts,
                    exit_code: ec,
                    duration_ms: dur,
                    cwd: parts[3].to_string(),
                    command: parts[4].to_string(),
                });
            }
        }
    }

    /// Save history to the VFS history file (if configured).
    pub async fn save_history(&self) {
        let path = match &self.history_file {
            Some(p) => p.clone(),
            None => return,
        };
        let mut content = String::new();
        for entry in &self.history {
            use std::fmt::Write;
            let _ = writeln!(
                content,
                "{}|{}|{}|{}|{}",
                entry.timestamp, entry.exit_code, entry.duration_ms, entry.cwd, entry.command
            );
        }
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            let _ = self.fs.mkdir(parent, true).await;
        }
        let _ = self.fs.write_file(&path, content.as_bytes()).await;
    }

    /// Capture the current shell state (variables, env, cwd, options).
    pub fn shell_state(&self) -> ShellState {
        ShellState {
            env: self.env.clone(),
            variables: self.variables.clone(),
            arrays: self.arrays.clone(),
            assoc_arrays: self.assoc_arrays.clone(),
            cwd: self.cwd.clone(),
            last_exit_code: self.last_exit_code,
            aliases: self.aliases.clone(),
            traps: self.traps.clone(),
        }
    }

    /// Restore shell state from a snapshot.
    pub fn restore_shell_state(&mut self, state: &ShellState) {
        self.env = state.env.clone();
        self.variables = state.variables.clone();
        self.arrays = state.arrays.clone();
        self.assoc_arrays = state.assoc_arrays.clone();
        self.cwd = state.cwd.clone();
        self.last_exit_code = state.last_exit_code;
        self.aliases = state.aliases.clone();
        self.traps = state.traps.clone();
    }

    /// Set an output callback for streaming output during execution.
    ///
    /// When set, the interpreter calls this callback with `(stdout_chunk, stderr_chunk)`
    /// after each loop iteration, command list element, and top-level command.
    /// Output is still accumulated in the returned `ExecResult` for the final result.
    pub fn set_output_callback(&mut self, callback: OutputCallback) {
        self.output_callback = Some(callback);
        self.output_emit_count = 0;
    }

    /// Clear the output callback.
    pub fn clear_output_callback(&mut self) {
        self.output_callback = None;
        self.output_emit_count = 0;
    }

    /// Emit output via the callback if set, and if sub-calls didn't already emit.
    /// Returns `true` if output was emitted.
    ///
    /// `emit_count_before` is the value of `output_emit_count` before the sub-call
    /// that produced this output. If the count advanced, sub-calls already emitted
    /// and we skip to avoid duplicates.
    fn maybe_emit_output(&mut self, stdout: &str, stderr: &str, emit_count_before: u64) -> bool {
        if self.output_callback.is_none() {
            return false;
        }
        // Sub-calls already emitted — skip to avoid duplicates
        if self.output_emit_count != emit_count_before {
            return false;
        }
        if stdout.is_empty() && stderr.is_empty() {
            return false;
        }
        if let Some(ref mut cb) = self.output_callback {
            cb(stdout, stderr);
            self.output_emit_count += 1;
        }
        true
    }

    /// Set the HTTP client for network builtins (curl, wget).
    ///
    /// This is only available when the `http_client` feature is enabled.
    #[cfg(feature = "http_client")]
    pub fn set_http_client(&mut self, client: crate::network::HttpClient) {
        self.http_client = Some(client);
    }

    /// Set the git client for git builtins.
    ///
    /// This is only available when the `git` feature is enabled.
    #[cfg(feature = "git")]
    pub fn set_git_client(&mut self, client: crate::git::GitClient) {
        self.git_client = Some(client);
    }

    /// Execute a script.
    pub async fn execute(&mut self, script: &Script) -> Result<ExecResult> {
        // Reset per-execution counters so each exec() gets a fresh budget.
        // Without this, hitting the limit in one exec() permanently poisons the session.
        self.counters.reset_for_execution();

        // Note: per-exec state reset (traps, exit code, options) is done in
        // Bash::exec() before calling this method, to avoid clearing state
        // set by internal callers like execute_bash_builtin.

        // THREAT[TM-DOS-059]: Increment session-level exec call counter and
        // check session limits before starting execution.
        self.counters.tick_exec_call();
        self.counters
            .check_session_limits(&self.session_limits)
            .map_err(|e| crate::error::Error::Execution(e.to_string()))?;

        self.execute_script_body(script, true).await
    }

    /// Inner script execution — runs commands without resetting counters.
    /// Used by `execute_source` to preserve function/source depth tracking.
    /// `run_exit_trap`: only the top-level `execute` runs the EXIT trap.
    async fn execute_script_body(
        &mut self,
        script: &Script,
        run_exit_trap: bool,
    ) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;
        let mut stdout_truncated = false;
        let mut stderr_truncated = false;
        let max_stdout = self.limits.max_stdout_bytes;
        let max_stderr = self.limits.max_stderr_bytes;

        for command in &script.commands {
            self.check_cancelled()?;
            let emit_before = self.output_emit_count;
            let result = self.execute_command(command).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);

            // Accumulate stdout with truncation
            if !stdout_truncated {
                let remaining = max_stdout.saturating_sub(stdout.len());
                if remaining == 0 {
                    if !result.stdout.is_empty() {
                        stdout_truncated = true;
                    }
                } else if result.stdout.len() <= remaining {
                    stdout.push_str(&result.stdout);
                } else {
                    stdout.push_str(&result.stdout[..remaining]);
                    stdout_truncated = true;
                }
            }

            // Accumulate stderr with truncation
            if !stderr_truncated {
                let remaining = max_stderr.saturating_sub(stderr.len());
                if remaining == 0 {
                    if !result.stderr.is_empty() {
                        stderr_truncated = true;
                    }
                } else if result.stderr.len() <= remaining {
                    stderr.push_str(&result.stderr);
                } else {
                    stderr.push_str(&result.stderr[..remaining]);
                    stderr_truncated = true;
                }
            }

            exit_code = result.exit_code;
            self.last_exit_code = exit_code;

            // Stop on control flow (e.g. nounset error uses Return to abort)
            if result.control_flow != ControlFlow::None {
                break;
            }

            // Run ERR trap on non-zero exit (unless in conditional chain)
            if exit_code != 0 {
                let suppressed = matches!(command, Command::List(_))
                    || matches!(command, Command::Pipeline(p) if p.negated)
                    || result.errexit_suppressed;
                if !suppressed {
                    self.run_err_trap(&mut stdout, &mut stderr).await;
                }
            }

            // errexit (set -e): stop on non-zero exit for top-level simple commands.
            // List commands handle errexit internally (with && / || chain awareness).
            // Negated pipelines (! cmd) explicitly handle the exit code.
            // Compound commands propagate errexit_suppressed from inner AND-OR chains.
            if self.is_errexit_enabled() && exit_code != 0 {
                let suppressed = matches!(command, Command::List(_))
                    || matches!(command, Command::Pipeline(p) if p.negated)
                    || result.errexit_suppressed;
                if !suppressed {
                    break;
                }
            }
        }

        // Run EXIT trap if registered (only for top-level execute)
        #[allow(clippy::collapsible_if)]
        if run_exit_trap {
            if let Some(trap_cmd) = self.traps.get("EXIT").cloned() {
                // THREAT[TM-DOS-030]: Propagate interpreter parser limits
                if let Ok(trap_script) = Parser::with_limits(
                    &trap_cmd,
                    self.limits.max_ast_depth,
                    self.limits.max_parser_operations,
                )
                .parse()
                {
                    let emit_before = self.output_emit_count;
                    if let Ok(trap_result) =
                        self.execute_command_sequence(&trap_script.commands).await
                    {
                        self.maybe_emit_output(
                            &trap_result.stdout,
                            &trap_result.stderr,
                            emit_before,
                        );
                        stdout.push_str(&trap_result.stdout);
                        stderr.push_str(&trap_result.stderr);
                    }
                }
            }
        }

        let final_env = if self.limits.capture_final_env {
            Some(self.variables.clone())
        } else {
            None
        };

        let events = self.trace.take_events();

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
            stdout_truncated,
            stderr_truncated,
            final_env,
            events,
            ..Default::default()
        })
    }

    /// Get the source line number from a command's span
    fn command_line(command: &Command) -> usize {
        match command {
            Command::Simple(c) => c.span.line(),
            Command::Pipeline(c) => c.span.line(),
            Command::List(c) => c.span.line(),
            Command::Compound(c, _) => match c {
                CompoundCommand::If(cmd) => cmd.span.line(),
                CompoundCommand::For(cmd) => cmd.span.line(),
                CompoundCommand::ArithmeticFor(cmd) => cmd.span.line(),
                CompoundCommand::While(cmd) => cmd.span.line(),
                CompoundCommand::Until(cmd) => cmd.span.line(),
                CompoundCommand::Case(cmd) => cmd.span.line(),
                CompoundCommand::Select(cmd) => cmd.span.line(),
                CompoundCommand::Time(cmd) => cmd.span.line(),
                CompoundCommand::Coproc(cmd) => cmd.span.line(),
                CompoundCommand::Subshell(_) | CompoundCommand::BraceGroup(_) => 1,
                CompoundCommand::Arithmetic(_) | CompoundCommand::Conditional(_) => 1,
            },
            Command::Function(c) => c.span.line(),
        }
    }

    fn execute_command<'a>(
        &'a mut self,
        command: &'a Command,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            self.check_cancelled()?;
            // Update current line for $LINENO
            self.current_line = Self::command_line(command);

            // Fail point: inject failures during command execution
            #[cfg(feature = "failpoints")]
            fail_point!("interp::execute_command", |action| {
                match action.as_deref() {
                    Some("panic") => {
                        // Test panic recovery
                        panic!("injected panic in execute_command");
                    }
                    Some("error") => {
                        return Err(Error::Execution("injected execution error".to_string()));
                    }
                    Some("exit_nonzero") => {
                        // Return non-zero exit code without error
                        return Ok(ExecResult {
                            stdout: String::new(),
                            stderr: "injected failure".to_string(),
                            exit_code: 127,
                            control_flow: ControlFlow::None,
                            ..Default::default()
                        });
                    }
                    _ => {}
                }
                Ok(ExecResult::ok(String::new()))
            });

            // Check command count limit (per-exec)
            self.counters.tick_command(&self.limits)?;
            // THREAT[TM-DOS-059]: Check session-level command limit
            self.counters
                .check_session_limits(&self.session_limits)
                .map_err(|e| crate::error::Error::Execution(e.to_string()))?;

            match command {
                Command::Simple(simple) => self.execute_simple_command(simple, None).await,
                Command::Pipeline(pipeline) => self.execute_pipeline(pipeline).await,
                Command::List(list) => self.execute_list(list).await,
                Command::Compound(compound, redirects) => {
                    // Process input redirections before executing compound
                    let stdin = self.process_input_redirections(None, redirects).await?;
                    let prev_pipeline_stdin = if stdin.is_some() {
                        let prev = self.pipeline_stdin.take();
                        self.pipeline_stdin = stdin;
                        Some(prev)
                    } else {
                        None
                    };
                    let result = self.execute_compound(compound).await?;
                    if let Some(prev) = prev_pipeline_stdin {
                        self.pipeline_stdin = prev;
                    }
                    if redirects.is_empty() {
                        Ok(result)
                    } else {
                        self.apply_redirections(result, redirects).await
                    }
                }
                Command::Function(func_def) => {
                    // THREAT[TM-DOS-060]: Check function count/size budget
                    let body_bytes = func_def
                        .span
                        .end
                        .offset
                        .saturating_sub(func_def.span.start.offset);
                    let is_new = !self.functions.contains_key(&func_def.name);
                    let old_body_bytes = if is_new {
                        0
                    } else {
                        self.functions
                            .get(&func_def.name)
                            .map(|f| f.span.end.offset.saturating_sub(f.span.start.offset))
                            .unwrap_or(0)
                    };
                    if self
                        .memory_budget
                        .check_function_insert(
                            body_bytes,
                            is_new,
                            old_body_bytes,
                            &self.memory_limits,
                        )
                        .is_ok()
                    {
                        self.memory_budget.record_function_insert(
                            body_bytes,
                            is_new,
                            old_body_bytes,
                        );
                        self.functions
                            .insert(func_def.name.clone(), func_def.clone());
                    }
                    Ok(ExecResult::ok(String::new()))
                }
            }
        })
    }

    /// Execute a compound command (if, for, while, etc.)
    async fn execute_compound(&mut self, compound: &CompoundCommand) -> Result<ExecResult> {
        match compound {
            CompoundCommand::If(if_cmd) => self.execute_if(if_cmd).await,
            CompoundCommand::For(for_cmd) => self.execute_for(for_cmd).await,
            CompoundCommand::ArithmeticFor(arith_for) => {
                self.execute_arithmetic_for(arith_for).await
            }
            CompoundCommand::While(while_cmd) => self.execute_while(while_cmd).await,
            CompoundCommand::Until(until_cmd) => self.execute_until(until_cmd).await,
            CompoundCommand::Subshell(commands) => {
                // Subshells run in fully isolated scope: variables, arrays,
                // functions, cwd, traps, positional params, and options are
                // all snapshot/restored so mutations don't leak to the parent.
                let saved_vars = self.variables.clone();
                let saved_arrays = self.arrays.clone();
                let saved_assoc = self.assoc_arrays.clone();
                let saved_functions = self.functions.clone();
                let saved_cwd = self.cwd.clone();
                let saved_traps = self.traps.clone();
                let saved_call_stack = self.call_stack.clone();
                let saved_exit = self.last_exit_code;
                let saved_aliases = self.aliases.clone();
                let saved_coproc = self.coproc_buffers.clone();

                let mut result = self.execute_command_sequence(commands).await;

                // Fire EXIT trap set inside the subshell before restoring parent state
                if let Some(trap_cmd) = self.traps.get("EXIT").cloned() {
                    // Only fire if the subshell set its own EXIT trap (different from parent)
                    let parent_had_same = saved_traps.get("EXIT") == Some(&trap_cmd);
                    if !parent_had_same {
                        // THREAT[TM-DOS-030]: Propagate interpreter parser limits
                        if let Ok(trap_script) = Parser::with_limits(
                            &trap_cmd,
                            self.limits.max_ast_depth,
                            self.limits.max_parser_operations,
                        )
                        .parse()
                        {
                            let emit_before = self.output_emit_count;
                            if let Ok(ref mut res) = result
                                && let Ok(trap_result) =
                                    self.execute_command_sequence(&trap_script.commands).await
                            {
                                self.maybe_emit_output(
                                    &trap_result.stdout,
                                    &trap_result.stderr,
                                    emit_before,
                                );
                                res.stdout.push_str(&trap_result.stdout);
                                res.stderr.push_str(&trap_result.stderr);
                            }
                        }
                    }
                }

                self.variables = saved_vars;
                self.arrays = saved_arrays;
                self.assoc_arrays = saved_assoc;
                self.functions = saved_functions;
                self.cwd = saved_cwd;
                self.traps = saved_traps;
                self.call_stack = saved_call_stack;
                self.last_exit_code = saved_exit;
                self.aliases = saved_aliases;
                self.coproc_buffers = saved_coproc;

                // Consume Exit control flow at subshell boundary — exit only
                // terminates the subshell, not the parent shell.
                if let Ok(ref mut res) = result
                    && let ControlFlow::Exit(code) = res.control_flow
                {
                    res.exit_code = code;
                    res.control_flow = ControlFlow::None;
                }

                result
            }
            CompoundCommand::BraceGroup(commands) => self.execute_command_sequence(commands).await,
            CompoundCommand::Case(case_cmd) => self.execute_case(case_cmd).await,
            CompoundCommand::Select(select_cmd) => self.execute_select(select_cmd).await,
            CompoundCommand::Arithmetic(expr) => self.execute_arithmetic_command(expr).await,
            CompoundCommand::Time(time_cmd) => self.execute_time(time_cmd).await,
            CompoundCommand::Conditional(words) => self.execute_conditional(words).await,
            CompoundCommand::Coproc(coproc_cmd) => self.execute_coproc(coproc_cmd).await,
        }
    }

    /// Execute an if statement
    async fn execute_if(&mut self, if_cmd: &IfCommand) -> Result<ExecResult> {
        // Accumulate stdout/stderr from all condition evaluations
        let mut cond_stdout = String::new();
        let mut cond_stderr = String::new();

        // Execute condition (no errexit checking - conditions are expected to fail)
        let condition_result = self.execute_condition_sequence(&if_cmd.condition).await?;
        cond_stdout.push_str(&condition_result.stdout);
        cond_stderr.push_str(&condition_result.stderr);

        if condition_result.exit_code == 0 {
            // Condition succeeded, execute then branch
            let mut result = self.execute_command_sequence(&if_cmd.then_branch).await?;
            result.stdout = cond_stdout + &result.stdout;
            result.stderr = cond_stderr + &result.stderr;
            return Ok(result);
        }

        // Check elif branches
        for (elif_condition, elif_body) in &if_cmd.elif_branches {
            let elif_result = self.execute_condition_sequence(elif_condition).await?;
            cond_stdout.push_str(&elif_result.stdout);
            cond_stderr.push_str(&elif_result.stderr);

            if elif_result.exit_code == 0 {
                let mut result = self.execute_command_sequence(elif_body).await?;
                result.stdout = cond_stdout + &result.stdout;
                result.stderr = cond_stderr + &result.stderr;
                return Ok(result);
            }
        }

        // Execute else branch if present
        if let Some(else_branch) = &if_cmd.else_branch {
            let mut result = self.execute_command_sequence(else_branch).await?;
            result.stdout = cond_stdout + &result.stdout;
            result.stderr = cond_stderr + &result.stderr;
            return Ok(result);
        }

        // No branch executed, return condition output with success exit code
        Ok(ExecResult {
            stdout: cond_stdout,
            stderr: cond_stderr,
            exit_code: 0,
            ..Default::default()
        })
    }

    /// Execute a for loop
    async fn execute_for(&mut self, for_cmd: &ForCommand) -> Result<ExecResult> {
        // Validate for-loop variable name (bash rejects invalid names at runtime, exit 1)
        if !is_valid_var_name(&for_cmd.variable) {
            return Ok(ExecResult::err(
                format!("bash: `{}': not a valid identifier\n", for_cmd.variable),
                1,
            ));
        }

        let mut acc = state::LoopAccumulator::new();

        // Get iteration values: expand fields, then apply brace/glob expansion
        let values: Vec<String> = if let Some(words) = &for_cmd.words {
            let mut vals = Vec::new();
            for w in words {
                let fields = self.expand_word_to_fields(w).await?;

                // Quoted words skip brace/glob expansion
                if w.quoted {
                    vals.extend(fields);
                    continue;
                }

                for expanded in fields {
                    let brace_expanded = self.expand_braces(&expanded);
                    for item in brace_expanded {
                        match self.expand_glob_item(&item).await {
                            Ok(items) => vals.extend(items),
                            Err(pat) => {
                                self.last_exit_code = 1;
                                return Ok(ExecResult::err(
                                    format!("-bash: no match: {}\n", pat),
                                    1,
                                ));
                            }
                        }
                    }
                }
            }
            vals
        } else {
            // No words specified - iterate over positional parameters ($@)
            self.call_stack
                .last()
                .map(|frame| frame.positional.clone())
                .unwrap_or_default()
        };

        // Reset loop counter for this loop
        self.counters.reset_loop();

        for value in values {
            // Check loop iteration limit
            self.counters.tick_loop(&self.limits)?;

            // Set loop variable (respects nameref)
            self.set_variable(for_cmd.variable.clone(), value.clone());

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(&for_cmd.body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            match acc.accumulate(result) {
                state::LoopAction::None => {}
                state::LoopAction::Break => break,
                state::LoopAction::Continue => continue,
                state::LoopAction::Exit(r) => return Ok(r),
            }
        }

        Ok(acc.finish())
    }

    /// Execute a select loop: select var in list; do body; done
    ///
    /// Reads lines from pipeline_stdin. Each line is treated as the user's
    /// menu selection. If the line is a valid number, the variable is set to
    /// the corresponding item; otherwise it is set to empty. REPLY is always
    /// set to the raw input. EOF ends the loop.
    async fn execute_select(&mut self, select_cmd: &SelectCommand) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        // Expand word list
        let mut values = Vec::new();
        for w in &select_cmd.words {
            let fields = self.expand_word_to_fields(w).await?;
            if w.quoted {
                values.extend(fields);
            } else {
                for expanded in fields {
                    let brace_expanded = self.expand_braces(&expanded);
                    for item in brace_expanded {
                        match self.expand_glob_item(&item).await {
                            Ok(items) => values.extend(items),
                            Err(pat) => {
                                self.last_exit_code = 1;
                                return Ok(ExecResult::err(
                                    format!("-bash: no match: {}\n", pat),
                                    1,
                                ));
                            }
                        }
                    }
                }
            }
        }

        if values.is_empty() {
            return Ok(ExecResult {
                stdout,
                stderr,
                exit_code,
                control_flow: ControlFlow::None,
                ..Default::default()
            });
        }

        // Build menu string
        let menu: String = values
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{}) {}", i + 1, v))
            .collect::<Vec<_>>()
            .join("\n");

        let ps3 = self
            .variables
            .get("PS3")
            .cloned()
            .unwrap_or_else(|| "#? ".to_string());

        // Reset loop counter
        self.counters.reset_loop();

        loop {
            self.counters.tick_loop(&self.limits)?;

            // Output menu to stderr
            stderr.push_str(&menu);
            stderr.push('\n');
            stderr.push_str(&ps3);

            // Read a line from pipeline_stdin
            let line = if let Some(ref ps) = self.pipeline_stdin {
                if ps.is_empty() {
                    // EOF: bash prints newline and exits with code 1
                    stdout.push('\n');
                    exit_code = 1;
                    break;
                }
                let data = ps.clone();
                if let Some(newline_pos) = data.find('\n') {
                    let line = data[..newline_pos].to_string();
                    self.pipeline_stdin = Some(data[newline_pos + 1..].to_string());
                    line
                } else {
                    self.pipeline_stdin = Some(String::new());
                    data
                }
            } else {
                // No stdin: bash prints newline and exits with code 1
                stdout.push('\n');
                exit_code = 1;
                break;
            };

            // Set REPLY to raw input
            self.insert_variable_checked("REPLY".to_string(), line.clone());

            // Parse selection number
            let selected = line
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|n| {
                    if n >= 1 && n <= values.len() {
                        Some(values[n - 1].clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            self.insert_variable_checked(select_cmd.variable.clone(), selected);

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(&select_cmd.body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;

            // Check for break/continue
            match result.control_flow {
                ControlFlow::Break(n) => {
                    if n <= 1 {
                        break;
                    } else {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Break(n - 1),
                            ..Default::default()
                        });
                    }
                }
                ControlFlow::Continue(n) => {
                    if n <= 1 {
                        continue;
                    } else {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Continue(n - 1),
                            ..Default::default()
                        });
                    }
                }
                ControlFlow::Return(code) => {
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code: code,
                        control_flow: ControlFlow::Return(code),
                        ..Default::default()
                    });
                }
                ControlFlow::Exit(code) => {
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code: code,
                        control_flow: ControlFlow::Exit(code),
                        ..Default::default()
                    });
                }
                ControlFlow::None => {}
            }
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
            ..Default::default()
        })
    }

    /// Execute a C-style arithmetic for loop: for ((init; cond; step))
    async fn execute_arithmetic_for(
        &mut self,
        arith_for: &ArithmeticForCommand,
    ) -> Result<ExecResult> {
        let mut acc = state::LoopAccumulator::new();

        // Execute initialization
        if !arith_for.init.is_empty() {
            self.execute_arithmetic_with_side_effects(&arith_for.init);
        }

        // Reset loop counter for this loop
        self.counters.reset_loop();

        loop {
            // Check loop iteration limit
            self.counters.tick_loop(&self.limits)?;

            // Check condition (if empty, always true)
            if !arith_for.condition.is_empty() {
                let cond_result = self.evaluate_arithmetic(&arith_for.condition);
                if cond_result == 0 {
                    break;
                }
            }

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(&arith_for.body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            match acc.accumulate(result) {
                state::LoopAction::None | state::LoopAction::Continue => {}
                state::LoopAction::Break => break,
                state::LoopAction::Exit(r) => return Ok(r),
            }

            // Execute step
            if !arith_for.step.is_empty() {
                self.execute_arithmetic_with_side_effects(&arith_for.step);
            }
        }

        Ok(acc.finish())
    }

    /// Execute an arithmetic command ((expression))
    /// Returns exit code 0 if result is non-zero, 1 if result is zero
    /// Execute a [[ conditional expression ]]
    async fn execute_conditional(&mut self, words: &[Word]) -> Result<ExecResult> {
        // Expand all words
        let mut expanded = Vec::new();
        for word in words {
            expanded.push(self.expand_word(word).await?);
        }

        let result = self.evaluate_conditional(&expanded).await;
        let exit_code = if result { 0 } else { 1 };
        self.last_exit_code = exit_code;

        Ok(ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
            control_flow: ControlFlow::None,
            ..Default::default()
        })
    }

    /// Evaluate a [[ ]] conditional expression from expanded words.
    fn evaluate_conditional<'a>(
        &'a mut self,
        args: &'a [String],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            if args.is_empty() {
                return false;
            }

            // Handle negation
            if args[0] == "!" {
                return !self.evaluate_conditional(&args[1..]).await;
            }

            // Handle parentheses
            if args.first().map(|s| s.as_str()) == Some("(")
                && args.last().map(|s| s.as_str()) == Some(")")
            {
                return self.evaluate_conditional(&args[1..args.len() - 1]).await;
            }

            // Look for logical operators: || has lowest precedence, then &&.
            // Scan for || first (split at lowest precedence first).
            for i in (0..args.len()).rev() {
                if args[i] == "||" && i > 0 {
                    return self.evaluate_conditional(&args[..i]).await
                        || self.evaluate_conditional(&args[i + 1..]).await;
                }
            }
            for i in (0..args.len()).rev() {
                if args[i] == "&&" && i > 0 {
                    return self.evaluate_conditional(&args[..i]).await
                        && self.evaluate_conditional(&args[i + 1..]).await;
                }
            }

            match args.len() {
                1 => !args[0].is_empty(),
                2 => {
                    // Unary operators
                    let resolve = |p: &str| -> std::path::PathBuf {
                        let path = std::path::Path::new(p);
                        if path.is_absolute() {
                            path.to_path_buf()
                        } else {
                            self.cwd.join(path)
                        }
                    };
                    match args[0].as_str() {
                        "-z" => args[1].is_empty(),
                        "-n" => !args[1].is_empty(),
                        "-e" | "-a" => self.fs.exists(&resolve(&args[1])).await.unwrap_or(false),
                        "-f" => self
                            .fs
                            .stat(&resolve(&args[1]))
                            .await
                            .map(|m| m.file_type.is_file())
                            .unwrap_or(false),
                        "-d" => self
                            .fs
                            .stat(&resolve(&args[1]))
                            .await
                            .map(|m| m.file_type.is_dir())
                            .unwrap_or(false),
                        "-r" | "-w" | "-x" => {
                            self.fs.exists(&resolve(&args[1])).await.unwrap_or(false)
                        }
                        "-s" => self
                            .fs
                            .stat(&resolve(&args[1]))
                            .await
                            .map(|m| m.size > 0)
                            .unwrap_or(false),
                        "-t" => {
                            // fd is a terminal — configurable via _TTY_N variables
                            let fd_key = format!("_TTY_{}", args[1]);
                            self.variables
                                .get(&fd_key)
                                .map(|v| v == "1")
                                .unwrap_or(false)
                        }
                        _ => !args[0].is_empty(),
                    }
                }
                3 => {
                    // Binary operators
                    match args[1].as_str() {
                        "=" | "==" => self.pattern_matches(&args[0], &args[2]),
                        "!=" => !self.pattern_matches(&args[0], &args[2]),
                        "<" => args[0] < args[2],
                        ">" => args[0] > args[2],
                        "-eq" => {
                            args[0].parse::<i64>().unwrap_or(0)
                                == args[2].parse::<i64>().unwrap_or(0)
                        }
                        "-ne" => {
                            args[0].parse::<i64>().unwrap_or(0)
                                != args[2].parse::<i64>().unwrap_or(0)
                        }
                        "-lt" => {
                            args[0].parse::<i64>().unwrap_or(0)
                                < args[2].parse::<i64>().unwrap_or(0)
                        }
                        "-le" => {
                            args[0].parse::<i64>().unwrap_or(0)
                                <= args[2].parse::<i64>().unwrap_or(0)
                        }
                        "-gt" => {
                            args[0].parse::<i64>().unwrap_or(0)
                                > args[2].parse::<i64>().unwrap_or(0)
                        }
                        "-ge" => {
                            args[0].parse::<i64>().unwrap_or(0)
                                >= args[2].parse::<i64>().unwrap_or(0)
                        }
                        "=~" => self.regex_match(&args[0], &args[2]),
                        "-nt" => {
                            let lm = self.fs.stat(std::path::Path::new(&args[0])).await;
                            let rm = self.fs.stat(std::path::Path::new(&args[2])).await;
                            match (lm, rm) {
                                (Ok(l), Ok(r)) => l.modified > r.modified,
                                (Ok(_), Err(_)) => true,
                                _ => false,
                            }
                        }
                        "-ot" => {
                            let lm = self.fs.stat(std::path::Path::new(&args[0])).await;
                            let rm = self.fs.stat(std::path::Path::new(&args[2])).await;
                            match (lm, rm) {
                                (Ok(l), Ok(r)) => l.modified < r.modified,
                                (Err(_), Ok(_)) => true,
                                _ => false,
                            }
                        }
                        "-ef" => {
                            let lp = crate::builtins::resolve_path(
                                &std::path::PathBuf::from("/"),
                                &args[0],
                            );
                            let rp = crate::builtins::resolve_path(
                                &std::path::PathBuf::from("/"),
                                &args[2],
                            );
                            lp == rp
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        })
    }

    /// Perform regex match and set BASH_REMATCH array.
    fn regex_match(&mut self, string: &str, pattern: &str) -> bool {
        match regex::Regex::new(pattern) {
            Ok(re) => {
                if let Some(captures) = re.captures(string) {
                    // Set BASH_REMATCH array
                    let mut rematch = HashMap::new();
                    for (i, m) in captures.iter().enumerate() {
                        rematch.insert(i, m.map(|m| m.as_str().to_string()).unwrap_or_default());
                    }
                    self.arrays.insert("BASH_REMATCH".to_string(), rematch);
                    true
                } else {
                    self.arrays.remove("BASH_REMATCH");
                    false
                }
            }
            Err(_) => {
                self.arrays.remove("BASH_REMATCH");
                false
            }
        }
    }

    async fn execute_arithmetic_command(&mut self, expr: &str) -> Result<ExecResult> {
        let result = self.execute_arithmetic_with_side_effects(expr);
        let exit_code = if result != 0 { 0 } else { 1 };

        Ok(ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
            control_flow: ControlFlow::None,
            ..Default::default()
        })
    }

    /// Execute arithmetic expression with side effects (assignments, ++, --)
    fn execute_arithmetic_with_side_effects(&mut self, expr: &str) -> i64 {
        let expr = expr.trim();

        // Handle comma-separated expressions
        if expr.contains(',') {
            let parts: Vec<&str> = expr.split(',').collect();
            let mut result = 0;
            for part in parts {
                result = self.execute_arithmetic_with_side_effects(part.trim());
            }
            return result;
        }

        // Handle assignment: var = expr or var op= expr
        if let Some(eq_pos) = expr.find('=') {
            // Check it's not ==, !=, <=, >=
            // eq_pos is a byte offset from find(), so use byte-safe slicing
            let before_eq = &expr[..eq_pos];
            let before = before_eq.chars().last();
            let after = expr[eq_pos + 1..].chars().next();

            if after != Some('=') && !matches!(before, Some('!' | '<' | '>' | '=')) {
                // This is an assignment
                let lhs = expr[..eq_pos].trim();
                let rhs = expr[eq_pos + 1..].trim();

                // Check for compound assignment (+=, -=, *=, /=, %=)
                let (var_name, op, effective_rhs) = if lhs.ends_with('+')
                    || lhs.ends_with('-')
                    || lhs.ends_with('*')
                    || lhs.ends_with('/')
                    || lhs.ends_with('%')
                {
                    let op = lhs.chars().last().unwrap();
                    let name = lhs[..lhs.len() - 1].trim();
                    (name, Some(op), rhs)
                } else {
                    (lhs, None, rhs)
                };

                let rhs_value = self.execute_arithmetic_with_side_effects(effective_rhs);
                let final_value = if let Some(op) = op {
                    let current = self.evaluate_arithmetic(var_name);
                    // THREAT[TM-DOS-043]: wrapping to prevent overflow panic
                    match op {
                        '+' => current.wrapping_add(rhs_value),
                        '-' => current.wrapping_sub(rhs_value),
                        '*' => current.wrapping_mul(rhs_value),
                        '/' => {
                            if rhs_value != 0 && !(current == i64::MIN && rhs_value == -1) {
                                current / rhs_value
                            } else {
                                0
                            }
                        }
                        '%' => {
                            if rhs_value != 0 && !(current == i64::MIN && rhs_value == -1) {
                                current % rhs_value
                            } else {
                                0
                            }
                        }
                        _ => rhs_value,
                    }
                } else {
                    rhs_value
                };

                self.set_variable(var_name.to_string(), final_value.to_string());
                return final_value;
            }
        }

        // Handle pre-increment/decrement: ++var or --var
        if let Some(stripped) = expr.strip_prefix("++") {
            let trimmed = stripped.trim_start();
            // Extract the variable name (leading identifier chars)
            let var_end = trimmed
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(trimmed.len());
            let var_name = &trimmed[..var_end];
            if !var_name.is_empty() && is_valid_var_name(var_name) {
                let current = self.evaluate_arithmetic(var_name);
                let new_value = current + 1;
                self.set_variable(var_name.to_string(), new_value.to_string());
                let rest = trimmed[var_end..].trim();
                if rest.is_empty() {
                    return new_value;
                }
                // Complex expression: substitute the incremented value and evaluate
                // e.g. "++i > 3" → increment i, then evaluate "1 > 3"
                let full_expr = format!("{new_value}{rest}");
                return self.evaluate_arithmetic(&full_expr);
            }
        }
        if let Some(stripped) = expr.strip_prefix("--") {
            let trimmed = stripped.trim_start();
            let var_end = trimmed
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(trimmed.len());
            let var_name = &trimmed[..var_end];
            if !var_name.is_empty() && is_valid_var_name(var_name) {
                let current = self.evaluate_arithmetic(var_name);
                let new_value = current - 1;
                self.set_variable(var_name.to_string(), new_value.to_string());
                let rest = trimmed[var_end..].trim();
                if rest.is_empty() {
                    return new_value;
                }
                let full_expr = format!("{new_value}{rest}");
                return self.evaluate_arithmetic(&full_expr);
            }
        }

        // Handle post-increment/decrement: var++ or var--
        if let Some(stripped) = expr.strip_suffix("++") {
            let var_name = stripped.trim();
            if is_valid_var_name(var_name) {
                let current = self.evaluate_arithmetic(var_name);
                let new_value = current + 1;
                self.set_variable(var_name.to_string(), new_value.to_string());
                return current; // Return old value for post-increment
            }
        }
        if let Some(stripped) = expr.strip_suffix("--") {
            let var_name = stripped.trim();
            if is_valid_var_name(var_name) {
                let current = self.evaluate_arithmetic(var_name);
                let new_value = current - 1;
                self.set_variable(var_name.to_string(), new_value.to_string());
                return current; // Return old value for post-decrement
            }
        }

        // No side effects, just evaluate
        self.evaluate_arithmetic(expr)
    }

    /// Execute a while loop
    async fn execute_while(&mut self, while_cmd: &WhileCommand) -> Result<ExecResult> {
        self.execute_condition_loop(&while_cmd.condition, &while_cmd.body, false)
            .await
    }

    /// Execute an until loop
    async fn execute_until(&mut self, until_cmd: &UntilCommand) -> Result<ExecResult> {
        self.execute_condition_loop(&until_cmd.condition, &until_cmd.body, true)
            .await
    }

    /// Shared implementation for while/until loops.
    /// `break_on_zero`: false = while (break when condition fails), true = until (break when condition succeeds)
    async fn execute_condition_loop(
        &mut self,
        condition: &[Command],
        body: &[Command],
        break_on_zero: bool,
    ) -> Result<ExecResult> {
        let mut acc = state::LoopAccumulator::new();

        // Reset loop counter for this loop
        self.counters.reset_loop();

        loop {
            // Check loop iteration limit
            self.counters.tick_loop(&self.limits)?;

            // Check condition (no errexit - conditions are expected to fail)
            let emit_before_cond = self.output_emit_count;
            let condition_result = self.execute_condition_sequence(condition).await?;
            // Condition commands produce visible output (e.g., `while cat <<EOF; do ... done`)
            self.maybe_emit_output(
                &condition_result.stdout,
                &condition_result.stderr,
                emit_before_cond,
            );
            acc.stdout.push_str(&condition_result.stdout);
            acc.stderr.push_str(&condition_result.stderr);
            let should_break = if break_on_zero {
                condition_result.exit_code == 0
            } else {
                condition_result.exit_code != 0
            };
            if should_break {
                break;
            }

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            match acc.accumulate(result) {
                state::LoopAction::None => {}
                state::LoopAction::Break => break,
                state::LoopAction::Continue => continue,
                state::LoopAction::Exit(r) => return Ok(r),
            }
        }

        Ok(acc.finish())
    }

    /// Execute a case statement
    async fn execute_case(&mut self, case_cmd: &CaseCommand) -> Result<ExecResult> {
        use crate::parser::CaseTerminator;
        let word_value = self.expand_word(&case_cmd.word).await?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;
        let mut fallthrough = false;

        for case_item in &case_cmd.cases {
            let matched = if fallthrough {
                true
            } else {
                let mut m = false;
                for pattern in &case_item.patterns {
                    let pattern_str = self.expand_word(pattern).await?;
                    if self.pattern_matches(&word_value, &pattern_str) {
                        m = true;
                        break;
                    }
                }
                m
            };

            if matched {
                let r = self.execute_command_sequence(&case_item.commands).await?;
                stdout.push_str(&r.stdout);
                stderr.push_str(&r.stderr);
                exit_code = r.exit_code;
                match case_item.terminator {
                    CaseTerminator::Break => {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: r.control_flow,
                            ..Default::default()
                        });
                    }
                    CaseTerminator::FallThrough => {
                        fallthrough = true;
                    }
                    CaseTerminator::Continue => {
                        fallthrough = false;
                    }
                }
            }
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
            ..Default::default()
        })
    }

    /// Execute a time command - measure wall-clock execution time
    ///
    /// Note: Bashkit only measures wall-clock (real) time.
    /// User and system CPU time are always reported as 0.
    /// This is a documented incompatibility with bash.
    async fn execute_time(&mut self, time_cmd: &TimeCommand) -> Result<ExecResult> {
        use std::time::Instant;

        let start = Instant::now();

        // Execute the wrapped command if present
        let mut result = if let Some(cmd) = &time_cmd.command {
            self.execute_command(cmd).await?
        } else {
            // time with no command - just output timing for nothing
            ExecResult::ok(String::new())
        };

        let elapsed = start.elapsed();

        // Calculate time components
        let total_secs = elapsed.as_secs_f64();
        let minutes = (total_secs / 60.0).floor() as u64;
        let seconds = total_secs % 60.0;

        // Format timing output (goes to stderr, per bash behavior)
        let timing = if time_cmd.posix_format {
            // POSIX format: simple, machine-readable
            format!("real {:.2}\nuser 0.00\nsys 0.00\n", total_secs)
        } else {
            // Default bash format
            format!(
                "\nreal\t{}m{:.3}s\nuser\t0m0.000s\nsys\t0m0.000s\n",
                minutes, seconds
            )
        };

        // Append timing to stderr (preserve command's stderr)
        result.stderr.push_str(&timing);

        Ok(result)
    }

    /// Execute a coprocess command.
    ///
    /// Runs the command body synchronously (bashkit's deterministic model),
    /// buffers its stdout for later reading via virtual FDs, sets the NAME
    /// array with FD numbers, and stores a virtual PID in NAME_PID.
    async fn execute_coproc(&mut self, coproc: &CoprocCommand) -> Result<ExecResult> {
        let name = &coproc.name;

        // Allocate virtual FD numbers (bash uses 63/60 by default)
        let read_fd = self.coproc_next_fd;
        let write_fd = self.coproc_next_fd - 1;
        self.coproc_next_fd -= 2; // reserve pair for next coproc

        // Execute the command body, capturing output
        let result = self.execute_command(&coproc.body).await?;

        // Buffer stdout lines for reading via the virtual read FD.
        // Lines are stored in reverse order so pop() yields the first line.
        let mut lines: Vec<String> = result.stdout.lines().map(|l| l.to_string()).collect();
        lines.reverse();
        self.coproc_buffers.insert(read_fd, lines);

        // Set NAME array: NAME[0] = read FD, NAME[1] = write FD
        let mut arr = HashMap::new();
        arr.insert(0, read_fd.to_string());
        arr.insert(1, write_fd.to_string());
        self.arrays.insert(name.clone(), arr);

        // Set NAME_PID to a virtual PID (use job table counter)
        let virtual_pid = {
            let table = self.jobs.lock().await;
            table.last_job_id().unwrap_or(0) + 1000
        };
        self.variables
            .insert(format!("{}_PID", name), virtual_pid.to_string());

        // Also set $! (last background PID)
        self.variables
            .insert("_LAST_BG_PID".to_string(), virtual_pid.to_string());

        // Coproc itself returns success with empty output (stdout was captured)
        Ok(ExecResult::ok(String::new()))
    }

    /// Check if `read -u FD` args reference a coproc FD and return next line if so.
    fn try_coproc_read_stdin(&mut self, args: &[String]) -> Option<String> {
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if arg == "-u"
                && let Some(fd_str) = iter.next()
                && let Ok(fd) = fd_str.parse::<i32>()
                && let Some(buf) = self.coproc_buffers.get_mut(&fd)
            {
                return if let Some(line) = buf.pop() {
                    Some(format!("{}\n", line))
                } else {
                    Some(String::new()) // EOF
                };
            } else if arg.starts_with("-u")
                && arg.len() > 2
                && let Ok(fd) = arg[2..].parse::<i32>()
                && let Some(buf) = self.coproc_buffers.get_mut(&fd)
            {
                return if let Some(line) = buf.pop() {
                    Some(format!("{}\n", line))
                } else {
                    Some(String::new()) // EOF
                };
            }
        }
        None
    }

    /// Execute `bash` or `sh` command - interpret scripts using this interpreter.
    ///
    /// Supports:
    /// - `bash -c "command"` - execute a command string
    /// - `bash -n script.sh` - syntax check only (noexec)
    /// - `bash script.sh [args...]` - execute a script file
    /// - `echo 'echo hello' | bash` - execute script from stdin
    /// - `bash --version` / `bash --help`
    ///
    /// SECURITY: This re-invokes the virtual interpreter, NOT external bash.
    /// See threat model TM-ESC-015 for security analysis.
    /// Map a `-o` option name to its internal variable representation.
    fn resolve_shell_option_name(opt: &str) -> Option<(&'static str, &'static str)> {
        match opt {
            "errexit" => Some(("SHOPT_e", "1")),
            "nounset" => Some(("SHOPT_u", "1")),
            "xtrace" => Some(("SHOPT_x", "1")),
            "verbose" => Some(("SHOPT_v", "1")),
            "pipefail" => Some(("SHOPT_pipefail", "1")),
            "noglob" => Some(("SHOPT_f", "1")),
            "noclobber" => Some(("SHOPT_C", "1")),
            _ => None,
        }
    }

    /// Parse `bash`/`sh` command-line arguments into structured form.
    /// Returns `Err(ExecResult)` for --version/--help (already produced output).
    #[allow(clippy::type_complexity, clippy::result_large_err)]
    fn parse_shell_args(
        shell_name: &str,
        args: &[String],
    ) -> std::result::Result<
        (
            Option<String>,                    // command_string (-c)
            Option<String>,                    // script_file
            Vec<String>,                       // script_args
            bool,                              // noexec
            Vec<(&'static str, &'static str)>, // shell_opts
        ),
        ExecResult,
    > {
        let mut command_string: Option<String> = None;
        let mut script_file: Option<String> = None;
        let mut script_args: Vec<String> = Vec::new();
        let mut noexec = false;
        let mut shell_opts: Vec<(&str, &str)> = Vec::new();
        let mut idx = 0;

        while idx < args.len() {
            let arg = &args[idx];
            match arg.as_str() {
                "--version" => {
                    return Err(ExecResult::ok(format!(
                        "Bashkit {} (virtual {} interpreter)\n",
                        env!("CARGO_PKG_VERSION"),
                        shell_name
                    )));
                }
                "--help" => {
                    return Err(ExecResult::ok(format!(
                        "Usage: {} [option] ... [file [argument] ...]\n\
                         Virtual shell interpreter (not GNU bash)\n\n\
                         Options:\n\
                         \t-c string\tExecute commands from string\n\
                         \t-n\t\tCheck syntax without executing (noexec)\n\
                         \t-e\t\tExit on error (errexit)\n\
                         \t-x\t\tPrint commands before execution (xtrace)\n\
                         \t-u\t\tError on unset variables (nounset)\n\
                         \t-o option\tSet option by name\n\
                         \t--version\tShow version\n\
                         \t--help\t\tShow this help\n",
                        shell_name
                    )));
                }
                "-c" => {
                    idx += 1;
                    if idx >= args.len() {
                        return Err(ExecResult::err(
                            format!("{}: -c: option requires an argument\n", shell_name),
                            2,
                        ));
                    }
                    command_string = Some(args[idx].clone());
                    idx += 1;
                    script_args = args[idx..].to_vec();
                    break;
                }
                "-n" => {
                    noexec = true;
                    idx += 1;
                }
                "-e" => {
                    shell_opts.push(("SHOPT_e", "1"));
                    idx += 1;
                }
                "-x" => {
                    shell_opts.push(("SHOPT_x", "1"));
                    idx += 1;
                }
                "-u" => {
                    shell_opts.push(("SHOPT_u", "1"));
                    idx += 1;
                }
                "-v" => {
                    shell_opts.push(("SHOPT_v", "1"));
                    idx += 1;
                }
                "-f" => {
                    shell_opts.push(("SHOPT_f", "1"));
                    idx += 1;
                }
                "-o" => {
                    idx += 1;
                    if idx >= args.len() {
                        return Err(ExecResult::err(
                            format!("{}: -o: option requires an argument\n", shell_name),
                            2,
                        ));
                    }
                    let opt = &args[idx];
                    if let Some(pair) = Self::resolve_shell_option_name(opt) {
                        shell_opts.push(pair);
                    } else {
                        return Err(ExecResult::err(
                            format!("{}: set: {}: invalid option name\n", shell_name, opt),
                            2,
                        ));
                    }
                    idx += 1;
                }
                "-i" | "-s" => {
                    idx += 1;
                }
                "--" => {
                    idx += 1;
                    if idx < args.len() {
                        script_file = Some(args[idx].clone());
                        idx += 1;
                        script_args = args[idx..].to_vec();
                    }
                    break;
                }
                s if s.starts_with("--") => {
                    idx += 1;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    let chars: Vec<char> = s.chars().skip(1).collect();
                    let mut ci = 0;
                    while ci < chars.len() {
                        match chars[ci] {
                            'n' => noexec = true,
                            'e' => shell_opts.push(("SHOPT_e", "1")),
                            'x' => shell_opts.push(("SHOPT_x", "1")),
                            'u' => shell_opts.push(("SHOPT_u", "1")),
                            'v' => shell_opts.push(("SHOPT_v", "1")),
                            'f' => shell_opts.push(("SHOPT_f", "1")),
                            'o' => {
                                idx += 1;
                                if idx < args.len()
                                    && let Some(pair) = Self::resolve_shell_option_name(&args[idx])
                                {
                                    shell_opts.push(pair);
                                }
                            }
                            _ => {}
                        }
                        ci += 1;
                    }
                    idx += 1;
                }
                _ => {
                    script_file = Some(arg.clone());
                    idx += 1;
                    script_args = args[idx..].to_vec();
                    break;
                }
            }
        }

        Ok((command_string, script_file, script_args, noexec, shell_opts))
    }

    async fn execute_shell(
        &mut self,
        shell_name: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Parse arguments — Err means early-return result (--version, --help, errors)
        let (command_string, script_file, script_args, noexec, shell_opts) =
            match Self::parse_shell_args(shell_name, args) {
                Ok(parsed) => parsed,
                Err(result) => return Ok(result),
            };

        // Determine what to execute
        let is_command_mode = command_string.is_some();
        let script_content = if let Some(cmd) = command_string {
            cmd
        } else if let Some(ref file) = script_file {
            let path = self.resolve_path(file);
            match self.fs.read_file(&path).await {
                Ok(content) => bytes_to_latin1_string(&content),
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("{}: {}: No such file or directory\n", shell_name, file),
                        127,
                    ));
                }
            }
        } else if let Some(ref stdin_content) = stdin {
            stdin_content.clone()
        } else {
            return Ok(ExecResult::ok(String::new()));
        };

        // THREAT[TM-DOS-021]: Propagate interpreter's parser limits to child shell
        let parser = Parser::with_limits(
            &script_content,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("{}: syntax error: {}\n", shell_name, e),
                    2,
                ));
            }
        };

        if noexec {
            return Ok(ExecResult::ok(String::new()));
        }

        // Determine $0 and positional parameters
        let (name_arg, positional_args) = if is_command_mode {
            if script_args.is_empty() {
                (shell_name.to_string(), Vec::new())
            } else {
                let name = script_args[0].clone();
                let positional = script_args[1..].to_vec();
                (name, positional)
            }
        } else if let Some(ref file) = script_file {
            (file.clone(), script_args)
        } else {
            (shell_name.to_string(), Vec::new())
        };

        // Push call frame, apply options, execute, restore, pop
        self.call_stack.push(CallFrame {
            name: name_arg,
            locals: HashMap::new(),
            positional: positional_args,
        });

        let mut saved_opts: Vec<(String, Option<String>)> = Vec::new();
        for (var, val) in &shell_opts {
            let prev = self.variables.get(*var).cloned();
            saved_opts.push((var.to_string(), prev));
            self.insert_variable_checked(var.to_string(), val.to_string());
        }
        let saved_optind = self.variables.get("OPTIND").cloned();
        let saved_optchar = self.variables.get("_OPTCHAR_IDX").cloned();
        self.insert_variable_checked("OPTIND".to_string(), "1".to_string());
        self.variables.remove("_OPTCHAR_IDX");

        let result = self.execute(&script).await;

        // Restore state
        if let Some(val) = saved_optind {
            self.insert_variable_checked("OPTIND".to_string(), val);
        } else {
            self.variables.remove("OPTIND");
        }
        if let Some(val) = saved_optchar {
            self.insert_variable_checked("_OPTCHAR_IDX".to_string(), val);
        } else {
            self.variables.remove("_OPTCHAR_IDX");
        }
        for (var, prev) in saved_opts {
            if let Some(val) = prev {
                self.insert_variable_checked(var, val);
            } else {
                self.variables.remove(&var);
            }
        }
        self.call_stack.pop();

        match result {
            Ok(exec_result) => self.apply_redirections(exec_result, redirects).await,
            Err(e) => Err(e),
        }
    }
}

/// Fd target for redirect fd-table modeling.
/// Bash processes redirects left-to-right, building an fd table where each
/// dup copies the *current* target of the source fd. This matters for
/// patterns like `2>&1 >file` where stderr must capture stdout's original
/// destination before stdout is redirected to the file.
#[derive(Clone, Debug)]
enum FdTarget {
    /// The original stdout pipe (terminal / command-substitution capture).
    Stdout,
    /// The original stderr pipe.
    Stderr,
    /// Write (truncate) to a file.
    WriteFile(PathBuf, String),
    /// Append to a file.
    AppendFile(PathBuf, String),
    /// Discard (/dev/null).
    DevNull,
}

impl Interpreter {
    /// Execute a sequence of commands (with errexit checking)
    async fn execute_command_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        self.execute_command_sequence_impl(commands, true).await
    }

    /// Execute a sequence of commands used as a condition (no errexit checking)
    /// Used for if/while/until conditions where failure is expected
    async fn execute_condition_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        self.execute_command_sequence_impl(commands, false).await
    }

    /// Execute a sequence of commands with optional errexit checking
    async fn execute_command_sequence_impl(
        &mut self,
        commands: &[Command],
        check_errexit: bool,
    ) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;
        let mut last_errexit_suppressed = false;

        for command in commands {
            let emit_before = self.output_emit_count;
            let result = self.execute_command(command).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
            self.last_exit_code = exit_code;

            // Propagate control flow
            if result.control_flow != ControlFlow::None {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: result.control_flow,
                    ..Default::default()
                });
            }

            // Check for errexit (set -e) if enabled.
            // Skip errexit for commands that are AND-OR lists — per POSIX, set -e
            // does not exit on failures that are part of && or || chains.
            // The list executor already handles errexit internally.
            // Also skip when the result has errexit_suppressed set — this means
            // a compound command (loop, etc.) ended with an AND-OR list exit code
            // that should not propagate errexit to the caller.
            let is_and_or_list = matches!(
                command,
                Command::List(list) if list.rest.iter().any(|(op, _)| matches!(op, ListOperator::And | ListOperator::Or))
            );
            let suppress = is_and_or_list || result.errexit_suppressed;
            if check_errexit && self.is_errexit_enabled() && exit_code != 0 && !suppress {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                    ..Default::default()
                });
            }
            last_errexit_suppressed = suppress && exit_code != 0;
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
            errexit_suppressed: last_errexit_suppressed,
            ..Default::default()
        })
    }

    /// Execute a pipeline (cmd1 | cmd2 | cmd3)
    async fn execute_pipeline(&mut self, pipeline: &Pipeline) -> Result<ExecResult> {
        let mut stdin_data: Option<String> = None;
        let mut last_result = ExecResult::ok(String::new());
        let mut pipe_statuses = Vec::new();

        for (i, command) in pipeline.commands.iter().enumerate() {
            let is_last = i == pipeline.commands.len() - 1;

            let result = match command {
                Command::Simple(simple) => {
                    self.execute_simple_command(simple, stdin_data.take())
                        .await?
                }
                _ => {
                    // Compound commands, lists, etc. in pipeline:
                    // set pipeline_stdin so inner commands (read, cat, etc.) can consume it
                    let prev_pipeline_stdin = self.pipeline_stdin.take();
                    self.pipeline_stdin = stdin_data.take();
                    let result = self.execute_command(command).await?;
                    self.pipeline_stdin = prev_pipeline_stdin;
                    result
                }
            };

            pipe_statuses.push(result.exit_code);

            if is_last {
                last_result = result;
            } else {
                stdin_data = Some(result.stdout);
            }
        }

        // Store PIPESTATUS array
        self.pipestatus = pipe_statuses.clone();
        let mut ps_arr = HashMap::new();
        for (i, code) in pipe_statuses.iter().enumerate() {
            ps_arr.insert(i, code.to_string());
        }
        self.arrays.insert("PIPESTATUS".to_string(), ps_arr);

        // pipefail: return rightmost non-zero exit code from pipeline
        if self.is_pipefail()
            && let Some(&nonzero) = pipe_statuses.iter().rev().find(|&&c| c != 0)
        {
            last_result.exit_code = nonzero;
        }

        // Handle negation
        if pipeline.negated {
            last_result.exit_code = if last_result.exit_code == 0 { 1 } else { 0 };
        }

        Ok(last_result)
    }

    /// Check if a command is the empty sentinel produced by the parser for trailing `&`.
    fn is_empty_sentinel(cmd: &Command) -> bool {
        if let Command::Simple(sc) = cmd {
            let name_is_empty = sc.name.parts.len() == 1
                && matches!(&sc.name.parts[0], WordPart::Literal(s) if s.is_empty());
            name_is_empty
                && sc.args.is_empty()
                && sc.redirects.is_empty()
                && sc.assignments.is_empty()
        } else {
            false
        }
    }

    /// Run a command as a "background" job.
    ///
    /// Executes the command synchronously (deterministic in virtual env) but
    /// stores the result in the job table so `wait` and `$!` work correctly.
    /// The command's stdout is emitted immediately (like real bash terminal output).
    async fn spawn_in_background(
        &mut self,
        cmd: &Command,
        parent_stdout: &mut String,
        parent_stderr: &mut String,
    ) -> Result<()> {
        // Execute the command synchronously
        let emit_before = self.output_emit_count;
        let result = self.execute_command(cmd).await?;
        self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);

        // Emit output immediately (background output goes to terminal in real bash)
        parent_stdout.push_str(&result.stdout);
        parent_stderr.push_str(&result.stderr);

        // Store only the exit code in the job table (output already emitted)
        let exit_code = result.exit_code;
        let job_result = ExecResult::with_code(String::new(), exit_code);
        let handle = tokio::spawn(async move { job_result });
        let job_id = self.jobs.lock().await.spawn(handle);
        self.variables
            .insert("_LAST_BG_PID".to_string(), job_id.to_string());

        // Background commands always return exit code 0 to the parent
        self.last_exit_code = 0;
        // But store the real exit code for $? after wait
        self.variables
            .insert("_BG_EXIT_CODE".to_string(), exit_code.to_string());
        Ok(())
    }

    /// Execute a command list (cmd1 && cmd2 || cmd3)
    #[allow(unused_assignments)] // control_flow may be set but overwritten
    async fn execute_list(&mut self, list: &CommandList) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code;
        let mut control_flow;

        // Determine if the first command should run in the background.
        // The `&` terminator for first appears as rest[0].op == Background.
        let first_is_bg = matches!(list.rest.first(), Some((ListOperator::Background, _)));

        if first_is_bg {
            self.spawn_in_background(&list.first, &mut stdout, &mut stderr)
                .await?;
            exit_code = 0;
            control_flow = ControlFlow::None;
        } else {
            let emit_before = self.output_emit_count;
            let result = self.execute_command(&list.first).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
            self.last_exit_code = exit_code;
            control_flow = result.control_flow;

            // If first command signaled control flow, return immediately
            if control_flow != ControlFlow::None {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow,
                    ..Default::default()
                });
            }

            // Check if first command in a semicolon-separated list failed => ERR trap
            let first_op_is_semicolon = list
                .rest
                .first()
                .is_some_and(|(op, _)| matches!(op, ListOperator::Semicolon));
            if exit_code != 0 && first_op_is_semicolon {
                self.run_err_trap(&mut stdout, &mut stderr).await;
            }
        }

        // Track if the list contains any && or || operators
        let has_conditional_operators = list
            .rest
            .iter()
            .any(|(op, _)| matches!(op, ListOperator::And | ListOperator::Or));

        // Track if we just exited a conditional chain (for errexit check)
        let mut just_exited_conditional_chain = false;

        for (i, (op, cmd)) in list.rest.iter().enumerate() {
            // Skip empty sentinel commands (produced by trailing `&`)
            if Self::is_empty_sentinel(cmd) {
                continue;
            }

            // Check if next operator (if any) is && or ||
            let next_op = list.rest.get(i + 1).map(|(op, _)| op);
            let current_is_conditional = matches!(op, ListOperator::And | ListOperator::Or);
            let next_is_conditional =
                matches!(next_op, Some(ListOperator::And) | Some(ListOperator::Or));

            // Determine if THIS command should be backgrounded.
            // A command is backgrounded when the NEXT separator is Background
            // (the `&` terminates the current command).
            let should_background =
                matches!(list.rest.get(i + 1), Some((ListOperator::Background, _)));

            // Check errexit before executing if:
            // - We just exited a conditional chain (and current op is semicolon)
            // - OR: current op is semicolon and previous wasn't in a conditional chain
            // - Exit code is non-zero
            let should_check_errexit = matches!(op, ListOperator::Semicolon)
                && !just_exited_conditional_chain
                && self.is_errexit_enabled()
                && exit_code != 0;

            if should_check_errexit {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                    ..Default::default()
                });
            }

            // Reset the flag
            just_exited_conditional_chain = false;

            // Mark that we're exiting a conditional chain
            if current_is_conditional && !next_is_conditional {
                just_exited_conditional_chain = true;
            }

            let should_execute = match op {
                ListOperator::And => exit_code == 0,
                ListOperator::Or => exit_code != 0,
                ListOperator::Semicolon | ListOperator::Background => true,
            };

            if should_execute {
                if should_background {
                    self.spawn_in_background(cmd, &mut stdout, &mut stderr)
                        .await?;
                    exit_code = 0;
                } else {
                    let emit_before = self.output_emit_count;
                    let result = self.execute_command(cmd).await?;
                    self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
                    stdout.push_str(&result.stdout);
                    stderr.push_str(&result.stderr);
                    exit_code = result.exit_code;
                    self.last_exit_code = exit_code;
                    control_flow = result.control_flow;

                    // If command signaled control flow, return immediately
                    if control_flow != ControlFlow::None {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow,
                            ..Default::default()
                        });
                    }

                    // ERR trap: fire on non-zero exit after semicolon commands
                    if exit_code != 0 && !current_is_conditional {
                        self.run_err_trap(&mut stdout, &mut stderr).await;
                    }
                }
            }
        }

        // Final errexit check for the last command
        let should_final_errexit_check =
            !has_conditional_operators && self.is_errexit_enabled() && exit_code != 0;

        if should_final_errexit_check {
            return Ok(ExecResult {
                stdout,
                stderr,
                exit_code,
                control_flow: ControlFlow::None,
                ..Default::default()
            });
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
            errexit_suppressed: has_conditional_operators && exit_code != 0,
            ..Default::default()
        })
    }

    /// Process variable assignments from a command's prefix (e.g. `VAR=val cmd`).
    async fn process_command_assignments(&mut self, assignments: &[Assignment]) -> Result<()> {
        for assignment in assignments {
            match &assignment.value {
                AssignmentValue::Scalar(word) => {
                    let value = self.expand_word(word).await?;
                    if let Some(index_str) = &assignment.index {
                        let resolved_name = self.resolve_nameref(&assignment.name).to_string();
                        if self.assoc_arrays.contains_key(&resolved_name) {
                            let key = self.expand_assoc_key(index_str).await?;
                            let is_new_entry = self
                                .assoc_arrays
                                .get(&resolved_name)
                                .is_none_or(|a| !a.contains_key(&key));
                            if is_new_entry
                                && self
                                    .memory_budget
                                    .check_array_entries(1, &self.memory_limits)
                                    .is_err()
                            {
                                // Budget exceeded — skip
                            } else {
                                if is_new_entry {
                                    self.memory_budget.record_array_insert(1);
                                }
                                let arr = self.assoc_arrays.entry(resolved_name).or_default();
                                if assignment.append {
                                    let existing = arr.get(&key).cloned().unwrap_or_default();
                                    arr.insert(key, existing + &value);
                                } else {
                                    arr.insert(key, value);
                                }
                            }
                        } else {
                            let raw_idx = self.evaluate_arithmetic(index_str);
                            let index = if raw_idx < 0 {
                                let len = self
                                    .arrays
                                    .get(&resolved_name)
                                    .and_then(|a| a.keys().max().map(|m| m + 1))
                                    .unwrap_or(0) as i64;
                                (len + raw_idx).max(0) as usize
                            } else {
                                raw_idx as usize
                            };
                            let is_new_entry = self
                                .arrays
                                .get(&resolved_name)
                                .is_none_or(|a| !a.contains_key(&index));
                            if is_new_entry
                                && self
                                    .memory_budget
                                    .check_array_entries(1, &self.memory_limits)
                                    .is_err()
                            {
                                // Budget exceeded — skip
                            } else {
                                if is_new_entry {
                                    self.memory_budget.record_array_insert(1);
                                }
                                let arr = self.arrays.entry(resolved_name).or_default();
                                if assignment.append {
                                    let existing = arr.get(&index).cloned().unwrap_or_default();
                                    arr.insert(index, existing + &value);
                                } else {
                                    arr.insert(index, value);
                                }
                            }
                        }
                    } else if assignment.append {
                        let existing = self.expand_variable(&assignment.name);
                        self.set_variable(assignment.name.clone(), existing + &value);
                    } else {
                        self.set_variable(assignment.name.clone(), value);
                    }
                }
                AssignmentValue::Array(words) => {
                    let mut expanded_values = Vec::new();
                    for word in words.iter() {
                        let has_command_subst = word
                            .parts
                            .iter()
                            .any(|p| matches!(p, WordPart::CommandSubstitution(_)));
                        let value = self.expand_word(word).await?;
                        expanded_values.push((value, has_command_subst));
                    }

                    // Resolve nameref for array assignments
                    let arr_name = self.resolve_nameref(&assignment.name).to_string();
                    let arr = self.arrays.entry(arr_name).or_default();
                    let mut idx = if assignment.append {
                        arr.keys().max().map(|k| k + 1).unwrap_or(0)
                    } else {
                        arr.clear();
                        0
                    };

                    for (value, has_command_subst) in expanded_values {
                        if has_command_subst && !value.is_empty() {
                            for part in value.split_whitespace() {
                                arr.insert(idx, part.to_string());
                                idx += 1;
                            }
                        } else if !value.is_empty() || !has_command_subst {
                            arr.insert(idx, value);
                            idx += 1;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Try alias expansion. Returns `Some(result)` if alias was expanded, `None` otherwise.
    async fn try_alias_expansion(
        &mut self,
        name: &str,
        command: &SimpleCommand,
        stdin: Option<String>,
        var_saves: Vec<(String, Option<String>)>,
    ) -> Option<Result<ExecResult>> {
        let is_plain_literal = !command.name.quoted
            && command
                .name
                .parts
                .iter()
                .all(|p| matches!(p, WordPart::Literal(_)));
        if !is_plain_literal
            || !self.is_expand_aliases_enabled()
            || self.expanding_aliases.contains(name)
        {
            return None;
        }
        let expansion = self.aliases.get(name).cloned()?;

        // Restore variable saves before re-executing
        for (vname, old) in var_saves.into_iter().rev() {
            match old {
                Some(v) => {
                    self.insert_variable_checked(vname, v);
                }
                None => {
                    self.variables.remove(&vname);
                }
            }
        }

        // Build expanded command: alias value + original args
        let mut expanded_cmd = expansion;
        let trailing_space = expanded_cmd.ends_with(' ');
        let mut args_iter = command.args.iter();
        if trailing_space && let Some(first_arg) = args_iter.next() {
            let arg_str = format!("{}", first_arg);
            if let Some(arg_expansion) = self.aliases.get(&arg_str).cloned() {
                expanded_cmd.push_str(&arg_expansion);
            } else {
                expanded_cmd.push_str(&arg_str);
            }
        }
        for word in args_iter {
            expanded_cmd.push(' ');
            expanded_cmd.push_str(&format!("{}", word));
        }
        for redir in &command.redirects {
            expanded_cmd.push(' ');
            expanded_cmd.push_str(&Self::format_redirect(redir));
        }

        self.expanding_aliases.insert(name.to_string());

        let prev_pipeline_stdin = self.pipeline_stdin.take();
        if stdin.is_some() {
            self.pipeline_stdin = stdin;
        }

        // THREAT[TM-DOS-030]: Propagate interpreter parser limits
        let parser = Parser::with_limits(
            &expanded_cmd,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let result = match parser.parse() {
            Ok(s) => self.execute(&s).await,
            Err(e) => Ok(ExecResult::err(
                format!("bash: alias expansion: parse error: {}\n", e),
                1,
            )),
        };

        self.pipeline_stdin = prev_pipeline_stdin;
        self.expanding_aliases.remove(name);
        Some(result)
    }

    /// Execute deferred output process substitutions (`>(cmd)`).
    async fn run_deferred_proc_subs(&mut self, result: &mut Result<ExecResult>) -> Result<()> {
        if self.deferred_proc_subs.is_empty() {
            return Ok(());
        }
        let deferred = std::mem::take(&mut self.deferred_proc_subs);
        for (path_str, commands) in deferred {
            let path = Path::new(&path_str);
            let stdin_data = if let Ok(bytes) = self.fs.read_file(path).await {
                let s = bytes_to_latin1_string(&bytes);
                if s.is_empty() { None } else { Some(s) }
            } else {
                None
            };
            for cmd in &commands {
                let prev_stdin = self.pipeline_stdin.take();
                self.pipeline_stdin = stdin_data.clone();
                let cmd_result = self.execute_command(cmd).await?;
                self.pipeline_stdin = prev_stdin;
                if let Ok(r) = result {
                    r.stdout.push_str(&cmd_result.stdout);
                    r.stderr.push_str(&cmd_result.stderr);
                }
            }
        }
        Ok(())
    }

    /// Restore saved variable values (used for prefix assignment cleanup).
    fn restore_variables(&mut self, saves: Vec<(String, Option<String>)>) {
        for (name, old) in saves {
            match old {
                Some(v) => {
                    self.insert_variable_checked(name, v);
                }
                None => {
                    self.variables.remove(&name);
                }
            }
        }
    }

    /// Build an xtrace line for `set -x` output.
    fn build_xtrace_line(&self, name: &str, args: &[String]) -> Option<String> {
        if !self.is_xtrace_enabled() {
            return None;
        }
        let ps4 = self
            .variables
            .get("PS4")
            .cloned()
            .unwrap_or_else(|| "+ ".to_string());
        let mut trace = ps4;
        trace.push_str(name);
        for expanded in args {
            trace.push(' ');
            if expanded.contains(' ') || expanded.contains('\t') || expanded.is_empty() {
                trace.push('\'');
                trace.push_str(&expanded.replace('\'', "'\\''"));
                trace.push('\'');
            } else {
                trace.push_str(expanded);
            }
        }
        trace.push('\n');
        Some(trace)
    }

    async fn execute_simple_command(
        &mut self,
        command: &SimpleCommand,
        stdin: Option<String>,
    ) -> Result<ExecResult> {
        let (_debug_stdout, _debug_stderr) = self.run_debug_trap().await;

        let name = self.expand_word(&command.name).await?;

        if let Some(err_msg) = self.nounset_error.take() {
            self.last_exit_code = 1;
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: err_msg,
                exit_code: 1,
                control_flow: ControlFlow::Return(1),
                ..Default::default()
            });
        }

        let pre_expanded_args = if !name.is_empty() {
            Some(self.expand_command_args(command).await?)
        } else {
            None
        };

        let var_saves: Vec<(String, Option<String>)> = command
            .assignments
            .iter()
            .map(|a| (a.name.clone(), self.variables.get(&a.name).cloned()))
            .collect();

        let pre_assign_subst_gen = self.subst_generation;

        self.process_command_assignments(&command.assignments)
            .await?;

        // Alias expansion
        if let Some(result) = self
            .try_alias_expansion(&name, command, stdin.clone(), var_saves.clone())
            .await
        {
            return result;
        }

        // Empty command handling
        if name.is_empty() {
            if command.name.quoted && command.assignments.is_empty() {
                self.last_exit_code = 127;
                return Ok(ExecResult::err(
                    "bash: : command not found\n".to_string(),
                    127,
                ));
            }
            let exit_code = if !command.assignments.is_empty()
                && self.subst_generation == pre_assign_subst_gen
            {
                0
            } else {
                self.last_exit_code
            };
            self.last_exit_code = exit_code;
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code,
                control_flow: crate::interpreter::ControlFlow::None,
                ..Default::default()
            });
        }

        // Inject prefix assignments into env for command duration
        let mut env_saves: Vec<(String, Option<String>)> = Vec::new();
        for assignment in &command.assignments {
            if assignment.index.is_none()
                && let Some(value) = self.variables.get(&assignment.name).cloned()
            {
                let old = self.env.insert(assignment.name.clone(), value);
                env_saves.push((assignment.name.clone(), old));
            }
        }

        let args = pre_expanded_args.unwrap_or_default();

        // Check for glob error sentinel
        if let Some(first) = args.first()
            && first.starts_with("\x00ERR\x00")
        {
            let err_msg = first.trim_start_matches("\x00ERR\x00").to_string();
            self.last_exit_code = 1;
            self.restore_variables(var_saves);
            return Ok(ExecResult::err(err_msg, 1));
        }

        let xtrace_line = self.build_xtrace_line(&name, &args);

        let result = self
            .execute_dispatched_command(&name, args, command, stdin)
            .await;

        // Restore env
        for (name, old) in env_saves {
            match old {
                Some(v) => {
                    self.env.insert(name, v);
                }
                None => {
                    self.env.remove(&name);
                }
            }
        }

        // Restore variables
        self.restore_variables(var_saves);

        // Prepend xtrace to stderr
        let mut result = if let Some(trace) = xtrace_line {
            result.map(|mut r| {
                r.stderr = trace + &r.stderr;
                r
            })
        } else {
            result
        };

        self.run_deferred_proc_subs(&mut result).await?;

        result
    }

    /// Expand command arguments with field splitting, brace, and glob expansion.
    async fn expand_command_args(&mut self, command: &SimpleCommand) -> Result<Vec<String>> {
        let mut args: Vec<String> = Vec::new();
        for word in &command.args {
            // Use field expansion so "${arr[@]}" produces multiple args
            let fields = self.expand_word_to_fields(word).await?;

            // Skip brace and glob expansion for quoted words
            if word.quoted {
                args.extend(fields);
                continue;
            }

            // For each field, apply brace and glob expansion
            for expanded in fields {
                // Step 1: Brace expansion (produces multiple strings)
                let brace_expanded = self.expand_braces(&expanded);

                // Step 2: For each brace-expanded item, do glob expansion
                for item in brace_expanded {
                    match self.expand_glob_item(&item).await {
                        Ok(items) => args.extend(items),
                        Err(pat) => {
                            self.last_exit_code = 1;
                            return Ok(vec![format!("\x00ERR\x00-bash: no match: {}\n", pat)]);
                        }
                    }
                }
            }
        }
        Ok(args)
    }

    /// Execute a command after name resolution and prefix assignment setup.
    ///
    /// Handles stdin processing and dispatch to functions, special builtins,
    /// regular builtins, or command-not-found. Args are pre-expanded.
    async fn execute_dispatched_command(
        &mut self,
        name: &str,
        args: Vec<String>,
        command: &SimpleCommand,
        stdin: Option<String>,
    ) -> Result<ExecResult> {
        // Track $_ (last argument of previous command, from already-expanded args)
        if let Some(last) = args.last() {
            self.insert_variable_checked("_".to_string(), last.clone());
        } else {
            self.insert_variable_checked("_".to_string(), name.to_string());
        }

        // Check for nounset error from argument expansion
        if let Some(err_msg) = self.nounset_error.take() {
            self.last_exit_code = 1;
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: err_msg,
                exit_code: 1,
                control_flow: ControlFlow::Return(1),
                ..Default::default()
            });
        }

        // Handle input redirections first
        let stdin = self
            .process_input_redirections(stdin, &command.redirects)
            .await?;

        // For `read -u FD`, check if FD is a coproc read FD and inject data as stdin
        let stdin = if name == "read" && stdin.is_none() {
            self.try_coproc_read_stdin(&args).or(stdin)
        } else {
            stdin
        };

        // If no explicit stdin, inherit from pipeline_stdin (for compound cmds in pipes).
        // For `read`, consume one line; for other commands, provide all remaining data.
        let stdin = if stdin.is_some() {
            stdin
        } else if let Some(ref ps) = self.pipeline_stdin {
            if !ps.is_empty() {
                if name == "read" {
                    // Consume one line from pipeline stdin
                    let data = ps.clone();
                    if let Some(newline_pos) = data.find('\n') {
                        let line = data[..=newline_pos].to_string();
                        self.pipeline_stdin = Some(data[newline_pos + 1..].to_string());
                        Some(line)
                    } else {
                        // Last line without trailing newline
                        self.pipeline_stdin = Some(String::new());
                        Some(data)
                    }
                } else {
                    Some(ps.clone())
                }
            } else {
                None
            }
        } else {
            None
        };

        // TRACE: Record command start event
        let trace_start = if self.trace.mode() != crate::trace::TraceMode::Off {
            self.trace
                .command_start(name, &args, self.cwd.to_string_lossy().as_ref());
            Some(std::time::Instant::now())
        } else {
            None
        };

        let result = self.dispatch_command(name, command, args, stdin).await;

        // TRACE: Record command exit event for all dispatch paths
        if let (Some(start), Ok(r)) = (trace_start, &result) {
            self.trace.command_exit(name, r.exit_code, start.elapsed());
        }

        result
    }

    /// Inner dispatch logic for command execution.
    /// Separated from `execute_dispatched_command` so trace start/exit events
    /// wrap all return paths uniformly.
    /// Handle `exec` builtin: apply redirections to current shell context.
    async fn execute_exec_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if !args.is_empty() {
            // exec cmd args... — execute command and exit with its exit code.
            // In a real shell this replaces the process; in VFS we run + exit.
            // Build the command as a script string and execute it.
            // Single-quote each arg to prevent re-expansion.
            let mut script_str = String::new();
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    script_str.push(' ');
                }
                script_str.push('\'');
                script_str.push_str(&arg.replace('\'', "'\\''"));
                script_str.push('\'');
            }

            let parser = Parser::with_limits(
                &script_str,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            );
            let script = match parser.parse() {
                Ok(s) => s,
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("-bash: exec: {}: command not found\n", args[0]),
                        127,
                    ));
                }
            };

            let result = self.execute(&script).await?;

            // Signal exit so subsequent statements don't execute
            return Ok(ExecResult {
                control_flow: ControlFlow::Return(result.exit_code),
                ..result
            });
        }
        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Input => {
                    if let Some(fd) = redirect.fd {
                        let target_path = self.expand_word(&redirect.target).await?;
                        let path = self.resolve_path(&target_path);
                        let content = self.fs.read_file(&path).await?;
                        let text = bytes_to_latin1_string(&content);
                        let lines: Vec<String> =
                            text.lines().rev().map(|l| l.to_string()).collect();
                        self.coproc_buffers.insert(fd, lines);
                    }
                }
                RedirectKind::DupInput => {
                    let target = self.expand_word(&redirect.target).await?;
                    if target == "-"
                        && let Some(fd) = redirect.fd
                    {
                        self.coproc_buffers.remove(&fd);
                    }
                }
                _ => {}
            }
        }
        let result = ExecResult::default();
        self.apply_redirections(result, redirects).await
    }

    /// Execute a registered (non-special) builtin with panic safety.
    /// The builtin must exist in `self.builtins` (caller checks with `contains_key`).
    async fn execute_registered_builtin(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<&str>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let builtin = self.builtins.get(name).unwrap();

        // Check for execution plan first
        {
            let shell_ref = ShellRef {
                builtins: &self.builtins,
                functions: &self.functions,
                aliases: &mut self.aliases,
                traps: &mut self.traps,
                call_stack: &self.call_stack,
                history: &self.history,
                jobs: &self.jobs,
            };
            let plan_ctx = builtins::Context {
                args,
                env: &self.env,
                variables: &mut self.variables,
                cwd: &mut self.cwd,
                fs: Arc::clone(&self.fs),
                stdin,
                #[cfg(feature = "http_client")]
                http_client: self.http_client.as_ref(),
                #[cfg(feature = "git")]
                git_client: self.git_client.as_ref(),
                shell: Some(shell_ref),
            };

            let plan_result = AssertUnwindSafe(builtin.execution_plan(&plan_ctx))
                .catch_unwind()
                .await;

            match plan_result {
                Ok(Ok(Some(plan))) => {
                    let result = self.execute_builtin_plan(plan, redirects).await?;
                    return Ok(result);
                }
                Ok(Ok(None)) => { /* fall through to normal execute() */ }
                Ok(Err(e)) => return Err(e),
                Err(_panic) => {
                    let result = ExecResult::err(
                        format!("bash: {}: builtin failed unexpectedly\n", name),
                        1,
                    );
                    return self.apply_redirections(result, redirects).await;
                }
            }
        }

        let builtin = self.builtins.get(name).unwrap();
        let shell_ref = ShellRef {
            builtins: &self.builtins,
            functions: &self.functions,
            aliases: &mut self.aliases,
            traps: &mut self.traps,
            call_stack: &self.call_stack,
            history: &self.history,
            jobs: &self.jobs,
        };
        let ctx = builtins::Context {
            args,
            env: &self.env,
            variables: &mut self.variables,
            cwd: &mut self.cwd,
            fs: Arc::clone(&self.fs),
            stdin,
            #[cfg(feature = "http_client")]
            http_client: self.http_client.as_ref(),
            #[cfg(feature = "git")]
            git_client: self.git_client.as_ref(),
            shell: Some(shell_ref),
        };

        // THREAT[TM-INT-001]: Execute builtin with panic catching for security
        let result = AssertUnwindSafe(builtin.execute(ctx)).catch_unwind().await;

        let result = match result {
            Ok(Ok(exec_result)) => exec_result,
            Ok(Err(e)) => return Err(e),
            Err(_panic) => {
                ExecResult::err(format!("bash: {}: builtin failed unexpectedly\n", name), 1)
            }
        };

        self.apply_builtin_side_effects(&result);

        // Sync exported variables into env so subprocess isolation can see them
        if name == "export" && result.exit_code == 0 {
            for arg in args {
                if let Some(eq_pos) = arg.find('=') {
                    let var_name = &arg[..eq_pos];
                    if let Some(value) = self.variables.get(var_name) {
                        self.env.insert(var_name.to_string(), value.clone());
                    }
                } else if let Some(value) = self.variables.get(arg.as_str()) {
                    // export NAME (without =) — mark existing variable as exported
                    self.env.insert(arg.to_string(), value.clone());
                }
            }
        }

        self.apply_redirections(result, redirects).await
    }

    /// Dispatch an interpreter-level (special) builtin by name.
    /// Returns `Some(result)` if handled, `None` if not a special builtin.
    async fn dispatch_special_builtin(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Option<Result<ExecResult>> {
        match name {
            "exec" => Some(self.execute_exec_builtin(args, redirects).await),
            "local" => Some(self.execute_local_builtin(args, redirects).await),
            "bash" | "sh" => Some(self.execute_shell(name, args, stdin, redirects).await),
            "source" | "." => Some(self.execute_source(args, redirects).await),
            "eval" => Some(self.execute_eval(args, stdin, redirects).await),
            "command" => Some(self.execute_command_builtin(args, stdin, redirects).await),
            "declare" | "typeset" => Some(self.execute_declare_builtin(args, redirects).await),
            "let" => Some(self.execute_let_builtin(args, redirects).await),
            "unset" => Some(self.execute_unset_builtin(args, redirects).await),
            "getopts" => Some(self.execute_getopts(args, redirects).await),
            _ => None,
        }
    }

    async fn dispatch_command(
        &mut self,
        name: &str,
        command: &SimpleCommand,
        args: Vec<String>,
        stdin: Option<String>,
    ) -> Result<ExecResult> {
        // Check for functions first
        if let Some(func_def) = self.functions.get(name).cloned() {
            return self
                .execute_function_call(name, &func_def, args, stdin, &command.redirects)
                .await;
        }

        // Interpreter-level special builtins
        if let Some(result) = self
            .dispatch_special_builtin(name, &args, stdin.clone(), &command.redirects)
            .await
        {
            return result;
        }

        // Registered builtins
        if self.builtins.contains_key(name) {
            return self
                .execute_registered_builtin(name, &args, stdin.as_deref(), &command.redirects)
                .await;
        }

        // Script execution by path
        if name.contains('/') {
            return self
                .try_execute_script_by_path(name, &args, stdin, &command.redirects)
                .await;
        }

        // $PATH search
        if let Some(result) = self
            .try_execute_script_via_path_search(name, &args, stdin, &command.redirects)
            .await?
        {
            return Ok(result);
        }

        // Command not found
        let known: Vec<&str> = self
            .builtins
            .keys()
            .map(|s| s.as_str())
            .chain(self.functions.keys().map(|s| s.as_str()))
            .chain(self.aliases.keys().map(|s| s.as_str()))
            .collect();
        let msg = command_not_found_message(name, &known);
        Ok(ExecResult::err(msg, 127))
    }

    /// Execute a script file by resolved path.
    ///
    /// Bash behavior for path-based commands (name contains `/`):
    /// 1. Resolve path (absolute or relative to cwd)
    /// 2. stat() — if not found: "No such file or directory" (exit 127)
    /// 3. If directory: "Is a directory" (exit 126)
    /// 4. If not executable (mode & 0o111 == 0): "Permission denied" (exit 126)
    /// 5. Read file, strip shebang, parse, execute in call frame
    async fn try_execute_script_by_path(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let path = self.resolve_path(name);

        // stat the file
        let meta = match self.fs.stat(&path).await {
            Ok(m) => m,
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("bash: {}: No such file or directory", name),
                    127,
                ));
            }
        };

        // Directory check
        if meta.file_type.is_dir() {
            return Ok(ExecResult::err(
                format!("bash: {}: Is a directory", name),
                126,
            ));
        }

        // Execute permission check
        if meta.mode & 0o111 == 0 {
            return Ok(ExecResult::err(
                format!("bash: {}: Permission denied", name),
                126,
            ));
        }

        // Read file content
        let content = match self.fs.read_file(&path).await {
            Ok(c) => bytes_to_latin1_string(&c),
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("bash: {}: No such file or directory", name),
                    127,
                ));
            }
        };

        self.execute_script_content(name, &content, args, stdin, redirects)
            .await
    }

    /// Search $PATH for an executable script and run it.
    ///
    /// Returns `Ok(None)` if no matching file found (caller emits "command not found").
    async fn try_execute_script_via_path_search(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<Option<ExecResult>> {
        let path_var = self
            .variables
            .get("PATH")
            .or_else(|| self.env.get("PATH"))
            .cloned()
            .unwrap_or_default();

        for dir in path_var.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(dir).join(name);
            if let Ok(meta) = self.fs.stat(&candidate).await {
                if meta.file_type.is_dir() {
                    continue;
                }
                if meta.mode & 0o111 == 0 {
                    continue;
                }
                if let Ok(content) = self.fs.read_file(&candidate).await {
                    let script_text = bytes_to_latin1_string(&content);
                    let result = self
                        .execute_script_content(name, &script_text, args, stdin, redirects)
                        .await?;
                    return Ok(Some(result));
                }
            }
        }

        Ok(None)
    }

    /// Parse and execute script content in a new call frame.
    ///
    /// Shared by path-based and $PATH-based script execution.
    /// Sets up $0 = script name, $1..N = args, strips shebang.
    async fn execute_script_content(
        &mut self,
        name: &str,
        content: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Strip shebang line if present
        let script_text = if content.starts_with("#!") {
            content
                .find('\n')
                .map(|pos| &content[pos + 1..])
                .unwrap_or("")
        } else {
            content
        };

        let parser = Parser::with_limits(
            script_text,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(format!("bash: {}: {}\n", name, e), 2));
            }
        };

        // Subprocess isolation: path-based script execution only inherits
        // exported variables (env), not the full parent shell state.
        // This matches real bash behavior where ./script.sh spawns a subprocess.
        let saved_vars = self.variables.clone();
        let saved_arrays = self.arrays.clone();
        let saved_assoc = self.assoc_arrays.clone();
        let saved_functions = self.functions.clone();
        let saved_traps = self.traps.clone();
        let saved_call_stack = self.call_stack.clone();
        let saved_exit = self.last_exit_code;
        let saved_aliases = self.aliases.clone();
        let saved_coproc = self.coproc_buffers.clone();

        // Child only sees exported variables (env), not all shell variables.
        // Reset last_exit_code so $? starts at 0 (matches real bash subprocess).
        // Clear nounset_error to prevent parent expansion errors from leaking.
        self.variables = self.env.clone();
        self.arrays.clear();
        self.assoc_arrays.clear();
        self.functions.clear();
        self.traps.clear();
        self.aliases.clear();
        self.coproc_buffers.clear();
        self.last_exit_code = 0;
        self.nounset_error = None;

        // Push call frame: $0 = script name, $1..N = args
        self.call_stack = vec![CallFrame {
            name: name.to_string(),
            locals: HashMap::new(),
            positional: args.to_vec(),
        }];

        // Set up BASH_SOURCE for the subprocess
        let saved_source_stack = self.bash_source_stack.clone();
        self.bash_source_stack = vec![name.to_string()];
        self.update_bash_source();

        // Forward pipeline stdin so commands inside the script (cat, read, etc.) can consume it
        let prev_pipeline_stdin = self.pipeline_stdin.take();
        self.pipeline_stdin = stdin;

        let result = self.execute(&script).await;

        // Restore full parent state — child mutations don't propagate
        self.variables = saved_vars;
        self.arrays = saved_arrays;
        self.assoc_arrays = saved_assoc;
        self.functions = saved_functions;
        self.traps = saved_traps;
        self.call_stack = saved_call_stack;
        self.last_exit_code = saved_exit;
        self.aliases = saved_aliases;
        self.coproc_buffers = saved_coproc;
        self.bash_source_stack = saved_source_stack;
        self.pipeline_stdin = prev_pipeline_stdin;

        match result {
            Ok(mut exec_result) => {
                // Handle return - convert Return control flow to exit code
                if let ControlFlow::Return(code) = exec_result.control_flow {
                    exec_result.exit_code = code;
                    exec_result.control_flow = ControlFlow::None;
                }
                self.apply_redirections(exec_result, redirects).await
            }
            Err(e) => Err(e),
        }
    }

    /// Execute `source` / `.` - read and execute commands from a file in current shell.
    ///
    /// Bash behavior:
    /// - If filename contains a slash, use it directly (absolute or relative to cwd)
    /// - If filename has no slash, search $PATH directories
    /// - Extra arguments become positional parameters ($1, $2, ...) during sourcing
    /// - Original positional parameters are restored after sourcing completes
    async fn execute_source(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let filename = match args.first() {
            Some(f) => f,
            None => {
                return Ok(ExecResult::err("source: filename argument required", 1));
            }
        };

        // Resolve the file path:
        // - If filename contains '/', resolve relative to cwd
        // - Otherwise, search $PATH directories (bash behavior)
        let content = if filename.contains('/') {
            let path = self.resolve_path(filename);
            match self.fs.read_file(&path).await {
                Ok(c) => bytes_to_latin1_string(&c),
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("source: {}: No such file or directory", filename),
                        1,
                    ));
                }
            }
        } else {
            // Search PATH for the file
            let mut found = None;
            let path_var = self
                .variables
                .get("PATH")
                .or_else(|| self.env.get("PATH"))
                .cloned()
                .unwrap_or_default();
            for dir in path_var.split(':') {
                if dir.is_empty() {
                    continue;
                }
                let candidate = PathBuf::from(dir).join(filename);
                if let Ok(c) = self.fs.read_file(&candidate).await {
                    found = Some(bytes_to_latin1_string(&c));
                    break;
                }
            }
            // Also try cwd as fallback (bash sources from cwd too)
            if found.is_none() {
                let path = self.resolve_path(filename);
                if let Ok(c) = self.fs.read_file(&path).await {
                    found = Some(bytes_to_latin1_string(&c));
                }
            }
            match found {
                Some(c) => c,
                None => {
                    return Ok(ExecResult::err(
                        format!("source: {}: No such file or directory", filename),
                        1,
                    ));
                }
            }
        };

        // THREAT[TM-DOS-030]: Propagate interpreter parser limits
        let parser = Parser::with_limits(
            &content,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("source: {}: parse error: {}", filename, e),
                    1,
                ));
            }
        };

        // Set positional parameters if extra arguments provided.
        // Save and restore the caller's positional params.
        let source_args: Vec<String> = args[1..].to_vec();
        let has_source_args = !source_args.is_empty();

        let saved_positional = if has_source_args {
            let saved = self.call_stack.last().map(|frame| frame.positional.clone());
            // Push a temporary call frame for positional params
            if self.call_stack.is_empty() {
                self.call_stack.push(CallFrame {
                    name: filename.clone(),
                    locals: HashMap::new(),
                    positional: source_args,
                });
            } else if let Some(frame) = self.call_stack.last_mut() {
                frame.positional = source_args;
            }
            saved
        } else {
            None
        };

        // THREAT[TM-DOS-056]: Check source depth (uses function depth limit)
        self.counters.push_function(&self.limits).map_err(|_| {
            crate::error::Error::Execution(format!(
                "source: {}: maximum source depth exceeded",
                filename
            ))
        })?;

        // Track source file for BASH_SOURCE
        self.bash_source_stack.push(filename.clone());
        self.update_bash_source();

        // Execute the script commands in the current shell context.
        // Use execute_script_body (not execute) to preserve depth counters.
        let exec_result = self.execute_script_body(&script, false).await;

        // Pop source depth and BASH_SOURCE (always, even on error)
        self.counters.pop_function();
        self.bash_source_stack.pop();
        self.update_bash_source();

        let mut result = exec_result?;

        // Restore positional parameters
        if has_source_args {
            if let Some(saved) = saved_positional {
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.positional = saved;
                }
            } else {
                // We pushed a frame; pop it
                self.call_stack.pop();
            }
        }

        // Apply redirections
        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Execute `eval` - parse and execute concatenated arguments
    async fn execute_eval(
        &mut self,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let cmd = args.join(" ");
        // THREAT[TM-DOS-030]: Propagate interpreter parser limits
        let parser = Parser::with_limits(
            &cmd,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(format!("eval: parse error: {}", e), 1));
            }
        };

        // Set up pipeline stdin if provided
        let prev_pipeline_stdin = self.pipeline_stdin.take();
        if stdin.is_some() {
            self.pipeline_stdin = stdin;
        }

        let mut result = self.execute(&script).await?;

        self.pipeline_stdin = prev_pipeline_stdin;

        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Check if expand_aliases is enabled via shopt.
    fn is_expand_aliases_enabled(&self) -> bool {
        self.variables
            .get("SHOPT_expand_aliases")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Format a Redirect back to its textual representation for alias expansion.
    fn format_redirect(redir: &Redirect) -> String {
        let fd_prefix = redir.fd.map(|fd| fd.to_string()).unwrap_or_default();
        let op = match redir.kind {
            RedirectKind::Output => ">",
            RedirectKind::Append => ">>",
            RedirectKind::Input => "<",
            RedirectKind::HereDoc => "<<",
            RedirectKind::HereDocStrip => "<<-",
            RedirectKind::HereString => "<<<",
            RedirectKind::Clobber => ">|",
            RedirectKind::DupOutput => ">&",
            RedirectKind::DupInput => "<&",
            RedirectKind::OutputBoth => "&>",
        };
        format!("{}{}{}", fd_prefix, op, redir.target)
    }

    /// Execute a shell function call with call frame management.
    async fn execute_function_call(
        &mut self,
        name: &str,
        func_def: &FunctionDef,
        args: Vec<String>,
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Check function depth limit
        self.counters.push_function(&self.limits)?;

        // Push call frame with positional parameters
        self.call_stack.push(CallFrame {
            name: name.to_string(),
            locals: HashMap::new(),
            positional: args,
        });

        // Set FUNCNAME array from call stack (index 0 = current, 1 = caller, ...)
        let funcname_arr: HashMap<usize, String> = self
            .call_stack
            .iter()
            .rev()
            .enumerate()
            .map(|(i, f)| (i, f.name.clone()))
            .collect();
        let prev_funcname = self.arrays.insert("FUNCNAME".to_string(), funcname_arr);

        // BASH_SOURCE: duplicate current top entry for function calls
        let current_source = self.bash_source_stack.last().cloned().unwrap_or_default();
        self.bash_source_stack.push(current_source);
        self.update_bash_source();

        // Forward pipeline stdin to function body
        let prev_pipeline_stdin = self.pipeline_stdin.take();
        self.pipeline_stdin = stdin;

        // Execute function body
        let mut result = self.execute_command(&func_def.body).await?;

        // Restore previous pipeline stdin
        self.pipeline_stdin = prev_pipeline_stdin;

        // Pop call frame, function counter, and BASH_SOURCE
        self.call_stack.pop();
        self.counters.pop_function();
        self.bash_source_stack.pop();
        self.update_bash_source();

        // Restore previous FUNCNAME (or set from remaining stack)
        if self.call_stack.is_empty() {
            self.arrays.remove("FUNCNAME");
        } else if let Some(prev) = prev_funcname {
            self.arrays.insert("FUNCNAME".to_string(), prev);
        }

        // Handle return - convert Return control flow to exit code
        if let ControlFlow::Return(code) = result.control_flow {
            result.exit_code = code;
            result.control_flow = ControlFlow::None;
        }

        self.apply_redirections(result, redirects).await
    }

    /// Execute the `local` builtin — set variables in function call frame.
    async fn execute_local_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let mut flags = DeclareFlags::default();
        let mut var_args: Vec<&String> = Vec::new();
        for arg in args {
            if arg.starts_with('-') && !arg.contains('=') {
                flags.parse_flag_chars(arg);
            } else {
                var_args.push(arg);
            }
        }

        let merged = merge_compound_assignments(&var_args);

        if !self.call_stack.is_empty() {
            // In a function - set in locals
            for arg in &merged {
                if let Some(eq_pos) = arg.find('=') {
                    let var_name = &arg[..eq_pos];
                    let value = &arg[eq_pos + 1..];
                    if !is_valid_var_name(var_name) {
                        let result = ExecResult::err(
                            format!("local: `{}': not a valid identifier\n", arg),
                            1,
                        );
                        return self.apply_redirections(result, redirects).await;
                    }
                    // THREAT[TM-INJ-014]: Block internal variable prefix injection via local
                    if is_internal_variable(var_name) {
                        continue;
                    }
                    // Handle compound array assignment: local arr=(1 2 3) or local -a/-A arr=(...)
                    let is_compound = value.starts_with('(') && value.ends_with(')');
                    if is_compound {
                        let inner = &value[1..value.len() - 1];
                        if flags.assoc {
                            let arr = self.assoc_arrays.entry(var_name.to_string()).or_default();
                            arr.clear();
                            let mut rest = inner.trim();
                            while let Some(bracket_start) = rest.find('[') {
                                if let Some(bracket_end) = rest[bracket_start..].find(']') {
                                    let key = &rest[bracket_start + 1..bracket_start + bracket_end];
                                    let after = &rest[bracket_start + bracket_end + 1..];
                                    if let Some(eq_rest) = after.strip_prefix('=') {
                                        let eq_rest = eq_rest.trim_start();
                                        let (val, remainder) =
                                            if let Some(stripped) = eq_rest.strip_prefix('"') {
                                                if let Some(end_q) = stripped.find('"') {
                                                    (
                                                        &stripped[..end_q],
                                                        stripped[end_q + 1..].trim_start(),
                                                    )
                                                } else {
                                                    (stripped.trim_end_matches('"'), "")
                                                }
                                            } else {
                                                match eq_rest.find(char::is_whitespace) {
                                                    Some(sp) => {
                                                        (&eq_rest[..sp], eq_rest[sp..].trim_start())
                                                    }
                                                    None => (eq_rest, ""),
                                                }
                                            };
                                        arr.insert(key.to_string(), val.to_string());
                                        rest = remainder;
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        } else {
                            let arr = self.arrays.entry(var_name.to_string()).or_default();
                            arr.clear();
                            for (idx, val) in inner.split_whitespace().enumerate() {
                                arr.insert(idx, val.trim_matches('"').to_string());
                            }
                        }
                        // Mark local
                        self.call_stack
                            .last_mut()
                            .unwrap()
                            .locals
                            .insert(var_name.to_string(), String::new());
                    } else if flags.nameref {
                        self.call_stack
                            .last_mut()
                            .unwrap()
                            .locals
                            .insert(var_name.to_string(), String::new());
                    } else if flags.integer {
                        let int_val = self.evaluate_arithmetic_with_assign(value);
                        self.call_stack
                            .last_mut()
                            .unwrap()
                            .locals
                            .insert(var_name.to_string(), int_val.to_string());
                        self.variables
                            .insert(format!("_INTEGER_{}", var_name), "1".to_string());
                    } else {
                        self.call_stack
                            .last_mut()
                            .unwrap()
                            .locals
                            .insert(var_name.to_string(), value.to_string());
                    }
                } else if !is_internal_variable(arg) {
                    self.call_stack
                        .last_mut()
                        .unwrap()
                        .locals
                        .insert(arg.to_string(), String::new());
                    if flags.assoc {
                        self.assoc_arrays.entry(arg.to_string()).or_default();
                    } else if flags.array {
                        self.arrays.entry(arg.to_string()).or_default();
                    }
                    if flags.integer {
                        self.variables
                            .insert(format!("_INTEGER_{}", arg), "1".to_string());
                    }
                }
            }
            // Set nameref markers (after frame borrow is released)
            if flags.nameref {
                for arg in &merged {
                    if let Some(eq_pos) = arg.find('=') {
                        let var_name = &arg[..eq_pos];
                        let value = &arg[eq_pos + 1..];
                        if !is_internal_variable(var_name) {
                            self.variables
                                .insert(format!("_NAMEREF_{}", var_name), value.to_string());
                        }
                    }
                }
            }
        } else {
            // Not in a function - set in global variables (bash behavior)
            for arg in &merged {
                if let Some(eq_pos) = arg.find('=') {
                    let var_name = &arg[..eq_pos];
                    let value = &arg[eq_pos + 1..];
                    // THREAT[TM-INJ-014]: Block internal variable prefix injection via local
                    if is_internal_variable(var_name) {
                        continue;
                    }
                    let is_compound = value.starts_with('(') && value.ends_with(')');
                    if is_compound {
                        let inner = &value[1..value.len() - 1];
                        if flags.assoc {
                            let arr = self.assoc_arrays.entry(var_name.to_string()).or_default();
                            arr.clear();
                            let mut rest = inner.trim();
                            while let Some(bracket_start) = rest.find('[') {
                                if let Some(bracket_end) = rest[bracket_start..].find(']') {
                                    let key = &rest[bracket_start + 1..bracket_start + bracket_end];
                                    let after = &rest[bracket_start + bracket_end + 1..];
                                    if let Some(eq_rest) = after.strip_prefix('=') {
                                        let eq_rest = eq_rest.trim_start();
                                        let (val, remainder) =
                                            if let Some(stripped) = eq_rest.strip_prefix('"') {
                                                if let Some(end_q) = stripped.find('"') {
                                                    (
                                                        &stripped[..end_q],
                                                        stripped[end_q + 1..].trim_start(),
                                                    )
                                                } else {
                                                    (stripped.trim_end_matches('"'), "")
                                                }
                                            } else {
                                                match eq_rest.find(char::is_whitespace) {
                                                    Some(sp) => {
                                                        (&eq_rest[..sp], eq_rest[sp..].trim_start())
                                                    }
                                                    None => (eq_rest, ""),
                                                }
                                            };
                                        arr.insert(key.to_string(), val.to_string());
                                        rest = remainder;
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        } else {
                            let arr = self.arrays.entry(var_name.to_string()).or_default();
                            arr.clear();
                            for (idx, val) in inner.split_whitespace().enumerate() {
                                arr.insert(idx, val.trim_matches('"').to_string());
                            }
                        }
                    } else if flags.nameref {
                        self.variables
                            .insert(format!("_NAMEREF_{}", var_name), value.to_string());
                    } else {
                        self.variables
                            .insert(var_name.to_string(), value.to_string());
                    }
                } else if !is_internal_variable(arg) {
                    if flags.assoc {
                        self.assoc_arrays.entry(arg.to_string()).or_default();
                    } else if flags.array {
                        self.arrays.entry(arg.to_string()).or_default();
                    } else {
                        self.insert_variable_checked(arg.to_string(), String::new());
                    }
                }
            }
        }
        Ok(ExecResult::ok(String::new()))
    }

    /// Execute the `let` builtin — evaluate arithmetic expressions.
    async fn execute_let_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let mut last_val = 0i64;
        for arg in args {
            last_val = self.evaluate_arithmetic_with_assign(arg);
        }
        let exit_code = if last_val == 0 { 1 } else { 0 };
        let result = ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
            control_flow: ControlFlow::None,
            ..Default::default()
        };
        self.apply_redirections(result, redirects).await
    }

    /// Execute the `unset` builtin — remove variables, array elements, and namerefs.
    async fn execute_unset_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let mut unset_nameref = false;
        let mut unset_function = false;
        let mut var_args: Vec<&String> = Vec::new();
        for arg in args {
            if arg == "-n" {
                unset_nameref = true;
            } else if arg == "-f" {
                unset_function = true;
            } else if arg == "-v" {
                // -v (variable, default) - explicit variable mode
            } else {
                var_args.push(arg);
            }
        }

        for arg in &var_args {
            if unset_function {
                self.functions.remove(arg.as_str());
                continue;
            }
            if let Some(bracket) = arg.find('[')
                && arg.ends_with(']')
            {
                let arr_name = &arg[..bracket];
                let key = &arg[bracket + 1..arg.len() - 1];
                let expanded_key = self.expand_variable_or_literal(key);
                let resolved_name = self.resolve_nameref(arr_name).to_string();
                if let Some(arr) = self.assoc_arrays.get_mut(&resolved_name) {
                    arr.remove(&expanded_key);
                } else if let Some(arr) = self.arrays.get_mut(&resolved_name)
                    && let Ok(idx) = key.parse::<usize>()
                {
                    arr.remove(&idx);
                }
                continue;
            }
            if unset_nameref {
                self.variables.remove(&format!("_NAMEREF_{}", arg));
            } else {
                let resolved = self.resolve_nameref(arg).to_string();
                // THREAT[TM-INJ-019]: Refuse to unset readonly variables
                if self
                    .variables
                    .contains_key(&format!("_READONLY_{}", resolved))
                {
                    continue;
                }
                self.variables.remove(&resolved);
                self.arrays.remove(&resolved);
                self.assoc_arrays.remove(&resolved);
                for frame in self.call_stack.iter_mut().rev() {
                    frame.locals.remove(&resolved);
                }
            }
        }
        let result = ExecResult::ok(String::new());
        self.apply_redirections(result, redirects).await
    }

    /// Usage: `getopts optstring name [args...]`
    ///
    /// Parses options from positional params (or `args`).
    /// Uses/updates `OPTIND` variable for tracking position.
    /// Sets `name` variable to the found option letter.
    /// Sets `OPTARG` for options that take arguments (marked with `:` in optstring).
    /// Returns 0 while options remain, 1 when done.
    async fn execute_getopts(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args.len() < 2 {
            let result = ExecResult::err("getopts: usage: getopts optstring name [arg ...]\n", 2);
            return Ok(result);
        }

        let optstring = &args[0];
        let varname = &args[1];

        // Get the arguments to parse (remaining args, or positional params)
        let parse_args: Vec<String> = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            // Use positional parameters $1, $2, ...
            self.call_stack
                .last()
                .map(|frame| frame.positional.clone())
                .unwrap_or_default()
        };

        // Get current OPTIND (1-based index into args)
        let optind: usize = self
            .variables
            .get("OPTIND")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        // Check if we're past the end
        if optind < 1 || optind > parse_args.len() {
            self.insert_variable_checked(varname.clone(), "?".to_string());
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                control_flow: crate::interpreter::ControlFlow::None,
                ..Default::default()
            });
        }

        let current_arg = &parse_args[optind - 1];

        // Check if this is an option (starts with -)
        if !current_arg.starts_with('-') || current_arg == "-" || current_arg == "--" {
            self.insert_variable_checked(varname.clone(), "?".to_string());
            if current_arg == "--" {
                self.variables
                    .insert("OPTIND".to_string(), (optind + 1).to_string());
            }
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                control_flow: crate::interpreter::ControlFlow::None,
                ..Default::default()
            });
        }

        // Parse the option character(s) from current arg
        // Handle multi-char option groups like -abc
        let opt_chars: Vec<char> = current_arg[1..].chars().collect();

        // Track position within the current argument for multi-char options
        let char_idx: usize = self
            .variables
            .get("_OPTCHAR_IDX")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        if char_idx >= opt_chars.len() {
            // Should not happen, but advance
            self.variables
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            self.variables.remove("_OPTCHAR_IDX");
            self.insert_variable_checked(varname.clone(), "?".to_string());
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                control_flow: crate::interpreter::ControlFlow::None,
                ..Default::default()
            });
        }

        let opt_char = opt_chars[char_idx];
        let silent = optstring.starts_with(':');
        let spec = if silent { &optstring[1..] } else { optstring };

        // Check if this option is in the optstring
        if let Some(pos) = spec.find(opt_char) {
            let needs_arg = spec.get(pos + 1..pos + 2) == Some(":");
            self.insert_variable_checked(varname.clone(), opt_char.to_string());

            if needs_arg {
                // Option needs an argument
                if char_idx + 1 < opt_chars.len() {
                    // Rest of current arg is the argument
                    let arg_val: String = opt_chars[char_idx + 1..].iter().collect();
                    self.insert_variable_checked("OPTARG".to_string(), arg_val);
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                } else if optind < parse_args.len() {
                    // Next arg is the argument
                    self.variables
                        .insert("OPTARG".to_string(), parse_args[optind].clone());
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 2).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                } else {
                    // Missing argument
                    self.variables.remove("OPTARG");
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                    if silent {
                        self.insert_variable_checked(varname.clone(), ":".to_string());
                        self.variables
                            .insert("OPTARG".to_string(), opt_char.to_string());
                    } else {
                        self.insert_variable_checked(varname.clone(), "?".to_string());
                        let mut result = ExecResult::ok(String::new());
                        result.stderr = format!(
                            "bash: getopts: option requires an argument -- '{}'\n",
                            opt_char
                        );
                        result = self.apply_redirections(result, redirects).await?;
                        return Ok(result);
                    }
                }
            } else {
                // No argument needed
                self.variables.remove("OPTARG");
                if char_idx + 1 < opt_chars.len() {
                    // More chars in this arg
                    self.variables
                        .insert("_OPTCHAR_IDX".to_string(), (char_idx + 1).to_string());
                } else {
                    // Move to next arg
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                }
            }
        } else {
            // Unknown option
            self.variables.remove("OPTARG");
            if char_idx + 1 < opt_chars.len() {
                self.variables
                    .insert("_OPTCHAR_IDX".to_string(), (char_idx + 1).to_string());
            } else {
                self.variables
                    .insert("OPTIND".to_string(), (optind + 1).to_string());
                self.variables.remove("_OPTCHAR_IDX");
            }

            if silent {
                self.insert_variable_checked(varname.clone(), "?".to_string());
                self.variables
                    .insert("OPTARG".to_string(), opt_char.to_string());
            } else {
                self.insert_variable_checked(varname.clone(), "?".to_string());
                let mut result = ExecResult::ok(String::new());
                result.stderr = format!("bash: getopts: illegal option -- '{}'\n", opt_char);
                result = self.apply_redirections(result, redirects).await?;
                return Ok(result);
            }
        }

        let mut result = ExecResult::ok(String::new());
        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Execute the `command` builtin.
    ///
    /// - `command -v name` — print command path/name if found (exit 0) or nothing (exit 1)
    /// - `command -V name` — verbose: describe what `name` is
    /// - `command name args...` — run `name` bypassing shell functions
    async fn execute_command_builtin(
        &mut self,
        args: &[String],
        _stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let mut mode = ' '; // default: run the command
        let mut cmd_args_start = 0;

        // Parse flags
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "-v" {
                mode = 'v';
                i += 1;
            } else if arg == "-V" {
                mode = 'V';
                i += 1;
            } else if arg == "-p" {
                // -p: use default PATH (ignore in sandboxed env)
                i += 1;
            } else {
                cmd_args_start = i;
                break;
            }
        }

        if cmd_args_start >= args.len() {
            return Ok(ExecResult::ok(String::new()));
        }

        let cmd_name = &args[cmd_args_start];

        match mode {
            'v' => {
                // command -v: print name if it's a known command
                let found = self.builtins.contains_key(cmd_name.as_str())
                    || self.functions.contains_key(cmd_name.as_str())
                    || is_keyword(cmd_name);
                let mut result = if found {
                    ExecResult::ok(format!("{}\n", cmd_name))
                } else {
                    ExecResult {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 1,
                        control_flow: crate::interpreter::ControlFlow::None,
                        ..Default::default()
                    }
                };
                result = self.apply_redirections(result, redirects).await?;
                Ok(result)
            }
            'V' => {
                // command -V: verbose description
                let description = if self.functions.contains_key(cmd_name.as_str()) {
                    format!("{} is a function\n", cmd_name)
                } else if self.builtins.contains_key(cmd_name.as_str()) {
                    format!("{} is a shell builtin\n", cmd_name)
                } else if is_keyword(cmd_name) {
                    format!("{} is a shell keyword\n", cmd_name)
                } else {
                    return Ok(ExecResult::err(
                        format!("bash: command: {}: not found\n", cmd_name),
                        1,
                    ));
                };
                let mut result = ExecResult::ok(description);
                result = self.apply_redirections(result, redirects).await?;
                Ok(result)
            }
            _ => {
                // command name args...: run bypassing functions (use builtin only)
                // Build a synthetic simple command and execute it, skipping function lookup
                let remaining = &args[cmd_args_start..];
                if let Some(builtin) = self.builtins.get(remaining[0].as_str()) {
                    let builtin_args = &remaining[1..];
                    let shell_ref = ShellRef {
                        builtins: &self.builtins,
                        functions: &self.functions,
                        aliases: &mut self.aliases,
                        traps: &mut self.traps,
                        call_stack: &self.call_stack,
                        history: &self.history,
                        jobs: &self.jobs,
                    };
                    let ctx = builtins::Context {
                        args: builtin_args,
                        env: &self.env,
                        variables: &mut self.variables,
                        cwd: &mut self.cwd,
                        fs: Arc::clone(&self.fs),
                        stdin: _stdin.as_deref(),
                        #[cfg(feature = "http_client")]
                        http_client: self.http_client.as_ref(),
                        #[cfg(feature = "git")]
                        git_client: self.git_client.as_ref(),
                        shell: Some(shell_ref),
                    };
                    let mut result = builtin.execute(ctx).await?;
                    self.apply_builtin_side_effects(&result);
                    result = self.apply_redirections(result, redirects).await?;
                    Ok(result)
                } else {
                    Ok(ExecResult::err(
                        format!("bash: {}: command not found\n", remaining[0]),
                        127,
                    ))
                }
            }
        }
    }

    /// Execute `declare`/`typeset` builtin — declare variables with attributes.
    ///
    /// - `declare var=value` — set variable
    /// - `declare -i var=value` — integer attribute (stored as-is)
    /// - `declare -r var=value` — readonly
    /// - `declare -x var=value` — export
    /// - `declare -a arr` — indexed array
    /// - `declare -p [var]` — print variable declarations
    async fn execute_declare_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args.is_empty() {
            // declare with no args: print all variables, filtering internal markers (TM-INF-017)
            let mut output = String::new();
            let mut entries: Vec<_> = self.variables.iter().collect();
            entries.sort_by_key(|(k, _)| (*k).clone());
            for (name, value) in entries {
                if is_internal_variable(name) {
                    continue;
                }
                output.push_str(&format!("declare -- {}=\"{}\"\n", name, value));
            }
            let mut result = ExecResult::ok(output);
            result = self.apply_redirections(result, redirects).await?;
            return Ok(result);
        }

        let mut print_mode = false;
        let mut is_readonly = false;
        let mut is_export = false;
        let mut is_function = false;
        let mut flags = DeclareFlags::default();
        let mut remove_nameref = false;
        let mut is_lowercase = false;
        let mut is_uppercase = false;
        let mut names: Vec<&str> = Vec::new();

        for arg in args {
            if arg.starts_with('-') && !arg.contains('=') {
                flags.parse_flag_chars(arg);
                for c in arg[1..].chars() {
                    match c {
                        'p' => print_mode = true,
                        'r' => is_readonly = true,
                        'x' => is_export = true,
                        'f' => is_function = true,
                        'l' => is_lowercase = true,
                        'u' => is_uppercase = true,
                        _ => {} // n, a, A, i handled by flags
                    }
                }
            } else if arg.starts_with('+') && !arg.contains('=') {
                // +n removes nameref attribute
                for c in arg[1..].chars() {
                    if c == 'n' {
                        remove_nameref = true;
                    }
                }
            } else {
                names.push(arg);
            }
        }

        // declare -f: function display mode
        if is_function {
            let mut output = String::new();
            if names.is_empty() {
                // List all functions
                let mut func_names: Vec<_> = self.functions.keys().cloned().collect::<Vec<_>>();
                func_names.sort();
                for fname in &func_names {
                    output.push_str(&format!("{} ()\n{{\n    ...\n}}\n", fname));
                }
            } else {
                // Print specific functions — return 1 if any not found
                for name in &names {
                    if self.functions.contains_key(*name) {
                        output.push_str(&format!("{} ()\n{{\n    ...\n}}\n", name));
                    } else {
                        let mut result = ExecResult::with_code(String::new(), 1);
                        result = self.apply_redirections(result, redirects).await?;
                        return Ok(result);
                    }
                }
            }
            let mut result = ExecResult::ok(output);
            result = self.apply_redirections(result, redirects).await?;
            return Ok(result);
        }

        if print_mode {
            let mut output = String::new();
            if names.is_empty() {
                // Print all variables, filtering internal markers (TM-INF-017)
                let mut entries: Vec<_> = self.variables.iter().collect();
                entries.sort_by_key(|(k, _)| (*k).clone());
                for (name, value) in entries {
                    if is_internal_variable(name) {
                        continue;
                    }
                    output.push_str(&format!("declare -- {}=\"{}\"\n", name, value));
                }
            } else {
                for name in &names {
                    // Strip =value if present
                    let var_name = name.split('=').next().unwrap_or(name);
                    if let Some(value) = self.variables.get(var_name) {
                        let mut attrs = String::from("--");
                        if self
                            .variables
                            .contains_key(&format!("_READONLY_{}", var_name))
                        {
                            attrs = String::from("-r");
                        }
                        output.push_str(&format!("declare {} {}=\"{}\"\n", attrs, var_name, value));
                    } else if let Some(arr) = self.assoc_arrays.get(var_name) {
                        let mut items: Vec<_> = arr.iter().collect();
                        items.sort_by_key(|(k, _)| (*k).clone());
                        let inner: String = items
                            .iter()
                            .map(|(k, v)| format!("[{}]=\"{}\"", k, v))
                            .collect::<Vec<_>>()
                            .join(" ");
                        output.push_str(&format!("declare -A {}=({})\n", var_name, inner));
                    } else if let Some(arr) = self.arrays.get(var_name) {
                        let mut items: Vec<_> = arr.iter().collect();
                        items.sort_by_key(|(k, _)| *k);
                        let inner: String = items
                            .iter()
                            .map(|(k, v)| format!("[{}]=\"{}\"", k, v))
                            .collect::<Vec<_>>()
                            .join(" ");
                        output.push_str(&format!("declare -a {}=({})\n", var_name, inner));
                    } else {
                        return Ok(ExecResult::err(
                            format!("bash: declare: {}: not found\n", var_name),
                            1,
                        ));
                    }
                }
            }
            let mut result = ExecResult::ok(output);
            result = self.apply_redirections(result, redirects).await?;
            return Ok(result);
        }

        // Reconstruct compound assignments: declare -A m=([a]="1" [b]="2")
        let merged_names = merge_compound_assignments(&names);

        // Set variables
        for name in &merged_names {
            if let Some(eq_pos) = name.find('=') {
                let var_name = &name[..eq_pos];
                let value = &name[eq_pos + 1..];

                // THREAT[TM-INJ-012]: Block internal variable prefix injection via declare
                if is_internal_variable(var_name) {
                    continue;
                }

                // THREAT[TM-INJ-020]: Refuse to overwrite readonly variables
                if self
                    .variables
                    .contains_key(&format!("_READONLY_{}", var_name))
                {
                    continue;
                }

                // Handle compound array assignment: declare -A m=([k]="v" ...)
                if (flags.assoc || flags.array) && value.starts_with('(') && value.ends_with(')') {
                    let inner = &value[1..value.len() - 1];
                    if flags.assoc {
                        let arr = self.assoc_arrays.entry(var_name.to_string()).or_default();
                        arr.clear();
                        // Parse [key]="value" pairs
                        let mut rest = inner.trim();
                        while let Some(bracket_start) = rest.find('[') {
                            if let Some(bracket_end) = rest[bracket_start..].find(']') {
                                let key = &rest[bracket_start + 1..bracket_start + bracket_end];
                                let after = &rest[bracket_start + bracket_end + 1..];
                                if let Some(eq_rest) = after.strip_prefix('=') {
                                    let eq_rest = eq_rest.trim_start();
                                    let (val, remainder) = if let Some(stripped) =
                                        eq_rest.strip_prefix('"')
                                    {
                                        // Quoted value
                                        if let Some(end_q) = stripped.find('"') {
                                            (&stripped[..end_q], stripped[end_q + 1..].trim_start())
                                        } else {
                                            (stripped.trim_end_matches('"'), "")
                                        }
                                    } else {
                                        // Unquoted value — up to next space or end
                                        match eq_rest.find(char::is_whitespace) {
                                            Some(sp) => {
                                                (&eq_rest[..sp], eq_rest[sp..].trim_start())
                                            }
                                            None => (eq_rest, ""),
                                        }
                                    };
                                    arr.insert(key.to_string(), val.to_string());
                                    rest = remainder;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    } else {
                        // Indexed array: declare -a arr=(a b c)
                        let arr = self.arrays.entry(var_name.to_string()).or_default();
                        arr.clear();
                        for (idx, val) in inner.split_whitespace().enumerate() {
                            arr.insert(idx, val.trim_matches('"').to_string());
                        }
                    }
                } else if flags.nameref {
                    // declare -n ref=target: create nameref
                    self.variables
                        .insert(format!("_NAMEREF_{}", var_name), value.to_string());
                } else if flags.integer {
                    // Evaluate as arithmetic expression
                    let int_val = self.evaluate_arithmetic_with_assign(value);
                    self.insert_variable_checked(var_name.to_string(), int_val.to_string());
                    // Set persistent integer attribute marker
                    self.variables
                        .insert(format!("_INTEGER_{}", var_name), "1".to_string());
                } else {
                    // Apply case conversion attributes
                    let final_value = if is_lowercase {
                        value.to_lowercase()
                    } else if is_uppercase {
                        value.to_uppercase()
                    } else {
                        value.to_string()
                    };
                    self.insert_variable_checked(var_name.to_string(), final_value);
                }

                // Set case conversion attribute markers
                if is_lowercase {
                    self.variables
                        .insert(format!("_LOWER_{}", var_name), "1".to_string());
                    self.variables.remove(&format!("_UPPER_{}", var_name));
                }
                if is_uppercase {
                    self.variables
                        .insert(format!("_UPPER_{}", var_name), "1".to_string());
                    self.variables.remove(&format!("_LOWER_{}", var_name));
                }
                if is_readonly {
                    self.variables
                        .insert(format!("_READONLY_{}", var_name), "1".to_string());
                }
                if is_export {
                    self.env.insert(
                        var_name.to_string(),
                        self.variables.get(var_name).cloned().unwrap_or_default(),
                    );
                }
            } else {
                // Declare without value
                if remove_nameref {
                    // typeset +n ref: remove nameref attribute
                    self.variables.remove(&format!("_NAMEREF_{}", name));
                } else if flags.nameref {
                    // typeset -n ref (without =value): use existing variable value as target
                    if let Some(existing) = self.variables.get(name.as_str()).cloned()
                        && !existing.is_empty()
                    {
                        self.variables
                            .insert(format!("_NAMEREF_{}", name), existing);
                    }
                } else if flags.assoc {
                    // Initialize empty associative array
                    self.assoc_arrays.entry(name.to_string()).or_default();
                } else if flags.array {
                    // Initialize empty indexed array
                    self.arrays.entry(name.to_string()).or_default();
                } else if !self.variables.contains_key(name.as_str()) {
                    self.insert_variable_checked(name.to_string(), String::new());
                }
                // Set case conversion attribute markers
                if is_lowercase {
                    self.variables
                        .insert(format!("_LOWER_{}", name), "1".to_string());
                    self.variables.remove(&format!("_UPPER_{}", name));
                }
                if is_uppercase {
                    self.variables
                        .insert(format!("_UPPER_{}", name), "1".to_string());
                    self.variables.remove(&format!("_LOWER_{}", name));
                }
                if is_readonly {
                    self.variables
                        .insert(format!("_READONLY_{}", name), "1".to_string());
                }
                if flags.integer {
                    self.variables
                        .insert(format!("_INTEGER_{}", name), "1".to_string());
                }
                if is_export {
                    self.env.insert(
                        name.to_string(),
                        self.variables
                            .get(name.as_str())
                            .cloned()
                            .unwrap_or_default(),
                    );
                }
            }
        }

        let mut result = ExecResult::ok(String::new());
        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Process input redirections (< file, <<< string)
    async fn process_input_redirections(
        &mut self,
        existing_stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<Option<String>> {
        let mut stdin = existing_stdin;

        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Input => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Handle /dev/null at interpreter level - cannot be bypassed
                    if is_dev_null(&path) {
                        stdin = Some(String::new()); // EOF
                    } else {
                        let content = self.fs.read_file(&path).await?;
                        stdin = Some(bytes_to_latin1_string(&content));
                    }
                }
                RedirectKind::HereString => {
                    // <<< string - use the target as stdin content
                    let content = self.expand_word(&redirect.target).await?;
                    stdin = Some(format!("{}\n", content));
                }
                RedirectKind::HereDoc | RedirectKind::HereDocStrip => {
                    // << EOF / <<- EOF - use the heredoc content as stdin
                    let content = self.expand_word(&redirect.target).await?;
                    stdin = Some(content);
                }
                RedirectKind::DupInput => {
                    // <&FD - if FD is a coproc read FD, consume next line
                    let target = self.expand_word(&redirect.target).await?;
                    if let Ok(fd) = target.parse::<i32>()
                        && let Some(buf) = self.coproc_buffers.get_mut(&fd)
                    {
                        if let Some(line) = buf.pop() {
                            stdin = Some(format!("{}\n", line));
                        } else {
                            stdin = Some(String::new()); // EOF
                        }
                    }
                }
                _ => {
                    // Output redirections handled separately
                }
            }
        }

        Ok(stdin)
    }

    /// Execute an [`ExecutionPlan`] returned by a builtin's `execution_plan()` method.
    ///
    /// This is the interpreter hook that fulfills sub-command execution requests
    /// from builtins like `timeout`, `xargs`, and `find -exec`.
    async fn execute_builtin_plan(
        &mut self,
        plan: builtins::ExecutionPlan,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let result = match plan {
            builtins::ExecutionPlan::Timeout {
                duration,
                preserve_status,
                command,
            } => {
                use tokio::time::timeout;

                // Build inner command with optional stdin via here-string
                let inner_redirects = if let Some(ref stdin_data) = command.stdin {
                    vec![Redirect {
                        fd: None,
                        kind: RedirectKind::HereString,
                        target: Word::literal(stdin_data.trim_end_matches('\n').to_string()),
                    }]
                } else {
                    Vec::new()
                };

                let inner_cmd = Command::Simple(SimpleCommand {
                    name: Word::literal(command.name),
                    args: command
                        .args
                        .iter()
                        .map(|s| Word::literal(s.clone()))
                        .collect(),
                    redirects: inner_redirects,
                    assignments: Vec::new(),
                    span: Span::new(),
                });

                let exec_future = self.execute_command(&inner_cmd);
                match timeout(duration, exec_future).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        // Timeout expired.
                        // --preserve-status: in real bash, returns the signal+128 status
                        // of the killed child.  We can't capture that from tokio::timeout,
                        // so we always use 124 (the standard timeout exit code).
                        // TODO: propagate child exit status when preserve_status is true
                        let exit_code = if preserve_status { 137 } else { 124 };
                        ExecResult::err(String::new(), exit_code)
                    }
                }
            }
            builtins::ExecutionPlan::Batch { commands } => {
                let mut combined_stdout = String::new();
                let mut combined_stderr = String::new();
                let mut last_exit_code = 0;

                for cmd in commands {
                    let cmd_redirects = if let Some(ref stdin_data) = cmd.stdin {
                        vec![Redirect {
                            fd: None,
                            kind: RedirectKind::HereString,
                            target: Word::literal(stdin_data.trim_end_matches('\n').to_string()),
                        }]
                    } else {
                        Vec::new()
                    };

                    let inner_cmd = Command::Simple(SimpleCommand {
                        name: Word::literal(cmd.name),
                        args: cmd.args.iter().map(|s| Word::literal(s.clone())).collect(),
                        redirects: cmd_redirects,
                        assignments: Vec::new(),
                        span: Span::new(),
                    });

                    let result = self.execute_command(&inner_cmd).await?;
                    combined_stdout.push_str(&result.stdout);
                    combined_stderr.push_str(&result.stderr);
                    last_exit_code = result.exit_code;
                }

                ExecResult {
                    stdout: combined_stdout,
                    stderr: combined_stderr,
                    exit_code: last_exit_code,
                    control_flow: ControlFlow::None,
                    ..Default::default()
                }
            }
        };

        self.apply_redirections(result, redirects).await
    }

    /// Process structured side effects from builtin execution.
    fn apply_builtin_side_effects(&mut self, result: &ExecResult) {
        for effect in &result.side_effects {
            match effect {
                builtins::BuiltinSideEffect::SetArray { name, elements } => {
                    let mut arr = HashMap::new();
                    for (i, word) in elements.iter().enumerate() {
                        if !word.is_empty() {
                            arr.insert(i, word.clone());
                        }
                    }
                    self.insert_array_checked(name.clone(), arr);
                }
                builtins::BuiltinSideEffect::SetIndexedArray { name, entries } => {
                    let arr: HashMap<usize, String> = entries.iter().cloned().collect();
                    // Remove existing array first (mirrors mapfile behavior)
                    self.arrays.remove(name);
                    if !arr.is_empty() {
                        self.insert_array_checked(name.clone(), arr);
                    }
                }
                builtins::BuiltinSideEffect::RemoveArray(name) => {
                    self.arrays.remove(name);
                }
                builtins::BuiltinSideEffect::ShiftPositional(n) => {
                    if let Some(frame) = self.call_stack.last_mut() {
                        if *n <= frame.positional.len() {
                            frame.positional.drain(..*n);
                        } else {
                            frame.positional.clear();
                        }
                    }
                }
                builtins::BuiltinSideEffect::SetPositional(new_positional) => {
                    if let Some(frame) = self.call_stack.last_mut() {
                        frame.positional = new_positional.clone();
                    } else {
                        self.call_stack.push(CallFrame {
                            name: String::new(),
                            locals: HashMap::new(),
                            positional: new_positional.clone(),
                        });
                    }
                }
                builtins::BuiltinSideEffect::ClearHistory => {
                    self.history.clear();
                }
                builtins::BuiltinSideEffect::SetLastExitCode(code) => {
                    self.last_exit_code = *code;
                }
                builtins::BuiltinSideEffect::SetVariable { name, value } => {
                    self.set_variable(name.clone(), value.clone());
                }
            }
        }
    }

    /// Apply output redirections to command output
    async fn apply_redirections(
        &mut self,
        mut result: ExecResult,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Skip the fd-table path when there are no DupOutput redirects mixed
        // with file redirects — the simple single-pass logic is sufficient and
        // avoids any behavioural delta for the common case.
        let has_dup_output = redirects.iter().any(|r| r.kind == RedirectKind::DupOutput);
        let has_file_redirect = redirects.iter().any(|r| {
            matches!(
                r.kind,
                RedirectKind::Output
                    | RedirectKind::Clobber
                    | RedirectKind::Append
                    | RedirectKind::OutputBoth
            )
        });

        if has_dup_output && has_file_redirect {
            return self.apply_redirections_fd_table(result, redirects).await;
        }

        // --- Fast path: no mixed dup+file redirects ---
        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Output | RedirectKind::Clobber => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if is_dev_null(&path) {
                        match redirect.fd {
                            Some(2) => result.stderr = String::new(),
                            _ => result.stdout = String::new(),
                        }
                    } else {
                        if redirect.kind == RedirectKind::Output
                            && self.variables.get("SHOPT_C").map(|v| v.as_str()) == Some("1")
                            && self.fs.stat(&path).await.is_ok()
                        {
                            result.stdout = String::new();
                            result.stderr =
                                format!("bash: {}: cannot overwrite existing file\n", target_path);
                            result.exit_code = 1;
                            return Ok(result);
                        }
                        match redirect.fd {
                            Some(2) => {
                                if let Err(e) =
                                    self.fs.write_file(&path, result.stderr.as_bytes()).await
                                {
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = String::new();
                            }
                            _ => {
                                if let Err(e) =
                                    self.fs.write_file(&path, result.stdout.as_bytes()).await
                                {
                                    result.stdout = String::new();
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = String::new();
                            }
                        }
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if is_dev_null(&path) {
                        match redirect.fd {
                            Some(2) => result.stderr = String::new(),
                            _ => result.stdout = String::new(),
                        }
                    } else {
                        match redirect.fd {
                            Some(2) => {
                                if let Err(e) =
                                    self.fs.append_file(&path, result.stderr.as_bytes()).await
                                {
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = String::new();
                            }
                            _ => {
                                if let Err(e) =
                                    self.fs.append_file(&path, result.stdout.as_bytes()).await
                                {
                                    result.stdout = String::new();
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = String::new();
                            }
                        }
                    }
                }
                RedirectKind::OutputBoth => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if is_dev_null(&path) {
                        result.stdout = String::new();
                        result.stderr = String::new();
                    } else {
                        let combined = format!("{}{}", result.stdout, result.stderr);
                        if let Err(e) = self.fs.write_file(&path, combined.as_bytes()).await {
                            result.stderr = format!("bash: {}: {}\n", target_path, e);
                            result.exit_code = 1;
                            return Ok(result);
                        }
                        result.stdout = String::new();
                        result.stderr = String::new();
                    }
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let target_fd: i32 = target.parse().unwrap_or(1);
                    let src_fd = redirect.fd.unwrap_or(1);

                    match (src_fd, target_fd) {
                        (2, 1) => {
                            result.stdout.push_str(&result.stderr);
                            result.stderr = String::new();
                        }
                        (1, 2) => {
                            result.stderr.push_str(&result.stdout);
                            result.stdout = String::new();
                        }
                        _ => {}
                    }
                }
                RedirectKind::Input
                | RedirectKind::HereString
                | RedirectKind::HereDoc
                | RedirectKind::HereDocStrip => {}
                RedirectKind::DupInput => {}
            }
        }

        Ok(result)
    }

    /// Apply redirections using an fd-table model for correct left-to-right
    /// ordering when DupOutput and file redirects are mixed (e.g. `2>&1 >file`).
    async fn apply_redirections_fd_table(
        &mut self,
        mut result: ExecResult,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Build fd table: fd1 = stdout pipe, fd2 = stderr pipe
        let mut fd1 = FdTarget::Stdout;
        let mut fd2 = FdTarget::Stderr;

        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Output | RedirectKind::Clobber => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);

                    if redirect.kind == RedirectKind::Output
                        && self.variables.get("SHOPT_C").map(|v| v.as_str()) == Some("1")
                        && !is_dev_null(&path)
                        && self.fs.stat(&path).await.is_ok()
                    {
                        result.stdout = String::new();
                        result.stderr =
                            format!("bash: {}: cannot overwrite existing file\n", target_path);
                        result.exit_code = 1;
                        return Ok(result);
                    }

                    let target = if is_dev_null(&path) {
                        FdTarget::DevNull
                    } else {
                        FdTarget::WriteFile(path, target_path)
                    };
                    match redirect.fd {
                        Some(2) => fd2 = target,
                        _ => fd1 = target,
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    let target = if is_dev_null(&path) {
                        FdTarget::DevNull
                    } else {
                        FdTarget::AppendFile(path, target_path)
                    };
                    match redirect.fd {
                        Some(2) => fd2 = target,
                        _ => fd1 = target,
                    }
                }
                RedirectKind::OutputBoth => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    let target = if is_dev_null(&path) {
                        FdTarget::DevNull
                    } else {
                        FdTarget::WriteFile(path, target_path)
                    };
                    fd1 = target.clone();
                    fd2 = target;
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let target_fd: i32 = target.parse().unwrap_or(1);
                    let src_fd = redirect.fd.unwrap_or(1);

                    match (src_fd, target_fd) {
                        (2, 1) => fd2 = fd1.clone(),
                        (1, 2) => fd1 = fd2.clone(),
                        _ => {}
                    }
                }
                RedirectKind::Input
                | RedirectKind::HereString
                | RedirectKind::HereDoc
                | RedirectKind::HereDocStrip
                | RedirectKind::DupInput => {}
            }
        }

        // Route original stdout/stderr based on final fd table.
        // Collect file writes to avoid double-borrow issues.
        let orig_stdout = std::mem::take(&mut result.stdout);
        let orig_stderr = std::mem::take(&mut result.stderr);

        // Determine what goes where
        let mut new_stdout = String::new();
        let mut new_stderr = String::new();
        // file_path -> (content, is_append, display_path)
        let mut file_writes: std::collections::HashMap<PathBuf, (String, bool, String)> =
            std::collections::HashMap::new();

        for (data, fd_target) in [(&orig_stdout, &fd1), (&orig_stderr, &fd2)].iter() {
            if data.is_empty() {
                continue;
            }
            match fd_target {
                FdTarget::Stdout => new_stdout.push_str(data),
                FdTarget::Stderr => new_stderr.push_str(data),
                FdTarget::DevNull => {}
                FdTarget::WriteFile(path, display) => {
                    file_writes
                        .entry(path.clone())
                        .or_insert_with(|| (String::new(), false, display.clone()))
                        .0
                        .push_str(data);
                }
                FdTarget::AppendFile(path, display) => {
                    file_writes
                        .entry(path.clone())
                        .or_insert_with(|| (String::new(), true, display.clone()))
                        .0
                        .push_str(data);
                }
            }
        }

        // Write files
        for (path, (content, is_append, display_path)) in &file_writes {
            let write_result = if *is_append {
                self.fs.append_file(path, content.as_bytes()).await
            } else {
                self.fs.write_file(path, content.as_bytes()).await
            };
            if let Err(e) = write_result {
                new_stderr = format!("bash: {}: {}\n", display_path, e);
                result.exit_code = 1;
                result.stdout = new_stdout;
                result.stderr = new_stderr;
                return Ok(result);
            }
        }

        result.stdout = new_stdout;
        result.stderr = new_stderr;
        Ok(result)
    }

    /// Resolve a path relative to cwd
    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }

    /// Expand an array access expression (`${arr[index]}`).
    fn expand_array_access_part(&self, name: &str, index: &str) -> String {
        let resolved_name = self.resolve_nameref(name);
        let (arr_name, extra_index) = if let Some(bracket) = resolved_name.find('[') {
            let idx_part = &resolved_name[bracket + 1..resolved_name.len() - 1];
            (&resolved_name[..bracket], Some(idx_part.to_string()))
        } else {
            (resolved_name, None)
        };

        let mut result = String::new();
        if index == "@" || index == "*" {
            let sep = if index == "*" {
                self.get_ifs_separator()
            } else {
                " ".to_string()
            };
            if let Some(arr) = self.assoc_arrays.get(arr_name) {
                let mut keys: Vec<_> = arr.keys().collect();
                keys.sort();
                let values: Vec<String> =
                    keys.iter().filter_map(|k| arr.get(*k).cloned()).collect();
                result.push_str(&values.join(&sep));
            } else if let Some(arr) = self.arrays.get(arr_name) {
                let mut indices: Vec<_> = arr.keys().collect();
                indices.sort();
                let values: Vec<_> = indices.iter().filter_map(|i| arr.get(i)).collect();
                result.push_str(&values.into_iter().cloned().collect::<Vec<_>>().join(&sep));
            }
        } else if let Some(extra_idx) = extra_index {
            if let Some(arr) = self.assoc_arrays.get(arr_name) {
                if let Some(value) = arr.get(&extra_idx) {
                    result.push_str(value);
                }
            } else {
                let idx: usize = self.evaluate_arithmetic(&extra_idx).try_into().unwrap_or(0);
                if let Some(arr) = self.arrays.get(arr_name)
                    && let Some(value) = arr.get(&idx)
                {
                    result.push_str(value);
                }
            }
        } else if let Some(arr) = self.assoc_arrays.get(arr_name) {
            let key = self.expand_variable_or_literal(index);
            if let Some(value) = arr.get(&key) {
                result.push_str(value);
            }
        } else {
            let raw_idx = self.evaluate_arithmetic(index);
            let idx = if raw_idx < 0 {
                let len = self
                    .arrays
                    .get(arr_name)
                    .map(|a| a.keys().max().map(|m| m + 1).unwrap_or(0))
                    .unwrap_or(0) as i64;
                (len + raw_idx).max(0) as usize
            } else {
                raw_idx as usize
            };
            if let Some(arr) = self.arrays.get(arr_name)
                && let Some(value) = arr.get(&idx)
            {
                result.push_str(value);
            }
        }
        result
    }

    /// Apply a `${var@operator}` transformation.
    fn apply_transformation(&self, name: &str, operator: char) -> String {
        let value = self.expand_variable(name);
        match operator {
            'Q' => format!("'{}'", value.replace('\'', "'\\''")),
            'E' => value
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\\", "\\"),
            'P' => value.clone(),
            'A' => format!("{}='{}'", name, value.replace('\'', "'\\''")),
            'K' => value.clone(),
            'a' => {
                let mut attrs = String::new();
                if self.variables.contains_key(&format!("_READONLY_{}", name)) {
                    attrs.push('r');
                }
                if self.env.contains_key(name) {
                    attrs.push('x');
                }
                attrs
            }
            'u' | 'U' => {
                if operator == 'U' {
                    value.to_uppercase()
                } else {
                    let mut chars = value.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                }
            }
            'L' => value.to_lowercase(),
            _ => value.clone(),
        }
    }

    /// Expand a process substitution (`<(cmd)` or `>(cmd)`).
    async fn expand_process_substitution(
        &mut self,
        commands: &[Command],
        is_input: bool,
    ) -> Result<String> {
        let path_str = format!(
            "/dev/fd/proc_sub_{}",
            PROC_SUB_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = Path::new(&path_str);

        if is_input {
            let mut stdout = String::new();
            for cmd in commands {
                let cmd_result = self.execute_command(cmd).await?;
                stdout.push_str(&cmd_result.stdout);
            }
            if self.fs.write_file(path, stdout.as_bytes()).await.is_err() {
                Ok(stdout)
            } else {
                Ok(path_str)
            }
        } else {
            let _ = self.fs.write_file(path, b"").await;
            self.deferred_proc_subs
                .push((path_str.clone(), commands.to_vec()));
            Ok(path_str)
        }
    }

    async fn expand_word(&mut self, word: &Word) -> Result<String> {
        let mut result = String::new();
        let mut is_first_part = true;

        for part in &word.parts {
            match part {
                WordPart::Literal(s) => {
                    // Tilde expansion: ~ at start of word expands to $HOME
                    if is_first_part && s.starts_with('~') {
                        let home = self
                            .env
                            .get("HOME")
                            .or_else(|| self.variables.get("HOME"))
                            .cloned()
                            .unwrap_or_else(|| "/home/user".to_string());

                        if s == "~" {
                            result.push_str(&home);
                        } else if s.starts_with("~/") {
                            result.push_str(&home);
                            result.push_str(&s[1..]);
                        } else {
                            result.push_str(s);
                        }
                    } else {
                        result.push_str(s);
                    }
                }
                WordPart::Variable(name) => {
                    if self.is_nounset() && !self.is_variable_set(name) {
                        self.nounset_error = Some(format!("bash: {}: unbound variable\n", name));
                    }
                    if name == "*" && word.quoted {
                        let positional = self
                            .call_stack
                            .last()
                            .map(|f| f.positional.clone())
                            .unwrap_or_default();
                        let sep = match self.variables.get("IFS") {
                            Some(ifs) => ifs
                                .chars()
                                .next()
                                .map(|c| c.to_string())
                                .unwrap_or_default(),
                            None => " ".to_string(),
                        };
                        result.push_str(&positional.join(&sep));
                    } else {
                        result.push_str(&self.expand_variable(name));
                    }
                }
                WordPart::CommandSubstitution(commands) => {
                    // THREAT[TM-DOS-044]: Track substitution depth to prevent stack overflow
                    if self.counters.push_function(&self.limits).is_err() {
                        return Err(crate::error::Error::Execution(
                            "maximum nesting depth exceeded in command substitution".to_string(),
                        ));
                    }
                    // Command substitution runs in a subshell: snapshot all
                    // mutable state so mutations don't leak to the parent.
                    let saved_traps = self.traps.clone();
                    let saved_functions = self.functions.clone();
                    let saved_vars = self.variables.clone();
                    let saved_arrays = self.arrays.clone();
                    let saved_assoc = self.assoc_arrays.clone();
                    let saved_aliases = self.aliases.clone();
                    let saved_cwd = self.cwd.clone();
                    let mut stdout = String::new();
                    for cmd in commands {
                        let cmd_result = self.execute_command(cmd).await?;
                        stdout.push_str(&cmd_result.stdout);
                        self.last_exit_code = cmd_result.exit_code;
                        if matches!(cmd_result.control_flow, ControlFlow::Exit(_)) {
                            break;
                        }
                    }
                    // Fire EXIT trap set inside the command substitution
                    if let Some(trap_cmd) = self.traps.get("EXIT").cloned()
                        && saved_traps.get("EXIT") != Some(&trap_cmd)
                        && let Ok(trap_script) = Parser::with_limits(
                            &trap_cmd,
                            self.limits.max_ast_depth,
                            self.limits.max_parser_operations,
                        )
                        .parse()
                        && let Ok(trap_result) =
                            self.execute_command_sequence(&trap_script.commands).await
                    {
                        stdout.push_str(&trap_result.stdout);
                    }
                    // Restore parent state
                    self.traps = saved_traps;
                    self.functions = saved_functions;
                    self.variables = saved_vars;
                    self.arrays = saved_arrays;
                    self.assoc_arrays = saved_assoc;
                    self.aliases = saved_aliases;
                    self.cwd = saved_cwd;
                    self.counters.pop_function();
                    self.subst_generation += 1;
                    let trimmed = stdout.trim_end_matches('\n');
                    result.push_str(trimmed);
                }
                WordPart::ArithmeticExpansion(expr) => {
                    let expanded_expr = if expr.contains("$(") {
                        self.expand_command_subs_in_arithmetic(expr).await?
                    } else {
                        expr.to_string()
                    };
                    let value = self.evaluate_arithmetic_with_assign(&expanded_expr);
                    result.push_str(&value.to_string());
                }
                WordPart::Length(name) => {
                    let value = if let Some(bracket_pos) = name.find('[') {
                        let arr_name = &name[..bracket_pos];
                        let index_end = name.find(']').unwrap_or(name.len());
                        let index_str = &name[bracket_pos + 1..index_end];
                        let idx: usize =
                            self.evaluate_arithmetic(index_str).try_into().unwrap_or(0);
                        if let Some(arr) = self.arrays.get(arr_name) {
                            arr.get(&idx).cloned().unwrap_or_default()
                        } else {
                            String::new()
                        }
                    } else {
                        self.expand_variable(name)
                    };
                    result.push_str(&value.chars().count().to_string());
                }
                WordPart::ParameterExpansion {
                    name,
                    operator,
                    operand,
                    colon_variant,
                } => {
                    if name.is_empty()
                        && !matches!(
                            operator,
                            ParameterOp::UseDefault
                                | ParameterOp::AssignDefault
                                | ParameterOp::UseReplacement
                                | ParameterOp::Error
                        )
                    {
                        self.nounset_error = Some("bash: ${}: bad substitution\n".to_string());
                        continue;
                    }

                    let suppress_nounset = matches!(
                        operator,
                        ParameterOp::UseDefault
                            | ParameterOp::AssignDefault
                            | ParameterOp::UseReplacement
                            | ParameterOp::Error
                    );

                    let (is_set, value) = self.resolve_param_expansion_name(name);

                    if self.is_nounset() && !suppress_nounset && !is_set {
                        self.nounset_error = Some(format!("bash: {}: unbound variable\n", name));
                    }
                    let expanded = self.apply_parameter_op(
                        &value,
                        name,
                        operator,
                        operand,
                        *colon_variant,
                        is_set,
                    );
                    result.push_str(&expanded);
                }
                WordPart::ArrayAccess { name, index } => {
                    result.push_str(&self.expand_array_access_part(name, index));
                }
                WordPart::ArrayIndices(name) => {
                    let resolved = self.resolve_nameref(name);
                    if let Some(arr) = self.assoc_arrays.get(resolved) {
                        let mut keys: Vec<_> = arr.keys().cloned().collect();
                        keys.sort();
                        result.push_str(&keys.join(" "));
                    } else if let Some(arr) = self.arrays.get(resolved) {
                        let mut indices: Vec<_> = arr.keys().collect();
                        indices.sort();
                        let index_strs: Vec<String> =
                            indices.iter().map(|i| i.to_string()).collect();
                        result.push_str(&index_strs.join(" "));
                    }
                }
                WordPart::Substring {
                    name,
                    offset,
                    length,
                } => {
                    let value = self.expand_variable(name);
                    let char_count = value.chars().count();
                    let offset_val: isize = self.evaluate_arithmetic(offset) as isize;
                    let start = if offset_val < 0 {
                        (char_count as isize + offset_val).max(0) as usize
                    } else {
                        (offset_val as usize).min(char_count)
                    };
                    let substr: String = if let Some(len_expr) = length {
                        let len_val = self.evaluate_arithmetic(len_expr) as usize;
                        value.chars().skip(start).take(len_val).collect()
                    } else {
                        value.chars().skip(start).collect()
                    };
                    result.push_str(&substr);
                }
                WordPart::ArraySlice {
                    name,
                    offset,
                    length,
                } => {
                    if let Some(arr) = self.arrays.get(name) {
                        let mut indices: Vec<_> = arr.keys().cloned().collect();
                        indices.sort();
                        let values: Vec<_> =
                            indices.iter().filter_map(|i| arr.get(i).cloned()).collect();

                        let offset_val: isize = self.evaluate_arithmetic(offset) as isize;
                        let start = if offset_val < 0 {
                            (values.len() as isize + offset_val).max(0) as usize
                        } else {
                            (offset_val as usize).min(values.len())
                        };

                        let sliced = if let Some(len_expr) = length {
                            let len_val = self.evaluate_arithmetic(len_expr) as usize;
                            let end = (start + len_val).min(values.len());
                            &values[start..end]
                        } else {
                            &values[start..]
                        };
                        result.push_str(&sliced.join(" "));
                    }
                }
                WordPart::IndirectExpansion(name) => {
                    let nameref_key = format!("_NAMEREF_{}", name);
                    if let Some(target) = self.variables.get(&nameref_key).cloned() {
                        result.push_str(&target);
                    } else {
                        let var_name = self.expand_variable(name);
                        if let Some(arr) = self.arrays.get(&var_name) {
                            if let Some(first) = arr.get(&0) {
                                result.push_str(first);
                            }
                        } else {
                            let value = self.expand_variable(&var_name);
                            result.push_str(&value);
                        }
                    }
                }
                WordPart::PrefixMatch(prefix) => {
                    let mut names: Vec<String> = self
                        .variables
                        .keys()
                        .filter(|k| k.starts_with(prefix.as_str()))
                        // THREAT[TM-INJ-009]: Hide internal marker variables
                        .filter(|k| !Self::is_internal_variable(k))
                        .cloned()
                        .collect();
                    for k in self.env.keys() {
                        if k.starts_with(prefix.as_str())
                            && !names.contains(k)
                            // THREAT[TM-INJ-009]: Hide internal marker variables
                            && !Self::is_internal_variable(k)
                        {
                            names.push(k.clone());
                        }
                    }
                    names.sort();
                    result.push_str(&names.join(" "));
                }
                WordPart::ArrayLength(name) => {
                    let resolved = self.resolve_nameref(name);
                    if let Some(arr) = self.assoc_arrays.get(resolved) {
                        result.push_str(&arr.len().to_string());
                    } else if let Some(arr) = self.arrays.get(resolved) {
                        result.push_str(&arr.len().to_string());
                    } else {
                        result.push('0');
                    }
                }
                WordPart::ProcessSubstitution { commands, is_input } => {
                    let expanded = self
                        .expand_process_substitution(commands, *is_input)
                        .await?;
                    result.push_str(&expanded);
                }
                WordPart::Transformation { name, operator } => {
                    result.push_str(&self.apply_transformation(name, *operator));
                }
            }
            is_first_part = false;
        }

        Ok(result)
    }

    /// Expand a word to multiple fields (for array iteration and command args)
    /// Returns Vec<String> where array expansions like "${arr[@]}" produce multiple fields.
    /// "${arr[*]}" in quoted context joins elements into a single field (bash behavior).
    async fn expand_word_to_fields(&mut self, word: &Word) -> Result<Vec<String>> {
        // Check if the word contains only an array expansion or $@/$*
        if word.parts.len() == 1 {
            // Handle $@ and $* as special parameters
            if let WordPart::Variable(name) = &word.parts[0] {
                if name == "@" {
                    let positional = self
                        .call_stack
                        .last()
                        .map(|f| f.positional.clone())
                        .unwrap_or_default();
                    if word.quoted {
                        // "$@" preserves individual positional params
                        return Ok(positional);
                    }
                    // $@ unquoted: each param is subject to further IFS splitting
                    let mut fields = Vec::new();
                    for p in &positional {
                        fields.extend(self.ifs_split(p));
                    }
                    return Ok(fields);
                }
                if name == "*" {
                    let positional = self
                        .call_stack
                        .last()
                        .map(|f| f.positional.clone())
                        .unwrap_or_default();
                    if word.quoted {
                        // "$*" joins with first char of IFS.
                        // IFS unset → space; IFS="" → no separator.
                        let sep = match self.variables.get("IFS") {
                            Some(ifs) => ifs
                                .chars()
                                .next()
                                .map(|c| c.to_string())
                                .unwrap_or_default(),
                            None => " ".to_string(),
                        };
                        return Ok(vec![positional.join(&sep)]);
                    }
                    // $* unquoted: each param is subject to IFS splitting
                    let mut fields = Vec::new();
                    for p in &positional {
                        fields.extend(self.ifs_split(p));
                    }
                    return Ok(fields);
                }
            }
            if let WordPart::ArrayAccess { name, index } = &word.parts[0]
                && (index == "@" || index == "*")
            {
                // Check assoc arrays first
                if let Some(arr) = self.assoc_arrays.get(name) {
                    let mut keys: Vec<_> = arr.keys().cloned().collect();
                    keys.sort();
                    let values: Vec<String> =
                        keys.iter().filter_map(|k| arr.get(k).cloned()).collect();
                    if word.quoted && index == "*" {
                        let sep = self.get_ifs_separator();
                        return Ok(vec![values.join(&sep)]);
                    }
                    return Ok(values);
                }
                if let Some(arr) = self.arrays.get(name) {
                    let mut indices: Vec<_> = arr.keys().collect();
                    indices.sort();
                    let values: Vec<String> =
                        indices.iter().filter_map(|i| arr.get(i).cloned()).collect();
                    // "${arr[*]}" joins into single field with IFS; "${arr[@]}" keeps separate
                    if word.quoted && index == "*" {
                        let sep = self.get_ifs_separator();
                        return Ok(vec![values.join(&sep)]);
                    }
                    return Ok(values);
                }
                return Ok(Vec::new());
            }
            // "${!arr[@]}" - array keys/indices as separate fields
            if let WordPart::ArrayIndices(name) = &word.parts[0] {
                let resolved = self.resolve_nameref(name);
                if let Some(arr) = self.assoc_arrays.get(resolved) {
                    let mut keys: Vec<_> = arr.keys().cloned().collect();
                    keys.sort();
                    return Ok(keys);
                }
                if let Some(arr) = self.arrays.get(resolved) {
                    let mut indices: Vec<_> = arr.keys().collect();
                    indices.sort();
                    return Ok(indices.iter().map(|i| i.to_string()).collect());
                }
                return Ok(Vec::new());
            }
        }

        // For other words, expand to a single field then apply IFS word splitting
        // when the word is unquoted and contains an expansion.
        // Per POSIX, unquoted variable/command/arithmetic expansion results undergo
        // field splitting on IFS.
        let expanded = self.expand_word(word).await?;

        // IFS splitting applies to unquoted expansions only.
        // Skip splitting for assignment-like words (e.g., result="$1") where
        // the lexer stripped quotes from a mixed-quoted word (produces Token::Word
        // with quoted: false even though the expansion was inside double quotes).
        let is_assignment_word =
            matches!(word.parts.first(), Some(WordPart::Literal(s)) if s.contains('='));
        let has_expansion = !word.quoted
            && !is_assignment_word
            && word.parts.iter().any(|p| {
                matches!(
                    p,
                    WordPart::Variable(_)
                        | WordPart::CommandSubstitution(_)
                        | WordPart::ArithmeticExpansion(_)
                        | WordPart::ParameterExpansion { .. }
                        | WordPart::ArrayAccess { .. }
                )
            });

        if has_expansion {
            Ok(self.ifs_split(&expanded))
        } else {
            Ok(vec![expanded])
        }
    }

    /// Resolve name for parameter expansion, handling array subscripts and special params.
    /// Returns (is_set, expanded_value).
    fn resolve_param_expansion_name(&self, name: &str) -> (bool, String) {
        // Check for array subscript pattern: name[@] or name[*]
        let is_star = name.ends_with("[*]");
        if let Some(arr_name) = name
            .strip_suffix("[@]")
            .or_else(|| name.strip_suffix("[*]"))
        {
            // Resolve nameref: if arr_name is a nameref, follow it to the target
            let resolved_arr_name = self.resolve_nameref(arr_name);
            let sep = if is_star {
                self.get_ifs_separator()
            } else {
                " ".to_string()
            };
            if let Some(arr) = self.assoc_arrays.get(resolved_arr_name) {
                let is_set = !arr.is_empty();
                let mut keys: Vec<_> = arr.keys().collect();
                keys.sort();
                let values: Vec<String> =
                    keys.iter().filter_map(|k| arr.get(*k).cloned()).collect();
                return (is_set, values.join(&sep));
            }
            if let Some(arr) = self.arrays.get(resolved_arr_name) {
                let is_set = !arr.is_empty();
                let mut indices: Vec<_> = arr.keys().collect();
                indices.sort();
                let values: Vec<_> = indices.iter().filter_map(|i| arr.get(i)).collect();
                return (
                    is_set,
                    values.into_iter().cloned().collect::<Vec<_>>().join(&sep),
                );
            }
            return (false, String::new());
        }

        // Check for array element subscript: name[key]
        if let Some(bracket) = name.find('[')
            && name.ends_with(']')
        {
            let arr_name = &name[..bracket];
            // Resolve nameref: if arr_name is a nameref, follow it to the target
            let resolved_arr_name = self.resolve_nameref(arr_name);
            let key = &name[bracket + 1..name.len() - 1];
            if let Some(arr) = self.assoc_arrays.get(resolved_arr_name) {
                let expanded_key = self.expand_variable_or_literal(key);
                return match arr.get(&expanded_key) {
                    Some(v) => (true, v.clone()),
                    None => (false, String::new()),
                };
            }
            if let Some(arr) = self.arrays.get(resolved_arr_name)
                && let Ok(idx) = key.parse::<usize>()
            {
                return match arr.get(&idx) {
                    Some(v) => (true, v.clone()),
                    None => (false, String::new()),
                };
            }
            return (false, String::new());
        }

        // Special parameters @ and *
        if name == "@" || name == "*" {
            if let Some(frame) = self.call_stack.last() {
                let is_set = !frame.positional.is_empty();
                let sep = if name == "*" {
                    self.get_ifs_separator()
                } else {
                    " ".to_string()
                };
                return (is_set, frame.positional.join(&sep));
            }
            return (false, String::new());
        }

        // Regular variable
        let is_set = self.is_variable_set(name);
        let value = self.expand_variable(name);
        (is_set, value)
    }

    /// Split a string on IFS characters according to POSIX rules.
    ///
    /// - IFS whitespace (space, tab, newline) collapses; leading/trailing stripped.
    /// - IFS non-whitespace chars are significant delimiters. Two adjacent produce
    ///   an empty field between them.
    /// - `<ws><nws><ws>` = single delimiter (ws absorbed into the nws delimiter).
    /// - Empty IFS → no splitting. Unset IFS → default " \t\n".
    fn ifs_split(&self, s: &str) -> Vec<String> {
        let ifs = self
            .variables
            .get("IFS")
            .cloned()
            .unwrap_or_else(|| " \t\n".to_string());

        if ifs.is_empty() {
            return vec![s.to_string()];
        }

        let is_ifs = |c: char| ifs.contains(c);
        let is_ifs_ws = |c: char| ifs.contains(c) && " \t\n".contains(c);
        let is_ifs_nws = |c: char| ifs.contains(c) && !" \t\n".contains(c);
        let all_whitespace_ifs = ifs.chars().all(|c| " \t\n".contains(c));

        if all_whitespace_ifs {
            // IFS is only whitespace: split on runs, elide empties
            return s
                .split(|c: char| is_ifs(c))
                .filter(|f| !f.is_empty())
                .map(|f| f.to_string())
                .collect();
        }

        // Mixed or pure non-whitespace IFS.
        let mut fields: Vec<String> = Vec::new();
        let mut current = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;

        // Skip leading IFS whitespace
        while i < chars.len() && is_ifs_ws(chars[i]) {
            i += 1;
        }
        // Leading non-whitespace IFS produces an empty first field
        if i < chars.len() && is_ifs_nws(chars[i]) {
            fields.push(String::new());
            i += 1;
            while i < chars.len() && is_ifs_ws(chars[i]) {
                i += 1;
            }
        }

        while i < chars.len() {
            let c = chars[i];
            if is_ifs_nws(c) {
                // Non-whitespace IFS delimiter: finalize current field
                fields.push(std::mem::take(&mut current));
                i += 1;
                // Consume trailing IFS whitespace
                while i < chars.len() && is_ifs_ws(chars[i]) {
                    i += 1;
                }
            } else if is_ifs_ws(c) {
                // IFS whitespace: skip it, then check for non-ws delimiter
                while i < chars.len() && is_ifs_ws(chars[i]) {
                    i += 1;
                }
                if i < chars.len() && is_ifs_nws(chars[i]) {
                    // <ws><nws> = single delimiter. Push current field.
                    fields.push(std::mem::take(&mut current));
                    i += 1; // consume the nws char
                    while i < chars.len() && is_ifs_ws(chars[i]) {
                        i += 1;
                    }
                } else if i < chars.len() {
                    // ws alone as delimiter (no nws follows)
                    fields.push(std::mem::take(&mut current));
                }
                // trailing ws at end → ignore (don't push empty field)
            } else {
                current.push(c);
                i += 1;
            }
        }

        if !current.is_empty() {
            fields.push(current);
        }

        fields
    }

    /// Expand an operand string from a parameter expansion (sync, lazy).
    /// Only called when the operand is actually needed, providing lazy evaluation.
    fn expand_operand(&mut self, operand: &str) -> String {
        if operand.is_empty() {
            return String::new();
        }
        // THREAT[TM-DOS-050]: Propagate caller-configured limits to word parsing
        let word = Parser::parse_word_string_with_limits(
            operand,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let mut result = String::new();
        for part in &word.parts {
            match part {
                WordPart::Literal(s) => result.push_str(s),
                WordPart::Variable(name) => {
                    result.push_str(&self.expand_variable(name));
                }
                WordPart::ArithmeticExpansion(expr) => {
                    let val = self.evaluate_arithmetic_with_assign(expr);
                    result.push_str(&val.to_string());
                }
                WordPart::ParameterExpansion {
                    name,
                    operator,
                    operand: inner_operand,
                    colon_variant,
                } => {
                    let (is_set, value) = self.resolve_param_expansion_name(name);
                    let expanded = self.apply_parameter_op(
                        &value,
                        name,
                        operator,
                        inner_operand,
                        *colon_variant,
                        is_set,
                    );
                    result.push_str(&expanded);
                }
                WordPart::Length(name) => {
                    let value = self.expand_variable(name);
                    result.push_str(&value.len().to_string());
                }
                // TODO: handle CommandSubstitution etc. in sync operand expansion
                _ => {}
            }
        }
        result
    }

    /// Apply parameter expansion operator.
    /// `colon_variant`: true = check unset-or-empty, false = check unset-only.
    /// `is_set`: whether the variable is defined (distinct from being empty).
    fn apply_parameter_op(
        &mut self,
        value: &str,
        name: &str,
        operator: &ParameterOp,
        operand: &str,
        colon_variant: bool,
        is_set: bool,
    ) -> String {
        // colon (:-) => trigger when unset OR empty
        // no-colon (-) => trigger only when unset
        let use_default = if colon_variant {
            !is_set || value.is_empty()
        } else {
            !is_set
        };
        let use_replacement = if colon_variant {
            is_set && !value.is_empty()
        } else {
            is_set
        };

        match operator {
            ParameterOp::UseDefault => {
                if use_default {
                    self.expand_operand(operand)
                } else {
                    value.to_string()
                }
            }
            ParameterOp::AssignDefault => {
                if use_default {
                    let expanded = self.expand_operand(operand);
                    self.set_variable(name.to_string(), expanded.clone());
                    expanded
                } else {
                    value.to_string()
                }
            }
            ParameterOp::UseReplacement => {
                if use_replacement {
                    self.expand_operand(operand)
                } else {
                    String::new()
                }
            }
            ParameterOp::Error => {
                if use_default {
                    let expanded = self.expand_operand(operand);
                    let msg = if expanded.is_empty() {
                        format!("bash: {}: parameter null or not set\n", name)
                    } else {
                        format!("bash: {}: {}\n", name, expanded)
                    };
                    self.nounset_error = Some(msg);
                    String::new()
                } else {
                    value.to_string()
                }
            }
            ParameterOp::RemovePrefixShort => {
                // ${var#pattern} - remove shortest prefix match
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, true, false)
            }
            ParameterOp::RemovePrefixLong => {
                // ${var##pattern} - remove longest prefix match
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, true, true)
            }
            ParameterOp::RemoveSuffixShort => {
                // ${var%pattern} - remove shortest suffix match
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, false, false)
            }
            ParameterOp::RemoveSuffixLong => {
                // ${var%%pattern} - remove longest suffix match
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, false, true)
            }
            ParameterOp::ReplaceFirst {
                pattern,
                replacement,
            } => {
                // ${var/pattern/replacement} - replace first occurrence
                self.replace_pattern(value, pattern, replacement, false)
            }
            ParameterOp::ReplaceAll {
                pattern,
                replacement,
            } => {
                // ${var//pattern/replacement} - replace all occurrences
                self.replace_pattern(value, pattern, replacement, true)
            }
            ParameterOp::UpperFirst => {
                // ${var^} - uppercase first character
                let mut chars = value.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            ParameterOp::UpperAll => {
                // ${var^^} - uppercase all characters
                value.to_uppercase()
            }
            ParameterOp::LowerFirst => {
                // ${var,} - lowercase first character
                let mut chars = value.chars();
                match chars.next() {
                    Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            ParameterOp::LowerAll => {
                // ${var,,} - lowercase all characters
                value.to_lowercase()
            }
        }
    }

    /// Replace pattern in value
    fn replace_pattern(
        &self,
        value: &str,
        pattern: &str,
        replacement: &str,
        global: bool,
    ) -> String {
        if pattern.is_empty() {
            return value.to_string();
        }

        // Handle # prefix anchor (match at start only)
        if let Some(rest) = pattern.strip_prefix('#') {
            if rest.is_empty() {
                return value.to_string();
            }
            if let Some(stripped) = value.strip_prefix(rest) {
                return format!("{}{}", replacement, stripped);
            }
            // Try glob match at prefix
            if rest.contains('*') {
                let matched = self.remove_pattern(value, rest, true, false);
                if matched != value {
                    let prefix_len = value.len() - matched.len();
                    return format!("{}{}", replacement, &value[prefix_len..]);
                }
            }
            return value.to_string();
        }

        // Handle % suffix anchor (match at end only)
        if let Some(rest) = pattern.strip_prefix('%') {
            if rest.is_empty() {
                return value.to_string();
            }
            if let Some(stripped) = value.strip_suffix(rest) {
                return format!("{}{}", stripped, replacement);
            }
            // Try glob match at suffix
            if rest.contains('*') {
                let matched = self.remove_pattern(value, rest, false, false);
                if matched != value {
                    return format!("{}{}", matched, replacement);
                }
            }
            return value.to_string();
        }

        // Handle glob pattern with *
        if pattern.contains('*') {
            // Convert glob to regex-like behavior
            // For simplicity, we'll handle basic cases: prefix*, *suffix, *middle*
            if pattern == "*" {
                // Replace everything
                return replacement.to_string();
            }

            if let Some(star_pos) = pattern.find('*') {
                let prefix = &pattern[..star_pos];
                let suffix = &pattern[star_pos + 1..];

                if prefix.is_empty() && !suffix.is_empty() {
                    // *suffix - match anything ending with suffix
                    if let Some(pos) = value.find(suffix) {
                        let after = &value[pos + suffix.len()..];
                        if global {
                            return replacement.to_string()
                                + &self.replace_pattern(after, pattern, replacement, true);
                        } else {
                            return replacement.to_string() + after;
                        }
                    }
                } else if !prefix.is_empty() && suffix.is_empty() {
                    // prefix* - match prefix and anything after
                    if value.starts_with(prefix) {
                        return replacement.to_string();
                    }
                }
            }
            // If we can't match the glob pattern, return as-is
            return value.to_string();
        }

        // Simple string replacement
        if global {
            value.replace(pattern, replacement)
        } else {
            value.replacen(pattern, replacement, 1)
        }
    }

    /// Remove prefix/suffix pattern from value
    fn remove_pattern(&self, value: &str, pattern: &str, prefix: bool, longest: bool) -> String {
        // Simple pattern matching with * glob
        if pattern.is_empty() {
            return value.to_string();
        }

        // Use glob_match for patterns with bracket expressions or extglob
        if pattern.contains('[') || self.contains_extglob(pattern) {
            return self.remove_pattern_glob(value, pattern, prefix, longest);
        }

        if prefix {
            // Remove from beginning
            if pattern == "*" {
                if longest {
                    return String::new();
                } else if !value.is_empty() {
                    return value.chars().skip(1).collect();
                } else {
                    return value.to_string();
                }
            }

            // Check if pattern contains *
            if let Some(star_pos) = pattern.find('*') {
                let prefix_part = &pattern[..star_pos];
                let suffix_part = &pattern[star_pos + 1..];

                if prefix_part.is_empty() {
                    // Pattern is "*suffix" - find suffix and remove everything before it
                    if longest {
                        // Find last occurrence of suffix
                        if let Some(pos) = value.rfind(suffix_part) {
                            return value[pos + suffix_part.len()..].to_string();
                        }
                    } else {
                        // Find first occurrence of suffix
                        if let Some(pos) = value.find(suffix_part) {
                            return value[pos + suffix_part.len()..].to_string();
                        }
                    }
                } else if suffix_part.is_empty() {
                    // Pattern is "prefix*" - match prefix and any chars after
                    if let Some(rest) = value.strip_prefix(prefix_part) {
                        if longest {
                            return String::new();
                        } else {
                            return rest.to_string();
                        }
                    }
                } else {
                    // Pattern is "prefix*suffix" - more complex matching
                    if let Some(rest) = value.strip_prefix(prefix_part) {
                        if longest {
                            if let Some(pos) = rest.rfind(suffix_part) {
                                return rest[pos + suffix_part.len()..].to_string();
                            }
                        } else if let Some(pos) = rest.find(suffix_part) {
                            return rest[pos + suffix_part.len()..].to_string();
                        }
                    }
                }
            } else if let Some(rest) = value.strip_prefix(pattern) {
                return rest.to_string();
            }
        } else {
            // Remove from end (suffix)
            if pattern == "*" {
                if longest {
                    return String::new();
                } else if !value.is_empty() {
                    let mut s = value.to_string();
                    s.pop();
                    return s;
                } else {
                    return value.to_string();
                }
            }

            // Check if pattern contains *
            if let Some(star_pos) = pattern.find('*') {
                let prefix_part = &pattern[..star_pos];
                let suffix_part = &pattern[star_pos + 1..];

                if suffix_part.is_empty() {
                    // Pattern is "prefix*" - find prefix and remove from there to end
                    if longest {
                        // Find first occurrence of prefix
                        if let Some(pos) = value.find(prefix_part) {
                            return value[..pos].to_string();
                        }
                    } else {
                        // Find last occurrence of prefix
                        if let Some(pos) = value.rfind(prefix_part) {
                            return value[..pos].to_string();
                        }
                    }
                } else if prefix_part.is_empty() {
                    // Pattern is "*suffix" - match any chars before suffix
                    if let Some(before) = value.strip_suffix(suffix_part) {
                        if longest {
                            return String::new();
                        } else {
                            return before.to_string();
                        }
                    }
                } else {
                    // Pattern is "prefix*suffix" - more complex matching
                    if let Some(before_suffix) = value.strip_suffix(suffix_part) {
                        if longest {
                            if let Some(pos) = before_suffix.find(prefix_part) {
                                return value[..pos].to_string();
                            }
                        } else if let Some(pos) = before_suffix.rfind(prefix_part) {
                            return value[..pos].to_string();
                        }
                    }
                }
            } else if let Some(before) = value.strip_suffix(pattern) {
                return before.to_string();
            }
        }

        value.to_string()
    }

    /// Remove prefix/suffix using glob_match for patterns with brackets or extglob.
    fn remove_pattern_glob(
        &self,
        value: &str,
        pattern: &str,
        prefix: bool,
        longest: bool,
    ) -> String {
        let chars: Vec<char> = value.chars().collect();
        if prefix {
            // Try each prefix length; shortest = first match, longest = last match
            let mut last_match = None;
            for i in 0..=chars.len() {
                let candidate: String = chars[..i].iter().collect();
                if self.glob_match(&candidate, pattern) {
                    if !longest {
                        return chars[i..].iter().collect();
                    }
                    last_match = Some(i);
                }
            }
            if let Some(i) = last_match {
                return chars[i..].iter().collect();
            }
        } else {
            // Suffix removal: try each suffix length
            let mut last_match = None;
            for i in (0..=chars.len()).rev() {
                let candidate: String = chars[i..].iter().collect();
                if self.glob_match(&candidate, pattern) {
                    if !longest {
                        return chars[..i].iter().collect();
                    }
                    last_match = Some(i);
                }
            }
            if let Some(i) = last_match {
                return chars[..i].iter().collect();
            }
        }
        value.to_string()
    }

    /// Maximum recursion depth for arithmetic expression evaluation.
    /// THREAT[TM-DOS-026]: Prevents stack overflow via deeply nested arithmetic like
    /// $(((((((...)))))))
    const MAX_ARITHMETIC_DEPTH: usize = 50;

    /// Evaluate arithmetic with assignment support (e.g. `X = X + 1`).
    /// Assignment must be handled before variable expansion so the LHS
    /// variable name is preserved.
    fn evaluate_arithmetic_with_assign(&mut self, expr: &str) -> i64 {
        let expr = expr.trim();

        // Handle comma operator (lowest precedence): evaluate all, return last
        // But not inside parentheses
        {
            let mut depth = 0i32;
            let chars: Vec<char> = expr.chars().collect();
            let byte_offsets: Vec<usize> = expr.char_indices().map(|(b, _)| b).collect();
            for i in (0..chars.len()).rev() {
                match chars[i] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    ',' if depth == 0 => {
                        let left = &expr[..byte_offsets[i]];
                        let right = &expr[byte_offsets[i] + 1..];
                        self.evaluate_arithmetic_with_assign(left);
                        return self.evaluate_arithmetic_with_assign(right);
                    }
                    _ => {}
                }
            }
        }

        // Handle pre-increment/pre-decrement: ++var, --var
        if let Some(var_name) = expr.strip_prefix("++") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0) + 1;
                self.set_variable(var_name.to_string(), val.to_string());
                return val;
            }
        }
        if let Some(var_name) = expr.strip_prefix("--") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0) - 1;
                self.set_variable(var_name.to_string(), val.to_string());
                return val;
            }
        }

        // Handle post-increment/post-decrement: var++, var--
        if let Some(var_name) = expr.strip_suffix("++") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let old_val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0);
                self.set_variable(var_name.to_string(), (old_val + 1).to_string());
                return old_val;
            }
        }
        if let Some(var_name) = expr.strip_suffix("--") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let old_val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0);
                self.set_variable(var_name.to_string(), (old_val - 1).to_string());
                return old_val;
            }
        }

        // Check for compound assignments: +=, -=, *=, /=, %=, &=, |=, ^=, <<=, >>=
        // and simple assignment: VAR = expr (but not == comparison)
        if let Some(eq_pos) = expr.find('=') {
            let before = &expr[..eq_pos];
            let after_char = expr.as_bytes().get(eq_pos + 1);
            // Not == or !=
            if !before.ends_with('!') && after_char != Some(&b'=') {
                // Detect compound operator: check multi-char ops first
                let (var_name, op) = if let Some(s) = before.strip_suffix("<<") {
                    (s.trim(), "<<")
                } else if let Some(s) = before.strip_suffix(">>") {
                    (s.trim(), ">>")
                } else if let Some(s) = before.strip_suffix('+') {
                    (s.trim(), "+")
                } else if let Some(s) = before.strip_suffix('-') {
                    (s.trim(), "-")
                } else if let Some(s) = before.strip_suffix('*') {
                    (s.trim(), "*")
                } else if let Some(s) = before.strip_suffix('/') {
                    (s.trim(), "/")
                } else if let Some(s) = before.strip_suffix('%') {
                    (s.trim(), "%")
                } else if let Some(s) = before.strip_suffix('&') {
                    (s.trim(), "&")
                } else if let Some(s) = before.strip_suffix('|') {
                    (s.trim(), "|")
                } else if let Some(s) = before.strip_suffix('^') {
                    (s.trim(), "^")
                } else if !before.ends_with('<') && !before.ends_with('>') {
                    (before.trim(), "")
                } else {
                    ("", "")
                };

                if is_valid_var_name(var_name) {
                    let rhs = &expr[eq_pos + 1..];
                    let rhs_val = self.evaluate_arithmetic(rhs);
                    let value = if op.is_empty() {
                        rhs_val
                    } else {
                        let lhs_val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0);
                        // THREAT[TM-DOS-043]: wrapping to prevent overflow panic
                        match op {
                            "+" => lhs_val.wrapping_add(rhs_val),
                            "-" => lhs_val.wrapping_sub(rhs_val),
                            "*" => lhs_val.wrapping_mul(rhs_val),
                            "/" => {
                                if rhs_val != 0 && !(lhs_val == i64::MIN && rhs_val == -1) {
                                    lhs_val / rhs_val
                                } else {
                                    0
                                }
                            }
                            "%" => {
                                if rhs_val != 0 && !(lhs_val == i64::MIN && rhs_val == -1) {
                                    lhs_val % rhs_val
                                } else {
                                    0
                                }
                            }
                            "&" => lhs_val & rhs_val,
                            "|" => lhs_val | rhs_val,
                            "^" => lhs_val ^ rhs_val,
                            "<<" => lhs_val.wrapping_shl((rhs_val & 63) as u32),
                            ">>" => lhs_val.wrapping_shr((rhs_val & 63) as u32),
                            _ => rhs_val,
                        }
                    };
                    self.set_variable(var_name.to_string(), value.to_string());
                    return value;
                }
            }
        }

        self.evaluate_arithmetic(expr)
    }

    /// Evaluate a simple arithmetic expression
    fn evaluate_arithmetic(&self, expr: &str) -> i64 {
        // Simple arithmetic evaluation - handles basic operations
        let expr = expr.trim();

        // First expand any variables in the expression
        let expanded = self.expand_arithmetic_vars(expr);

        // Parse and evaluate with depth tracking (TM-DOS-026)
        self.parse_arithmetic_impl(&expanded, 0)
    }

    /// Recursively resolve a variable value in arithmetic context.
    /// In bash arithmetic, bare variable names are recursively evaluated:
    /// if b=a and a=3, then $((b)) evaluates b -> "a" -> 3.
    /// If x='1 + 2', then $((x)) evaluates x -> "1 + 2" -> 3 (as sub-expression).
    /// THREAT[TM-DOS-026]: `depth` prevents infinite recursion.
    fn resolve_arith_var(&self, value: &str, depth: usize) -> String {
        if depth >= Self::MAX_ARITHMETIC_DEPTH {
            return "0".to_string();
        }
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return "0".to_string();
        }
        // If value is a simple integer, return it directly
        if trimmed.parse::<i64>().is_ok() {
            return trimmed.to_string();
        }
        // If value looks like a variable name, recursively dereference
        if is_valid_var_name(trimmed) {
            let inner = self.expand_variable(trimmed);
            return self.resolve_arith_var(&inner, depth + 1);
        }
        // Value contains an expression (e.g. "1 + 2") — expand vars in it
        // and wrap in parens to preserve grouping
        let expanded = self.expand_arithmetic_vars_depth(trimmed, depth + 1);
        format!("({})", expanded)
    }

    /// Expand variables in arithmetic expression (no $ needed in $((...)))
    fn expand_arithmetic_vars(&self, expr: &str) -> String {
        self.expand_arithmetic_vars_depth(expr, 0)
    }

    /// Inner implementation with depth tracking for recursive expansion.
    /// THREAT[TM-DOS-026]: `depth` prevents stack overflow via recursive variable values.
    fn expand_arithmetic_vars_depth(&self, expr: &str, depth: usize) -> String {
        if depth >= Self::MAX_ARITHMETIC_DEPTH {
            return "0".to_string();
        }

        // Strip double quotes — "$x" in arithmetic is the same as $x
        let expr = expr.replace('"', "");

        let mut result = String::new();
        let mut chars = expr.chars().peekable();
        // Track whether we're in a numeric literal context (after # or 0x)
        let mut in_numeric_literal = false;

        while let Some(ch) = chars.next() {
            if ch == '$' {
                in_numeric_literal = false;
                if chars.peek() == Some(&'{') {
                    // Handle ${...} syntax inside arithmetic
                    chars.next(); // consume '{'
                    let mut brace_content = String::new();
                    let mut brace_depth = 1i32;
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '{' {
                            brace_depth += 1;
                            brace_content.push(c);
                        } else if c == '}' {
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                break;
                            }
                            brace_content.push(c);
                        } else {
                            brace_content.push(c);
                        }
                    }
                    let expanded = self.expand_brace_expr_in_arithmetic(&brace_content);
                    if expanded.is_empty() {
                        result.push('0');
                    } else {
                        result.push_str(&expanded);
                    }
                } else if let Some(&c) = chars.peek()
                    && matches!(c, '#' | '?' | '$' | '!' | '@' | '*' | '-')
                {
                    // Handle special variables: $#, $?, $$, $!, $@, $*, $-
                    chars.next();
                    let value = self.expand_variable(&c.to_string());
                    if value.is_empty() {
                        result.push('0');
                    } else {
                        result.push_str(&value);
                    }
                } else {
                    // Handle $var syntax (common in arithmetic)
                    let mut name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_alphanumeric() || c == '_' {
                            name.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    if !name.is_empty() {
                        // $var is direct text substitution — no recursive arithmetic eval.
                        // Only bare names (without $) get recursive resolution.
                        let value = self.expand_variable(&name);
                        if value.is_empty() {
                            result.push('0');
                        } else {
                            result.push_str(&value);
                        }
                    } else {
                        result.push(ch);
                    }
                }
            } else if ch == '#' {
                // base#value syntax: digits before # are base, chars after are literal digits
                result.push(ch);
                in_numeric_literal = true;
            } else if in_numeric_literal && (ch.is_ascii_alphanumeric() || ch == '_') {
                // Part of a base#value literal — don't expand as variable
                result.push(ch);
            } else if ch.is_ascii_digit() {
                result.push(ch);
                // Check for 0x/0X hex prefix
                if ch == '0'
                    && let Some(&next) = chars.peek()
                    && (next == 'x' || next == 'X')
                {
                    result.push(chars.next().unwrap());
                    in_numeric_literal = true;
                }
            } else if ch.is_ascii_alphabetic() || ch == '_' {
                in_numeric_literal = false;
                // Could be a variable name
                let mut name = String::new();
                name.push(ch);
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }

                if chars.peek() == Some(&'[') {
                    // Check for array access: name[expr]
                    chars.next(); // consume '['
                    let mut index_expr = String::new();
                    let mut bracket_depth = 1;
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '[' {
                            bracket_depth += 1;
                            index_expr.push(c);
                        } else if c == ']' {
                            bracket_depth -= 1;
                            if bracket_depth == 0 {
                                break;
                            }
                            index_expr.push(c);
                        } else {
                            index_expr.push(c);
                        }
                    }
                    // Evaluate the index expression as arithmetic
                    let idx = self.evaluate_arithmetic(&index_expr);
                    // Look up array element
                    if let Some(arr) = self.arrays.get(&name) {
                        let idx_usize: usize = idx.try_into().unwrap_or(0);
                        let value = arr.get(&idx_usize).cloned().unwrap_or_default();
                        result.push_str(&self.resolve_arith_var(&value, depth));
                    } else {
                        // Not an array — treat as scalar (index 0 returns the var value)
                        let value = self.expand_variable(&name);
                        if idx == 0 {
                            result.push_str(&self.resolve_arith_var(&value, depth));
                        } else {
                            result.push('0');
                        }
                    }
                } else {
                    // Expand the variable with recursive arithmetic resolution
                    let value = self.expand_variable(&name);
                    result.push_str(&self.resolve_arith_var(&value, depth));
                }
            } else {
                in_numeric_literal = false;
                result.push(ch);
            }
        }

        result
    }

    /// Expand a `${...}` expression encountered inside arithmetic context.
    /// Handles: `${#arr[@]}`, `${#arr[*]}`, `${#var}`, `${arr[idx]}`, `${var}`.
    fn expand_brace_expr_in_arithmetic(&self, inner: &str) -> String {
        // ${#arr[@]} or ${#arr[*]} — array length
        if let Some(rest) = inner.strip_prefix('#') {
            if let Some(bracket) = rest.find('[') {
                // Guard against malformed input like ${#[} where bracket+1 > len-1
                let end = rest.len().saturating_sub(1);
                if bracket + 1 > end {
                    // Malformed — treat as string length of empty var
                    return "0".to_string();
                }
                let arr_name = &rest[..bracket];
                let idx = &rest[bracket + 1..end];
                if idx == "@" || idx == "*" {
                    if let Some(arr) = self.arrays.get(arr_name) {
                        return arr.len().to_string();
                    }
                    if let Some(arr) = self.assoc_arrays.get(arr_name) {
                        return arr.len().to_string();
                    }
                    return "0".to_string();
                }
                // ${#arr[n]} — length of element
                let idx_val = self.evaluate_arithmetic(idx);
                let idx_usize: usize = idx_val.try_into().unwrap_or(0);
                if let Some(arr) = self.arrays.get(arr_name) {
                    return arr
                        .get(&idx_usize)
                        .map(|v| v.len().to_string())
                        .unwrap_or_else(|| "0".to_string());
                }
                return "0".to_string();
            }
            // ${#var} — string length
            let val = self.expand_variable(rest);
            return val.len().to_string();
        }

        // ${arr[idx]} — array access
        if let Some(bracket) = inner.find('[')
            && inner.ends_with(']')
        {
            let arr_name = &inner[..bracket];
            let idx_str = &inner[bracket + 1..inner.len() - 1];
            if let Some(arr) = self.assoc_arrays.get(arr_name) {
                let key = self.expand_variable_or_literal(idx_str);
                return arr.get(&key).cloned().unwrap_or_default();
            }
            if let Some(arr) = self.arrays.get(arr_name) {
                let idx_val = self.evaluate_arithmetic(idx_str);
                let idx_usize: usize = idx_val.try_into().unwrap_or(0);
                return arr.get(&idx_usize).cloned().unwrap_or_default();
            }
            return String::new();
        }

        // ${var} — plain variable
        self.expand_variable(inner)
    }

    /// Parse and evaluate a simple arithmetic expression with depth tracking.
    /// THREAT[TM-DOS-026]: `arith_depth` prevents stack overflow from deeply nested expressions.
    /// Parse an arithmetic atom: unary operators, parenthesized expressions, and literals.
    fn parse_arith_atom(&self, expr: &str, arith_depth: usize) -> i64 {
        // Unary negation and bitwise NOT
        if let Some(rest) = expr.strip_prefix('-') {
            let rest = rest.trim();
            if !rest.is_empty() {
                // THREAT[TM-DOS-029]: wrapping to prevent i64::MIN negation panic
                return self
                    .parse_arithmetic_impl(rest, arith_depth + 1)
                    .wrapping_neg();
            }
        }
        if let Some(rest) = expr.strip_prefix('~') {
            let rest = rest.trim();
            if !rest.is_empty() {
                return !self.parse_arithmetic_impl(rest, arith_depth + 1);
            }
        }
        if let Some(rest) = expr.strip_prefix('!') {
            let rest = rest.trim();
            if !rest.is_empty() {
                let val = self.parse_arithmetic_impl(rest, arith_depth + 1);
                return if val == 0 { 1 } else { 0 };
            }
        }

        // Base conversion: base#value (e.g., 16#ff = 255, 2#1010 = 10)
        if let Some(hash_pos) = expr.find('#') {
            let base_str = &expr[..hash_pos];
            let value_str = &expr[hash_pos + 1..];
            if let Ok(base) = base_str.parse::<u32>() {
                if (2..=36).contains(&base) {
                    return i64::from_str_radix(value_str, base).unwrap_or(0);
                } else if (37..=64).contains(&base) {
                    return Self::parse_base_n(value_str, base);
                }
            }
        }

        // Hex (0x...), octal (0...) literals
        if expr.starts_with("0x") || expr.starts_with("0X") {
            return i64::from_str_radix(&expr[2..], 16).unwrap_or(0);
        }
        if expr.starts_with('0') && expr.len() > 1 && expr.chars().all(|c| c.is_ascii_digit()) {
            return i64::from_str_radix(&expr[1..], 8).unwrap_or(0);
        }

        // Parse as number or variable
        expr.trim().parse().unwrap_or(0)
    }

    /// Try to parse a binary operator at the current precedence level.
    /// Scans `chars`/`bo` for operators, splitting and recursing.
    /// Returns `Some(value)` if an operator was found, `None` to try next level.
    fn try_parse_arith_addmul(
        &self,
        expr: &str,
        chars: &[char],
        bo: &[usize],
        arith_depth: usize,
    ) -> Option<i64> {
        let mut depth: i32 = 0;

        // Addition/Subtraction
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '+' | '-' if depth == 0 && i > 0 => {
                    if chars[i] == '+' && i + 1 < chars.len() && chars[i + 1] == '+' {
                        continue;
                    }
                    if chars[i] == '+' && i > 0 && chars[i - 1] == '+' {
                        continue;
                    }
                    if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
                        continue;
                    }
                    if chars[i] == '-' && i > 0 && chars[i - 1] == '-' {
                        continue;
                    }
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if chars[i] == '+' {
                        left.wrapping_add(right)
                    } else {
                        left.wrapping_sub(right)
                    });
                }
                _ => {}
            }
        }

        // Multiplication/Division/Modulo
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0 => {
                    if i + 1 < chars.len() && chars[i + 1] == '*' {
                        continue;
                    }
                    if i > 0 && chars[i - 1] == '*' {
                        continue;
                    }
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left.wrapping_mul(right));
                }
                '/' | '%' if depth == 0 => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(match chars[i] {
                        '/' => {
                            if right != 0 {
                                left.wrapping_div(right)
                            } else {
                                0
                            }
                        }
                        '%' => {
                            if right != 0 {
                                left.wrapping_rem(right)
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    });
                }
                _ => {}
            }
        }

        // Exponentiation ** (right-associative)
        depth = 0;
        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0 && i + 1 < chars.len() && chars[i + 1] == '*' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 2..], arith_depth + 1);
                    let exp = right.clamp(0, 63) as u32;
                    return Some(left.wrapping_pow(exp));
                }
                _ => {}
            }
        }

        None
    }

    /// Try to parse comparison and logical/bitwise operators.
    fn try_parse_arith_comparison(
        &self,
        expr: &str,
        chars: &[char],
        bo: &[usize],
        arith_depth: usize,
    ) -> Option<i64> {
        let mut depth: i32 = 0;

        // Ternary operator (lowest precedence)
        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '?' if depth == 0 => {
                    let mut colon_depth = 0;
                    for j in (i + 1)..chars.len() {
                        match chars[j] {
                            '(' => colon_depth += 1,
                            ')' => colon_depth -= 1,
                            '?' => colon_depth += 1,
                            ':' if colon_depth == 0 => {
                                let cond =
                                    self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                                let then_val = self.parse_arithmetic_impl(
                                    &expr[bo[i] + 1..bo[j]],
                                    arith_depth + 1,
                                );
                                let else_val =
                                    self.parse_arithmetic_impl(&expr[bo[j] + 1..], arith_depth + 1);
                                return Some(if cond != 0 { then_val } else { else_val });
                            }
                            ':' => colon_depth -= 1,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Logical OR (||)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '|' if depth == 0 && i > 0 && chars[i - 1] == '|' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    if left != 0 {
                        return Some(1);
                    }
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if right != 0 { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Logical AND (&&)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '&' if depth == 0 && i > 0 && chars[i - 1] == '&' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    if left == 0 {
                        return Some(0);
                    }
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if right != 0 { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Bitwise OR (|) - but not ||
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '|' if depth == 0
                    && (i == 0 || chars[i - 1] != '|')
                    && (i + 1 >= chars.len() || chars[i + 1] != '|') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left | right);
                }
                _ => {}
            }
        }

        // Bitwise XOR (^)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '^' if depth == 0 => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left ^ right);
                }
                _ => {}
            }
        }

        // Bitwise AND (&) - but not &&
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '&' if depth == 0
                    && (i == 0 || chars[i - 1] != '&')
                    && (i + 1 >= chars.len() || chars[i + 1] != '&') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left & right);
                }
                _ => {}
            }
        }

        // Equality operators (==, !=)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '=' if depth == 0 && i > 0 && chars[i - 1] == '=' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left == right { 1 } else { 0 });
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '!' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left != right { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Relational operators (<, >, <=, >=)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '=' if depth == 0 && i > 0 && chars[i - 1] == '<' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left <= right { 1 } else { 0 });
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '>' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left >= right { 1 } else { 0 });
                }
                '<' if depth == 0
                    && (i + 1 >= chars.len() || (chars[i + 1] != '=' && chars[i + 1] != '<'))
                    && (i == 0 || chars[i - 1] != '<') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left < right { 1 } else { 0 });
                }
                '>' if depth == 0
                    && (i + 1 >= chars.len() || (chars[i + 1] != '=' && chars[i + 1] != '>'))
                    && (i == 0 || chars[i - 1] != '>') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left > right { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Bitwise shift (<< >>)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '<' if depth == 0
                    && i > 0
                    && chars[i - 1] == '<'
                    && (i < 2 || chars[i - 2] != '<')
                    && (i + 1 >= chars.len() || chars[i + 1] != '=') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    let shift = right.clamp(0, 63) as u32;
                    return Some(left.wrapping_shl(shift));
                }
                '>' if depth == 0
                    && i > 0
                    && chars[i - 1] == '>'
                    && (i < 2 || chars[i - 2] != '>')
                    && (i + 1 >= chars.len() || chars[i + 1] != '=') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    let shift = right.clamp(0, 63) as u32;
                    return Some(left.wrapping_shr(shift));
                }
                _ => {}
            }
        }

        None
    }

    fn parse_arithmetic_impl(&self, expr: &str, arith_depth: usize) -> i64 {
        let expr = expr.trim();

        if expr.is_empty() {
            return 0;
        }

        if !expr.is_ascii() {
            return 0;
        }

        // THREAT[TM-DOS-026]: Bail out if arithmetic nesting is too deep
        if arith_depth >= Self::MAX_ARITHMETIC_DEPTH {
            return 0;
        }

        // Handle parentheses
        if expr.starts_with('(') && expr.ends_with(')') {
            let mut depth = 0;
            let mut balanced = true;
            for (i, ch) in expr.chars().enumerate() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 && i < expr.len() - 1 {
                            balanced = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if balanced && depth == 0 {
                return self.parse_arithmetic_impl(&expr[1..expr.len() - 1], arith_depth + 1);
            }
        }

        let chars: Vec<char> = expr.chars().collect();
        let bo: Vec<usize> = expr.char_indices().map(|(b, _)| b).collect();

        // Try comparison/logical/bitwise operators (lowest precedence first)
        if let Some(val) = self.try_parse_arith_comparison(expr, &chars, &bo, arith_depth) {
            return val;
        }

        // Try additive/multiplicative/power operators
        if let Some(val) = self.try_parse_arith_addmul(expr, &chars, &bo, arith_depth) {
            return val;
        }

        // Atom: unary operators and literals
        self.parse_arith_atom(expr, arith_depth)
    }

    /// Parse a number in base 37-64 using bash's extended charset: 0-9, a-z, A-Z, @, _
    fn parse_base_n(value_str: &str, base: u32) -> i64 {
        let mut result: i64 = 0;
        for ch in value_str.chars() {
            let digit = match ch {
                '0'..='9' => ch as u32 - '0' as u32,
                'a'..='z' => 10 + ch as u32 - 'a' as u32,
                'A'..='Z' => 36 + ch as u32 - 'A' as u32,
                '@' => 62,
                '_' => 63,
                _ => return 0,
            };
            if digit >= base {
                return 0;
            }
            result = result.wrapping_mul(base as i64).wrapping_add(digit as i64);
        }
        result
    }

    /// Expand a string as a variable reference, or return as literal.
    /// Used for associative array keys which may be variable refs or literals.
    ///
    /// In real bash, associative array subscripts are treated as literal strings
    /// unless they contain explicit `$var` or `${var}` references. A bare name
    /// like `key` in `${assoc[key]}` is the string "key", NOT the value of
    /// variable `$key`. (Issue #861)
    fn expand_variable_or_literal(&self, s: &str) -> String {
        // Handle $var and ${var} references in assoc array keys
        let trimmed = s.trim();
        if let Some(var_name) = trimmed.strip_prefix('$') {
            let var_name = var_name.trim_start_matches('{').trim_end_matches('}');
            return self.expand_variable(var_name);
        }
        // Bare names are literal string keys — do NOT look up as variables.
        s.to_string()
    }

    /// Fully expand an associative array key, including command substitutions.
    /// Falls back to `expand_variable_or_literal` for keys without `$(` or backtick.
    async fn expand_assoc_key(&mut self, s: &str) -> Result<String> {
        if s.contains("$(") || s.contains('`') {
            let word = Parser::parse_word_string_with_limits(
                s,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            );
            self.expand_word(&word).await
        } else {
            Ok(self.expand_variable_or_literal(s))
        }
    }

    /// THREAT[TM-INJ-009]: Check if a variable name is an internal marker.
    fn is_internal_variable(name: &str) -> bool {
        is_internal_variable(name)
    }

    /// Set a variable, respecting dynamic scoping.
    /// If the variable is declared `local` in any active call frame, update that frame.
    /// Otherwise, set in global variables.
    /// THREAT[TM-DOS-060]: Checks memory budget before inserting.
    fn set_variable(&mut self, name: String, value: String) {
        // THREAT[TM-INJ-009]: Block user assignment to internal marker variables
        if Self::is_internal_variable(&name) {
            return;
        }
        // Resolve nameref: if `name` is a nameref, assign to the target instead
        let resolved = self.resolve_nameref(&name).to_string();
        // THREAT[TM-INJ-019/020/021]: Block assignment to readonly variables
        if self
            .variables
            .contains_key(&format!("_READONLY_{}", resolved))
        {
            return;
        }
        // Apply integer attribute (declare -i): evaluate as arithmetic
        let value = if self
            .variables
            .get(&format!("_INTEGER_{}", resolved))
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            self.evaluate_arithmetic_with_assign(&value).to_string()
        } else {
            value
        };
        // Apply case conversion attributes (declare -l / declare -u)
        let value = if self
            .variables
            .get(&format!("_LOWER_{}", resolved))
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            value.to_lowercase()
        } else if self
            .variables
            .get(&format!("_UPPER_{}", resolved))
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            value.to_uppercase()
        } else {
            value
        };
        // Check allexport (set -a): auto-export to env
        let allexport = self
            .variables
            .get("SHOPT_a")
            .map(|v| v == "1")
            .unwrap_or(false);

        for frame in self.call_stack.iter_mut().rev() {
            if let std::collections::hash_map::Entry::Occupied(mut e) =
                frame.locals.entry(resolved.clone())
            {
                // Local variable update — track byte delta but no count change
                let old_val_len = e.get().len();
                self.memory_budget.variable_bytes = self
                    .memory_budget
                    .variable_bytes
                    .saturating_add(value.len())
                    .saturating_sub(old_val_len);
                if allexport {
                    self.env.insert(resolved, value.clone());
                }
                e.insert(value);
                return;
            }
        }
        if allexport {
            self.env.insert(resolved.clone(), value.clone());
        }
        self.insert_variable_checked(resolved, value);
    }

    /// Insert a variable into the global variables map with memory budget checking.
    /// Silently drops the insert if the budget would be exceeded.
    /// Internal marker variables (_READONLY_, _NAMEREF_, etc.) bypass budget checks.
    fn insert_variable_checked(&mut self, key: String, value: String) {
        let is_internal = Self::is_internal_variable(&key);
        if !is_internal {
            let is_new = !self.variables.contains_key(&key);
            let (old_key_len, old_value_len) = if is_new {
                (0, 0)
            } else {
                (key.len(), self.variables.get(&key).map_or(0, |v| v.len()))
            };
            if self
                .memory_budget
                .check_variable_insert(
                    key.len(),
                    value.len(),
                    is_new,
                    old_key_len,
                    old_value_len,
                    &self.memory_limits,
                )
                .is_err()
            {
                return; // silently reject — budget exceeded
            }
            self.memory_budget.record_variable_insert(
                key.len(),
                value.len(),
                is_new,
                old_key_len,
                old_value_len,
            );
        }
        self.variables.insert(key, value);
    }

    /// Insert an array with memory budget checking.
    /// Returns true if the insert succeeded.
    fn insert_array_checked(&mut self, name: String, arr: HashMap<usize, String>) -> bool {
        let new_entries = arr.len();
        let old_entries = self.arrays.get(&name).map_or(0, |a| a.len());
        let net = new_entries.saturating_sub(old_entries);
        if net > 0
            && self
                .memory_budget
                .check_array_entries(net, &self.memory_limits)
                .is_err()
        {
            return false;
        }
        self.memory_budget.array_entries =
            self.memory_budget.array_entries.saturating_sub(old_entries) + new_entries;
        self.arrays.insert(name, arr);
        true
    }

    /// Insert an associative array with memory budget checking.
    /// Returns true if the insert succeeded.
    #[allow(dead_code)]
    fn insert_assoc_array_checked(&mut self, name: String, arr: HashMap<String, String>) -> bool {
        let new_entries = arr.len();
        let old_entries = self.assoc_arrays.get(&name).map_or(0, |a| a.len());
        let net = new_entries.saturating_sub(old_entries);
        if net > 0
            && self
                .memory_budget
                .check_array_entries(net, &self.memory_limits)
                .is_err()
        {
            return false;
        }
        self.memory_budget.array_entries =
            self.memory_budget.array_entries.saturating_sub(old_entries) + new_entries;
        self.assoc_arrays.insert(name, arr);
        true
    }

    /// Resolve nameref chains: if `name` has a `_NAMEREF_<name>` marker,
    /// follow the chain (up to 10 levels to prevent infinite loops).
    fn resolve_nameref<'a>(&'a self, name: &'a str) -> &'a str {
        let mut current = name;
        let mut visited = std::collections::HashSet::new();
        visited.insert(name);
        for _ in 0..10 {
            let key = format!("_NAMEREF_{}", current);
            if let Some(target) = self.variables.get(&key) {
                // THREAT[TM-INJ-011]: Detect cyclic namerefs and stop.
                if !visited.insert(target.as_str()) {
                    // Cycle detected — return original name (Bash emits a warning)
                    return name;
                }
                current = target.as_str();
            } else {
                break;
            }
        }
        current
    }

    /// Expand command substitutions `$(...)` within an arithmetic expression string.
    /// Parses the expr, executes any embedded command subs, and replaces them with output.
    async fn expand_command_subs_in_arithmetic(&mut self, expr: &str) -> Result<String> {
        let mut result = String::new();
        let mut chars = expr.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '$' && chars.peek() == Some(&'(') {
                // Check it's not $(( ... )) (arithmetic)
                let remaining: String = chars.clone().collect();
                if remaining.starts_with("((") {
                    // $(( ... )) — keep as-is for arithmetic eval
                    result.push('$');
                    continue;
                }
                // $( ... ) — command substitution, find matching close paren
                chars.next(); // consume '('
                let mut depth = 1i32;
                let mut cmd = String::new();
                for c in chars.by_ref() {
                    if c == '(' {
                        depth += 1;
                        cmd.push(c);
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        cmd.push(c);
                    } else {
                        cmd.push(c);
                    }
                }
                // Execute the command and substitute
                let parser = Parser::with_limits(
                    &cmd,
                    self.limits.max_ast_depth,
                    self.limits.max_parser_operations,
                );
                match parser.parse() {
                    Ok(script) => {
                        if self.counters.push_function(&self.limits).is_err() {
                            result.push('0');
                        } else {
                            let cmd_result =
                                self.execute_command_sequence(&script.commands).await?;
                            self.counters.pop_function();
                            let trimmed = cmd_result.stdout.trim_end_matches('\n');
                            if trimmed.is_empty() {
                                result.push('0');
                            } else {
                                result.push_str(trimmed);
                            }
                        }
                    }
                    Err(_) => result.push('0'),
                }
            } else {
                result.push(ch);
            }
        }
        Ok(result)
    }

    /// Get the separator for `[*]` array joins: first char of IFS, or space if IFS unset.
    fn get_ifs_separator(&self) -> String {
        let ifs = self.expand_variable("IFS");
        if ifs.is_empty() && !self.is_variable_set("IFS") {
            // IFS unset: default separator is space
            " ".to_string()
        } else {
            // IFS set (possibly empty): first char or empty
            ifs.chars()
                .next()
                .map(|c| c.to_string())
                .unwrap_or_default()
        }
    }

    fn expand_variable(&self, name: &str) -> String {
        // Resolve nameref before expansion
        let name = self.resolve_nameref(name);

        // If resolved name is an array element ref like "a[2]", expand as array access
        if let Some(bracket) = name.find('[')
            && name.ends_with(']')
        {
            let arr_name = &name[..bracket];
            let idx_str = &name[bracket + 1..name.len() - 1];
            if let Some(arr) = self.assoc_arrays.get(arr_name) {
                return arr.get(idx_str).cloned().unwrap_or_default();
            } else if let Some(arr) = self.arrays.get(arr_name) {
                let idx: usize = self.evaluate_arithmetic(idx_str).try_into().unwrap_or(0);
                return arr.get(&idx).cloned().unwrap_or_default();
            }
            return String::new();
        }

        // Check for special parameters (POSIX required)
        match name {
            "?" => return self.last_exit_code.to_string(),
            "#" => {
                // Number of positional parameters
                if let Some(frame) = self.call_stack.last() {
                    return frame.positional.len().to_string();
                }
                return "0".to_string();
            }
            "@" => {
                // All positional parameters (space-separated as string)
                if let Some(frame) = self.call_stack.last() {
                    return frame.positional.join(" ");
                }
                return String::new();
            }
            "*" => {
                // All positional parameters joined by IFS first char
                if let Some(frame) = self.call_stack.last() {
                    let sep = self.get_ifs_separator();
                    return frame.positional.join(&sep);
                }
                return String::new();
            }
            "$" => {
                // THREAT[TM-INF-014]: Return sandboxed PID, not real host PID.
                return "1".to_string();
            }
            "!" => {
                // $! - PID of most recent background command
                // In Bashkit's virtual environment, background jobs run synchronously
                // Return empty string or last job ID placeholder
                if let Some(last_bg_pid) = self.variables.get("_LAST_BG_PID") {
                    return last_bg_pid.clone();
                }
                return String::new();
            }
            "-" => {
                // $- - Current option flags as a string
                // Build from SHOPT_* variables
                let mut flags = String::new();
                for opt in ['e', 'x', 'u', 'f', 'n', 'v', 'a', 'b', 'h', 'm'] {
                    let opt_name = format!("SHOPT_{}", opt);
                    if self
                        .variables
                        .get(&opt_name)
                        .map(|v| v == "1")
                        .unwrap_or(false)
                    {
                        flags.push(opt);
                    }
                }
                return flags;
            }
            "RANDOM" => {
                // $RANDOM - random number between 0 and 32767
                use std::collections::hash_map::RandomState;
                use std::hash::{BuildHasher, Hasher};
                let random = RandomState::new().build_hasher().finish() as u32;
                return (random % 32768).to_string();
            }
            "LINENO" => {
                // $LINENO - current line number from command span
                return self.current_line.to_string();
            }
            "PWD" => {
                return self.cwd.to_string_lossy().to_string();
            }
            "OLDPWD" => {
                if let Some(v) = self.variables.get("OLDPWD") {
                    return v.clone();
                }
                return self.cwd.to_string_lossy().to_string();
            }
            "HOSTNAME" => {
                if let Some(v) = self.variables.get("HOSTNAME") {
                    return v.clone();
                }
                return "localhost".to_string();
            }
            "BASH_VERSION" => {
                return format!("{}-bashkit", env!("CARGO_PKG_VERSION"));
            }
            "SECONDS" => {
                // Seconds since shell started - always 0 in stateless model
                if let Some(v) = self.variables.get("SECONDS") {
                    return v.clone();
                }
                return "0".to_string();
            }
            _ => {}
        }

        // Check for numeric positional parameter ($1, $2, etc.)
        if let Ok(n) = name.parse::<usize>() {
            if n == 0 {
                // $0 is the script/function name
                if let Some(frame) = self.call_stack.last() {
                    return frame.name.clone();
                }
                return "bash".to_string();
            }
            // $1, $2, etc. (1-indexed)
            if let Some(frame) = self.call_stack.last()
                && n > 0
                && n <= frame.positional.len()
            {
                return frame.positional[n - 1].clone();
            }
            return String::new();
        }

        // Check local variables in call stack (top to bottom)
        for frame in self.call_stack.iter().rev() {
            if let Some(value) = frame.locals.get(name) {
                return value.clone();
            }
        }

        // Check shell variables
        if let Some(value) = self.variables.get(name) {
            return value.clone();
        }

        // Check environment
        if let Some(value) = self.env.get(name) {
            return value.clone();
        }

        // Not found - expand to empty string (bash behavior)
        String::new()
    }

    /// Check if a variable is set (for `set -u` / nounset).
    /// Follows nameref indirection so that a nameref pointing to a defined
    /// target is considered "set".
    fn is_variable_set(&self, name: &str) -> bool {
        // Resolve nameref before checking — a nameref whose target exists is "set".
        let name = self.resolve_nameref(name);

        // Special variables are always "set"
        if matches!(
            name,
            "?" | "#"
                | "@"
                | "*"
                | "$"
                | "!"
                | "-"
                | "RANDOM"
                | "LINENO"
                | "PWD"
                | "OLDPWD"
                | "HOSTNAME"
                | "BASH_VERSION"
                | "SECONDS"
        ) {
            return true;
        }
        // Positional params $0..$N
        if let Ok(n) = name.parse::<usize>() {
            if n == 0 {
                return true;
            }
            return self
                .call_stack
                .last()
                .map(|f| n <= f.positional.len())
                .unwrap_or(false);
        }
        // Local variables
        for frame in self.call_stack.iter().rev() {
            if frame.locals.contains_key(name) {
                return true;
            }
        }
        // Shell variables
        if self.variables.contains_key(name) {
            return true;
        }
        // Environment
        self.env.contains_key(name)
    }

    /// Check if nounset (`set -u`) is active.
    fn is_nounset(&self) -> bool {
        self.variables
            .get("SHOPT_u")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if pipefail (`set -o pipefail`) is active.
    fn is_pipefail(&self) -> bool {
        self.variables
            .get("SHOPT_pipefail")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Run ERR trap if registered. Appends trap output to stdout/stderr.
    /// Run the DEBUG trap handler (fires before each simple command).
    /// Returns (stdout, stderr) from the trap handler.
    async fn run_debug_trap(&mut self) -> (String, String) {
        if let Some(trap_cmd) = self.traps.get("DEBUG").cloned() {
            // THREAT[TM-DOS-030]: Propagate interpreter parser limits
            if let Ok(trap_script) = Parser::with_limits(
                &trap_cmd,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            )
            .parse()
            {
                let emit_before = self.output_emit_count;
                if let Ok(trap_result) = self.execute_command_sequence(&trap_script.commands).await
                {
                    self.maybe_emit_output(&trap_result.stdout, &trap_result.stderr, emit_before);
                    return (trap_result.stdout, trap_result.stderr);
                }
            }
        }
        (String::new(), String::new())
    }

    async fn run_err_trap(&mut self, stdout: &mut String, stderr: &mut String) {
        if let Some(trap_cmd) = self.traps.get("ERR").cloned() {
            // THREAT[TM-DOS-030]: Propagate interpreter parser limits
            if let Ok(trap_script) = Parser::with_limits(
                &trap_cmd,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            )
            .parse()
            {
                let emit_before = self.output_emit_count;
                if let Ok(trap_result) = self.execute_command_sequence(&trap_script.commands).await
                {
                    self.maybe_emit_output(&trap_result.stdout, &trap_result.stderr, emit_before);
                    stdout.push_str(&trap_result.stdout);
                    stderr.push_str(&trap_result.stderr);
                }
            }
        }
    }

    /// Set a local variable in the current call frame
    #[allow(dead_code)]
    fn set_local(&mut self, name: &str, value: &str) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.locals.insert(name.to_string(), value.to_string());
        }
    }

    /// Check if a string contains glob characters
    /// Expand brace patterns like {a,b,c} or {1..5}
    /// Returns a Vec of expanded strings, or a single-element Vec if no braces
    /// THREAT[TM-DOS-042]: Cap total expansion count to prevent combinatorial OOM.
    fn expand_braces(&self, s: &str) -> Vec<String> {
        const MAX_BRACE_EXPANSION_TOTAL: usize = 100_000;
        let mut count = 0;
        self.expand_braces_capped(s, &mut count, MAX_BRACE_EXPANSION_TOTAL)
    }

    fn expand_braces_capped(&self, s: &str, count: &mut usize, max: usize) -> Vec<String> {
        if *count >= max {
            return vec![s.to_string()];
        }

        // Find the first brace that has a matching close brace
        let mut depth = 0;
        let mut brace_start = None;
        let mut brace_end = None;
        let chars: Vec<char> = s.chars().collect();

        for (i, &ch) in chars.iter().enumerate() {
            match ch {
                '{' => {
                    if depth == 0 {
                        brace_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 && brace_start.is_some() {
                        brace_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        // No valid brace pattern found
        let (start, end) = match (brace_start, brace_end) {
            (Some(s), Some(e)) => (s, e),
            _ => return vec![s.to_string()],
        };

        let prefix: String = chars[..start].iter().collect();
        let suffix: String = chars[end + 1..].iter().collect();
        let brace_content: String = chars[start + 1..end].iter().collect();

        // Brace content with leading/trailing space is not expanded
        if brace_content.starts_with(' ') || brace_content.ends_with(' ') {
            return vec![s.to_string()];
        }

        // Check for range expansion like {1..5} or {a..z}
        if let Some(range_result) = self.try_expand_range(&brace_content) {
            let mut results = Vec::new();
            for item in range_result {
                if *count >= max {
                    break;
                }
                let expanded = format!("{}{}{}", prefix, item, suffix);
                let sub = self.expand_braces_capped(&expanded, count, max);
                *count += sub.len();
                results.extend(sub);
            }
            return results;
        }

        // List expansion like {a,b,c}
        // Need to split by comma, but respect nested braces
        let items = self.split_brace_items(&brace_content);
        if items.len() <= 1 && !brace_content.contains(',') {
            // Not a valid brace expansion (e.g., just {foo})
            return vec![s.to_string()];
        }

        let mut results = Vec::new();
        for item in items {
            if *count >= max {
                break;
            }
            let expanded = format!("{}{}{}", prefix, item, suffix);
            let sub = self.expand_braces_capped(&expanded, count, max);
            *count += sub.len();
            results.extend(sub);
        }

        results
    }

    /// Try to expand a range like 1..5, a..z, or 1..10..2
    /// THREAT[TM-DOS-041]: Cap range size to prevent OOM from {1..999999999}
    fn try_expand_range(&self, content: &str) -> Option<Vec<String>> {
        /// Maximum number of elements in a brace range expansion
        const MAX_BRACE_RANGE: u64 = 10_000;

        // Check for .. separator: accept {start..end} or {start..end..step}
        let parts: Vec<&str> = content.split("..").collect();
        if parts.len() != 2 && parts.len() != 3 {
            return None;
        }

        let start = parts[0];
        let end = parts[1];

        // Try numeric range
        if let (Ok(start_num), Ok(end_num)) = (start.parse::<i64>(), end.parse::<i64>()) {
            // Parse optional step (default: 1 or -1 based on direction)
            let step: i64 = if parts.len() == 3 {
                match parts[2].parse::<i64>() {
                    Ok(0) => return None, // step=0 is invalid
                    Ok(s) => s,
                    Err(_) => return None,
                }
            } else if start_num <= end_num {
                1
            } else {
                -1
            };

            let abs_step = step.unsigned_abs() as u128;
            let abs_diff = (end_num as i128 - start_num as i128).unsigned_abs();
            let range_size = abs_diff / abs_step + 1;
            if range_size > MAX_BRACE_RANGE as u128 {
                return None; // Treat as literal — too large
            }

            let mut results = Vec::new();
            // Bash behavior: direction is determined by start/end,
            // step sign determines actual increment direction
            let effective_step = if start_num <= end_num {
                step.abs()
            } else {
                -(step.abs())
            };

            let mut i = start_num;
            if effective_step > 0 {
                while i <= end_num {
                    results.push(i.to_string());
                    i += effective_step;
                }
            } else {
                while i >= end_num {
                    results.push(i.to_string());
                    i += effective_step;
                }
            }
            return Some(results);
        }

        // Try character range (single chars only)
        if start.len() == 1 && end.len() == 1 {
            let start_char = start.chars().next().unwrap();
            let end_char = end.chars().next().unwrap();

            if start_char.is_ascii_alphabetic() && end_char.is_ascii_alphabetic() {
                let step: i64 = if parts.len() == 3 {
                    match parts[2].parse::<i64>() {
                        Ok(0) => return None,
                        Ok(s) => s,
                        Err(_) => return None,
                    }
                } else {
                    1
                };
                let abs_step = step.unsigned_abs() as u8;

                let mut results = Vec::new();
                let start_byte = start_char as u8;
                let end_byte = end_char as u8;

                if start_byte <= end_byte {
                    let mut b = start_byte;
                    while b <= end_byte {
                        results.push((b as char).to_string());
                        b = match b.checked_add(abs_step) {
                            Some(v) => v,
                            None => break,
                        };
                    }
                } else {
                    let mut b = start_byte;
                    while b >= end_byte {
                        results.push((b as char).to_string());
                        b = match b.checked_sub(abs_step) {
                            Some(v) => v,
                            None => break,
                        };
                    }
                }
                return Some(results);
            }
        }

        None
    }

    /// Split brace content by commas, respecting nested braces
    fn split_brace_items(&self, content: &str) -> Vec<String> {
        let mut items = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for ch in content.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    current.push(ch);
                }
                '}' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    items.push(current);
                    current = String::new();
                }
                _ => {
                    current.push(ch);
                }
            }
        }
        items.push(current);

        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use crate::parser::Parser;

    /// Test timeout with paused time for deterministic behavior
    #[tokio::test(start_paused = true)]
    async fn test_timeout_expires_deterministically() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));

        // timeout 0.001 sleep 10 - should timeout (1ms << 10s)
        let parser = Parser::new("timeout 0.001 sleep 10; echo $?");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(
            result.stdout.trim(),
            "124",
            "Expected exit code 124 for timeout"
        );
    }

    /// Test zero timeout
    #[tokio::test(start_paused = true)]
    async fn test_timeout_zero_deterministically() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));

        // timeout 0 sleep 1 - should timeout immediately
        let parser = Parser::new("timeout 0 sleep 1; echo $?");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(
            result.stdout.trim(),
            "124",
            "Expected exit code 124 for zero timeout"
        );
    }

    /// Test that parse_duration preserves subsecond precision
    #[test]
    fn test_parse_timeout_duration_subsecond() {
        use crate::builtins::timeout::parse_duration;
        use std::time::Duration;

        // Should preserve subsecond precision
        let d = parse_duration("0.001").unwrap();
        assert_eq!(d, Duration::from_secs_f64(0.001));

        let d = parse_duration("0.5").unwrap();
        assert_eq!(d, Duration::from_millis(500));

        let d = parse_duration("1.5s").unwrap();
        assert_eq!(d, Duration::from_millis(1500));

        // Zero should work
        let d = parse_duration("0").unwrap();
        assert_eq!(d, Duration::ZERO);
    }

    // POSIX special builtins tests

    /// Helper to run a script and return result
    async fn run_script(script: &str) -> ExecResult {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        interp.execute(&ast).await.unwrap()
    }

    #[tokio::test]
    async fn test_colon_null_utility() {
        // POSIX : (colon) - null utility, should return success
        let result = run_script(":").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_colon_with_args() {
        // Colon should ignore arguments and still succeed
        let result = run_script(": arg1 arg2 arg3").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_colon_in_while_loop() {
        // Common use case: while : (infinite loop, but we limit iterations)
        let result = run_script(
            "x=0; while :; do x=$((x+1)); if [ $x -ge 3 ]; then break; fi; done; echo $x",
        )
        .await;
        assert_eq!(result.stdout.trim(), "3");
    }

    #[tokio::test]
    async fn test_times_builtin() {
        // POSIX times - returns process times (zeros in virtual mode)
        let result = run_script("times").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("0m0.000s"));
    }

    #[tokio::test]
    async fn test_readonly_basic() {
        // POSIX readonly - mark variable as read-only
        let result = run_script("readonly X=value; echo $X").await;
        assert_eq!(result.stdout.trim(), "value");
    }

    #[tokio::test]
    async fn test_special_param_dash() {
        // $- should return current option flags
        let result = run_script("set -e; echo \"$-\"").await;
        assert!(result.stdout.contains('e'));
    }

    #[tokio::test]
    async fn test_special_param_bang() {
        // $! - last background PID (empty in virtual mode with no bg jobs)
        let result = run_script("echo \"$!\"").await;
        // Should be empty or a placeholder
        assert_eq!(result.exit_code, 0);
    }

    // =========================================================================
    // Additional POSIX positive tests
    // =========================================================================

    #[tokio::test]
    async fn test_colon_variable_side_effect() {
        // Common pattern: use : with parameter expansion for defaults
        let result = run_script(": ${X:=default}; echo $X").await;
        assert_eq!(result.stdout.trim(), "default");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_colon_in_if_then() {
        // Use : as no-op in then branch
        let result = run_script("if true; then :; fi; echo done").await;
        assert_eq!(result.stdout.trim(), "done");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_readonly_set_and_read() {
        // Set readonly variable and verify it's accessible
        let result = run_script("readonly FOO=bar; readonly BAR=baz; echo $FOO $BAR").await;
        assert_eq!(result.stdout.trim(), "bar baz");
    }

    #[tokio::test]
    async fn test_readonly_mark_existing() {
        // Mark an existing variable as readonly
        let result = run_script("X=hello; readonly X; echo $X").await;
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_times_two_lines() {
        // times should output exactly two lines
        let result = run_script("times").await;
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn test_eval_simple_command() {
        // eval should execute the constructed command
        let result = run_script("cmd='echo hello'; eval $cmd").await;
        // Note: eval stores command for interpreter, actual execution depends on interpreter support
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_special_param_dash_multiple_options() {
        // Set multiple options and verify $- contains them
        let result = run_script("set -e; set -x; echo \"$-\"").await;
        assert!(result.stdout.contains('e'));
        // Note: x is stored but we verify at least e is present
    }

    #[tokio::test]
    async fn test_special_param_dash_no_options() {
        // With no options set, $- should be empty or minimal
        let result = run_script("echo \"flags:$-:end\"").await;
        assert!(result.stdout.contains("flags:"));
        assert!(result.stdout.contains(":end"));
        assert_eq!(result.exit_code, 0);
    }

    // =========================================================================
    // POSIX negative tests (error cases / edge cases)
    // =========================================================================

    #[tokio::test]
    async fn test_colon_does_not_produce_output() {
        // Colon should never produce any output
        let result = run_script(": 'this should not appear'").await;
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
    }

    #[tokio::test]
    async fn test_eval_empty_args() {
        // eval with no arguments should succeed silently
        let result = run_script("eval; echo $?").await;
        assert!(result.stdout.contains('0'));
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_readonly_empty_value() {
        // readonly with empty value
        let result = run_script("readonly EMPTY=; echo \"[$EMPTY]\"").await;
        assert_eq!(result.stdout.trim(), "[]");
    }

    #[tokio::test]
    async fn test_times_no_args_accepted() {
        // times should ignore any arguments
        let result = run_script("times ignored args here").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("0m0.000s"));
    }

    #[tokio::test]
    async fn test_special_param_bang_empty_without_bg() {
        // $! should be empty when no background jobs have run
        let result = run_script("x=\"$!\"; [ -z \"$x\" ] && echo empty || echo not_empty").await;
        assert_eq!(result.stdout.trim(), "empty");
    }

    #[tokio::test]
    async fn test_colon_exit_code_zero() {
        // Verify colon always returns 0 even after failed command
        let result = run_script("false; :; echo $?").await;
        assert_eq!(result.stdout.trim(), "0");
    }

    #[tokio::test]
    async fn test_readonly_without_value_preserves_existing() {
        // readonly on existing var preserves value
        let result = run_script("VAR=existing; readonly VAR; echo $VAR").await;
        assert_eq!(result.stdout.trim(), "existing");
    }

    // bash/sh command tests

    #[tokio::test]
    async fn test_bash_c_simple_command() {
        // bash -c "command" should execute the command
        let result = run_script("bash -c 'echo hello'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_sh_c_simple_command() {
        // sh -c "command" should also work
        let result = run_script("sh -c 'echo world'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "world");
    }

    #[tokio::test]
    async fn test_bash_c_multiple_commands() {
        // bash -c with multiple commands separated by semicolon
        let result = run_script("bash -c 'echo one; echo two'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "one\ntwo\n");
    }

    #[tokio::test]
    async fn test_bash_c_with_positional_args() {
        // bash -c "cmd" arg0 arg1 - positional parameters
        let result = run_script("bash -c 'echo $0 $1' zero one").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "zero one");
    }

    #[tokio::test]
    async fn test_bash_script_file() {
        // bash script.sh - execute a script file
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/test.sh"), b"echo 'from script'")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/test.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "from script");
    }

    #[tokio::test]
    async fn test_bash_script_file_with_args() {
        // bash script.sh arg1 arg2 - script with arguments
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/args.sh"), b"echo $1 $2")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/args.sh first second");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "first second");
    }

    #[tokio::test]
    async fn test_bash_piped_script() {
        // echo "script" | bash - execute from stdin
        let result = run_script("echo 'echo piped' | bash").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "piped");
    }

    #[tokio::test]
    async fn test_bash_nonexistent_file() {
        // bash missing.sh - should error with exit code 127
        let result = run_script("bash /nonexistent/missing.sh").await;
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("No such file"));
    }

    #[tokio::test]
    async fn test_bash_c_missing_argument() {
        // bash -c without command string - should error
        let result = run_script("bash -c").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("option requires an argument"));
    }

    #[tokio::test]
    async fn test_bash_c_syntax_error() {
        // bash -c with invalid syntax
        let result = run_script("bash -c 'if then'").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_preserves_variables() {
        // Variables set in bash -c should affect the parent
        // (since we share the interpreter state)
        let result = run_script("bash -c 'X=inner'; echo $X").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "inner");
    }

    #[tokio::test]
    async fn test_bash_c_exit_code_propagates() {
        // Exit code from bash -c should propagate
        let result = run_script("bash -c 'exit 42'; echo $?").await;
        assert_eq!(result.stdout.trim(), "42");
    }

    #[tokio::test]
    async fn test_bash_nested() {
        // Nested bash -c calls
        let result = run_script("bash -c \"bash -c 'echo nested'\"").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "nested");
    }

    #[tokio::test]
    async fn test_sh_script_file() {
        // sh script.sh - same as bash script.sh
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/sh_test.sh"), b"echo 'sh works'")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("sh /tmp/sh_test.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "sh works");
    }

    #[tokio::test]
    async fn test_bash_with_option_e() {
        // bash -e -c "command" - -e is accepted but doesn't change behavior in virtual mode
        let result = run_script("bash -e -c 'echo works'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "works");
    }

    #[tokio::test]
    async fn test_bash_empty_input() {
        // bash with no arguments or stdin does nothing
        let result = run_script("bash; echo done").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "done");
    }

    // Additional bash/sh tests for noexec, version, help

    #[tokio::test]
    async fn test_bash_n_syntax_check_success() {
        // bash -n parses but doesn't execute
        let result = run_script("bash -n -c 'echo should not print'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, ""); // Nothing printed - didn't execute
    }

    #[tokio::test]
    async fn test_bash_n_syntax_error_detected() {
        // bash -n catches syntax errors
        let result = run_script("bash -n -c 'if then'").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_n_combined_flags() {
        // -n can be combined with other flags like -ne
        let result = run_script("bash -ne -c 'echo test'; echo done").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "done"); // Only "done" - bash -n didn't execute
    }

    #[tokio::test]
    async fn test_bash_version() {
        // --version shows Bashkit version
        let result = run_script("bash --version").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Bashkit"));
        assert!(result.stdout.contains("virtual"));
    }

    #[tokio::test]
    async fn test_sh_version() {
        // sh --version also works
        let result = run_script("sh --version").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("virtual sh"));
    }

    #[tokio::test]
    async fn test_bash_help() {
        // --help shows usage
        let result = run_script("bash --help").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Usage:"));
        assert!(result.stdout.contains("-c string"));
        assert!(result.stdout.contains("-n"));
    }

    #[tokio::test]
    async fn test_bash_double_dash() {
        // -- ends option processing
        let result = run_script("bash -- --help").await;
        // Should try to run file named "--help", which doesn't exist
        assert_eq!(result.exit_code, 127);
    }

    // Negative test cases

    #[tokio::test]
    async fn test_bash_invalid_syntax_in_file() {
        // Syntax error in script file - unclosed if
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/bad.sh"), b"if true; then echo x")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/bad.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_permission_in_sandbox() {
        // Filesystem operations work through bash -c
        let result = run_script("bash -c 'echo test > /tmp/out.txt && cat /tmp/out.txt'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "test");
    }

    #[tokio::test]
    async fn test_bash_all_positional() {
        // $@ and $* work correctly
        let result = run_script("bash -c 'echo $@' _ a b c").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "a b c");
    }

    #[tokio::test]
    async fn test_bash_arg_count() {
        // $# counts positional params
        let result = run_script("bash -c 'echo $#' _ 1 2 3 4").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "4");
    }

    // Security-focused tests

    #[tokio::test]
    async fn test_bash_no_real_bash_escape() {
        // Verify bash -c doesn't escape sandbox
        // Try to run a command that would work in real bash but not here
        let result = run_script("bash -c 'which bash 2>/dev/null || echo not found'").await;
        // 'which' is not a builtin, so this should fail
        assert!(result.stdout.contains("not found") || result.exit_code == 127);
    }

    #[tokio::test]
    async fn test_bash_nested_limits_respected() {
        // Deep nesting should eventually hit limits
        // This tests that bash -c doesn't bypass command limits
        let result = run_script("bash -c 'for i in 1 2 3; do echo $i; done'").await;
        assert_eq!(result.exit_code, 0);
        // Loop executed successfully within limits
    }

    #[tokio::test]
    async fn test_bash_c_injection_safe() {
        // Variable expansion doesn't allow injection
        let result = run_script("INJECT='; rm -rf /'; bash -c 'echo safe'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "safe");
    }

    #[tokio::test]
    async fn test_bash_version_no_host_info() {
        // Version output doesn't leak host information
        let result = run_script("bash --version").await;
        assert!(!result.stdout.contains("/usr"));
        assert!(!result.stdout.contains("GNU"));
        // Should only contain virtual version info
    }

    // Additional positive tests

    #[tokio::test]
    async fn test_bash_c_with_quotes() {
        // Handles quoted strings correctly
        let result = run_script(r#"bash -c 'echo "hello world"'"#).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello world");
    }

    #[tokio::test]
    async fn test_bash_c_with_variables() {
        // Variables expand correctly in bash -c
        let result = run_script("X=test; bash -c 'echo $X'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "test");
    }

    #[tokio::test]
    async fn test_bash_c_pipe_in_command() {
        // Pipes work inside bash -c
        let result = run_script("bash -c 'echo hello | cat'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_bash_c_subshell() {
        // Command substitution works in bash -c
        let result = run_script("bash -c 'echo $(echo inner)'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "inner");
    }

    #[tokio::test]
    async fn test_bash_c_conditional() {
        // Conditionals work in bash -c
        let result = run_script("bash -c 'if true; then echo yes; fi'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "yes");
    }

    #[tokio::test]
    async fn test_bash_script_with_shebang() {
        // Script with shebang is handled (shebang line ignored)
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(
            std::path::Path::new("/tmp/shebang.sh"),
            b"#!/bin/bash\necho works",
        )
        .await
        .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/shebang.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "works");
    }

    #[tokio::test]
    async fn test_bash_n_with_valid_multiline() {
        // -n validates multiline scripts
        let result = run_script("bash -n -c 'echo one\necho two\necho three'").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_sh_behaves_like_bash() {
        // sh and bash produce same results
        let bash_result = run_script("bash -c 'echo $((1+2))'").await;
        let sh_result = run_script("sh -c 'echo $((1+2))'").await;
        assert_eq!(bash_result.stdout, sh_result.stdout);
        assert_eq!(bash_result.exit_code, sh_result.exit_code);
    }

    // Additional negative tests

    #[tokio::test]
    async fn test_bash_n_unclosed_if() {
        // -n catches unclosed control structures
        let result = run_script("bash -n -c 'if true; then echo x'").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_n_unclosed_while() {
        // -n catches unclosed while
        let result = run_script("bash -n -c 'while true; do echo x'").await;
        assert_eq!(result.exit_code, 2);
    }

    #[tokio::test]
    async fn test_bash_empty_c_string() {
        // Empty -c string is valid (does nothing)
        let result = run_script("bash -c ''").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_bash_whitespace_only_c_string() {
        // Whitespace-only -c string is valid
        let result = run_script("bash -c '   '").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_bash_directory_not_file() {
        // Trying to execute a directory fails
        let result = run_script("bash /tmp").await;
        // Should fail - /tmp is a directory
        assert_ne!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_bash_c_exit_propagates() {
        // Exit code from bash -c is captured in $?
        let result = run_script("bash -c 'exit 42'; echo \"code: $?\"").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("code: 42"));
    }

    #[tokio::test]
    async fn test_bash_multiple_scripts_sequential() {
        // Multiple bash calls work sequentially
        let result = run_script("bash -c 'echo 1'; bash -c 'echo 2'; bash -c 'echo 3'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n3\n");
    }

    // Security edge cases

    #[tokio::test]
    async fn test_bash_c_path_traversal_blocked() {
        // Path traversal in bash -c doesn't escape sandbox
        let result =
            run_script("bash -c 'cat /../../etc/passwd 2>/dev/null || echo blocked'").await;
        assert!(result.stdout.contains("blocked") || result.exit_code != 0);
    }

    #[tokio::test]
    async fn test_bash_nested_deeply() {
        // Deeply nested bash calls work within limits
        let result = run_script("bash -c \"bash -c 'bash -c \\\"echo deep\\\"'\"").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "deep");
    }

    #[tokio::test]
    async fn test_bash_c_special_chars() {
        // Special characters in commands handled safely
        let result = run_script("bash -c 'echo \"$HOME\"'").await;
        // Should use virtual home directory, not real system path
        assert!(!result.stdout.contains("/root"));
        assert!(result.stdout.contains("/home/sandbox"));
    }

    #[tokio::test]
    async fn test_bash_c_dollar_substitution() {
        // $() substitution works in bash -c
        let result = run_script("bash -c 'echo $(echo subst)'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "subst");
    }

    #[tokio::test]
    async fn test_bash_help_contains_expected_options() {
        // Help output contains documented options
        let result = run_script("bash --help").await;
        assert!(result.stdout.contains("-c"));
        assert!(result.stdout.contains("-n"));
        assert!(result.stdout.contains("--version"));
    }

    #[tokio::test]
    async fn test_bash_c_array_operations() {
        // Array operations work in bash -c
        let result = run_script("bash -c 'arr=(a b c); echo ${arr[1]}'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "b");
    }

    #[tokio::test]
    async fn test_bash_positional_special_vars() {
        // Special positional vars work
        let result = run_script("bash -c 'echo \"args: $#, first: $1, all: $*\"' prog a b c").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("args: 3"));
        assert!(result.stdout.contains("first: a"));
        assert!(result.stdout.contains("all: a b c"));
    }

    #[tokio::test]
    async fn test_xtrace_basic() {
        // set -x sends trace to stderr
        let result = run_script("set -x; echo hello").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\n");
        assert!(
            result.stderr.contains("+ echo hello"),
            "stderr should contain xtrace: {:?}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn test_xtrace_multiple_commands() {
        let result = run_script("set -x; echo one; echo two").await;
        assert_eq!(result.stdout, "one\ntwo\n");
        assert!(result.stderr.contains("+ echo one"));
        assert!(result.stderr.contains("+ echo two"));
    }

    #[tokio::test]
    async fn test_xtrace_expanded_variables() {
        // Trace shows expanded values, not variable names
        let result = run_script("x=hello; set -x; echo $x").await;
        assert_eq!(result.stdout, "hello\n");
        assert!(
            result.stderr.contains("+ echo hello"),
            "xtrace should show expanded value: {:?}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn test_xtrace_disable() {
        // set +x disables tracing; set +x itself is traced
        let result = run_script("set -x; echo traced; set +x; echo not_traced").await;
        assert_eq!(result.stdout, "traced\nnot_traced\n");
        assert!(result.stderr.contains("+ echo traced"));
        assert!(
            result.stderr.contains("+ set +x"),
            "set +x should be traced: {:?}",
            result.stderr
        );
        assert!(
            !result.stderr.contains("+ echo not_traced"),
            "echo after set +x should NOT be traced: {:?}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn test_xtrace_no_trace_without_flag() {
        let result = run_script("echo hello").await;
        assert_eq!(result.stdout, "hello\n");
        assert!(
            result.stderr.is_empty(),
            "no xtrace without set -x: {:?}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn test_xtrace_not_captured_by_redirect() {
        // 2>&1 should NOT capture xtrace (matches real bash behavior)
        let result = run_script("set -x; echo hello 2>&1").await;
        assert_eq!(result.stdout, "hello\n");
        assert!(
            result.stderr.contains("+ echo hello"),
            "xtrace should stay in stderr even with 2>&1: {:?}",
            result.stderr
        );
    }

    // ==================== xargs execution tests ====================

    #[tokio::test]
    async fn test_xargs_executes_command() {
        // xargs should execute the command, not echo it
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/workspace"), true)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/workspace/file.txt"), b"hello world")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        let parser = Parser::new("echo /workspace/file.txt | xargs cat");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout.trim(),
            "hello world",
            "xargs should execute cat, not echo it. Got: {:?}",
            result.stdout
        );
    }

    #[tokio::test]
    async fn test_xargs_default_echo() {
        // With no command, xargs defaults to echo
        let result = run_script("echo 'a b c' | xargs").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "a b c");
    }

    #[tokio::test]
    async fn test_xargs_splits_newlines() {
        // xargs should split input on whitespace/newlines into separate args
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/workspace"), true)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/workspace/a.txt"), b"AAA")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/workspace/b.txt"), b"BBB")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        let script = "printf '/workspace/a.txt\\n/workspace/b.txt' | xargs cat";
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("AAA"),
            "should contain contents of a.txt"
        );
        assert!(
            result.stdout.contains("BBB"),
            "should contain contents of b.txt"
        );
    }

    #[tokio::test]
    async fn test_xargs_n1_executes_per_item() {
        // xargs -n 1 should execute once per argument
        let result = run_script("echo 'a b c' | xargs -n 1 echo item:").await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "item: a");
        assert_eq!(lines[1], "item: b");
        assert_eq!(lines[2], "item: c");
    }

    #[tokio::test]
    async fn test_xargs_replace_str() {
        // xargs -I {} should substitute {} with each input line
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/workspace"), true)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/workspace/hello.txt"), b"Hello!")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        let script = "echo /workspace/hello.txt | xargs -I {} cat {}";
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "Hello!");
    }

    // ==================== find -exec tests ====================

    #[tokio::test]
    async fn test_find_exec_per_file() {
        // find -exec cmd {} \; should execute cmd for each matched file
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/project"), true)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/project/a.txt"), b"content-a")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/project/b.txt"), b"content-b")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /project -name "*.txt" -exec echo {} \;"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(result.stdout.contains("/project/a.txt"));
        assert!(result.stdout.contains("/project/b.txt"));
    }

    #[tokio::test]
    async fn test_find_exec_batch_mode() {
        // find -exec cmd {} + should execute cmd once with all matched paths
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/project"), true)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/project/a.txt"), b"aaa")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/project/b.txt"), b"bbb")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /project -name "*.txt" -exec echo {} +"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        // Should be a single line with both paths
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(result.stdout.contains("/project/a.txt"));
        assert!(result.stdout.contains("/project/b.txt"));
    }

    #[tokio::test]
    async fn test_find_exec_cat_reads_files() {
        // find -exec cat {} \; should actually read file contents
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/data"), true).await.unwrap();
        fs.write_file(std::path::Path::new("/data/hello.txt"), b"Hello World")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /data -name "hello.txt" -exec cat {} \;"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello World");
    }

    #[tokio::test]
    async fn test_find_exec_with_type_filter() {
        // find -type f -exec should only process files
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/root/subdir"), true)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/root/file.txt"), b"data")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /root -type f -exec echo found {} \;"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("found /root/file.txt"));
        assert!(!result.stdout.contains("found /root/subdir"));
    }

    #[tokio::test]
    async fn test_find_exec_nonexistent_path() {
        let fs = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /nonexistent -exec echo {} \;"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn test_find_exec_no_matches() {
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/empty"), true)
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /empty -name "*.xyz" -exec echo {} \;"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_find_exec_multiple_placeholder() {
        // {} can appear multiple times in the command template
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/src"), true).await.unwrap();
        fs.write_file(std::path::Path::new("/src/test.txt"), b"hi")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /src -name "test.txt" -exec echo {} {} \;"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "/src/test.txt /src/test.txt");
    }

    #[tokio::test]
    async fn test_star_join_with_ifs() {
        // "$*" joins with IFS first char; empty IFS = no separator
        let result = run_script("set -- x y z\nIFS=:\necho \"$*\"").await;
        assert_eq!(result.stdout, "x:y:z\n");
        let result = run_script("set -- x y z\nIFS=\necho \"$*\"").await;
        assert_eq!(result.stdout, "xyz\n");
        // echo ["$*"] — brackets are literal, quotes are stripped
        let result = run_script("set -- x y z\necho [\"$*\"]").await;
        assert_eq!(result.stdout, "[x y z]\n");
        // "$*" in assignment
        let result = run_script("IFS=:\nset -- x 'y z'\ns=\"$*\"\necho \"star=$s\"").await;
        assert_eq!(result.stdout, "star=x:y z\n");
        // set a b c (without --)
        let result = run_script("set a b c\necho $#\necho $1 $2 $3").await;
        assert_eq!(result.stdout, "3\na b c\n");
    }

    #[tokio::test]
    async fn test_arithmetic_exponent_negative_no_panic() {
        let result = run_script("echo $(( 2 ** -1 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_exponent_large_no_panic() {
        let result = run_script("echo $(( 2 ** 100 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_shift_large_no_panic() {
        let result = run_script("echo $(( 1 << 64 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_shift_negative_no_panic() {
        let result = run_script("echo $(( 1 << -1 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_div_min_neg1_no_panic() {
        let result = run_script("echo $(( -9223372036854775808 / -1 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_mod_min_neg1_no_panic() {
        let result = run_script("echo $(( -9223372036854775808 % -1 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_overflow_add_no_panic() {
        let result = run_script("echo $(( 9223372036854775807 + 1 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_overflow_mul_no_panic() {
        let result = run_script("echo $(( 9223372036854775807 * 2 ))").await;
        assert_eq!(result.exit_code, 0);
    }

    /// Regression test for fuzz crash: base > 36 in arithmetic
    /// (crash-802347e7f64e6cb69da447b343e4f16081ffe48d)
    #[tokio::test]
    async fn test_arithmetic_base_gt_36_no_panic() {
        let result = run_script("echo $(( 64#A ))").await;
        assert_eq!(result.exit_code, 0);
        // 64#A = 36 (A is position 36 in the extended charset)
        assert_eq!(result.stdout.trim(), "36");
    }

    #[tokio::test]
    async fn test_arithmetic_base_gt_36_special_chars() {
        // @ = 62, _ = 63 in bash base-64 encoding
        let result = run_script("echo $(( 64#@ ))").await;
        assert_eq!(result.stdout.trim(), "62");
        let result = run_script("echo $(( 64#_ ))").await;
        assert_eq!(result.stdout.trim(), "63");
    }

    #[tokio::test]
    async fn test_arithmetic_base_gt_36_invalid_digit() {
        // Invalid char for base — should return 0
        let result = run_script("echo $(( 37#! ))").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_eval_respects_parser_limits() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.limits.max_ast_depth = 5;
        let parser = Parser::new("eval 'echo hello'");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_source_respects_parser_limits() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/test.sh"), b"echo sourced")
            .await
            .unwrap();
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.limits.max_ast_depth = 5;
        let parser = Parser::new("source /tmp/test.sh");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "sourced");
    }

    #[tokio::test]
    async fn test_internal_var_prefix_not_exposed() {
        // ${!_NAMEREF*} must not expose internal markers
        let result = run_script("echo \"${!_NAMEREF*}\"").await;
        assert_eq!(result.stdout.trim(), "");
    }

    #[tokio::test]
    async fn test_internal_var_readonly_not_exposed() {
        let result = run_script("echo \"${!_READONLY*}\"").await;
        assert_eq!(result.stdout.trim(), "");
    }

    #[tokio::test]
    async fn test_internal_var_assignment_blocked() {
        // Direct assignment to _NAMEREF_ prefix should be silently ignored
        let result = run_script("_NAMEREF_x=PATH; echo ${!x}").await;
        assert!(!result.stdout.contains("/usr"));
    }

    #[tokio::test]
    async fn test_internal_var_readonly_injection_blocked() {
        // Should not be able to fake readonly
        let result = run_script("_READONLY_myvar=1; myvar=hello; echo $myvar").await;
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_extglob_no_hang() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let result = run_script(
            r#"shopt -s extglob; [[ "aaaaaaaaaaaa" == +(a|aa) ]] && echo yes || echo no"#,
        )
        .await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "extglob took too long: {:?}",
            elapsed
        );
        assert_eq!(result.exit_code, 0);
    }

    // Issue #425: $$ should not leak real host PID
    #[tokio::test]
    async fn test_dollar_dollar_no_host_pid_leak() {
        let mut bash = crate::Bash::new();
        let result = bash.exec("echo $$").await.unwrap();
        let pid: u32 = result.stdout.trim().parse().unwrap();
        // Should be sandboxed value (1), not real PID
        assert_eq!(pid, 1, "$$ should return sandboxed PID, not real host PID");
    }

    // Issue #426: cyclic nameref should not resolve to wrong variable
    #[tokio::test]
    async fn test_cyclic_nameref_detected() {
        let mut bash = crate::Bash::new();
        // Create cycle: a -> b -> a
        let result = bash
            .exec("declare -n a=b; declare -n b=a; a=hello; echo $a")
            .await
            .unwrap();
        // With the bug, this would silently resolve to an arbitrary variable.
        // With the fix, the cycle is detected and 'a' resolves to itself.
        assert_eq!(result.exit_code, 0);
    }

    // Issue #437: arithmetic expansion byte/char index mismatch
    #[tokio::test]
    async fn test_arithmetic_compound_assign_ascii() {
        let mut bash = crate::Bash::new();
        let result = bash.exec("x=10; (( x += 5 )); echo $x").await.unwrap();
        assert_eq!(result.stdout.trim(), "15");
    }

    #[tokio::test]
    async fn test_getopts_while_loop() {
        // Issue #397: getopts in while loop should iterate over all options
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(
                r#"
set -- -f json -v
while getopts "f:vh" opt; do
  case "$opt" in
    f) FORMAT="$OPTARG" ;;
    v) VERBOSE=1 ;;
  esac
done
echo "FORMAT=$FORMAT VERBOSE=$VERBOSE"
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "FORMAT=json VERBOSE=1");
    }

    #[tokio::test]
    async fn test_getopts_script_with_args() {
        // Issue #397: getopts via bash -c with script args
        let mut bash = crate::Bash::new();
        // Write a script that uses getopts, then invoke it with arguments
        let result = bash
            .exec(
                r#"
cat > /tmp/test_getopts.sh << 'SCRIPT'
while getopts "f:vh" opt; do
  case "$opt" in
    f) FORMAT="$OPTARG" ;;
    v) VERBOSE=1 ;;
  esac
done
echo "FORMAT=$FORMAT VERBOSE=$VERBOSE"
SCRIPT
bash /tmp/test_getopts.sh -f json -v
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "FORMAT=json VERBOSE=1");
    }

    #[tokio::test]
    async fn test_getopts_bash_c_with_args() {
        // Issue #397: getopts via bash -c 'script' -- args
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(
                r#"bash -c '
FORMAT="csv"
VERBOSE=0
while getopts "f:vh" opt; do
    case "$opt" in
        f) FORMAT="$OPTARG" ;;
        v) VERBOSE=1 ;;
    esac
done
echo "FORMAT=$FORMAT VERBOSE=$VERBOSE"
' -- -f json -v"#,
            )
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "FORMAT=json VERBOSE=1");
    }

    #[tokio::test]
    async fn test_getopts_optind_reset_between_scripts() {
        // Issue #397: OPTIND persists across bash script invocations, causing
        // getopts to skip all options on the second run
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(
                r#"
cat > /tmp/opts.sh << 'SCRIPT'
FORMAT="csv"
VERBOSE=0
while getopts "f:vh" opt; do
    case "$opt" in
        f) FORMAT="$OPTARG" ;;
        v) VERBOSE=1 ;;
    esac
done
echo "FORMAT=$FORMAT VERBOSE=$VERBOSE"
SCRIPT
bash /tmp/opts.sh -f json -v
bash /tmp/opts.sh -f xml -v
"#,
            )
            .await
            .unwrap();
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 2, "expected 2 lines: {}", result.stdout);
        assert_eq!(lines[0], "FORMAT=json VERBOSE=1");
        assert_eq!(lines[1], "FORMAT=xml VERBOSE=1");
    }

    #[tokio::test]
    async fn test_wc_l_in_pipe() {
        let mut bash = crate::Bash::new();
        let result = bash.exec(r#"echo -e "a\nb\nc" | wc -l"#).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "3");
    }

    #[tokio::test]
    async fn test_wc_l_in_pipe_subst() {
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(
                r#"
cat > /tmp/data.csv << 'EOF'
name,score
alice,95
bob,87
carol,92
EOF
COUNT=$(tail -n +2 /tmp/data.csv | wc -l)
echo "count=$COUNT"
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "count=3");
    }

    #[tokio::test]
    async fn test_wc_l_counts_newlines() {
        let mut bash = crate::Bash::new();
        let result = bash.exec(r#"printf "a\nb\nc" | wc -l"#).await.unwrap();
        assert_eq!(result.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_regex_match_from_variable() {
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(r#"re="200"; line="hello 200 world"; [[ $line =~ $re ]] && echo "match" || echo "no""#)
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "match");
    }

    #[tokio::test]
    async fn test_regex_match_literal() {
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(r#"line="hello 200 world"; [[ $line =~ 200 ]] && echo "match" || echo "no""#)
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "match");
    }

    #[tokio::test]
    async fn test_assoc_array_in_double_quotes() {
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(r#"declare -A arr; arr["foo"]="bar"; echo "value: ${arr["foo"]}""#)
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "value: bar");
    }

    #[tokio::test]
    async fn test_assoc_array_keys_in_quotes() {
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(r#"declare -A arr; arr["a"]=1; arr["b"]=2; echo "keys: ${!arr[@]}""#)
            .await
            .unwrap();
        let output = result.stdout.trim();
        assert!(output.starts_with("keys: "), "got: {}", output);
        assert!(output.contains("a"), "got: {}", output);
        assert!(output.contains("b"), "got: {}", output);
    }

    #[tokio::test]
    async fn test_glob_with_quoted_prefix() {
        let mut bash = crate::Bash::new();
        bash.fs()
            .mkdir(std::path::Path::new("/testdir"), true)
            .await
            .unwrap();
        bash.fs()
            .write_file(std::path::Path::new("/testdir/a.txt"), b"a")
            .await
            .unwrap();
        bash.fs()
            .write_file(std::path::Path::new("/testdir/b.txt"), b"b")
            .await
            .unwrap();
        let result = bash
            .exec(r#"DIR="/testdir"; for f in "$DIR"/*; do echo "$f"; done"#)
            .await
            .unwrap();
        let mut lines: Vec<&str> = result.stdout.trim().lines().collect();
        lines.sort();
        assert_eq!(lines, vec!["/testdir/a.txt", "/testdir/b.txt"]);
    }

    #[tokio::test]
    async fn test_mkfifo_creates_fifo_in_vfs() {
        let result = run_script("mkfifo /tmp/mypipe && test -p /tmp/mypipe && echo ok").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "ok");
    }

    #[tokio::test]
    async fn test_mkfifo_test_p_returns_true() {
        let result = run_script("mkfifo /tmp/mypipe && test -p /tmp/mypipe && echo yes").await;
        assert_eq!(result.stdout.trim(), "yes");
    }

    // /dev/urandom integration tests

    #[tokio::test]
    async fn test_od_dev_urandom() {
        let result = run_script("od -An -N8 -tx1 /dev/urandom").await;
        assert_eq!(result.exit_code, 0);
        // Should produce hex output - 8 bytes = 8 hex pairs
        let trimmed = result.stdout.trim();
        assert!(!trimmed.is_empty(), "od /dev/urandom should produce output");
    }

    #[tokio::test]
    async fn test_dev_urandom_read_succeeds() {
        // Reading /dev/urandom should succeed (not error with "file not found")
        let result = run_script("cat /dev/urandom > /dev/null && echo ok").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "ok");
    }

    #[tokio::test]
    async fn test_dev_urandom_input_redirect() {
        // Input redirect from /dev/urandom should provide data to stdin
        let result = run_script("od -An -N4 -tx1 < /dev/urandom").await;
        assert_eq!(result.exit_code, 0);
        assert!(
            !result.stdout.trim().is_empty(),
            "should produce hex output"
        );
    }

    #[tokio::test]
    async fn test_dev_random_also_works() {
        let result = run_script("od -An -N4 -tx1 /dev/random").await;
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.trim().is_empty());
    }

    // find -printf tests

    #[tokio::test]
    async fn test_find_printf_filename() {
        let result = run_script(
            r#"mkdir -p /tmp/fp1 && touch /tmp/fp1/hello.txt && find /tmp/fp1 -type f -printf '%f\n'"#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello.txt");
    }

    #[tokio::test]
    async fn test_find_printf_path() {
        let result = run_script(
            r#"mkdir -p /tmp/fp2 && touch /tmp/fp2/a.txt && find /tmp/fp2 -type f -printf '%p\n'"#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "/tmp/fp2/a.txt");
    }

    #[tokio::test]
    async fn test_find_printf_size() {
        let result = run_script(
            r#"mkdir -p /tmp/fp3 && echo -n "hello" > /tmp/fp3/five.txt && find /tmp/fp3 -type f -printf '%s\n'"#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "5");
    }

    #[tokio::test]
    async fn test_find_printf_type() {
        let result =
            run_script(r#"mkdir -p /tmp/fp4/sub && find /tmp/fp4 -maxdepth 0 -printf '%y\n'"#)
                .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "d");
    }

    #[tokio::test]
    async fn test_find_printf_combined() {
        let result = run_script(
            r#"mkdir -p /tmp/fp5 && touch /tmp/fp5/x.txt && find /tmp/fp5 -type f -printf '%f %y\n'"#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "x.txt f");
    }

    #[tokio::test]
    async fn test_posix_character_class_suffix_remove() {
        // ${x%%[![:space:]]*} should remove from first non-space to end
        let result = run_script(r#"x="  hello world  "; echo "[${x%%[![:space:]]*}]""#).await;
        assert_eq!(
            result.stdout.trim(),
            "[  ]",
            "%%[![:space:]]* should remove from first non-space to end"
        );
    }

    #[tokio::test]
    async fn test_posix_character_class_chained_trim() {
        // Issue #677: [![:space:]] character class in parameter expansion
        // Test the core fix: suffix removal with POSIX classes
        let result = run_script(r#"x="  hello world  "; echo "[${x%%[![:space:]]*}]""#).await;
        assert_eq!(
            result.stdout.trim(),
            "[  ]",
            "%%[![:space:]]* should remove from first non-space to end"
        );
        // Test digit class
        let result = run_script(r#"x="abc123def"; echo "${x%%[[:digit:]]*}""#).await;
        assert_eq!(result.stdout.trim(), "abc");
        // Test alpha class
        let result = run_script(r#"x="123abc"; echo "${x%%[[:alpha:]]*}""#).await;
        assert_eq!(result.stdout.trim(), "123");
    }

    #[tokio::test]
    async fn test_posix_digit_class_in_parameter_expansion() {
        let result = run_script(r#"x="abc123def"; echo "${x%%[[:digit:]]*}""#).await;
        assert_eq!(result.stdout.trim(), "abc");
    }

    #[test]
    fn test_command_not_found_suggestions_use_stable_tie_break() {
        let msg = command_not_found_message("grpe", &["type", "true", "tree", "grep"]);
        assert_eq!(
            msg,
            "bash: grpe: command not found. Did you mean: grep, tree, true?"
        );
    }

    #[tokio::test]
    async fn test_debug_trap() {
        let result = run_script(
            r#"count=0; trap '((count++))' DEBUG; echo a; echo b; trap - DEBUG; echo $count"#,
        )
        .await;
        assert_eq!(result.stdout, "a\nb\n3\n");
    }

    #[tokio::test]
    async fn test_noclobber_prevents_overwrite() {
        let result = run_script(
            r#"echo first > /tmp/test_nc; set -o noclobber; echo second > /tmp/test_nc 2>/dev/null; echo $?; cat /tmp/test_nc"#,
        )
        .await;
        assert_eq!(result.stdout.trim(), "1\nfirst");
    }

    #[tokio::test]
    async fn test_indirect_expansion_array() {
        // Issue #672: ${!ref} should resolve to array's first element
        let result = run_script(r#"arr=(a b c); ref=arr; echo ${!ref}"#).await;
        assert_eq!(result.stdout.trim(), "a");
    }

    #[tokio::test]
    async fn test_noclobber_clobber_override() {
        let result = run_script(
            r#"echo first > /tmp/test_nc2; set -o noclobber; echo second >| /tmp/test_nc2; echo $?; cat /tmp/test_nc2"#,
        )
        .await;
        assert_eq!(result.stdout.trim(), "0\nsecond");
    }

    #[tokio::test]
    async fn test_debug_trap_removal() {
        // After trap - DEBUG, the trap should no longer fire
        let result = run_script(
            r#"count=0; trap '((count++))' DEBUG; echo x; trap - DEBUG; echo y; echo $count"#,
        )
        .await;
        // DEBUG fires before: echo x (1), trap - DEBUG (2)
        // After removal: echo y, echo $count don't trigger
        assert_eq!(result.stdout, "x\ny\n2\n");
    }

    #[tokio::test]
    async fn test_array_join_with_ifs() {
        // Issue #668: ${arr[*]} should join with first char of IFS
        let result = run_script(r#"arr=(a b c); IFS=,; echo "${arr[*]}""#).await;
        assert_eq!(result.stdout.trim(), "a,b,c");
    }

    #[tokio::test]
    async fn test_array_join_with_ifs_at_sign() {
        // ${arr[@]} should NOT use IFS, keeps elements separate
        let result = run_script(r#"arr=(a b c); IFS=,; echo "${arr[@]}""#).await;
        assert_eq!(result.stdout.trim(), "a b c");
    }

    #[tokio::test]
    async fn test_underscore_last_arg() {
        // Issue #668: $_ should track last argument of previous command
        let result = run_script("echo hello; echo $_").await;
        assert_eq!(result.stdout, "hello\nhello\n");
    }

    #[tokio::test]
    async fn test_underscore_no_args() {
        // $_ with no args should be the command name
        let result = run_script("true; echo $_").await;
        assert_eq!(result.stdout.trim(), "true");
    }

    #[tokio::test]
    async fn test_temp_assignment_expansion_order() {
        // Issue #671: args expanded before temporary prefix assignment
        let result = run_script(r#"x=hello; x=world echo $x"#).await;
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_process_sub_multiline() {
        // Issue #666: process substitution should handle multiline output
        let result = run_script(r#"cat <(echo hello; echo world)"#).await;
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_process_sub_echo_e() {
        // Issue #666: echo -e in process substitution
        let result = run_script(r#"cat <(echo -e "a\nb")"#).await;
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_process_sub_output() {
        // Issue #666: output process substitution >(cmd) forwards output
        let result = run_script(r#"echo hello > >(cat)"#).await;
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_process_sub_paste() {
        // Issue #666: paste with multiline process substitutions
        let result = run_script(r#"paste <(echo -e "a\nb") <(echo -e "1\n2")"#).await;
        assert_eq!(result.stdout, "a\t1\nb\t2\n");
    }
}
