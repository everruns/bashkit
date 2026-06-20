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

mod arithmetic;
mod brace_expansion;
mod expansion;
mod glob;
mod jobs;
mod redirection;
mod state;

#[allow(unused_imports)]
pub use jobs::{JobTable, SharedJobTable};
pub use state::{BuiltinSideEffect, ControlFlow, ExecResult};
// Re-export snapshot type for public API

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

/// Monotonic counter for unique process substitution file paths
static PROC_SUB_COUNTER: AtomicU64 = AtomicU64::new(0);

// Important decision: report a bash-compatible version surface instead of the
// bashkit crate semver so scripts that gate on Bash features keep working.
const COMPAT_BASH_VERSION: &str = "5.2.15(1)-release";
const COMPAT_BASH_VERSINFO: [&str; 6] = ["5", "2", "15", "1", "release", "virtual"];
// Important decision: lexer emits these only for mixed words where an initial
// quoted segment is followed by an unquoted expansion. Keep them internal and
// strip before observable output.
const QUOTED_SEGMENT_START: char = '\x01';
const QUOTED_SEGMENT_END: char = '\x02';

// Important decision: operand quote sentinels must be selected from a small,
// parser-inert set. Exhaustive Unicode probing is attacker-amplifiable CPU work.
const OPERAND_QUOTE_MARK_CANDIDATES: &[char] = &[
    '\u{E000}', '\u{E001}', '\u{E002}', '\u{E003}', '\u{E004}', '\u{E005}', '\u{E006}', '\u{E007}',
    '\u{E008}', '\u{E009}', '\u{E00A}', '\u{E00B}', '\u{E00C}', '\u{E00D}', '\u{E00E}', '\u{E00F}',
    '\u{FDD0}', '\u{FDD1}', '\u{FDD2}', '\u{FDD3}', '\u{FDD4}', '\u{FDD5}', '\u{FDD6}', '\u{FDD7}',
    '\u{FDD8}', '\u{FDD9}', '\u{FDDA}', '\u{FDDB}', '\u{FDDC}', '\u{FDDD}', '\u{FDDE}', '\u{FDDF}',
];

use futures_util::FutureExt;

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

impl HistoryEntry {
    fn retained_bytes(&self) -> usize {
        self.command.len().saturating_add(self.cwd.len())
    }
}

fn format_history_entries(entries: &[HistoryEntry]) -> String {
    let mut content = String::new();
    for entry in entries {
        use std::fmt::Write;
        let _ = writeln!(
            content,
            "{}|{}|{}|{}|{}",
            entry.timestamp, entry.exit_code, entry.duration_ms, entry.cwd, entry.command
        );
    }
    content
}

/// Runtime command surface for an interpreter instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ShellProfile {
    /// Full Bashkit shell with VFS-backed commands.
    #[default]
    Full,
    /// Logic-only shell for ScriptedTool code mode: no filesystem primitives.
    LogicOnly,
}

impl ShellProfile {
    pub(crate) fn is_logic_only(self) -> bool {
        self == Self::LogicOnly
    }
}

fn logic_only_builtin_allowed(name: &str) -> bool {
    matches!(
        name,
        // Core shell/data flow
        "echo"
            | "true"
            | "false"
            | "exit"
            | "break"
            | "continue"
            | "return"
            | "test"
            | "["
            | "printf"
            | "export"
            | "read"
            | "set"
            | "unset"
            | "shift"
            | "local"
            | ":"
            | "readonly"
            | "times"
            | "eval"
            // Text and data transforms that work from stdin
            | "grep"
            | "sed"
            | "awk"
            | "head"
            | "tail"
            | "sort"
            | "uniq"
            | "cut"
            | "tr"
            | "wc"
            | "nl"
            | "paste"
            | "column"
            | "comm"
            | "strings"
            | "tac"
            | "rev"
            | "fold"
            | "expand"
            | "unexpand"
            | "join"
            | "split"
            | "jq"
            | "seq"
            | "expr"
            | "bc"
            | "numfmt"
            // Shell state, introspection, and structured transforms
            | "env"
            | "printenv"
            | "type"
            | "which"
            | "hash"
            | "alias"
            | "unalias"
            | "trap"
            | "caller"
            | "mapfile"
            | "readarray"
            | "shopt"
            | "clear"
            | "envsubst"
            | "assert"
            | "log"
            | "retry"
            | "semver"
            | "verify"
            | "compgen"
            | "csv"
            | "help"
            | "iconv"
            | "json"
            | "parallel"
            | "template"
            | "tomlq"
            | "yaml"
            | "timeout"
            | "xargs"
            | "wait"
    )
}

fn word_literal_text(word: &Word) -> Option<&str> {
    if word.parts.len() == 1
        && let WordPart::Literal(s) = &word.parts[0]
    {
        return Some(s);
    }
    None
}

fn word_has_process_substitution(word: &Word) -> bool {
    word.parts
        .iter()
        .any(|part| matches!(part, WordPart::ProcessSubstitution { .. }))
}

fn word_is_literal_dev_null(word: &Word) -> bool {
    word_literal_text(word) == Some(DEV_NULL)
}

fn redirect_target_label(word: &Word) -> String {
    word_literal_text(word)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| word.to_string())
}

fn compat_bash_versinfo_array() -> HashMap<usize, String> {
    COMPAT_BASH_VERSINFO
        .iter()
        .enumerate()
        .map(|(idx, value)| (idx, (*value).to_string()))
        .collect()
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
    /// Direct mutable access to shell aliases. Backed by `Arc::make_mut` of
    /// the parent's Arc-wrapped aliases map.
    pub(crate) aliases: &'a mut HashMap<String, String>,
    /// Direct mutable access to trap handlers.
    pub(crate) traps: &'a mut HashMap<String, String>,
    /// Variable attribute table (readonly/integer/lower/upper). Mutable so
    /// `readonly`/`declare`/`unset` builtins can update attributes without
    /// re-allocating `_READONLY_X`-style marker strings.
    pub(crate) var_attrs: &'a mut HashMap<String, VarAttrs>,
    /// Nameref bindings (`declare -n`). Mutable so `unset -n` can clear them.
    pub(crate) namerefs: &'a mut HashMap<String, String>,
    /// Directory stack for `pushd`/`popd`/`dirs`. Direct mutable access backed
    /// by `Arc::make_mut` of the parent's Arc-wrapped stack.
    pub(crate) dir_stack: &'a mut Vec<String>,
    /// Registered builtin commands (read-only, accessed via `has_builtin`).
    pub(crate) builtins: &'a HashMap<String, Arc<dyn Builtin>>,
    /// Host-owned builtin registry, when configured (read-only). Needed so
    /// introspection builtins (`compgen -b`) list host-registered commands
    /// alongside baked-in ones, matching `Bash::builtin_names()`.
    pub(crate) host_builtins: Option<&'a crate::builtins::BuiltinRegistry>,
    /// Defined shell functions (read-only, accessed via `has_function`).
    pub(crate) functions: &'a HashMap<String, FunctionDef>,
    /// Call stack frames (read-only, accessed via `call_stack_depth`/`call_stack_frame_name`).
    call_stack: &'a [CallFrame],
    /// Command history (read-only, accessed via `history_entries`).
    pub(crate) history: &'a [HistoryEntry],
    /// Execution limits used by read-only builtins to avoid unbounded formatting.
    limits: &'a ExecutionLimits,
    /// Shared job table (read-only, accessed via `jobs`).
    pub(crate) jobs: &'a SharedJobTable,
    /// Typed per-execution extensions for the current `exec*()` call.
    pub(crate) execution_extensions: Arc<builtins::ExecutionExtensions>,
}

// Interpreter-dispatched "special" builtins, listed here so the public
// inventory and special-builtin dispatch share one source of truth. Some names
// (e.g. `eval`, `local`, `unset`) are also in the registered builtin map, but
// the interpreter dispatches them ahead of it because they need parser/
// interpreter state; others (e.g. `bash`, `command`, `exec`, `getopts`) live
// only here. Listing every name guarantees inventory completeness regardless of
// map membership.
const SPECIAL_BUILTIN_NAMES: &[&str] = &[
    ".", "bash", "command", "declare", "eval", "exec", "getopts", "let", "local", "sh", "source",
    "typeset", "unset",
];

/// Sorted, deduped union of baked-in/custom builtins, interpreter-special
/// builtins, and the host registry.
fn merged_builtin_names(
    builtins: &HashMap<String, Arc<dyn Builtin>>,
    host_builtins: Option<&crate::builtins::BuiltinRegistry>,
) -> Vec<String> {
    let mut names: Vec<String> = builtins.keys().cloned().collect();
    names.extend(SPECIAL_BUILTIN_NAMES.iter().map(|name| (*name).to_string()));
    if let Some(reg) = host_builtins {
        names.extend(reg.names());
    }
    names.sort();
    names.dedup();
    names
}

impl ShellRef<'_> {
    /// Get execution limits visible to read-only builtins.
    pub(crate) fn limits(&self) -> &ExecutionLimits {
        self.limits
    }

    /// Check if a name is a registered builtin command.
    pub(crate) fn has_builtin(&self, name: &str) -> bool {
        self.builtins.contains_key(name)
    }

    /// Sorted names of all dispatchable builtins (registered + special + host
    /// registry) — same contract as [`crate::Bash::builtin_names`].
    pub(crate) fn builtin_names(&self) -> Vec<String> {
        merged_builtin_names(self.builtins, self.host_builtins)
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

    /// Check if a variable is marked readonly via the attribute table.
    pub(crate) fn is_var_readonly(&self, name: &str) -> bool {
        self.var_attrs
            .get(name)
            .copied()
            .unwrap_or_default()
            .contains(VarAttrs::READONLY)
    }

    /// Mark a variable as readonly. The ShellRef already holds a `&mut HashMap`
    /// borrowed via `Arc::make_mut` from the interpreter, so this touches the
    /// HashMap directly with no extra refcount work.
    pub(crate) fn mark_var_readonly(&mut self, name: &str) {
        let entry = self.var_attrs.entry(name.to_string()).or_default();
        entry.insert(VarAttrs::READONLY);
    }

    /// Iterator over names of variables currently marked readonly. Used by
    /// `readonly -p` to render the marker list without scanning `variables`
    /// for legacy `_READONLY_X` prefixes.
    pub(crate) fn readonly_names(&self) -> impl Iterator<Item = &str> {
        self.var_attrs
            .iter()
            .filter(|(_, attrs)| attrs.contains(VarAttrs::READONLY))
            .map(|(name, _)| name.as_str())
    }
}

pub(crate) struct ExecutionExtensionsGuard {
    slot: Arc<StdMutex<Arc<builtins::ExecutionExtensions>>>,
    previous: Option<Arc<builtins::ExecutionExtensions>>,
}

impl Drop for ExecutionExtensionsGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            *self
                .slot
                .lock()
                .expect("interpreter execution extensions lock") = previous;
        }
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
        #[cfg(not(feature = "ssh"))]
        "ssh" | "scp" | "sftp" => {
            Some("SSH requires the 'ssh' feature. Enable with: features = [\"ssh\"]")
        }
        "rsync" => Some("Network access is limited to curl/wget."),
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

/// Decode file bytes for String-backed interpreter paths. Prefer valid UTF-8
/// so scripts and text files keep Unicode intact; force the existing Latin-1
/// byte model for random devices, and use it as a fallback for other non-UTF-8
/// data that cannot be represented as text without replacement.
fn decode_file_bytes(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .unwrap_or_else(|_| latin1_bytes_to_string(bytes))
}

fn normalize_vfs_path(path: &Path) -> std::path::PathBuf {
    path.components()
        .fold(std::path::PathBuf::new(), |mut acc, c| match c {
            std::path::Component::ParentDir => {
                acc.pop();
                acc
            }
            std::path::Component::CurDir => acc,
            c => {
                acc.push(c);
                acc
            }
        })
}

fn decode_file_bytes_for_path(path: &Path, bytes: &[u8]) -> String {
    let normalized = normalize_vfs_path(path);
    if normalized == Path::new("/dev/urandom") || normalized == Path::new("/dev/random") {
        latin1_bytes_to_string(bytes)
    } else {
        decode_file_bytes(bytes)
    }
}

fn latin1_bytes_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Check if a path refers to /dev/null after normalization.
/// Handles attempts to bypass via paths like `/dev/../dev/null`.
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
/// Note: `_TTY_` is intentionally excluded — it is user-configurable (bashkit extension).
pub(crate) fn is_internal_variable(name: &str) -> bool {
    name.starts_with("SHOPT_")
        || name.starts_with("_NAMEREF_")
        || name.starts_with("_READONLY_")
        || name.starts_with("_UPPER_")
        || name.starts_with("_LOWER_")
        || name.starts_with("_INTEGER_")
        || name.starts_with("_ARRAY_READ_")
        || name == "_SHIFT_COUNT"
        || name == "_SET_POSITIONAL"
}

/// THREAT[TM-INF-017]: Check if a variable should be hidden from user-visible output.
/// Superset of `is_internal_variable()` — also includes `_TTY_` which is user-settable
/// but should not appear in `set`, `declare -p`, or environment exports.
pub(crate) fn is_hidden_variable(name: &str) -> bool {
    is_internal_variable(name) || name.starts_with("_TTY_")
}

/// THREAT[TM-DOS-090]: Nameref targets are script-controlled. Only treat a
/// resolved target as an embedded array element when it is exactly `name[index]`;
/// malformed strings containing `[` must remain ordinary names, never sliced.
fn parse_embedded_array_ref(resolved_name: &str) -> Option<(&str, &str)> {
    let (arr_name, rest) = resolved_name.split_once('[')?;
    let idx_part = rest.strip_suffix(']')?;
    if !is_valid_var_name(arr_name) || idx_part.contains('[') || idx_part.contains(']') {
        return None;
    }
    Some((arr_name, idx_part))
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
    /// Indexed arrays shadowed by local declarations in this scope.
    local_arrays: HashMap<String, Option<HashMap<usize, String>>>,
    /// Associative arrays shadowed by local declarations in this scope.
    local_assoc_arrays: HashMap<String, Option<HashMap<String, String>>>,
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
    /// Variable attribute bitset per variable name.
    ///
    /// Serialized as raw bits for forward/backward compatibility.
    #[serde(default)]
    pub var_attrs: HashMap<String, u8>,
    /// Nameref bindings (`declare -n`): name -> target variable name.
    #[serde(default)]
    pub namerefs: HashMap<String, String>,
    /// Indexed arrays
    pub arrays: HashMap<String, HashMap<usize, String>>,
    /// Associative arrays
    pub assoc_arrays: HashMap<String, HashMap<String, String>>,
    /// Current working directory
    pub cwd: PathBuf,
    /// Last exit code
    pub last_exit_code: i32,
    /// PID/job id of the most recent background command, surfaced as `$!`.
    /// `Option` so older snapshots without the field deserialize cleanly.
    #[serde(default)]
    pub last_bg_pid: Option<String>,
    /// Defined shell functions
    #[serde(
        default,
        serialize_with = "serialize_snapshotted_functions",
        deserialize_with = "deserialize_snapshotted_functions"
    )]
    pub functions: HashMap<String, FunctionDef>,
    /// Shell aliases
    pub aliases: HashMap<String, String>,
    /// Trap handlers
    pub traps: HashMap<String, String>,
    /// Directory stack (`pushd`/`popd`/`dirs`); bottom-to-top, excluding `cwd`.
    #[serde(default)]
    pub dir_stack: Vec<String>,
}

/// Lightweight inspection view of shell state.
///
/// Omits AST-backed function definitions so prompt rendering and other UI-only
/// inspection paths don't pay to clone data they never expose or restore.
#[derive(Debug, Clone, Default)]
pub struct ShellStateView {
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct ShellStateOptions {
    pub(crate) include_functions: bool,
}

impl Default for ShellStateOptions {
    fn default() -> Self {
        Self {
            include_functions: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshottedFunction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ast: Option<FunctionDef>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum SnapshottedFunctionRepr {
    Snapshot(SnapshottedFunction),
    Legacy(FunctionDef),
}

fn serialize_snapshotted_functions<S>(
    functions: &HashMap<String, FunctionDef>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let snapshotted: HashMap<String, SnapshottedFunction> = functions
        .iter()
        .map(|(name, func)| {
            let mut ast = func.clone();
            ast.source = None;
            (
                name.clone(),
                SnapshottedFunction {
                    source: func.source.clone(),
                    ast: Some(ast),
                },
            )
        })
        .collect();
    serde::Serialize::serialize(&snapshotted, serializer)
}

fn deserialize_snapshotted_functions<'de, D>(
    deserializer: D,
) -> std::result::Result<HashMap<String, FunctionDef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let snapshotted =
        <HashMap<String, SnapshottedFunctionRepr> as serde::Deserialize>::deserialize(
            deserializer,
        )?;
    snapshotted
        .into_iter()
        .map(|(name, repr)| {
            let func = match repr {
                SnapshottedFunctionRepr::Legacy(func) => func,
                SnapshottedFunctionRepr::Snapshot(snapshot) => {
                    match (snapshot.ast, snapshot.source) {
                        (Some(mut func), source) => {
                            if func.source.is_none() {
                                func.source = source;
                            }
                            func
                        }
                        (None, Some(source)) => deserialize_function_from_source(&name, &source)
                            .map_err(serde::de::Error::custom)?,
                        (None, None) => {
                            return Err(serde::de::Error::custom(format!(
                                "snapshot function '{name}' missing both ast and source"
                            )));
                        }
                    }
                }
            };
            if func.name != name {
                return Err(serde::de::Error::custom(format!(
                    "snapshot function key '{name}' does not match parsed name '{}'",
                    func.name
                )));
            }
            Ok((name, func))
        })
        .collect()
}

fn deserialize_function_from_source_with_limits(
    name: &str,
    source: &str,
    max_ast_depth: usize,
    max_parser_operations: usize,
) -> std::result::Result<FunctionDef, String> {
    let script = Parser::with_limits(source, max_ast_depth, max_parser_operations)
        .parse()
        .map_err(|err| format!("failed to parse function '{name}' from source: {err}"))?;
    let mut commands = script.commands.into_iter();
    let command = commands.next().ok_or_else(|| {
        format!("failed to parse function '{name}' from source: missing function command")
    })?;
    if commands.next().is_some() {
        return Err(format!(
            "failed to parse function '{name}' from source: expected exactly one command"
        ));
    }
    match command {
        Command::Function(mut func) => {
            func.source = Some(source.to_string());
            Ok(func)
        }
        other => Err(format!(
            "failed to parse function '{name}' from source: expected function definition, got {other:?}"
        )),
    }
}

fn deserialize_function_from_source(
    name: &str,
    source: &str,
) -> std::result::Result<FunctionDef, String> {
    deserialize_function_from_source_with_limits(name, source, 100, 100_000)
}

fn function_storage_bytes(func: &FunctionDef) -> usize {
    func.source.as_ref().map_or_else(
        || func.span.end.offset.saturating_sub(func.span.start.offset),
        |source| source.len(),
    )
}

// Important decision: variable attributes (readonly/integer/lower/upper) and
// namerefs are stored in dedicated maps rather than the `variables` HashMap with
// `_READONLY_X` / `_INTEGER_X` / `_LOWER_X` / `_UPPER_X` / `_NAMEREF_X` keys.
// The legacy format!()-based marker scheme allocated 4-5 Strings per assignment
// and per attribute read; the bitset/map approach removes those allocations
// from the hot path. `is_internal_variable` no longer needs to filter these
// prefixes because they never enter `variables` at runtime.
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct VarAttrs: u8 {
        const READONLY = 0b0000_0001;
        const INTEGER  = 0b0000_0010;
        const LOWER    = 0b0000_0100;
        const UPPER    = 0b0000_1000;
        const EXPORT   = 0b0001_0000;
    }
}

// Important decision: shell option flags (set -e, set -u, set -x, set -o
// pipefail, etc.) are cached in a bitfield in addition to the SHOPT_X entries
// in `variables`. Hot-path checks (errexit after every command, nounset on
// every $VAR, etc.) read the bitfield directly instead of doing a HashMap
// lookup + string compare. Writes go through `set_shopt_flag` which keeps
// the bitfield and the SHOPT_X variable in sync.
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct BashFlags: u16 {
        const ERREXIT      = 0b0000_0000_0000_0001; // set -e / SHOPT_e
        const XTRACE       = 0b0000_0000_0000_0010; // set -x / SHOPT_x
        const NOUNSET      = 0b0000_0000_0000_0100; // set -u / SHOPT_u
        const NOGLOB       = 0b0000_0000_0000_1000; // set -f / SHOPT_f
        const VERBOSE      = 0b0000_0000_0001_0000; // set -v / SHOPT_v
        const ALLEXPORT    = 0b0000_0000_0010_0000; // set -a / SHOPT_a
        const NOEXEC       = 0b0000_0000_0100_0000; // set -n / SHOPT_n
        const NOCLOBBER    = 0b0000_0000_1000_0000; // set -C / SHOPT_C
        const PIPEFAIL     = 0b0000_0001_0000_0000; // set -o pipefail / SHOPT_pipefail
        const EXPAND_ALIAS = 0b0000_0010_0000_0000; // shopt expand_aliases
    }
}

impl BashFlags {
    /// Map a shell option variable name to its flag bit (None if unknown).
    fn from_shopt_name(name: &str) -> Option<Self> {
        match name {
            "SHOPT_e" => Some(Self::ERREXIT),
            "SHOPT_x" => Some(Self::XTRACE),
            "SHOPT_u" => Some(Self::NOUNSET),
            "SHOPT_f" => Some(Self::NOGLOB),
            "SHOPT_v" => Some(Self::VERBOSE),
            "SHOPT_a" => Some(Self::ALLEXPORT),
            "SHOPT_n" => Some(Self::NOEXEC),
            "SHOPT_C" => Some(Self::NOCLOBBER),
            "SHOPT_pipefail" => Some(Self::PIPEFAIL),
            "SHOPT_expand_aliases" => Some(Self::EXPAND_ALIAS),
            _ => None,
        }
    }
}

/// CoW-snapshotted scoped shell state.
///
/// Groups the maps captured at every `$(...)` / arithmetic-substitution and
/// subshell boundary. Each field is `Arc<HashMap>`, so cloning this struct is
/// an O(1) refcount bump per field — a whole-state snapshot is one
/// `ScopedState::clone()`. Mutations go through the `*_mut` accessors on
/// [`Interpreter`], which `Arc::make_mut` (paying one deep clone only when a
/// snapshot is still live).
#[derive(Clone, Default)]
struct ScopedState {
    /// Shell variables.
    variables: Arc<HashMap<String, String>>,
    /// Variable attribute flags (readonly/integer/lower/upper/export), keyed by
    /// the *resolved* (post-nameref) variable name. Empty entry == no attrs.
    var_attrs: Arc<HashMap<String, VarAttrs>>,
    /// Nameref bindings (`declare -n`): name -> target variable name.
    namerefs: Arc<HashMap<String, String>>,
    /// Indexed arrays: name -> index -> value.
    arrays: Arc<HashMap<String, HashMap<usize, String>>>,
    /// Associative arrays (`declare -A`): name -> key -> value.
    assoc_arrays: Arc<HashMap<String, HashMap<String, String>>>,
    /// Defined shell functions.
    functions: Arc<HashMap<String, FunctionDef>>,
    /// Trap handlers: signal/event name -> command string.
    traps: Arc<HashMap<String, String>>,
    /// Shell aliases: name -> expansion value.
    aliases: Arc<HashMap<String, String>>,
    /// Directory stack for `pushd`/`popd`/`dirs`. Index 0 is the bottom of the
    /// stack; the top (most recently pushed) is last. The current directory is
    /// `cwd`, not part of this vec. Previously stored as `_DIRSTACK_*` shell
    /// variables; now typed state so it can't be forged from a script.
    dir_stack: Arc<Vec<String>>,
}

/// All interpreter state mutated inside a `$(...)` / arithmetic substitution
/// subshell. Captured before the substitution runs and restored after, so
/// mutations don't leak to the parent. The [`ScopedState`] snapshot is O(1)
/// (per-field refcount bumps); only mutations inside the subshell pay a clone
/// (`Arc::make_mut`). For substitutions that don't mutate state at all (the
/// common case — `$(echo $x)`, command queries) this saves an entire deep
/// HashMap clone per substitution.
struct SubshellSnapshot {
    scoped: ScopedState,
    flags: BashFlags,
    cwd: PathBuf,
    memory_budget: crate::limits::MemoryBudget,
    exec_fd_table: HashMap<i32, FdTarget>,
    random_state: u32,
    getopts_char_idx: usize,
    last_bg_pid: Option<String>,
}

/// Interpreter state.
pub struct Interpreter {
    fs: Arc<dyn FileSystem>,
    env: HashMap<String, String>,
    // Important decision: the maps that get snapshotted by subshell ($(...))
    // boundaries live in `scoped: ScopedState`, whose fields are `Arc<HashMap>`
    // so the snapshot is an O(1) refcount bump instead of an O(n) HashMap clone.
    // Mutations go through `vars_mut()` / `arrays_mut()` / etc. which call
    // `Arc::make_mut`; when the refcount is 1 (no live snapshot) this is just a
    // `&mut` borrow with zero clone cost. When the refcount is 2 (a subshell is
    // active and mutates), it pays one clone — the same cost the eager-clone
    // scheme paid unconditionally, but only when actually needed.
    scoped: ScopedState,
    /// Cached shell option flags. Synchronized with `SHOPT_*` entries in
    /// `variables` via `set_shopt_flag` / `set_shopt_value`.
    flags: BashFlags,
    cwd: PathBuf,
    last_exit_code: i32,
    /// Built-in commands (default + custom).
    ///
    /// Stored as `Arc` so the dispatcher can clone the handle out of the map
    /// and execute the builtin without keeping a borrow on `self.builtins`,
    /// which lets it freely take `&mut self` for `self.scoped.variables`,
    /// `self.cwd`, etc. during execution.
    builtins: HashMap<String, Arc<dyn Builtin>>,
    /// Optional host-owned mutable registry. Consulted after shell functions
    /// and special builtins but before `builtins` — so host entries can
    /// override baked-in commands. Survives `reset_transient_state` because
    /// it lives behind an `Arc<RwLock>` shared with the embedder.
    host_builtins: Option<crate::builtins::BuiltinRegistry>,
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
    git_client: Option<crate::builtins::git::GitClient>,
    /// SSH client for ssh/scp/sftp builtins
    #[cfg(feature = "ssh")]
    ssh_client: Option<crate::builtins::ssh::SshClient>,
    /// Stdin inherited from pipeline for compound commands (while read, etc.)
    /// Each read operation consumes one line, advancing through the data.
    pipeline_stdin: Option<String>,
    /// Position within the current argument while `getopts` walks a clustered
    /// short-option group (e.g. `-abc`). Interpreter-internal working state for
    /// `execute_getopts`; `0` means "at the start of the next option group".
    /// Previously stored in the user variable namespace as `_OPTCHAR_IDX`.
    getopts_char_idx: usize,
    /// Sandboxed PID/job id of the most recent background command, surfaced as
    /// `$!`. Interpreter-internal state (not a host PID); subshell-isolated like
    /// any other shell state. Previously stored as the `_LAST_BG_PID` variable.
    last_bg_pid: Option<String>,
    /// Optional callback for streaming output chunks during execution.
    /// When set, output is emitted incrementally via this callback in addition
    /// to being accumulated in the returned ExecResult.
    output_callback: Option<OutputCallback>,
    /// Typed per-execution extensions visible to builtins for the current
    /// `exec*()` call. Stored behind a mutex so drop guards can restore it
    /// without borrowing the interpreter across `.await`.
    execution_extensions: Arc<StdMutex<Arc<builtins::ExecutionExtensions>>>,
    /// Monotonic counter incremented each time output is emitted via callback.
    /// Used to detect whether sub-calls already emitted output, preventing duplicates.
    output_emit_count: u64,
    /// Bytes already delivered to streaming output callbacks for this execution.
    /// Mirrors ExecResult caps so live consumers cannot bypass output limits.
    output_stream_stdout_bytes: usize,
    output_stream_stderr_bytes: usize,
    /// Pending nounset (set -u) error message, consumed by execute_command.
    nounset_error: Option<String>,
    /// PIPESTATUS: exit codes of the last pipeline's commands
    pipestatus: Vec<i32>,
    /// Aliases currently being expanded (prevents infinite recursion).
    /// When alias `foo` expands to `foo bar`, the inner `foo` is not re-expanded.
    expanding_aliases: HashSet<String>,
    /// Command history entries for the current session.
    history: Vec<HistoryEntry>,
    /// Retained command/cwd bytes for bounded history accounting.
    history_bytes: usize,
    /// Number of retained entries already flushed to the VFS history file.
    history_saved_entries: usize,
    /// Whether the VFS history file needs compaction after trimming or clearing.
    history_needs_rewrite: bool,
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
    /// Persistent fd output table set by `exec N>/path` redirections.
    /// Maps fd number to its output target. Used by `>&N` redirections.
    exec_fd_table: HashMap<i32, FdTarget>,
    /// Temporary buffer for fd3+ output during compound body execution.
    /// Populated by `1>&N` (N>=3) in apply_redirections, consumed by
    /// apply_redirections_fd_table for compound redirect routing.
    pending_fd_output: HashMap<i32, String>,
    /// Fd3+ targets from compound redirect processing (e.g. `3>&1` maps fd3→Stdout).
    /// Populated during apply_redirections_fd_table redirect loop, consumed during routing.
    pending_fd_targets: Vec<(i32, FdTarget)>,
    /// Depth counter for compound execution contexts that need fd3+ buffering.
    /// Only when >0 should `1>&N` (N>=3) output be captured in pending_fd_output.
    pending_fd_capture_depth: usize,
    /// Cancellation token: when set to `true`, execution aborts at the next
    /// command boundary with `Error::Cancelled`.
    cancelled: Arc<AtomicBool>,
    /// Interceptor hooks registry (shared with Bash callers).
    hooks: crate::hooks::Hooks,
    /// True while executing a trap handler. Suppresses recursive DEBUG trap
    /// invocation to prevent amplification attacks (TM-DOS-035).
    in_trap: bool,
    /// Depth of if/while/until condition evaluation.
    /// Important decision: condition context is tracked as interpreter state so
    /// nested AND-OR lists can suppress ERR traps without weakening top-level
    /// final-command errexit behavior.
    condition_sequence_depth: usize,
    /// Deferred output process substitutions: after a command writes to the
    /// virtual file path, run these commands with the file content as stdin.
    /// Each entry is (virtual_path, commands_to_run).
    deferred_proc_subs: Vec<(String, Vec<Command>)>,
    /// Process substitution paths created by this interpreter instance.
    /// Used to avoid deleting paths owned by other sessions sharing the same VFS.
    proc_sub_paths: HashSet<String>,
    /// PRNG state for $RANDOM (LCG seeded per-instance from OS entropy).
    /// NOT cryptographically secure — matches real bash behavior.
    /// Uses `AtomicU32` for interior mutability so $RANDOM can advance state
    /// in `expand_variable(&self, ...)` while remaining `Send + Sync`.
    random_state: AtomicU32,
    /// Runtime command surface. ScriptedTool uses LogicOnly to prevent scripts
    /// from reaching VFS-backed commands while preserving shell logic.
    shell_profile: ShellProfile,
}

struct ArithmeticExpansionState {
    resolving_vars: Vec<String>,
    fuel: usize,
}

impl ArithmeticExpansionState {
    fn new(fuel: usize) -> Self {
        Self {
            resolving_vars: Vec::new(),
            fuel,
        }
    }

    fn spend(&mut self, amount: usize) -> bool {
        if self.fuel < amount {
            return false;
        }
        self.fuel -= amount;
        true
    }

    fn enter_var(&mut self, name: &str) -> bool {
        if self.resolving_vars.iter().any(|var| var == name) {
            return false;
        }
        self.resolving_vars.push(name.to_string());
        true
    }

    fn exit_var(&mut self) {
        self.resolving_vars.pop();
    }
}

impl Interpreter {
    fn utf8_prefix_at_most(s: &str, max_bytes: usize) -> &str {
        if s.len() <= max_bytes {
            return s;
        }
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }

    const MAX_GLOB_DEPTH: usize = 50;

    /// Create a new interpreter with the given filesystem.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self::with_config(
            fs,
            None,
            None,
            None,
            None,
            HashMap::new(),
            None,
            ShellProfile::Full,
        )
    }

    /// Create a new interpreter with custom username, hostname, and builtins.
    ///
    /// # Arguments
    ///
    /// * `fs` - The virtual filesystem to use
    /// * `username` - Optional custom username for virtual identity
    /// * `hostname` - Optional custom hostname for virtual identity
    /// * `custom_builtins` - Custom builtins to register (override defaults if same name)
    #[allow(clippy::too_many_arguments)]
    pub fn with_config(
        fs: Arc<dyn FileSystem>,
        username: Option<String>,
        hostname: Option<String>,
        fixed_epoch: Option<i64>,
        epoch_offset: Option<i64>,
        custom_builtins: HashMap<String, Box<dyn Builtin>>,
        host_builtins: Option<crate::builtins::BuiltinRegistry>,
        shell_profile: ShellProfile,
    ) -> Self {
        // Macro to reduce boilerplate for simple zero-arg builtin registration.
        // Custom-construction builtins (date, source, hostname, etc.) are registered below.
        macro_rules! register_builtins {
            ($map:ident, $( $name:literal => $type:ident ),+ $(,)?) => {
                $( $map.insert($name.to_string(), Arc::new(builtins::$type) as Arc<dyn Builtin>); )+
            };
        }

        let mut builtins: HashMap<String, Arc<dyn Builtin>> = HashMap::new();

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
            "truncate" => Truncate,
            "shuf" => Shuf,
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

        // jq builtin (requires jq feature)
        #[cfg(feature = "jq")]
        builtins.insert("jq".to_string(), Arc::new(builtins::Jq));

        // Custom-construction builtins that need parameters

        // source/. requires filesystem access
        builtins.insert(
            "source".to_string(),
            Arc::new(builtins::Source::new(fs.clone())),
        );
        builtins.insert(".".to_string(), Arc::new(builtins::Source::new(fs.clone())));

        // THREAT[TM-INF-018]: Resolve the virtual clock mode for `date`.
        // Priority: fixed_epoch > epoch_offset > real clock.
        builtins.insert(
            "date".to_string(),
            Arc::new(if let Some(epoch) = fixed_epoch {
                use chrono::DateTime;
                builtins::Date::with_fixed_epoch(
                    DateTime::from_timestamp(epoch, 0).unwrap_or_default(),
                )
            } else if let Some(offset) = epoch_offset {
                builtins::Date::with_offset_seconds(offset)
            } else {
                builtins::Date::new()
            }),
        );

        // System info builtins (configurable virtual values)
        let hostname_val = hostname.unwrap_or_else(|| builtins::DEFAULT_HOSTNAME.to_string());
        let username_val = username.unwrap_or_else(|| builtins::DEFAULT_USERNAME.to_string());
        builtins.insert(
            "hostname".to_string(),
            Arc::new(builtins::Hostname::with_hostname(&hostname_val)),
        );
        builtins.insert(
            "uname".to_string(),
            Arc::new(builtins::Uname::with_hostname(&hostname_val)),
        );
        builtins.insert(
            "whoami".to_string(),
            Arc::new(builtins::Whoami::with_username(&username_val)),
        );
        builtins.insert(
            "id".to_string(),
            Arc::new(builtins::Id::with_username(&username_val)),
        );

        // Git builtin (requires git feature and configuration at runtime)
        #[cfg(feature = "git")]
        builtins.insert("git".to_string(), Arc::new(builtins::Git));

        // SSH builtins (requires ssh feature and configuration at runtime)
        #[cfg(feature = "ssh")]
        {
            builtins.insert("ssh".to_string(), Arc::new(builtins::Ssh));
            builtins.insert("scp".to_string(), Arc::new(builtins::Scp));
            builtins.insert("sftp".to_string(), Arc::new(builtins::Sftp));
        }

        if shell_profile.is_logic_only() {
            builtins.retain(|name, _| logic_only_builtin_allowed(name));
        }

        // Merge custom builtins (override defaults if same name).
        // `Arc::from(Box<dyn Builtin>)` reuses the existing allocation.
        for (name, builtin) in custom_builtins {
            builtins.insert(name, Arc::from(builtin));
        }

        // Initialize default shell variables
        let mut variables = HashMap::new();
        variables.insert("HOME".to_string(), format!("/home/{}", &username_val));
        variables.insert("USER".to_string(), username_val.clone());
        variables.insert("UID".to_string(), "1000".to_string());
        variables.insert("EUID".to_string(), "1000".to_string());
        variables.insert("PPID".to_string(), "0".to_string());
        variables.insert("HOSTNAME".to_string(), hostname_val.clone());

        // BASH_VERSINFO array: (major minor patch build status machine)
        let mut arrays = HashMap::new();
        arrays.insert("BASH_VERSINFO".to_string(), compat_bash_versinfo_array());

        // Seed PRNG for $RANDOM from OS entropy via RandomState
        let random_seed = {
            use std::collections::hash_map::RandomState;
            use std::hash::{BuildHasher, Hasher};
            RandomState::new().build_hasher().finish() as u32
        };

        Self {
            fs,
            env: HashMap::new(),
            scoped: ScopedState {
                variables: Arc::new(variables),
                arrays: Arc::new(arrays),
                ..Default::default()
            },
            flags: BashFlags::empty(),
            cwd: PathBuf::from("/home/user"),
            last_exit_code: 0,
            builtins,
            host_builtins,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            pipeline_stdin: None,
            getopts_char_idx: 0,
            last_bg_pid: None,
            output_callback: None,
            execution_extensions: Arc::new(StdMutex::new(Arc::new(
                builtins::ExecutionExtensions::new(),
            ))),
            output_emit_count: 0,
            output_stream_stdout_bytes: 0,
            output_stream_stderr_bytes: 0,
            nounset_error: None,
            pipestatus: Vec::new(),
            expanding_aliases: HashSet::new(),
            history: Vec::new(),
            history_bytes: 0,
            history_saved_entries: 0,
            history_needs_rewrite: false,
            history_file: None,
            history_loaded: false,
            subst_generation: 0,
            coproc_buffers: HashMap::new(),
            coproc_next_fd: 63,
            exec_fd_table: HashMap::new(),
            pending_fd_output: HashMap::new(),
            pending_fd_targets: Vec::new(),
            pending_fd_capture_depth: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
            hooks: crate::hooks::Hooks::default(),
            in_trap: false,
            condition_sequence_depth: 0,
            deferred_proc_subs: Vec::new(),
            proc_sub_paths: HashSet::new(),
            random_state: AtomicU32::new(random_seed),
            shell_profile,
        }
    }

    /// Return a shared cancellation token. Set it to `true` from any thread
    /// to abort execution at the next command boundary.
    pub fn cancellation_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Return a reference to the hooks registry.
    pub fn hooks(&self) -> &crate::hooks::Hooks {
        &self.hooks
    }

    pub(crate) fn current_execution_extensions(&self) -> Arc<builtins::ExecutionExtensions> {
        self.execution_extensions
            .lock()
            .expect("interpreter execution extensions lock")
            .clone()
    }

    /// Drop builtin-owned hidden state after snapshot restore.
    ///
    /// Security: snapshots define the shell/VFS boundary. Builtin caches (for
    /// example SQLite engines) must not retain stale state that can be read or
    /// flushed back after the VFS has been restored.
    pub(crate) fn reset_builtin_session_state(&self) {
        for builtin in self.builtins.values() {
            builtin.reset_session_state();
        }
    }

    pub(crate) fn scoped_execution_extensions(
        &self,
        extensions: builtins::ExecutionExtensions,
    ) -> ExecutionExtensionsGuard {
        let previous = {
            let mut slot = self
                .execution_extensions
                .lock()
                .expect("interpreter execution extensions lock");
            std::mem::replace(&mut *slot, Arc::new(extensions))
        };
        ExecutionExtensionsGuard {
            slot: self.execution_extensions.clone(),
            previous: Some(previous),
        }
    }

    /// Replace the hooks registry (called from BashBuilder::build).
    pub(crate) fn set_hooks(&mut self, hooks: crate::hooks::Hooks) {
        self.hooks = hooks;
    }

    // === CoW accessors ===
    // `Arc::make_mut` returns a `&mut HashMap`, cloning the inner map only
    // when the Arc has more than one strong reference (i.e. a live subshell
    // snapshot). In the steady state (refcount==1) this is just a plain
    // mutable borrow with zero clone cost. The compiler can usually inline
    // these into the call site.

    #[inline]
    fn vars_mut(&mut self) -> &mut HashMap<String, String> {
        Arc::make_mut(&mut self.scoped.variables)
    }

    #[inline]
    fn arrays_mut(&mut self) -> &mut HashMap<String, HashMap<usize, String>> {
        Arc::make_mut(&mut self.scoped.arrays)
    }

    #[inline]
    fn assoc_arrays_mut(&mut self) -> &mut HashMap<String, HashMap<String, String>> {
        Arc::make_mut(&mut self.scoped.assoc_arrays)
    }

    #[inline]
    fn functions_mut(&mut self) -> &mut HashMap<String, FunctionDef> {
        Arc::make_mut(&mut self.scoped.functions)
    }

    #[inline]
    fn traps_mut(&mut self) -> &mut HashMap<String, String> {
        Arc::make_mut(&mut self.scoped.traps)
    }

    #[inline]
    fn var_attrs_mut(&mut self) -> &mut HashMap<String, VarAttrs> {
        Arc::make_mut(&mut self.scoped.var_attrs)
    }

    #[inline]
    fn namerefs_mut(&mut self) -> &mut HashMap<String, String> {
        Arc::make_mut(&mut self.scoped.namerefs)
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
        if self.bash_source_stack.is_empty() {
            self.arrays_mut().remove("BASH_SOURCE");
            return;
        }

        let arr: HashMap<usize, String> = self
            .bash_source_stack
            .iter()
            .rev()
            .enumerate()
            .map(|(i, s)| (i, s.clone()))
            .collect();
        self.arrays_mut().insert("BASH_SOURCE".to_string(), arr);
    }

    fn is_errexit_enabled(&self) -> bool {
        self.flags.contains(BashFlags::ERREXIT)
    }

    /// Check if xtrace (set -x) is enabled.
    fn is_xtrace_enabled(&self) -> bool {
        self.flags.contains(BashFlags::XTRACE)
    }

    /// Rehydrate the SHOPT flag cache from any `SHOPT_*` entries currently in
    /// `self.scoped.variables`. Call after bulk-restoring `variables` from a snapshot
    /// or builder so the cache doesn't drift.
    fn refresh_shopt_flags(&mut self) {
        self.flags = BashFlags::empty();
        for (name, value) in self.scoped.variables.iter() {
            if let Some(bit) = BashFlags::from_shopt_name(name)
                && value == "1"
            {
                self.flags.insert(bit);
            }
        }
    }

    // === Variable attribute helpers ===
    // Reading/writing readonly/integer/lower/upper attributes via the
    // dedicated `var_attrs` HashMap. The old `_READONLY_X`/etc. format!()
    // approach has been removed from the hot path; see VarAttrs above.

    fn var_attrs_get(&self, name: &str) -> VarAttrs {
        self.scoped.var_attrs.get(name).copied().unwrap_or_default()
    }

    fn is_var_readonly(&self, name: &str) -> bool {
        self.var_attrs_get(name).contains(VarAttrs::READONLY)
    }

    fn add_var_attr(&mut self, name: &str, attr: VarAttrs) {
        // entry-by-string-slice — only allocates when inserting new entry
        match self.var_attrs_mut().get_mut(name) {
            Some(existing) => existing.insert(attr),
            None => {
                self.var_attrs_mut().insert(name.to_string(), attr);
            }
        }
    }

    fn remove_var_attr(&mut self, name: &str, attr: VarAttrs) {
        if let Some(existing) = self.var_attrs_mut().get_mut(name) {
            existing.remove(attr);
            if existing.is_empty() {
                self.var_attrs_mut().remove(name);
            }
        }
    }

    fn clear_var_attrs(&mut self, name: &str) {
        self.var_attrs_mut().remove(name);
    }

    fn set_nameref(&mut self, name: &str, target: String) {
        self.namerefs_mut().insert(name.to_string(), target);
    }

    fn remove_nameref(&mut self, name: &str) {
        self.namerefs_mut().remove(name);
    }

    /// Set execution limits.
    pub fn set_limits(&mut self, limits: ExecutionLimits) {
        self.limits = limits;
    }

    /// Set session-level limits.
    pub fn set_session_limits(&mut self, limits: SessionLimits) {
        self.session_limits = limits;
    }

    /// Count a host-level Bash::exec invocation before parsing untrusted input.
    pub(crate) fn begin_exec_invocation(&mut self) -> Result<()> {
        self.counters.reset_for_execution();
        self.counters.tick_exec_call();
        self.counters
            .check_session_limits(&self.session_limits)
            .map_err(|e| crate::error::Error::Execution(e.to_string()))
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

    /// `set -o` option variable names (SHOPT_e, SHOPT_x, etc.) that are
    /// transient and must be reset between exec() calls (TM-ISO-023).
    /// `shopt` options (SHOPT_expand_aliases, SHOPT_extglob, etc.) are
    /// persistent session configuration and are NOT reset.
    const SET_OPTION_VARS: &'static [&'static str] = &[
        "SHOPT_a",
        "SHOPT_b",
        "SHOPT_e",
        "SHOPT_f",
        "SHOPT_h",
        "SHOPT_m",
        "SHOPT_n",
        "SHOPT_u",
        "SHOPT_v",
        "SHOPT_x",
        "SHOPT_C",
        "SHOPT_pipefail",
    ];

    /// THREAT[TM-ISO-005/006/007]: Reset per-exec transient state.
    /// Called by Bash::exec() before each top-level execution to prevent
    /// traps, exit code, `set` options, transient stdin, and fd3+ redirect
    /// capture buffers from leaking across calls.
    /// `shopt` options (expand_aliases, extglob, etc.) are intentionally
    /// preserved — they are persistent session configuration.
    pub fn reset_transient_state(&mut self) {
        self.traps_mut().clear();
        self.last_exit_code = 0;
        // THREAT[TM-DOS-035/057]: A timeout can drop execution while a trap
        // handler is awaited; clear the re-entrancy guard before each exec so
        // one cancelled script cannot suppress traps in the next script.
        self.in_trap = false;
        self.condition_sequence_depth = 0;
        self.deferred_proc_subs.clear();
        self.clear_pending_fd_redirect_state();
        // Top-level timeouts drop the interpreter future at await points, so
        // BASH_SOURCE cleanup after script execution may not run. Reset both
        // the private stack and public array before reusing the Bash instance.
        self.bash_source_stack.clear();
        self.arrays_mut().remove("BASH_SOURCE");
        for var in Self::SET_OPTION_VARS {
            self.vars_mut().remove(*var);
            if let Some(bit) = BashFlags::from_shopt_name(var) {
                self.flags.remove(bit);
            }
        }
        self.pipeline_stdin = None;
        self.bash_source_stack.clear();
        self.arrays_mut().remove("BASH_SOURCE");
    }

    pub(crate) fn clear_cancelled_execution_state(&mut self) {
        self.reconcile_cancelled_execution_state(0, 0, 0, None);
    }

    fn clear_pending_fd_redirect_state(&mut self) {
        self.pending_fd_output.clear();
        self.pending_fd_targets.clear();
        self.pending_fd_capture_depth = 0;
    }

    fn append_pending_fd_output(&mut self, fd: i32, data: &str) {
        if data.is_empty() {
            return;
        }
        let used: usize = self.pending_fd_output.values().map(String::len).sum();
        let remaining = self.limits.max_stdout_bytes.saturating_sub(used);
        if remaining == 0 {
            return;
        }
        let entry = self.pending_fd_output.entry(fd).or_default();
        entry.push_str(Self::utf8_prefix_at_most(data, remaining));
    }

    /// Set an environment variable.
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_string(), value.to_string());
    }

    /// Set a shell variable (public API for builder).
    pub fn set_var(&mut self, key: &str, value: &str) {
        if let Some(bit) = BashFlags::from_shopt_name(key) {
            if value == "1" {
                self.flags.insert(bit);
            } else {
                self.flags.remove(bit);
            }
        }
        self.vars_mut().insert(key.to_string(), value.to_string());
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
        self.push_history_entry(HistoryEntry {
            command,
            timestamp,
            cwd,
            exit_code,
            duration_ms,
        });
    }

    fn push_history_entry(&mut self, entry: HistoryEntry) {
        // THREAT[TM-DOS-094]: Long-lived Bash instances retain history across
        // exec() calls. Enforce entry/byte caps before persistence or listing.
        if self.limits.max_history_entries == 0 || self.limits.max_history_bytes == 0 {
            return;
        }

        let entry_bytes = entry.retained_bytes();
        if entry_bytes > self.limits.max_history_bytes {
            return;
        }

        self.history_bytes = self.history_bytes.saturating_add(entry_bytes);
        self.history.push(entry);
        if self.trim_history_to_limits() {
            self.history_needs_rewrite = true;
        }
    }

    fn trim_history_to_limits(&mut self) -> bool {
        // Evict oldest-first. Compute how many leading entries to drop for both
        // the entry-count and byte budgets, then remove them in a single
        // `drain` instead of repeated `remove(0)` calls (each of which shifts
        // the whole vector), so trimming a batch is O(n) rather than O(n*k).
        let len = self.history.len();
        let mut drop_count = len.saturating_sub(self.limits.max_history_entries);
        let mut freed_bytes: usize = self.history[..drop_count]
            .iter()
            .map(|e| e.retained_bytes())
            .sum();
        while drop_count < len
            && self.history_bytes.saturating_sub(freed_bytes) > self.limits.max_history_bytes
        {
            freed_bytes = freed_bytes.saturating_add(self.history[drop_count].retained_bytes());
            drop_count += 1;
        }

        if drop_count == 0 {
            return false;
        }

        self.history.drain(..drop_count);
        self.history_bytes = self.history_bytes.saturating_sub(freed_bytes);
        self.history_saved_entries = self.history_saved_entries.saturating_sub(drop_count);
        if self.history.is_empty() {
            self.history_bytes = 0;
        }
        true
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

    /// Clear all history entries and reset persistence accounting.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_bytes = 0;
        self.history_saved_entries = 0;
        self.history_needs_rewrite = true;
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
                let before = self.history.len();
                self.push_history_entry(HistoryEntry {
                    timestamp: ts,
                    exit_code: ec,
                    duration_ms: dur,
                    cwd: parts[3].to_string(),
                    command: parts[4].to_string(),
                });
                if self.history.len() == before {
                    self.history_needs_rewrite = true;
                }
            }
        }
        self.history_saved_entries = self.history.len();
    }

    /// Save history to the VFS history file (if configured).
    pub async fn save_history(&mut self) {
        let path = match &self.history_file {
            Some(p) => p.clone(),
            None => return,
        };
        if let Some(parent) = path.parent() {
            let _ = self.fs.mkdir(parent, true).await;
        }

        if self.history_needs_rewrite || self.history_saved_entries > self.history.len() {
            let content = format_history_entries(&self.history);
            // Only advance persistence accounting when the write succeeds. On a
            // transient FS error keep `history_needs_rewrite` set so the next
            // save retries a full rewrite instead of silently dropping deltas.
            if self.fs.write_file(&path, content.as_bytes()).await.is_ok() {
                self.history_saved_entries = self.history.len();
                self.history_needs_rewrite = false;
            } else {
                self.history_needs_rewrite = true;
            }
            return;
        }

        if self.history_saved_entries < self.history.len() {
            let content = format_history_entries(&self.history[self.history_saved_entries..]);
            if self.fs.append_file(&path, content.as_bytes()).await.is_ok() {
                self.history_saved_entries = self.history.len();
            } else {
                // Append failed: force a full rewrite next time so the missed
                // delta is not lost.
                self.history_needs_rewrite = true;
            }
        }
    }

    /// Capture the current shell state (variables, env, cwd, options).
    pub fn shell_state(&self) -> ShellState {
        self.shell_state_with_options(ShellStateOptions::default())
    }

    pub(crate) fn shell_state_with_options(&self, options: ShellStateOptions) -> ShellState {
        // Deref through Arc and clone the inner HashMap for the public
        // ShellState struct (which holds plain HashMaps so users can mutate
        // it freely).
        ShellState {
            env: self.env.clone(),
            variables: (*self.scoped.variables).clone(),
            var_attrs: self
                .scoped
                .var_attrs
                .iter()
                .map(|(name, attrs)| (name.clone(), attrs.bits()))
                .collect(),
            namerefs: (*self.scoped.namerefs).clone(),
            arrays: (*self.scoped.arrays).clone(),
            assoc_arrays: (*self.scoped.assoc_arrays).clone(),
            cwd: self.cwd.clone(),
            last_exit_code: self.last_exit_code,
            last_bg_pid: self.last_bg_pid.clone(),
            functions: if options.include_functions {
                (*self.scoped.functions).clone()
            } else {
                HashMap::new()
            },
            aliases: (*self.scoped.aliases).clone(),
            traps: (*self.scoped.traps).clone(),
            dir_stack: (*self.scoped.dir_stack).clone(),
        }
    }

    /// Capture a lightweight shell-state view for prompt/UI inspection.
    pub fn shell_state_view(&self) -> ShellStateView {
        ShellStateView {
            env: self.env.clone(),
            variables: (*self.scoped.variables).clone(),
            arrays: (*self.scoped.arrays).clone(),
            assoc_arrays: (*self.scoped.assoc_arrays).clone(),
            cwd: self.cwd.clone(),
            last_exit_code: self.last_exit_code,
            aliases: (*self.scoped.aliases).clone(),
            traps: (*self.scoped.traps).clone(),
        }
    }

    /// Restore shell state from a snapshot.
    pub fn restore_shell_state(&mut self, state: &ShellState) {
        self.env = state.env.clone();
        let mut restored_variables = state.variables.clone();
        let mut restored_var_attrs: HashMap<String, VarAttrs> = state
            .var_attrs
            .iter()
            .map(|(name, bits)| (name.clone(), VarAttrs::from_bits_truncate(*bits)))
            .collect();
        let mut restored_namerefs = state.namerefs.clone();
        self.migrate_legacy_attr_markers(
            &mut restored_variables,
            &mut restored_var_attrs,
            &mut restored_namerefs,
        );
        self.scoped.variables = Arc::new(restored_variables);
        self.scoped.var_attrs = Arc::new(restored_var_attrs);
        self.scoped.namerefs = Arc::new(restored_namerefs);
        self.refresh_shopt_flags();
        self.scoped.arrays = Arc::new(state.arrays.clone());
        self.scoped.assoc_arrays = Arc::new(state.assoc_arrays.clone());
        self.cwd = state.cwd.clone();
        self.last_exit_code = state.last_exit_code;
        self.last_bg_pid = state.last_bg_pid.clone();
        // THREAT[TM-DOS-061]: Re-parse and budget-check restored functions so
        // snapshots cannot bypass parser/memory limits via serialized AST.
        let mut restored_functions = HashMap::new();
        let mut function_memory_budget = crate::limits::MemoryBudget::default();
        let mut function_names = state.functions.keys().cloned().collect::<Vec<_>>();
        function_names.sort_unstable();
        for name in function_names {
            let Some(snapshot_func) = state.functions.get(&name) else {
                continue;
            };
            let Some(source) = snapshot_func.source.as_deref() else {
                continue;
            };
            let Ok(parsed_func) = deserialize_function_from_source_with_limits(
                &name,
                source,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            ) else {
                continue;
            };
            let body_bytes = function_storage_bytes(&parsed_func);
            if function_memory_budget
                .check_function_insert(body_bytes, true, 0, &self.memory_limits)
                .is_err()
            {
                continue;
            }
            function_memory_budget.record_function_insert(body_bytes, true, 0);
            restored_functions.insert(name, parsed_func);
        }
        self.scoped.functions = Arc::new(restored_functions);
        self.scoped.aliases = Arc::new(state.aliases.clone());
        self.scoped.traps = Arc::new(state.traps.clone());
        self.scoped.dir_stack = Arc::new(state.dir_stack.clone());
        // Recompute memory budget from restored state to prevent desync
        let func_count = self.scoped.functions.len();
        let func_bytes: usize = self
            .scoped
            .functions
            .values()
            .map(function_storage_bytes)
            .sum();
        self.memory_budget = crate::limits::MemoryBudget::recompute_from_state(
            &self.scoped.variables,
            &self.scoped.arrays,
            &self.scoped.assoc_arrays,
            func_count,
            func_bytes,
            Self::is_internal_variable,
        );
    }

    fn migrate_legacy_attr_markers(
        &self,
        variables: &mut HashMap<String, String>,
        var_attrs: &mut HashMap<String, VarAttrs>,
        namerefs: &mut HashMap<String, String>,
    ) {
        // Preserve marker values: legacy `_NAMEREF_<name>` stores its target in the value.
        fn take_prefixed(
            variables: &mut HashMap<String, String>,
            prefix: &str,
        ) -> Vec<(String, String)> {
            let markers = variables
                .keys()
                .filter_map(|key| {
                    key.strip_prefix(prefix)
                        .map(|stripped| (key.clone(), stripped.to_string()))
                })
                .collect::<Vec<_>>();
            markers
                .into_iter()
                .filter_map(|(marker_key, stripped)| {
                    variables.remove(&marker_key).map(|value| (stripped, value))
                })
                .collect()
        }

        for (key, _) in take_prefixed(variables, "_READONLY_") {
            var_attrs
                .entry(key)
                .and_modify(|attrs| attrs.insert(VarAttrs::READONLY))
                .or_insert(VarAttrs::READONLY);
        }
        for (key, _) in take_prefixed(variables, "_INTEGER_") {
            var_attrs
                .entry(key)
                .and_modify(|attrs| attrs.insert(VarAttrs::INTEGER))
                .or_insert(VarAttrs::INTEGER);
        }
        for (key, _) in take_prefixed(variables, "_LOWER_") {
            var_attrs
                .entry(key)
                .and_modify(|attrs| attrs.insert(VarAttrs::LOWER))
                .or_insert(VarAttrs::LOWER);
        }
        for (key, _) in take_prefixed(variables, "_UPPER_") {
            var_attrs
                .entry(key)
                .and_modify(|attrs| attrs.insert(VarAttrs::UPPER))
                .or_insert(VarAttrs::UPPER);
        }
        for (key, target) in take_prefixed(variables, "_NAMEREF_") {
            namerefs.entry(key).or_insert(target);
        }
    }

    /// Validate restored shell state against configured memory limits.
    ///
    /// Used by snapshot restore paths before applying untrusted state.
    pub(crate) fn validate_shell_state_restore_limits(&self, state: &ShellState) -> Result<()> {
        let budget = crate::limits::MemoryBudget::recompute_from_state(
            &state.variables,
            &state.arrays,
            &state.assoc_arrays,
            0,
            0,
            Self::is_internal_variable,
        );

        if budget.variable_count > self.memory_limits.max_variable_count {
            return Err(crate::limits::LimitExceeded::Memory(format!(
                "variable count limit ({}) exceeded",
                self.memory_limits.max_variable_count
            ))
            .into());
        }
        if budget.variable_bytes > self.memory_limits.max_total_variable_bytes {
            return Err(crate::limits::LimitExceeded::Memory(format!(
                "variable byte limit ({}) exceeded",
                self.memory_limits.max_total_variable_bytes
            ))
            .into());
        }
        if budget.array_entries > self.memory_limits.max_array_entries {
            return Err(crate::limits::LimitExceeded::Memory(format!(
                "array entry limit ({}) exceeded",
                self.memory_limits.max_array_entries
            ))
            .into());
        }

        Ok(())
    }

    /// Get a reference to the current execution counters.
    pub fn counters(&self) -> &crate::limits::ExecutionCounters {
        &self.counters
    }

    /// Merge session-level counters from a snapshot without lowering live usage.
    pub fn restore_session_counters(&mut self, session_commands: u64, session_exec_calls: u64) {
        self.counters.session_commands = self.counters.session_commands.max(session_commands);
        self.counters.session_exec_calls = self.counters.session_exec_calls.max(session_exec_calls);
    }

    /// Set an output callback for streaming output during execution.
    ///
    /// When set, the interpreter calls this callback with `(stdout_chunk, stderr_chunk)`
    /// after each loop iteration, command list element, and top-level command.
    /// Output is still accumulated in the returned `ExecResult` for the final result.
    pub fn set_output_callback(&mut self, callback: OutputCallback) {
        self.output_callback = Some(callback);
        self.output_emit_count = 0;
        self.output_stream_stdout_bytes = 0;
        self.output_stream_stderr_bytes = 0;
    }

    /// Clear the output callback.
    pub fn clear_output_callback(&mut self) {
        self.output_callback = None;
        self.output_emit_count = 0;
        self.output_stream_stdout_bytes = 0;
        self.output_stream_stderr_bytes = 0;
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

        let stdout_remaining = self
            .limits
            .max_stdout_bytes
            .saturating_sub(self.output_stream_stdout_bytes);
        let stderr_remaining = self
            .limits
            .max_stderr_bytes
            .saturating_sub(self.output_stream_stderr_bytes);
        let stdout_chunk = Self::utf8_prefix_at_most(stdout, stdout_remaining);
        let stderr_chunk = Self::utf8_prefix_at_most(stderr, stderr_remaining);
        if stdout_chunk.is_empty() && stderr_chunk.is_empty() {
            return false;
        }

        if let Some(ref mut cb) = self.output_callback {
            cb(stdout_chunk, stderr_chunk);
            self.output_emit_count += 1;
            self.output_stream_stdout_bytes += stdout_chunk.len();
            self.output_stream_stderr_bytes += stderr_chunk.len();
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

    /// Get a mutable reference to the HTTP client (for setting hooks after build).
    #[cfg(feature = "http_client")]
    pub(crate) fn http_client_mut(&mut self) -> Option<&mut crate::network::HttpClient> {
        self.http_client.as_mut()
    }

    /// Set the git client for git builtins.
    ///
    /// This is only available when the `git` feature is enabled.
    #[cfg(feature = "git")]
    pub fn set_git_client(&mut self, client: crate::builtins::git::GitClient) {
        self.git_client = Some(client);
    }

    /// Set the SSH client for ssh/scp/sftp builtins.
    ///
    /// This is only available when the `ssh` feature is enabled.
    #[cfg(feature = "ssh")]
    pub fn set_ssh_client(&mut self, client: crate::builtins::ssh::SshClient) {
        self.ssh_client = Some(client);
    }

    /// Execute a script.
    pub async fn execute(&mut self, script: &Script) -> Result<ExecResult> {
        // Note: Bash::exec() resets per-exec counters and counts the session
        // invocation before parsing, so parse/budget failures also consume the
        // max_exec_calls budget. Internal callers of Interpreter::execute() do
        // not represent host-level exec() invocations.

        let result = {
            let result = self.execute_script_body(script, true, true).await;
            // Script boundary cleanup: background jobs are scoped to a single exec()
            // call, so they cannot accumulate across long-lived sessions.
            let _ = self.jobs.lock().await.wait_all_results().await;
            result
        };

        if result.is_err() {
            // THREAT[TM-INF-019]: Trace events are per exec() result data.
            // Error paths have no ExecResult to carry them, so discard them before
            // a reused Bash instance can expose stale events to the next caller.
            let _ = self.trace.take_events();
        }

        result
    }

    /// Clean up process substitution temp files (`/dev/fd/proc_sub_*`).
    /// Called from Bash::exec() after execute() returns, outside the
    /// recursive async call chain to avoid increasing stack frame size.
    pub(crate) async fn cleanup_proc_sub_files(&mut self) {
        let paths = std::mem::take(&mut self.proc_sub_paths);
        for path in paths {
            let _ = self.fs.remove(Path::new(&path), false).await;
        }
    }

    /// Inner script execution — runs commands without resetting counters.
    /// Used by `execute_source` and nested shell contexts.
    /// `run_exit_trap`: whether this shell context runs its EXIT trap.
    /// `fire_exit_hook`: whether `exit` notifies host-level on_exit hooks.
    async fn execute_script_body(
        &mut self,
        script: &Script,
        run_exit_trap: bool,
        fire_exit_hook: bool,
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
                    stdout.push_str(Self::utf8_prefix_at_most(&result.stdout, remaining));
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
                    stderr.push_str(Self::utf8_prefix_at_most(&result.stderr, remaining));
                    stderr_truncated = true;
                }
            }

            exit_code = result.exit_code;
            self.last_exit_code = exit_code;

            // Stop on control flow (e.g. nounset error uses Return to abort)
            if result.control_flow != ControlFlow::None {
                if let ControlFlow::Exit(code) = result.control_flow {
                    if fire_exit_hook {
                        match self.hooks.fire_on_exit(crate::hooks::ExitEvent { code }) {
                            Some(event) => {
                                exit_code = event.code;
                                self.last_exit_code = exit_code;
                                break;
                            }
                            None => continue,
                        }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Run ERR trap on non-zero exit (unless in conditional chain).
            // Lists are suppressed here because execute_list already fired
            // the ERR trap for the failing subcommand; firing again would
            // double-invoke the trap (e.g. `set -e; trap 'f' ERR; false`).
            if exit_code != 0 {
                let suppressed = matches!(command, Command::List(_))
                    || matches!(command, Command::Pipeline(p) if p.negated)
                    || result.errexit_suppressed;
                if !suppressed {
                    self.run_err_trap(&mut stdout, &mut stderr).await;
                }
            }

            // errexit (set -e): stop on non-zero exit unless the callee marks
            // the status as suppressed (for example, a short-circuited AND-OR
            // list) or the command is an explicitly negated pipeline.
            // Lists are NOT suppressed here so set -e fires for failing lists.
            if self.is_errexit_enabled() && exit_code != 0 {
                let suppressed = matches!(command, Command::Pipeline(p) if p.negated)
                    || result.errexit_suppressed;
                if !suppressed {
                    break;
                }
            }
        }

        // Run EXIT trap if registered (only for top-level execute)
        #[allow(clippy::collapsible_if)]
        if run_exit_trap {
            if let Some(trap_cmd) = self.scoped.traps.get("EXIT").cloned() {
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
            // THREAT[TM-INF-031]: final_env is a user-visible output channel.
            // Apply visibility filtering + output-byte cap to prevent marker leaks
            // and bypass of stdout/stderr output limits.
            let mut final_env = HashMap::new();
            let mut remaining = self.limits.max_stdout_bytes;
            let mut keys: Vec<&String> = self.scoped.variables.keys().collect();
            keys.sort_unstable();
            for key in keys {
                if is_hidden_variable(key) {
                    continue;
                }
                let Some(value) = self.scoped.variables.get(key) else {
                    continue;
                };
                let entry_bytes = key.len().saturating_add(value.len());
                if entry_bytes > remaining {
                    continue;
                }
                final_env.insert(key.clone(), value.clone());
                remaining = remaining.saturating_sub(entry_bytes);
                if remaining == 0 {
                    break;
                }
            }
            Some(final_env)
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
                    if let Some(stderr) = self.logic_only_redirect_error(redirects) {
                        return Ok(ExecResult::err(stderr, 1));
                    }

                    // Process input redirections before executing compound
                    let stdin = match self.process_input_redirections(None, redirects).await {
                        Ok(s) => s,
                        Err(crate::error::Error::CommandFailure(msg)) => {
                            return Ok(ExecResult::err(msg, 1));
                        }
                        Err(e) => return Err(e),
                    };
                    let prev_pipeline_stdin = if stdin.is_some() {
                        let prev = self.pipeline_stdin.take();
                        self.pipeline_stdin = stdin;
                        Some(prev)
                    } else {
                        None
                    };

                    // Suspend output callback while output redirects are active
                    // so that maybe_emit_output inside the compound body does not
                    // leak output that will be redirected (e.g. `{ cmd; } 2>/dev/null`).
                    let has_output_redirect = redirects.iter().any(|r| {
                        !matches!(
                            r.kind,
                            RedirectKind::Input | RedirectKind::HereDoc | RedirectKind::HereString
                        )
                    });
                    let saved_callback = if has_output_redirect {
                        self.output_callback.take()
                    } else {
                        None
                    };

                    let has_dup_output =
                        redirects.iter().any(|r| r.kind == RedirectKind::DupOutput);
                    let has_file_redirect = redirects.iter().any(|r| {
                        matches!(
                            r.kind,
                            RedirectKind::Output
                                | RedirectKind::Clobber
                                | RedirectKind::Append
                                | RedirectKind::OutputBoth
                        )
                    });
                    let capture_pending_fd = has_dup_output && has_file_redirect;
                    if capture_pending_fd {
                        if self.pending_fd_capture_depth == 0 {
                            self.clear_pending_fd_redirect_state();
                        }
                        self.pending_fd_capture_depth += 1;
                    }
                    let result = self.execute_compound(compound).await;
                    if capture_pending_fd {
                        self.pending_fd_capture_depth =
                            self.pending_fd_capture_depth.saturating_sub(1);
                        if result.is_err() {
                            self.clear_pending_fd_redirect_state();
                        }
                    }
                    let result = result?;

                    // Restore callback before applying redirections
                    if let Some(cb) = saved_callback {
                        self.output_callback = Some(cb);
                    }

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
                    let body_bytes = function_storage_bytes(func_def);
                    let is_new = !self.scoped.functions.contains_key(&func_def.name);
                    let old_body_bytes = if is_new {
                        0
                    } else {
                        self.scoped
                            .functions
                            .get(&func_def.name)
                            .map(function_storage_bytes)
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
                        self.functions_mut()
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
                self.counters.push_subshell(&self.limits)?;
                // Subshells run in fully isolated scope: variables, arrays,
                // functions, cwd, traps, positional params, and options are
                // all snapshot/restored so mutations don't leak to the parent.
                // The Arc-wrapped maps make each snapshot an O(1) refcount
                // bump; only mutations inside the subshell pay a clone.
                let snap = self.snapshot_subshell_state();
                let saved_call_stack = self.call_stack.clone();
                let saved_exit = self.last_exit_code;
                let saved_coproc = self.coproc_buffers.clone();

                let mut result = self.execute_command_sequence(commands).await;

                // Fire EXIT trap set inside the subshell before restoring parent state
                if let Some(trap_cmd) = self.scoped.traps.get("EXIT").cloned() {
                    // Only fire if the subshell set its own EXIT trap (different from parent)
                    let parent_had_same = snap.scoped.traps.get("EXIT") == Some(&trap_cmd);
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

                self.restore_subshell_state(snap);
                self.call_stack = saved_call_stack;
                self.last_exit_code = saved_exit;
                self.coproc_buffers = saved_coproc;
                self.counters.pop_subshell();

                // Consume Exit and Return control flow at subshell boundary —
                // they only terminate the subshell, not the parent shell.
                // Return is used by ${var:?msg} error handling and nounset errors.
                // Also clear errexit_suppressed: inner AND/OR suppression must not
                // escape the subshell boundary and prevent the parent set -e from
                // firing on the subshell's non-zero exit code.
                if let Ok(ref mut res) = result {
                    match res.control_flow {
                        ControlFlow::Exit(code) | ControlFlow::Return(code) => {
                            res.exit_code = code;
                            res.control_flow = ControlFlow::None;
                        }
                        _ => {}
                    }
                    res.errexit_suppressed = false;
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

                // Quoted words skip brace/glob expansion — unless the
                // word has unquoted glob chars (e.g. `"$var"*.ext`)
                if w.quoted && !w.has_unquoted_glob {
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

        self.counters.enter_loop();
        let result = async {
            for value in values {
                // Check loop iteration limit
                self.counters.tick_loop(&self.limits)?;

                // Set loop variable (respects nameref). `value` is moved
                // straight into `set_variable` — previously we cloned it
                // even though `values` already owned the String for us.
                self.set_variable(for_cmd.variable.clone(), value);

                // Execute body
                let emit_before = self.output_emit_count;
                let result = self.execute_command_sequence(&for_cmd.body).await?;
                self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
                let should_errexit = self.is_errexit_enabled()
                    && result.exit_code != 0
                    && result.control_flow == ControlFlow::None
                    && !result.errexit_suppressed;
                match acc.accumulate(result) {
                    state::LoopAction::None => {
                        if should_errexit {
                            return Ok(acc.finish());
                        }
                    }
                    state::LoopAction::Break => break,
                    state::LoopAction::Continue => continue,
                    state::LoopAction::Exit(r) => return Ok(r),
                }
            }

            Ok(acc.finish())
        }
        .await;
        self.counters.exit_loop();
        result
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
            if w.quoted && !w.has_unquoted_glob {
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
            .scoped
            .variables
            .get("PS3")
            .cloned()
            .unwrap_or_else(|| "#? ".to_string());

        self.counters.enter_loop();
        let result = async {
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
        .await;
        self.counters.exit_loop();
        result
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

        self.counters.enter_loop();
        let result = async {
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
                let should_errexit = self.is_errexit_enabled()
                    && result.exit_code != 0
                    && result.control_flow == ControlFlow::None
                    && !result.errexit_suppressed;
                match acc.accumulate(result) {
                    state::LoopAction::None | state::LoopAction::Continue => {
                        if should_errexit {
                            return Ok(acc.finish());
                        }
                    }
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
        .await;
        self.counters.exit_loop();
        result
    }

    /// Execute an arithmetic command ((expression))
    /// Returns exit code 0 if result is non-zero, 1 if result is zero
    /// Execute a [[ conditional expression ]]
    async fn execute_conditional(&mut self, words: &[Word]) -> Result<ExecResult> {
        // Evaluate with lazy expansion to support short-circuit semantics.
        // In `[[ -n "${X:-}" && "$X" != "off" ]]`, if the left side is false,
        // the right side must NOT be expanded (to avoid set -u errors).
        let result = self.evaluate_conditional_words(words).await?;
        // If a nounset error occurred during evaluation, propagate it.
        if let Some(err_msg) = self.nounset_error.take() {
            self.last_exit_code = 1;
            return Ok(ExecResult {
                stderr: err_msg,
                exit_code: 1,
                control_flow: ControlFlow::Return(1),
                ..Default::default()
            });
        }
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

    fn conditional_word_literal(word: &Word) -> Option<&str> {
        if word.parts.len() == 1
            && let WordPart::Literal(s) = &word.parts[0]
        {
            return Some(s);
        }
        None
    }

    fn conditional_words_wrapped(words: &[Word]) -> bool {
        if words.len() < 2
            || Self::conditional_word_literal(&words[0]) != Some("(")
            || Self::conditional_word_literal(&words[words.len() - 1]) != Some(")")
        {
            return false;
        }

        let mut depth = 0usize;
        for (i, word) in words.iter().enumerate() {
            match Self::conditional_word_literal(word) {
                Some("(") => depth += 1,
                Some(")") => {
                    let Some(next_depth) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = next_depth;
                    if depth == 0 && i < words.len() - 1 {
                        return false;
                    }
                }
                _ => {}
            }
        }

        depth == 0
    }

    fn conditional_args_wrapped(args: &[String]) -> bool {
        if args.len() < 2
            || args.first().map(|s| s.as_str()) != Some("(")
            || args.last().map(|s| s.as_str()) != Some(")")
        {
            return false;
        }

        let mut depth = 0usize;
        for (i, arg) in args.iter().enumerate() {
            match arg.as_str() {
                "(" => depth += 1,
                ")" => {
                    let Some(next_depth) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = next_depth;
                    if depth == 0 && i < args.len() - 1 {
                        return false;
                    }
                }
                _ => {}
            }
        }

        depth == 0
    }

    fn find_top_level_conditional_word_operator(words: &[Word], op: &str) -> Option<usize> {
        let mut depth = 0usize;
        for i in (0..words.len()).rev() {
            match Self::conditional_word_literal(&words[i]) {
                Some(")") => depth += 1,
                Some("(") => depth = depth.saturating_sub(1),
                Some(found) if found == op && depth == 0 && i > 0 => return Some(i),
                _ => {}
            }
        }
        None
    }

    fn find_top_level_conditional_arg_operator(args: &[String], op: &str) -> Option<usize> {
        let mut depth = 0usize;
        for i in (0..args.len()).rev() {
            match args[i].as_str() {
                ")" => depth += 1,
                "(" => depth = depth.saturating_sub(1),
                found if found == op && depth == 0 && i > 0 => return Some(i),
                _ => {}
            }
        }
        None
    }

    /// Evaluate [[ ]] from raw words with lazy expansion for short-circuit.
    fn evaluate_conditional_words<'a>(
        &'a mut self,
        words: &'a [Word],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            if words.is_empty() {
                return Ok(false);
            }

            // Handle negation
            if Self::conditional_word_literal(&words[0]) == Some("!") {
                return Ok(!self.evaluate_conditional_words(&words[1..]).await?);
            }

            // Handle parentheses only when they wrap the whole expression.
            if Self::conditional_words_wrapped(words) {
                return self
                    .evaluate_conditional_words(&words[1..words.len() - 1])
                    .await;
            }

            // Look for || (lowest precedence), then && — only at current paren depth.
            if let Some(i) = Self::find_top_level_conditional_word_operator(words, "||") {
                let left = self.evaluate_conditional_words(&words[..i]).await?;
                if left {
                    return Ok(true); // short-circuit: skip right side
                }
                return self.evaluate_conditional_words(&words[i + 1..]).await;
            }
            if let Some(i) = Self::find_top_level_conditional_word_operator(words, "&&") {
                let left = self.evaluate_conditional_words(&words[..i]).await?;
                if !left {
                    return Ok(false); // short-circuit: skip right side
                }
                return self.evaluate_conditional_words(&words[i + 1..]).await;
            }

            // Leaf: expand words and evaluate as a simple condition
            let mut expanded = Vec::new();
            for word in words {
                expanded.push(self.expand_word(word).await?);
            }
            Ok(self.evaluate_conditional(&expanded).await)
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

            // Handle parentheses only when they wrap the whole expression.
            if Self::conditional_args_wrapped(args) {
                return self.evaluate_conditional(&args[1..args.len() - 1]).await;
            }

            // Look for logical operators at current paren depth: || lowest, then &&.
            if let Some(i) = Self::find_top_level_conditional_arg_operator(args, "||") {
                return self.evaluate_conditional(&args[..i]).await
                    || self.evaluate_conditional(&args[i + 1..]).await;
            }
            if let Some(i) = Self::find_top_level_conditional_arg_operator(args, "&&") {
                return self.evaluate_conditional(&args[..i]).await
                    && self.evaluate_conditional(&args[i + 1..]).await;
            }

            match args.len() {
                1 => !args[0].is_empty(),
                2 => {
                    // Unary operators
                    let resolve = |p: &str| -> std::path::PathBuf {
                        let path = std::path::Path::new(p);
                        let joined = if path.is_absolute() {
                            path.to_path_buf()
                        } else {
                            self.cwd.join(path)
                        };
                        crate::fs::normalize_path(&joined)
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
                            self.scoped
                                .variables
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
                    self.arrays_mut()
                        .insert("BASH_REMATCH".to_string(), rematch);
                    true
                } else {
                    self.arrays_mut().remove("BASH_REMATCH");
                    false
                }
            }
            Err(_) => {
                self.arrays_mut().remove("BASH_REMATCH");
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

        self.counters.enter_loop();
        let result = async {
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
                let should_errexit = self.is_errexit_enabled()
                    && result.exit_code != 0
                    && result.control_flow == ControlFlow::None
                    && !result.errexit_suppressed;
                match acc.accumulate(result) {
                    state::LoopAction::None => {
                        if should_errexit {
                            return Ok(acc.finish());
                        }
                    }
                    state::LoopAction::Break => break,
                    state::LoopAction::Continue => continue,
                    state::LoopAction::Exit(r) => return Ok(r),
                }
            }

            Ok(acc.finish())
        }
        .await;
        self.counters.exit_loop();
        result
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
                if r.control_flow != ControlFlow::None {
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code,
                        control_flow: r.control_flow,
                        ..Default::default()
                    });
                }
                match case_item.terminator {
                    CaseTerminator::Break => {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::None,
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

        // Execute the command body while suppressing streaming callbacks.
        // Coproc output must stay internal and be consumed only via read -u / <&FD.
        let saved_callback = self.output_callback.take();
        let result = self.execute_command(&coproc.body).await;
        if let Some(callback) = saved_callback {
            self.output_callback = Some(callback);
        }
        let result = result?;

        // Buffer stdout lines for reading via the virtual read FD.
        // Lines are stored in reverse order so pop() yields the first line.
        let mut lines: Vec<String> = result.stdout.lines().map(|l| l.to_string()).collect();
        lines.reverse();
        self.coproc_buffers.insert(read_fd, lines);

        // Set NAME array: NAME[0] = read FD, NAME[1] = write FD
        let mut arr = HashMap::new();
        arr.insert(0, read_fd.to_string());
        arr.insert(1, write_fd.to_string());
        self.arrays_mut().insert(name.clone(), arr);

        // Set NAME_PID to a virtual PID (use job table counter)
        let virtual_pid = {
            let table = self.jobs.lock().await;
            table.last_job_id().unwrap_or(0) + 1000
        };
        self.vars_mut()
            .insert(format!("{}_PID", name), virtual_pid.to_string());

        // Also set $! (last background PID)
        self.last_bg_pid = Some(virtual_pid.to_string());

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
                Ok(content) => decode_file_bytes_for_path(&path, &content),
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

        if script_content.len() > self.limits.max_input_bytes {
            return Ok(ExecResult::err(
                format!(
                    "{}: input exceeds maximum size ({} > {})\n",
                    shell_name,
                    script_content.len(),
                    self.limits.max_input_bytes
                ),
                2,
            ));
        }

        // THREAT[TM-DOS-021]: Propagate interpreter's parser limits to child shell
        let script_owned = script_content.clone();
        let max_ast_depth = self.limits.max_ast_depth;
        let max_parser_operations = self.limits.max_parser_operations;
        let parse_result = tokio::time::timeout(self.limits.parser_timeout, async move {
            tokio::task::spawn_blocking(move || {
                let parser =
                    Parser::with_limits(&script_owned, max_ast_depth, max_parser_operations);
                parser.parse()
            })
            .await
        })
        .await;
        let script = match parse_result {
            Ok(Ok(Ok(s))) => s,
            Ok(Ok(Err(e))) => {
                return Ok(ExecResult::err(
                    format!("{}: syntax error: {}\n", shell_name, e),
                    2,
                ));
            }
            Ok(Err(e)) => {
                return Ok(ExecResult::err(
                    format!("{}: parser task failed: {}\n", shell_name, e),
                    2,
                ));
            }
            Err(_) => {
                return Ok(ExecResult::err(
                    format!(
                        "{}: parser timeout after {}ms\n",
                        shell_name,
                        self.limits.parser_timeout.as_millis()
                    ),
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

        // Real bash spawns a child process for `bash`/`sh`, so non-exportable
        // state (arrays/assoc_arrays/functions/aliases/namerefs, plus
        // non-exported scalars) must not be visible to the child, and
        // mutations the child performs must not leak back to the parent.
        // Snapshot first so a full restore handles both directions; then
        // wipe the isolated state before running. See issue #1777.
        let child_snapshot = self.snapshot_subshell_state();
        self.reset_state_for_child_shell();

        // Push call frame, apply options, execute, restore, pop
        self.call_stack.push(CallFrame {
            name: name_arg,
            locals: HashMap::new(),
            local_arrays: HashMap::new(),
            local_assoc_arrays: HashMap::new(),
            positional: positional_args,
        });

        let mut saved_opt_names: HashSet<&'static str> = HashSet::new();
        for (var, val) in &shell_opts {
            if !saved_opt_names.insert(*var) {
                self.insert_variable_checked(var.to_string(), val.to_string());
                continue;
            }
            self.insert_variable_checked(var.to_string(), val.to_string());
        }
        self.insert_variable_checked("OPTIND".to_string(), "1".to_string());
        self.getopts_char_idx = 0;

        // Forward piped stdin to child when executing a script file or -c command
        let saved_stdin = self.pipeline_stdin.take();
        if script_file.is_some() || is_command_mode {
            self.pipeline_stdin = stdin.clone();
        }

        // Set BASH_SOURCE for script file execution
        if let Some(ref file) = script_file {
            self.bash_source_stack.push(file.clone());
            self.update_bash_source();
        }

        let result = self.execute_script_body(&script, true, false).await;

        // Restore BASH_SOURCE
        if script_file.is_some() {
            self.bash_source_stack.pop();
            self.update_bash_source();
        }

        // Restore stdin
        self.pipeline_stdin = saved_stdin;

        self.pop_call_frame();

        // Restore parent state — full revert of the snapshot since the child
        // is process-isolated. This also undoes OPTIND/SHOPT_* writes above.
        self.restore_subshell_state(child_snapshot);

        match result {
            Ok(exec_result) => self.apply_redirections(exec_result, redirects).await,
            Err(e) => Err(e),
        }
    }

    /// Reset interpreter state to what a freshly-forked `bash`/`sh` child
    /// would see: drop arrays/assoc_arrays/functions/aliases/namerefs, and
    /// keep only exported scalars in `variables`. The caller is expected to
    /// have just taken a snapshot to undo this on return. See issue #1777.
    fn reset_state_for_child_shell(&mut self) {
        let exported_names: Vec<String> = self
            .scoped
            .var_attrs
            .iter()
            .filter(|(_, attrs)| attrs.contains(VarAttrs::EXPORT))
            .map(|(name, _)| name.clone())
            .collect();
        let mut next_vars: HashMap<String, String> = HashMap::with_capacity(exported_names.len());
        for name in &exported_names {
            if let Some(val) = self.scoped.variables.get(name) {
                next_vars.insert(name.clone(), val.clone());
            }
        }
        // Also preserve hidden/internal markers (e.g. SHOPT_* are set later
        // by shell_opts; BASH_VERSION, IFS, etc. need to remain accessible).
        for name in [
            "BASH_VERSION",
            "BASH_VERSINFO",
            "IFS",
            "PATH",
            "PWD",
            "SHELL",
            "HOSTNAME",
            "HOME",
            "PS1",
            "PS2",
            "PS4",
            "RANDOM",
            "LINENO",
            "SECONDS",
            "UID",
            "EUID",
            "HOSTTYPE",
            "OSTYPE",
            "MACHTYPE",
        ] {
            if !next_vars.contains_key(name)
                && let Some(val) = self.scoped.variables.get(name)
            {
                next_vars.insert(name.to_string(), val.clone());
            }
        }
        *self.vars_mut() = next_vars;
        self.arrays_mut().clear();
        self.assoc_arrays_mut().clear();
        self.functions_mut().clear();
        self.namerefs_mut().clear();
        // Aliases are parse-time anyway, but a fresh `bash -c` would not have
        // user-defined aliases — drop them for consistency.
        self.scoped.aliases = Arc::new(HashMap::new());
        // Reset SHOPT_* flag bitfield so options from the parent don't leak.
        self.flags = BashFlags::empty();
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

/// Route fd1/fd2/fd3+ content to their targets. Extracted from the async
/// `apply_redirections_fd_table` to keep these locals out of the async state machine.
#[inline(never)]
fn route_fd_table_content(
    orig_stdout: &str,
    orig_stderr: &str,
    fd1: &FdTarget,
    fd2: &FdTarget,
    extra_fd_targets: &[(i32, FdTarget)],
    pending: &HashMap<i32, String>,
) -> (
    String,
    String,
    std::collections::HashMap<PathBuf, (String, bool, String)>,
) {
    let mut new_stdout = String::new();
    let mut new_stderr = String::new();
    let mut file_writes: std::collections::HashMap<PathBuf, (String, bool, String)> =
        std::collections::HashMap::new();

    let route = |data: &str,
                 target: &FdTarget,
                 fw: &mut std::collections::HashMap<PathBuf, (String, bool, String)>,
                 out: &mut String,
                 err: &mut String| match target {
        FdTarget::Stdout => {
            if !data.is_empty() {
                out.push_str(data);
            }
        }
        FdTarget::Stderr => {
            if !data.is_empty() {
                err.push_str(data);
            }
        }
        FdTarget::DevNull => {}
        FdTarget::WriteFile(p, d) => {
            let entry = fw
                .entry(p.clone())
                .or_insert_with(|| (String::new(), false, d.clone()));
            if !data.is_empty() {
                entry.0.push_str(data);
            }
        }
        FdTarget::AppendFile(p, d) => {
            let entry = fw
                .entry(p.clone())
                .or_insert_with(|| (String::new(), true, d.clone()));
            if !data.is_empty() {
                entry.0.push_str(data);
            }
        }
    };

    route(
        orig_stdout,
        fd1,
        &mut file_writes,
        &mut new_stdout,
        &mut new_stderr,
    );
    route(
        orig_stderr,
        fd2,
        &mut file_writes,
        &mut new_stdout,
        &mut new_stderr,
    );

    // Route pending fd3+ output
    for (fd_num, data) in pending {
        let target = extra_fd_targets
            .iter()
            .find(|(n, _)| n == fd_num)
            .map(|(_, t)| t);
        if let Some(target) = target {
            route(
                data,
                target,
                &mut file_writes,
                &mut new_stdout,
                &mut new_stderr,
            );
        }
    }

    (new_stdout, new_stderr, file_writes)
}

impl Interpreter {
    /// Execute a sequence of commands (with errexit checking)
    async fn execute_command_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        self.execute_command_sequence_impl(commands, true).await
    }

    /// Execute a sequence of commands used as a condition (no errexit checking)
    /// Used for if/while/until conditions where failure is expected
    async fn execute_condition_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        self.condition_sequence_depth += 1;
        let result = self.execute_command_sequence_impl(commands, false).await;
        self.condition_sequence_depth -= 1;
        result
    }

    fn is_in_condition_sequence(&self) -> bool {
        self.condition_sequence_depth > 0
    }

    /// Execute commands whose stdout is captured by command substitution.
    /// Streaming callbacks must stay suspended so hidden capture output cannot
    /// leak to observers before it is assigned or otherwise consumed.
    async fn execute_capture_only_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        let saved_callback = self.output_callback.take();
        let result = self.execute_command_sequence(commands).await;
        self.output_callback = saved_callback;
        result
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
            // Suppression is decided by the callee and surfaced through
            // result.errexit_suppressed (e.g. AND-OR lists).
            let suppress = result.errexit_suppressed;
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
                    self.counters.tick_command(&self.limits)?;
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
        self.arrays_mut().insert("PIPESTATUS".to_string(), ps_arr);

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
        self.last_bg_pid = Some(job_id.to_string());

        // Background commands always return exit code 0 to the parent.
        // The real exit code lives in the job table for `wait` to read.
        self.last_exit_code = 0;
        Ok(())
    }

    /// Execute a command list (cmd1 && cmd2 || cmd3)
    #[allow(unused_assignments)] // control_flow may be set but overwritten
    async fn execute_list(&mut self, list: &CommandList) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code;
        let mut control_flow;
        let mut exit_code_from_conditional_context = false;

        // Determine if the first command should run in the background.
        // The `&` terminator for first appears as rest[0].op == Background.
        let first_is_bg = matches!(list.rest.first(), Some((ListOperator::Background, _)));

        if first_is_bg {
            self.spawn_in_background(&list.first, &mut stdout, &mut stderr)
                .await?;
            exit_code = 0;
            control_flow = ControlFlow::None;
            exit_code_from_conditional_context = false;
        } else {
            let emit_before = self.output_emit_count;
            let result = self.execute_command(&list.first).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
            self.last_exit_code = exit_code;
            control_flow = result.control_flow;
            exit_code_from_conditional_context = result.errexit_suppressed
                || list
                    .rest
                    .first()
                    .is_some_and(|(op, _)| matches!(op, ListOperator::And | ListOperator::Or));

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
            if exit_code != 0 && first_op_is_semicolon && !self.is_in_condition_sequence() {
                self.run_err_trap(&mut stdout, &mut stderr).await;
            }
        }

        for (i, (op, cmd)) in list.rest.iter().enumerate() {
            // Skip empty sentinel commands (produced by trailing `&`)
            if Self::is_empty_sentinel(cmd) {
                continue;
            }

            // Check if this command is followed by another && / || operator.
            // POSIX `errexit` suppression applies to non-final commands in an
            // AND-OR list; the final executed command can still abort on failure.
            let current_is_conditional = matches!(op, ListOperator::And | ListOperator::Or);

            // Determine if THIS command should be backgrounded.
            // A command is backgrounded when the NEXT separator is Background
            // (the `&` terminates the current command).
            let should_background =
                matches!(list.rest.get(i + 1), Some((ListOperator::Background, _)));

            // Check errexit before executing next semicolon-separated command:
            // if previous command failed outside conditional context, exit now.
            let should_check_errexit = matches!(op, ListOperator::Semicolon)
                && self.is_errexit_enabled()
                && exit_code != 0
                && !exit_code_from_conditional_context;

            if should_check_errexit {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                    ..Default::default()
                });
            }

            let should_execute = match op {
                ListOperator::And => exit_code == 0,
                ListOperator::Or => exit_code != 0,
                ListOperator::Semicolon | ListOperator::Background => true,
            };

            if !should_execute && current_is_conditional {
                // Short-circuited && / ||: the carried exit code came from
                // a conditional chain, so errexit must not fire on it.
                exit_code_from_conditional_context = true;
            }

            if should_execute {
                if should_background {
                    self.spawn_in_background(cmd, &mut stdout, &mut stderr)
                        .await?;
                    exit_code = 0;
                    exit_code_from_conditional_context = false;
                } else {
                    let emit_before = self.output_emit_count;
                    let result = self.execute_command(cmd).await?;
                    self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
                    stdout.push_str(&result.stdout);
                    stderr.push_str(&result.stderr);
                    exit_code = result.exit_code;
                    self.last_exit_code = exit_code;
                    control_flow = result.control_flow;
                    let followed_by_conditional_op =
                        list.rest.get(i + 1).is_some_and(|(op, cmd)| {
                            !Self::is_empty_sentinel(cmd)
                                && matches!(op, ListOperator::And | ListOperator::Or)
                        });
                    // Bash suppresses errexit for AND-OR list elements except the
                    // command following the final &&/|| operator.
                    exit_code_from_conditional_context =
                        followed_by_conditional_op || result.errexit_suppressed;

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

                    // ERR trap follows the same AND-OR suppression as errexit.
                    if exit_code != 0
                        && !exit_code_from_conditional_context
                        && !self.is_in_condition_sequence()
                    {
                        self.run_err_trap(&mut stdout, &mut stderr).await;
                    }
                }
            }
        }

        // Final errexit check for the last command. A non-zero status only
        // remains suppressed when it was carried from a short-circuited or
        // non-final AND-OR list element; a failing final &&/|| command exits.
        let should_final_errexit_check = self.is_errexit_enabled()
            && exit_code != 0
            && !exit_code_from_conditional_context
            && !self.is_in_condition_sequence();

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
            errexit_suppressed: exit_code_from_conditional_context && exit_code != 0,
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
                        if self.scoped.assoc_arrays.contains_key(&resolved_name) {
                            let key = self.expand_assoc_key(index_str).await?;
                            let is_new_entry = self
                                .scoped
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
                                let arr = self.assoc_arrays_mut().entry(resolved_name).or_default();
                                if assignment.append {
                                    let existing = arr.get(&key).cloned().unwrap_or_default();
                                    arr.insert(key, existing + &value);
                                } else {
                                    arr.insert(key, value);
                                }
                            }
                        } else {
                            let index =
                                self.resolve_indexed_array_subscript(&resolved_name, index_str);
                            let is_new_entry = self
                                .scoped
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
                                let arr = self.arrays_mut().entry(resolved_name).or_default();
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
                    // Expand directly into the replacement array so word-split expansions cannot
                    // accumulate an unbounded temporary Vec before max_array_entries is enforced.
                    let arr_name = self.resolve_nameref(&assignment.name).to_string();
                    let old_entries = self.scoped.arrays.get(&arr_name).map_or(0, |arr| arr.len());
                    let remaining_entries = self
                        .memory_limits
                        .max_array_entries
                        .saturating_sub(self.memory_budget.array_entries);
                    let max_new_entries = old_entries.saturating_add(remaining_entries);
                    let mut next_arr = if assignment.append {
                        self.scoped
                            .arrays
                            .get(&arr_name)
                            .cloned()
                            .unwrap_or_default()
                    } else {
                        HashMap::new()
                    };
                    let mut idx = if assignment.append {
                        next_arr.keys().max().map(|k| k + 1).unwrap_or(0)
                    } else {
                        0
                    };

                    'array_words: for word in words.iter() {
                        let is_unquoted_expansion = !word.quoted
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
                        // "${arr[@]}" or "$@" in array context should splat
                        // individual elements, not join into a single string.
                        let is_quoted_splat = word.quoted
                            && word.parts.len() == 1
                            && matches!(
                                &word.parts[0],
                                WordPart::ArrayAccess { index, .. } if index == "@"
                            );
                        let is_quoted_positional_splat = word.quoted
                            && word.parts.len() == 1
                            && matches!(
                                &word.parts[0],
                                WordPart::Variable(name) if name == "@"
                            );

                        if is_unquoted_expansion {
                            let remaining = max_new_entries.saturating_sub(next_arr.len());
                            if remaining == 0 {
                                break;
                            }
                            let expanded = self.expand_word(word).await?;
                            for field in self.ifs_split_limited(&expanded, remaining)? {
                                next_arr.insert(idx, field);
                                idx += 1;
                            }
                            if next_arr.len() >= max_new_entries {
                                break 'array_words;
                            }
                        } else if is_quoted_splat || is_quoted_positional_splat {
                            for field in self.expand_word_to_fields(word).await? {
                                if next_arr.len() >= max_new_entries {
                                    break 'array_words;
                                }
                                next_arr.insert(idx, field);
                                idx += 1;
                            }
                        } else {
                            let value = self.expand_word(word).await?;
                            if next_arr.len() >= max_new_entries {
                                break;
                            }
                            next_arr.insert(idx, value);
                            idx += 1;
                        }
                    }

                    let _ = self.insert_array_checked(arr_name, next_arr);
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
        let expansion = self.scoped.aliases.get(name).cloned()?;

        // Restore variable saves before re-executing
        for (vname, old) in var_saves.into_iter().rev() {
            match old {
                Some(v) => {
                    self.insert_variable_checked(vname, v);
                }
                None => {
                    self.vars_mut().remove(&vname);
                }
            }
        }

        // Build expanded command: alias value + original args
        let mut expanded_cmd = expansion;
        let trailing_space = expanded_cmd.ends_with(' ');
        let mut args_iter = command.args.iter();
        if trailing_space && let Some(first_arg) = args_iter.next() {
            let arg_str = Self::format_word_for_alias_reparse(first_arg);
            if let Some(arg_expansion) = self.scoped.aliases.get(&arg_str).cloned() {
                expanded_cmd.push_str(&arg_expansion);
            } else {
                expanded_cmd.push_str(&arg_str);
            }
        }
        for word in args_iter {
            expanded_cmd.push(' ');
            expanded_cmd.push_str(&Self::format_word_for_alias_reparse(word));
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
            Ok(s) => {
                // THREAT[TM-DOS-031]: Validate budget on expanded alias AST
                // to prevent bypassing static budget checks via alias expansion.
                if let Err(e) = crate::parser::validate_budget(&s, &self.limits) {
                    Ok(ExecResult::err(
                        format!("bash: alias expansion: budget validation failed: {e}\n"),
                        1,
                    ))
                } else {
                    // Alias expansion runs in the current shell: use
                    // execute_script_body (like source/eval), NOT execute(),
                    // so an aliased command does not fire the EXIT trap.
                    self.execute_script_body(&s, false, true).await
                }
            }
            Err(e) => Ok(ExecResult::err(
                format!("bash: alias expansion: parse error: {}\n", e),
                1,
            )),
        };

        self.pipeline_stdin = prev_pipeline_stdin;
        self.expanding_aliases.remove(name);
        Some(result)
    }

    /// Discard deferred output process substitutions queued by the current simple command.
    fn discard_deferred_proc_subs_from(&mut self, start: usize) {
        self.deferred_proc_subs.truncate(start);
    }

    /// Execute deferred output process substitutions (`>(cmd)`) queued by the
    /// current simple command. Older entries belong to an outer expansion frame
    /// and must not be drained by nested command substitutions.
    async fn run_deferred_proc_subs_from(
        &mut self,
        start: usize,
        result: &mut Result<ExecResult>,
    ) -> Result<()> {
        if self.deferred_proc_subs.len() <= start {
            return Ok(());
        }
        let deferred = self.deferred_proc_subs.split_off(start);
        for (path_str, commands) in deferred {
            let path = Path::new(&path_str);
            let stdin_data = if let Ok(bytes) = self.fs.read_file(path).await {
                let s = decode_file_bytes_for_path(path, &bytes);
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
                    self.vars_mut().remove(&name);
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
            .scoped
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

    // THREAT[TM-DOS-089]: Box the full simple-command path because nested
    // `echo $(echo $(...))` repeatedly polls this helper, and its large async
    // state (name/arg expansion, alias/env handling, xtrace, redirects) was
    // still enough to overflow smaller Linux/tarpaulin stacks.
    fn execute_simple_command<'a>(
        &'a mut self,
        command: &'a SimpleCommand,
        stdin: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            let deferred_proc_sub_start = self.deferred_proc_subs.len();
            let (_debug_stdout, _debug_stderr) = self.run_debug_trap().await;

            let name = match self.expand_word(&command.name).await {
                Ok(name) => name,
                Err(err) => {
                    self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
                    return Err(err);
                }
            };

            if let Some(err_msg) = self.nounset_error.take() {
                self.last_exit_code = 1;
                self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
                return Ok(ExecResult {
                    stdout: String::new(),
                    stderr: err_msg,
                    exit_code: 1,
                    control_flow: ControlFlow::Return(1),
                    ..Default::default()
                });
            }

            let pre_expanded_args = if !name.is_empty() {
                match self.expand_command_args(command).await {
                    Ok(args) => Some(args),
                    Err(err) => {
                        self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
                        return Err(err);
                    }
                }
            } else {
                None
            };

            let var_saves: Vec<(String, Option<String>)> = command
                .assignments
                .iter()
                .map(|a| (a.name.clone(), self.scoped.variables.get(&a.name).cloned()))
                .collect();

            let pre_assign_subst_gen = self.subst_generation;

            if let Err(err) = self.process_command_assignments(&command.assignments).await {
                self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
                return Err(err);
            }

            // Alias expansion
            if let Some(result) = self
                .try_alias_expansion(&name, command, stdin.clone(), var_saves.clone())
                .await
            {
                self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
                return result;
            }

            // Empty command handling
            if name.is_empty() {
                if command.name.quoted && command.assignments.is_empty() {
                    self.last_exit_code = 127;
                    self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
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
                self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
                return Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code,
                    control_flow: crate::interpreter::ControlFlow::None,
                    ..Default::default()
                });
            }

            // Inject prefix assignments into env for command duration.
            // Save original env value once per name so duplicate assignments
            // (e.g., `A=1 A=2 cmd`) restore to pre-command state.
            let mut env_saves: HashMap<String, Option<String>> = HashMap::new();
            for assignment in &command.assignments {
                if assignment.index.is_none()
                    && let Some(value) = self.scoped.variables.get(&assignment.name).cloned()
                {
                    env_saves
                        .entry(assignment.name.clone())
                        .or_insert_with(|| self.env.get(&assignment.name).cloned());
                    self.env.insert(assignment.name.clone(), value);
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
                self.discard_deferred_proc_subs_from(deferred_proc_sub_start);
                let result = ExecResult::err(err_msg, 1);
                return self.apply_redirections(result, &command.redirects).await;
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

            self.run_deferred_proc_subs_from(deferred_proc_sub_start, &mut result)
                .await?;

            result
        })
    }

    /// Expand command arguments with field splitting, brace, and glob expansion.
    /// Boxed because nested command substitution repeatedly expands `echo` args,
    /// and the combined field/glob state still materially contributes to per-level
    /// poll-stack growth on smaller Linux/tarpaulin stacks.
    fn expand_command_args<'a>(
        &'a mut self,
        command: &'a SimpleCommand,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let mut args: Vec<String> = Vec::new();
            for word in &command.args {
                // Use field expansion so "${arr[@]}" produces multiple args
                let fields = self.expand_word_to_fields(word).await?;

                // Skip brace and glob expansion for quoted words — unless the
                // word has unquoted glob chars (e.g. `"$var"*.ext`) in which case
                // the quoted expansion suppresses IFS splitting but the unquoted
                // portion must still undergo glob expansion.
                if word.quoted && !word.has_unquoted_glob {
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
        })
    }

    /// Execute a command after name resolution and prefix assignment setup.
    ///
    /// Handles stdin processing and dispatch to functions, special builtins,
    /// regular builtins, or command-not-found. Args are pre-expanded.
    // THREAT[TM-DOS-089]: Box the dispatch wrapper too so per-level stdin
    // plumbing, trace bookkeeping, and dispatch future selection stay off the
    // recursive poll stack during nested command substitution.
    fn execute_dispatched_command<'a>(
        &'a mut self,
        name: &'a str,
        args: Vec<String>,
        command: &'a SimpleCommand,
        stdin: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
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

            if let Some(stderr) = self.logic_only_redirect_error(&command.redirects) {
                return Ok(ExecResult::err(stderr, 1));
            }

            // Handle input redirections first
            let stdin = match self
                .process_input_redirections(stdin, &command.redirects)
                .await
            {
                Ok(s) => s,
                Err(crate::error::Error::CommandFailure(msg)) => {
                    return Ok(ExecResult::err(msg, 1));
                }
                Err(e) => return Err(e),
            };

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
        })
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
            // Security: never reconstruct shell source from argv.
            // Execute argv directly to avoid quote/parse injection.
            let target_name = args[0].clone();
            let target_args = args[1..].to_vec();
            let target_command = SimpleCommand {
                name: Word::literal(target_name.clone()),
                args: target_args.iter().cloned().map(Word::literal).collect(),
                redirects: redirects.to_vec(),
                assignments: Vec::new(),
                span: Span::new(),
            };
            let result = self
                .execute_dispatched_command(&target_name, target_args, &target_command, None)
                .await?;

            // Signal exit so subsequent statements don't execute
            return Ok(ExecResult {
                control_flow: ControlFlow::Return(result.exit_code),
                ..result
            });
        }
        for redirect in redirects {
            // Resolve fd from either explicit fd or {var} fd-variable syntax
            let resolved_fd_var: Option<i32> = redirect.fd_var.as_ref().and_then(|var_name| {
                self.scoped
                    .variables
                    .get(var_name)
                    .and_then(|val| val.parse::<i32>().ok())
            });
            match redirect.kind {
                RedirectKind::Input => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    let content = match self.fs.read_file(&path).await {
                        Ok(c) => c,
                        Err(e) => {
                            return Ok(ExecResult::err(format!("bash: {target_path}: {e}\n"), 1));
                        }
                    };
                    let text = decode_file_bytes_for_path(&path, &content);
                    let fd = redirect.fd.or(resolved_fd_var);
                    if let Some(fd) = fd {
                        self.ensure_persistent_fd_capacity(fd)?;
                        let lines: Vec<String> =
                            text.lines().rev().map(|l| l.to_string()).collect();
                        self.coproc_buffers.insert(fd, lines);
                    } else {
                        // exec < file: redirect stdin for subsequent commands
                        self.pipeline_stdin = Some(text);
                    }
                }
                RedirectKind::DupInput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let fd = redirect.fd.or(resolved_fd_var);
                    if (target == "-" || target == "&-")
                        && let Some(fd) = fd
                    {
                        self.coproc_buffers.remove(&fd);
                    }
                }
                RedirectKind::Output | RedirectKind::Clobber => {
                    let fd = redirect.fd.or(resolved_fd_var).unwrap_or(1);
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    self.ensure_persistent_fd_capacity(fd)?;
                    if is_dev_null(&path) {
                        self.exec_fd_table.insert(fd, FdTarget::DevNull);
                    } else {
                        // Truncate file on open (like real exec >file)
                        let _ = self.fs.write_file(&path, b"").await;
                        self.exec_fd_table
                            .insert(fd, FdTarget::WriteFile(path, target_path));
                    }
                }
                RedirectKind::Append => {
                    let fd = redirect.fd.or(resolved_fd_var).unwrap_or(1);
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    self.ensure_persistent_fd_capacity(fd)?;
                    if is_dev_null(&path) {
                        self.exec_fd_table.insert(fd, FdTarget::DevNull);
                    } else {
                        self.exec_fd_table
                            .insert(fd, FdTarget::AppendFile(path, target_path));
                    }
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let fd = redirect.fd.or(resolved_fd_var).unwrap_or(1);
                    if target == "-" || target == "&-" {
                        // exec N>&- closes the fd
                        self.exec_fd_table.remove(&fd);
                    } else if let Ok(target_fd) = target.parse::<i32>() {
                        // exec N>&M duplicates fd M to fd N
                        let target_entry = if target_fd == 1 {
                            FdTarget::Stdout
                        } else if target_fd == 2 {
                            FdTarget::Stderr
                        } else {
                            self.exec_fd_table
                                .get(&target_fd)
                                .cloned()
                                .unwrap_or(FdTarget::Stdout)
                        };
                        self.ensure_persistent_fd_capacity(fd)?;
                        self.exec_fd_table.insert(fd, target_entry);
                    }
                }
                _ => {}
            }
        }
        let result = ExecResult::default();
        self.apply_redirections(result, redirects).await
    }

    fn ensure_persistent_fd_capacity(&self, fd: i32) -> Result<()> {
        if fd < 0 {
            return Err(crate::error::Error::Execution(format!(
                "invalid file descriptor: {}",
                fd
            )));
        }

        if (0..=2).contains(&fd)
            || self.exec_fd_table.contains_key(&fd)
            || self.coproc_buffers.contains_key(&fd)
        {
            return Ok(());
        }

        let mut open_fds: HashSet<i32> = self.exec_fd_table.keys().copied().collect();
        open_fds.extend(self.coproc_buffers.keys().copied());

        if open_fds.len() >= self.limits.max_file_descriptors {
            return Err(crate::limits::LimitExceeded::MaxFileDescriptors(
                self.limits.max_file_descriptors,
            )
            .into());
        }

        Ok(())
    }

    /// Execute a registered (non-special) builtin with panic safety.
    /// The builtin must exist in `self.builtins` (caller checks with `contains_key`).
    ///
    /// Keep this helper boxed: the builtin path now carries execution-extension
    /// plumbing plus panic-catching state, and nested command substitution hits it
    /// on every `echo $(...)` level. Boxing keeps that larger state machine off the
    /// recursive poll stack so the stack-overflow regression stays fixed.
    fn execute_registered_builtin<'a>(
        &'a mut self,
        name: &'a str,
        args: &'a [String],
        stdin: Option<&'a str>,
        redirects: &'a [Redirect],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        // Clone the Arc out of the map so the call doesn't hold a borrow on
        // self.builtins while we take &mut self for the execution body.
        let builtin = self.builtins.get(name).unwrap().clone();
        self.execute_builtin_arc(name, builtin, args, stdin, redirects)
    }

    /// Execute a builtin resolved via the host-owned [`BuiltinRegistry`].
    fn execute_host_builtin<'a>(
        &'a mut self,
        name: &'a str,
        builtin: Arc<dyn Builtin>,
        args: &'a [String],
        stdin: Option<&'a str>,
        redirects: &'a [Redirect],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        self.execute_builtin_arc(name, builtin, args, stdin, redirects)
    }

    /// Shared execution path for builtins regardless of source
    /// (baked-in, builder-`builtin`, or host registry).
    fn execute_builtin_arc<'a>(
        &'a mut self,
        name: &'a str,
        builtin: Arc<dyn Builtin>,
        args: &'a [String],
        stdin: Option<&'a str>,
        redirects: &'a [Redirect],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            // Fire before_tool hooks — may modify args or cancel the invocation
            let args = if !self.hooks.before_tool.is_empty() {
                let event = crate::hooks::ToolEvent {
                    name: name.to_string(),
                    args: args.to_vec(),
                };
                match self.hooks.fire_before_tool(event) {
                    Some(modified) => std::borrow::Cow::Owned(modified.args),
                    None => {
                        let result = ExecResult::err(
                            format!("bash: {name}: cancelled by before_tool hook\n"),
                            1,
                        );
                        return self.apply_redirections(result, redirects).await;
                    }
                }
            } else {
                std::borrow::Cow::Borrowed(args)
            };
            let args: &[String] = &args;

            // Check for execution plan first
            {
                let execution_extensions = self.current_execution_extensions();
                let shell_ref = ShellRef {
                    builtins: &self.builtins,
                    host_builtins: self.host_builtins.as_ref(),
                    functions: &self.scoped.functions,
                    aliases: Arc::make_mut(&mut self.scoped.aliases),
                    traps: Arc::make_mut(&mut self.scoped.traps),
                    var_attrs: Arc::make_mut(&mut self.scoped.var_attrs),
                    namerefs: Arc::make_mut(&mut self.scoped.namerefs),
                    dir_stack: Arc::make_mut(&mut self.scoped.dir_stack),
                    call_stack: &self.call_stack,
                    history: &self.history,
                    limits: &self.limits,
                    jobs: &self.jobs,
                    execution_extensions,
                };
                let plan_ctx = builtins::Context {
                    args,
                    env: &self.env,
                    variables: Arc::make_mut(&mut self.scoped.variables),
                    cwd: &mut self.cwd,
                    fs: Arc::clone(&self.fs),
                    stdin,
                    #[cfg(feature = "http_client")]
                    http_client: self.http_client.as_ref(),
                    #[cfg(feature = "git")]
                    git_client: self.git_client.as_ref(),
                    #[cfg(feature = "ssh")]
                    ssh_client: self.ssh_client.as_ref(),
                    shell: Some(shell_ref),
                };

                let plan_result = AssertUnwindSafe(builtin.execution_plan(&plan_ctx))
                    .catch_unwind()
                    .await;

                match plan_result {
                    Ok(Ok(Some(plan))) => {
                        let result = self.execute_builtin_plan(plan, redirects).await?;
                        return Ok(self.apply_after_tool(name, result));
                    }
                    Ok(Ok(None)) => { /* fall through to normal execute() */ }
                    Ok(Err(e)) => return Err(e),
                    Err(_panic) => {
                        let result = ExecResult::err(
                            format!("bash: {}: builtin failed unexpectedly\n", name),
                            1,
                        );
                        let result = self.apply_redirections(result, redirects).await?;
                        return Ok(self.apply_after_tool(name, result));
                    }
                }
            }

            let execution_extensions = self.current_execution_extensions();
            let shell_ref = ShellRef {
                builtins: &self.builtins,
                host_builtins: self.host_builtins.as_ref(),
                functions: &self.scoped.functions,
                aliases: Arc::make_mut(&mut self.scoped.aliases),
                traps: Arc::make_mut(&mut self.scoped.traps),
                var_attrs: Arc::make_mut(&mut self.scoped.var_attrs),
                namerefs: Arc::make_mut(&mut self.scoped.namerefs),
                dir_stack: Arc::make_mut(&mut self.scoped.dir_stack),
                call_stack: &self.call_stack,
                history: &self.history,
                limits: &self.limits,
                jobs: &self.jobs,
                execution_extensions,
            };
            let ctx = builtins::Context {
                args,
                env: &self.env,
                variables: Arc::make_mut(&mut self.scoped.variables),
                cwd: &mut self.cwd,
                fs: Arc::clone(&self.fs),
                stdin,
                #[cfg(feature = "http_client")]
                http_client: self.http_client.as_ref(),
                #[cfg(feature = "git")]
                git_client: self.git_client.as_ref(),
                #[cfg(feature = "ssh")]
                ssh_client: self.ssh_client.as_ref(),
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

            self.apply_builtin_side_effects(&result).await;

            // Sync successful export operands into env so subprocess isolation can see them.
            // Keep syncing even if export returned nonzero for other args (bash-compatible).
            if name == "export" {
                for arg in args {
                    if let Some(eq_pos) = arg.find('=') {
                        let var_name = &arg[..eq_pos];
                        if self.is_var_readonly(var_name) {
                            continue;
                        }
                        if let Some(value) = self.scoped.variables.get(var_name) {
                            self.env.insert(var_name.to_string(), value.clone());
                        }
                    } else if let Some(value) = self.scoped.variables.get(arg.as_str()) {
                        // export NAME (without =) — mark existing variable as exported
                        self.env.insert(arg.to_string(), value.clone());
                    }
                }
            }

            let result = self.apply_redirections(result, redirects).await?;
            Ok(self.apply_after_tool(name, result))
        })
    }

    /// Apply `after_tool` interceptor decisions to the result returned to callers.
    fn apply_after_tool(&self, name: &str, result: ExecResult) -> ExecResult {
        if self.hooks.after_tool.is_empty() {
            return result;
        }
        let event = crate::hooks::ToolResult {
            name: name.to_string(),
            stdout: result.stdout.clone(),
            exit_code: result.exit_code,
        };
        match self.hooks.fire_after_tool(event) {
            Some(event) => ExecResult {
                stdout: event.stdout,
                exit_code: event.exit_code,
                ..result
            },
            None => ExecResult::err(format!("bash: {name}: cancelled by after_tool hook\n"), 1),
        }
    }

    fn is_special_builtin_name(name: &str) -> bool {
        SPECIAL_BUILTIN_NAMES.contains(&name)
    }

    async fn execute_special_builtin_with_hooks(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let args = if !self.hooks.before_tool.is_empty() {
            let event = crate::hooks::ToolEvent {
                name: name.to_string(),
                args: args.to_vec(),
            };
            match self.hooks.fire_before_tool(event) {
                Some(modified) => std::borrow::Cow::Owned(modified.args),
                None => {
                    let result = ExecResult::err(
                        format!("bash: {name}: cancelled by before_tool hook\n"),
                        1,
                    );
                    return self.apply_redirections(result, redirects).await;
                }
            }
        } else {
            std::borrow::Cow::Borrowed(args)
        };

        let result = self
            .dispatch_special_builtin(name, &args, stdin, redirects)
            .await
            .expect("special builtin name checked before dispatch")?;
        Ok(self.apply_after_tool(name, result))
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
        if self.shell_profile.is_logic_only()
            && matches!(name, "exec" | "bash" | "sh" | "source" | ".")
        {
            return Some(Ok(ExecResult::err(
                format!("bash: {}: command not found", name),
                127,
            )));
        }

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

    /// True if `name` resolves through the host-owned builtin registry.
    fn has_host_builtin(&self, name: &str) -> bool {
        self.host_builtins
            .as_ref()
            .is_some_and(|reg| reg.lookup(name).is_some())
    }

    /// Sorted names of all dispatchable builtins (registered + special + host
    /// registry). See [`crate::Bash::builtin_names`].
    pub(crate) fn builtin_names(&self) -> Vec<String> {
        merged_builtin_names(&self.builtins, self.host_builtins.as_ref())
    }

    // THREAT[TM-DOS-089]: Box the final dispatch split so function lookup,
    // special builtin handling, registered builtin execution, and path search
    // do not contribute another large async frame per nested substitution level.
    fn dispatch_command<'a>(
        &'a mut self,
        name: &'a str,
        command: &'a SimpleCommand,
        args: Vec<String>,
        stdin: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            // Check for functions first
            if let Some(func_def) = self.scoped.functions.get(name).cloned() {
                return self
                    .execute_function_call(name, &func_def, args, stdin, &command.redirects)
                    .await;
            }

            // Interpreter-level special builtins
            if Self::is_special_builtin_name(name) {
                return self
                    .execute_special_builtin_with_hooks(
                        name,
                        &args,
                        stdin.clone(),
                        &command.redirects,
                    )
                    .await;
            }

            // Host-registered builtins (mutable, may override baked-in builtins).
            if let Some(builtin) = self.host_builtins.as_ref().and_then(|reg| reg.lookup(name)) {
                return self
                    .execute_host_builtin(
                        name,
                        builtin,
                        &args,
                        stdin.as_deref(),
                        &command.redirects,
                    )
                    .await;
            }

            // Registered builtins
            if self.builtins.contains_key(name) {
                return self
                    .execute_registered_builtin(name, &args, stdin.as_deref(), &command.redirects)
                    .await;
            }

            // Script execution by path
            if name.contains('/') {
                if self.shell_profile.is_logic_only() {
                    return Ok(ExecResult::err(
                        format!("bash: {}: command not found", name),
                        127,
                    ));
                }
                return self
                    .try_execute_script_by_path(name, &args, stdin, &command.redirects)
                    .await;
            }

            // $PATH search
            if !self.shell_profile.is_logic_only()
                && let Some(result) = self
                    .try_execute_script_via_path_search(name, &args, stdin, &command.redirects)
                    .await?
            {
                return Ok(result);
            }

            // Command not found
            let host_names: Vec<String> = self
                .host_builtins
                .as_ref()
                .map(|reg| reg.names())
                .unwrap_or_default();
            let known: Vec<&str> = self
                .builtins
                .keys()
                .map(|s| s.as_str())
                .chain(self.scoped.functions.keys().map(|s| s.as_str()))
                .chain(self.scoped.aliases.keys().map(|s| s.as_str()))
                .chain(host_names.iter().map(|s| s.as_str()))
                .collect();
            let msg = command_not_found_message(name, &known);
            Ok(ExecResult::err(msg, 127))
        })
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
            Ok(c) => decode_file_bytes_for_path(&path, &c),
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
    /// Resolve a command name to its full path via PATH search on VFS.
    /// Returns the resolved path string if found, None otherwise.
    async fn resolve_command_path(&self, name: &str) -> Option<String> {
        if self.shell_profile.is_logic_only() {
            return None;
        }

        let path_var = self
            .scoped
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
                return Some(candidate.to_string_lossy().to_string());
            }
        }
        None
    }

    async fn try_execute_script_via_path_search(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<Option<ExecResult>> {
        let path_var = self
            .scoped
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
                    let script_text = decode_file_bytes_for_path(&candidate, &content);
                    let resolved = candidate.to_string_lossy();
                    let result = self
                        .execute_script_content(&resolved, &script_text, args, stdin, redirects)
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
        // `bash -c '...'` subprocess: save then reset. Each Arc clone is an
        // O(1) refcount bump now; the child resets its own state and the
        // parent restores by dropping the child's Arcs and putting these back.
        let saved_vars = Arc::clone(&self.scoped.variables);
        let saved_arrays = Arc::clone(&self.scoped.arrays);
        let saved_assoc = Arc::clone(&self.scoped.assoc_arrays);
        let saved_functions = Arc::clone(&self.scoped.functions);
        let saved_traps = Arc::clone(&self.scoped.traps);
        let saved_aliases = Arc::clone(&self.scoped.aliases);
        let saved_var_attrs = Arc::clone(&self.scoped.var_attrs);
        let saved_namerefs = Arc::clone(&self.scoped.namerefs);
        let saved_flags = self.flags;
        let saved_call_stack = self.call_stack.clone();
        let saved_exit = self.last_exit_code;
        let saved_coproc = self.coproc_buffers.clone();
        let saved_env = self.env.clone();
        let saved_memory_budget = self.memory_budget.clone();
        let saved_exec_fd_table = self.exec_fd_table.clone();

        // Child only sees exported variables (env), not all shell variables.
        // Reset last_exit_code so $? starts at 0 (matches real bash subprocess).
        // Clear nounset_error to prevent parent expansion errors from leaking.
        // Reset attributes/namerefs/flags too — the child gets a fresh option
        // surface like real bash.
        self.scoped.variables = Arc::new(self.env.clone());
        self.scoped.arrays = Arc::new(HashMap::new());
        self.arrays_mut()
            .insert("BASH_VERSINFO".to_string(), compat_bash_versinfo_array());
        self.scoped.assoc_arrays = Arc::new(HashMap::new());
        self.scoped.functions = Arc::new(HashMap::new());
        self.scoped.traps = Arc::new(HashMap::new());
        self.scoped.aliases = Arc::new(HashMap::new());
        self.scoped.var_attrs = Arc::new(HashMap::new());
        self.scoped.namerefs = Arc::new(HashMap::new());
        self.flags = BashFlags::empty();
        self.coproc_buffers.clear();
        self.last_exit_code = 0;
        self.nounset_error = None;

        // Push call frame: $0 = script name, $1..N = args
        self.call_stack = vec![CallFrame {
            name: name.to_string(),
            locals: HashMap::new(),
            local_arrays: HashMap::new(),
            local_assoc_arrays: HashMap::new(),
            positional: args.to_vec(),
        }];

        // Set up BASH_SOURCE for the subprocess
        let saved_source_stack = self.bash_source_stack.clone();
        self.bash_source_stack = vec![name.to_string()];
        self.update_bash_source();

        // Forward pipeline stdin so commands inside the script (cat, read, etc.) can consume it
        let prev_pipeline_stdin = self.pipeline_stdin.take();
        self.pipeline_stdin = stdin;

        let result = self.execute_script_body(&script, true, false).await;

        // Restore full parent state — child mutations don't propagate
        self.scoped.variables = saved_vars;
        self.scoped.arrays = saved_arrays;
        self.scoped.assoc_arrays = saved_assoc;
        self.scoped.functions = saved_functions;
        self.scoped.traps = saved_traps;
        self.scoped.aliases = saved_aliases;
        self.scoped.var_attrs = saved_var_attrs;
        self.scoped.namerefs = saved_namerefs;
        self.flags = saved_flags;
        self.call_stack = saved_call_stack;
        self.last_exit_code = saved_exit;
        self.coproc_buffers = saved_coproc;
        self.env = saved_env;
        self.memory_budget = saved_memory_budget;
        self.exec_fd_table = saved_exec_fd_table;
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
                Ok(c) => decode_file_bytes_for_path(&path, &c),
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
                .scoped
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
                    found = Some(decode_file_bytes_for_path(&candidate, &c));
                    break;
                }
            }
            // Also try cwd as fallback (bash sources from cwd too)
            if found.is_none() {
                let path = self.resolve_path(filename);
                if let Ok(c) = self.fs.read_file(&path).await {
                    found = Some(decode_file_bytes_for_path(&path, &c));
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

        let script = match self.parse_embedded_script(&content).await {
            Ok(script) => script,
            Err(crate::error::Error::Parse { message, .. }) => {
                return Ok(ExecResult::err(
                    format!("source: {}: parse error: {}", filename, message),
                    1,
                ));
            }
            Err(e) => return Err(e),
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
                    local_arrays: HashMap::new(),
                    local_assoc_arrays: HashMap::new(),
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
        let exec_result = self.execute_script_body(&script, false, true).await;

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
                self.pop_call_frame();
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
        let script = match self.parse_embedded_script(&cmd).await {
            Ok(script) => script,
            Err(crate::error::Error::Parse { message, .. }) => {
                return Ok(ExecResult::err(
                    format!("eval: parse error: {}", message),
                    1,
                ));
            }
            Err(e) => return Err(e),
        };

        // Set up pipeline stdin if provided
        let prev_pipeline_stdin = self.pipeline_stdin.take();
        if stdin.is_some() {
            self.pipeline_stdin = stdin;
        }

        // eval runs in the current shell: use execute_script_body (like source),
        // NOT execute(), so it does not fire the EXIT trap. execute() runs the
        // EXIT trap, which is wrong for eval and — because the top-level EXIT
        // trap has no re-entrancy guard — lets `trap 'eval :' EXIT` recurse one
        // command per level until the budget aborts, risking stack overflow.
        let mut result = self.execute_script_body(&script, false, true).await?;

        self.pipeline_stdin = prev_pipeline_stdin;

        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Parse embedded script text (`eval`, `source`) with full parser defenses.
    async fn parse_embedded_script(&self, input: &str) -> Result<Script> {
        if input.len() > self.limits.max_input_bytes {
            return Err(crate::error::Error::ResourceLimit(
                crate::limits::LimitExceeded::InputTooLarge(
                    input.len(),
                    self.limits.max_input_bytes,
                ),
            ));
        }

        #[cfg(target_family = "wasm")]
        {
            Parser::with_limits(
                input,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            )
            .parse()
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let input_owned = input.to_owned();
            let max_depth = self.limits.max_ast_depth;
            let max_ops = self.limits.max_parser_operations;
            let timeout = self.limits.parser_timeout;

            let parse_result = tokio::time::timeout(timeout, async move {
                tokio::task::spawn_blocking(move || {
                    let parser = Parser::with_limits(&input_owned, max_depth, max_ops);
                    parser.parse()
                })
                .await
            })
            .await;

            match parse_result {
                Ok(Ok(result)) => result,
                Ok(Err(join_error)) => Err(crate::error::Error::parse(format!(
                    "parser task failed: {}",
                    join_error
                ))),
                Err(_) => Err(crate::error::Error::ResourceLimit(
                    crate::limits::LimitExceeded::ParserTimeout(timeout),
                )),
            }
        }
    }

    /// Check if expand_aliases is enabled via shopt.
    fn is_expand_aliases_enabled(&self) -> bool {
        self.scoped
            .variables
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
        format!(
            "{}{}{}",
            fd_prefix,
            op,
            Self::format_word_for_alias_reparse(&redir.target)
        )
    }

    /// Serialize a parsed word for alias re-parse without dropping quoted literalness.
    fn format_word_for_alias_reparse(word: &Word) -> String {
        if !word.quoted {
            return format!("{}", word);
        }

        if word.has_unquoted_glob {
            // Keep glob metacharacters outside quotes while quoting expansions.
            // Wrapping the whole word would erase the QuotedGlobWord boundary.
            let mut out = String::new();
            for part in &word.parts {
                match part {
                    WordPart::Literal(s) => Self::push_alias_reparse_literal(&mut out, s, true),
                    _ => {
                        out.push('"');
                        Self::push_alias_reparse_word_part(&mut out, part);
                        out.push('"');
                    }
                }
            }
            return out;
        }

        let mut out = String::from("\"");
        for part in &word.parts {
            Self::push_alias_reparse_word_part(&mut out, part);
        }
        out.push('"');
        out
    }

    fn push_alias_reparse_literal(out: &mut String, s: &str, preserve_glob: bool) {
        for ch in s.chars() {
            if preserve_glob && matches!(ch, '*' | '?' | '[' | ']') {
                out.push(ch);
                continue;
            }
            if preserve_glob {
                match ch {
                    'a'..='z'
                    | 'A'..='Z'
                    | '0'..='9'
                    | '_'
                    | '-'
                    | '.'
                    | '/'
                    | ':'
                    | ','
                    | '+'
                    | '='
                    | '%'
                    | '@' => out.push(ch),
                    _ => {
                        out.push('\\');
                        out.push(ch);
                    }
                }
            } else {
                if matches!(ch, '\\' | '"' | '$' | '`') {
                    out.push('\\');
                }
                out.push(ch);
            }
        }
    }

    fn push_alias_reparse_word_part(out: &mut String, part: &WordPart) {
        match part {
            WordPart::Literal(s) => Self::push_alias_reparse_literal(out, s, false),
            WordPart::Variable(name) => out.push_str(&format!("${}", name)),
            WordPart::CommandSubstitution(cmd) => out.push_str(&format!("$({:?})", cmd)),
            WordPart::ArithmeticExpansion(expr) => out.push_str(&format!("$(({}))", expr)),
            WordPart::ParameterExpansion {
                name,
                operator,
                operand,
                colon_variant,
            } => match operator {
                ParameterOp::UseDefault => {
                    let c = if *colon_variant { ":" } else { "" };
                    out.push_str(&format!("${{{}{}-{}}}", name, c, operand));
                }
                ParameterOp::AssignDefault => {
                    let c = if *colon_variant { ":" } else { "" };
                    out.push_str(&format!("${{{}{}={}}}", name, c, operand));
                }
                ParameterOp::UseReplacement => {
                    let c = if *colon_variant { ":" } else { "" };
                    out.push_str(&format!("${{{}{}+{}}}", name, c, operand));
                }
                ParameterOp::Error => {
                    let c = if *colon_variant { ":" } else { "" };
                    out.push_str(&format!("${{{}{}?{}}}", name, c, operand));
                }
                ParameterOp::RemovePrefixShort => {
                    out.push_str(&format!("${{{}#{}}}", name, operand))
                }
                ParameterOp::RemovePrefixLong => {
                    out.push_str(&format!("${{{}##{}}}", name, operand))
                }
                ParameterOp::RemoveSuffixShort => {
                    out.push_str(&format!("${{{}%{}}}", name, operand))
                }
                ParameterOp::RemoveSuffixLong => {
                    out.push_str(&format!("${{{}%%{}}}", name, operand))
                }
                ParameterOp::ReplaceFirst {
                    pattern,
                    replacement,
                } => out.push_str(&format!("${{{}/{}/{}}}", name, pattern, replacement)),
                ParameterOp::ReplaceAll {
                    pattern,
                    replacement,
                } => out.push_str(&format!("${{{}//{}/{}}}", name, pattern, replacement)),
                ParameterOp::UpperFirst => out.push_str(&format!("${{{}^}}", name)),
                ParameterOp::UpperAll => out.push_str(&format!("${{{}^^}}", name)),
                ParameterOp::LowerFirst => out.push_str(&format!("${{{},}}", name)),
                ParameterOp::LowerAll => out.push_str(&format!("${{{},,}}", name)),
            },
            WordPart::Length(name) => out.push_str(&format!("${{#{}}}", name)),
            WordPart::ArrayAccess { name, index } => {
                out.push_str(&format!("${{{}[{}]}}", name, index))
            }
            WordPart::ArrayLength(name) => out.push_str(&format!("${{#{}[@]}}", name)),
            WordPart::ArrayIndices(name) => out.push_str(&format!("${{!{}[@]}}", name)),
            WordPart::Substring {
                name,
                offset,
                length,
            } => {
                if let Some(len) = length {
                    out.push_str(&format!("${{{}:{}:{}}}", name, offset, len));
                } else {
                    out.push_str(&format!("${{{}:{}}}", name, offset));
                }
            }
            WordPart::ArraySlice {
                name,
                offset,
                length,
            } => {
                if let Some(len) = length {
                    out.push_str(&format!("${{{}[@]:{}:{}}}", name, offset, len));
                } else {
                    out.push_str(&format!("${{{}[@]:{}}}", name, offset));
                }
            }
            WordPart::IndirectExpansion {
                name,
                operator,
                operand,
                colon_variant,
            } => {
                if let Some(op) = operator {
                    let c = if *colon_variant { ":" } else { "" };
                    let op_char = match op {
                        ParameterOp::UseDefault => "-",
                        ParameterOp::AssignDefault => "=",
                        ParameterOp::UseReplacement => "+",
                        ParameterOp::Error => "?",
                        _ => "",
                    };
                    out.push_str(&format!("${{!{}{}{}{}}}", name, c, op_char, operand));
                } else {
                    out.push_str(&format!("${{!{}}}", name));
                }
            }
            WordPart::PrefixMatch(prefix) => out.push_str(&format!("${{!{}*}}", prefix)),
            WordPart::ProcessSubstitution { commands, is_input } => {
                let prefix = if *is_input { "<" } else { ">" };
                out.push_str(&format!("{}({:?})", prefix, commands));
            }
            WordPart::Transformation { name, operator } => {
                out.push_str(&format!("${{{}@{}}}", name, operator));
            }
        }
    }

    fn shadow_local_array_bindings(&mut self, name: &str, keep_indexed: bool, keep_assoc: bool) {
        // A newly retained snapshot keeps the removed binding's entries charged
        // (released at frame pop). When no new snapshot is retained — a second
        // shadow of the same name in the same frame keeps the first snapshot —
        // the binding being removed is a transient local that is not retained
        // anywhere, so its entries must be released now to avoid budget drift.
        let retained_indexed = self.remember_local_array_binding(name);
        let retained_assoc = self.remember_local_assoc_array_binding(name);
        if !keep_indexed {
            let removed = self.arrays_mut().remove(name).map_or(0, |arr| arr.len());
            if !retained_indexed {
                self.memory_budget.record_array_remove(removed);
            }
        }
        if !keep_assoc {
            let removed = self
                .assoc_arrays_mut()
                .remove(name)
                .map_or(0, |arr| arr.len());
            if !retained_assoc {
                self.memory_budget.record_array_remove(removed);
            }
        }
    }

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
            local_arrays: HashMap::new(),
            local_assoc_arrays: HashMap::new(),
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
        // Interpreter-set FUNCNAME entries are metadata and are inserted
        // uncharged. Remember how many there are so that on return we credit
        // back only the *user-added* entries (e.g. `FUNCNAME[7]=x`), which were
        // charged via the normal array-assignment path. Without this, repeated
        // FUNCNAME mutation would leak array budget across calls (over-count).
        let funcname_meta_len = funcname_arr.len();
        let prev_funcname = self
            .arrays_mut()
            .insert("FUNCNAME".to_string(), funcname_arr);

        // BASH_SOURCE: duplicate current top entry for function calls
        let current_source = self.bash_source_stack.last().cloned().unwrap_or_default();
        self.bash_source_stack.push(current_source);
        self.update_bash_source();

        // Forward pipeline stdin to function body
        let prev_pipeline_stdin = self.pipeline_stdin.take();
        self.pipeline_stdin = stdin;

        // Execute function body. Always restore call state even on error.
        let result = self.execute_command(&func_def.body).await;

        // Restore previous pipeline stdin
        self.pipeline_stdin = prev_pipeline_stdin;

        // Pop call frame, restore local array bindings, function counter, and BASH_SOURCE
        self.pop_call_frame();
        self.counters.pop_function();
        self.bash_source_stack.pop();
        self.update_bash_source();

        // Restore previous FUNCNAME (or set from remaining stack). Interpreter
        // metadata entries are never charged, but a script may have added its
        // own entries to FUNCNAME while inside the function; those were charged,
        // so credit them back as the array is discarded to avoid budget drift.
        let funcname_user_entries = self
            .scoped
            .arrays
            .get("FUNCNAME")
            .map_or(0, |a| a.len())
            .saturating_sub(funcname_meta_len);
        if funcname_user_entries > 0 {
            self.memory_budget
                .record_array_remove(funcname_user_entries);
        }
        if self.call_stack.is_empty() {
            self.arrays_mut().remove("FUNCNAME");
        } else if let Some(prev) = prev_funcname {
            self.arrays_mut().insert("FUNCNAME".to_string(), prev);
        }

        let mut result = result?;

        // Handle return - convert Return control flow to exit code
        if let ControlFlow::Return(code) = result.control_flow {
            result.exit_code = code;
            result.control_flow = ControlFlow::None;
        }

        // Clear errexit_suppressed at function boundary: AND/OR suppression
        // from inside the function must not prevent the caller's set -e from
        // firing on the function's non-zero exit code.
        result.errexit_suppressed = false;

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
                        self.shadow_local_array_bindings(var_name, false, false);
                        let inner = &value[1..value.len() - 1];
                        let inserted = if flags.assoc {
                            self.remember_local_assoc_array_binding(var_name);
                            let mut arr = HashMap::new();
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
                            self.insert_assoc_array_checked(var_name.to_string(), arr)
                        } else {
                            self.remember_local_array_binding(var_name);
                            let mut arr = HashMap::new();
                            for (idx, val) in inner.split_whitespace().enumerate() {
                                arr.insert(idx, val.trim_matches('"').to_string());
                            }
                            self.insert_array_checked(var_name.to_string(), arr)
                        };
                        // Mark local only when the backing array fit the memory budget.
                        if inserted {
                            self.insert_local_checked(var_name.to_string(), String::new());
                        }
                    } else if flags.nameref {
                        self.shadow_local_array_bindings(var_name, false, false);
                        self.insert_local_checked(var_name.to_string(), String::new());
                    } else if flags.integer {
                        self.shadow_local_array_bindings(var_name, false, false);
                        let int_val = self.evaluate_arithmetic_with_assign(value);
                        self.insert_local_checked(var_name.to_string(), int_val.to_string());
                        self.add_var_attr(var_name, VarAttrs::INTEGER);
                    } else {
                        self.shadow_local_array_bindings(var_name, false, false);
                        self.insert_local_checked(var_name.to_string(), value.to_string());
                    }
                } else if !is_internal_variable(arg) {
                    if flags.assoc {
                        self.shadow_local_array_bindings(arg, false, false);
                        if self.insert_assoc_array_checked(arg.to_string(), HashMap::new()) {
                            self.insert_local_checked(arg.to_string(), String::new());
                        }
                    } else if flags.array {
                        self.shadow_local_array_bindings(arg, false, false);
                        if self.insert_array_checked(arg.to_string(), HashMap::new()) {
                            self.insert_local_checked(arg.to_string(), String::new());
                        }
                    } else {
                        self.shadow_local_array_bindings(arg, false, false);
                        self.insert_local_checked(arg.to_string(), String::new());
                    }
                    if flags.integer {
                        self.add_var_attr(arg, VarAttrs::INTEGER);
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
                            self.set_nameref(var_name, value.to_string());
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
                            let mut arr = HashMap::new();
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
                            let _ = self.insert_assoc_array_checked(var_name.to_string(), arr);
                        } else {
                            let mut arr = HashMap::new();
                            for (idx, val) in inner.split_whitespace().enumerate() {
                                arr.insert(idx, val.trim_matches('"').to_string());
                            }
                            let _ = self.insert_array_checked(var_name.to_string(), arr);
                        }
                    } else if flags.nameref {
                        self.set_nameref(var_name, value.to_string());
                    } else {
                        self.insert_variable_checked(var_name.to_string(), value.to_string());
                    }
                } else if !is_internal_variable(arg) {
                    if flags.assoc {
                        self.assoc_arrays_mut().entry(arg.to_string()).or_default();
                    } else if flags.array {
                        self.arrays_mut().entry(arg.to_string()).or_default();
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

        let mut stderr = String::new();
        let mut exit_code: i32 = 0;

        for arg in &var_args {
            if unset_function {
                self.functions_mut().remove(arg.as_str());
                continue;
            }
            if let Some(bracket) = arg.find('[')
                && arg.ends_with(']')
            {
                let arr_name = &arg[..bracket];
                let key = &arg[bracket + 1..arg.len() - 1];
                let expanded_key = self.expand_variable_or_literal(key);
                let resolved_name = self.resolve_nameref(arr_name).to_string();
                if let Some(arr) = self.assoc_arrays_mut().get_mut(&resolved_name) {
                    arr.remove(&expanded_key);
                } else if let Some(arr) = self.arrays_mut().get_mut(&resolved_name)
                    && let Ok(idx) = key.parse::<usize>()
                {
                    arr.remove(&idx);
                }
                continue;
            }
            if unset_nameref {
                self.remove_nameref(arg);
            } else {
                let resolved = self.resolve_nameref(arg).to_string();
                // THREAT[TM-INJ-009]: Block unset of internal marker variables
                if is_internal_variable(&resolved) {
                    stderr.push_str(&format!(
                        "bash: unset: {resolved}: cannot unset: readonly variable\n"
                    ));
                    exit_code = 1;
                    continue;
                }
                // THREAT[TM-INJ-019]: Refuse to unset readonly variables and surface
                // the error so callers cannot mistake a silent skip for success.
                if self.is_var_readonly(&resolved) {
                    stderr.push_str(&format!(
                        "bash: unset: {resolved}: cannot unset: readonly variable\n"
                    ));
                    exit_code = 1;
                    continue;
                }
                self.vars_mut().remove(&resolved);
                self.env.remove(&resolved);
                self.arrays_mut().remove(&resolved);
                self.assoc_arrays_mut().remove(&resolved);
                self.clear_var_attrs(&resolved);
                self.remove_nameref(&resolved);
                for frame in self.call_stack.iter_mut().rev() {
                    frame.locals.remove(&resolved);
                }
            }
        }
        let result = ExecResult {
            stderr,
            exit_code,
            ..Default::default()
        };
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
            .scoped
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
                self.vars_mut()
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
        let char_idx: usize = self.getopts_char_idx;

        if char_idx >= opt_chars.len() {
            // Should not happen, but advance
            self.vars_mut()
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            self.getopts_char_idx = 0;
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
                    self.vars_mut()
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.getopts_char_idx = 0;
                } else if optind < parse_args.len() {
                    // Next arg is the argument
                    self.vars_mut()
                        .insert("OPTARG".to_string(), parse_args[optind].clone());
                    self.vars_mut()
                        .insert("OPTIND".to_string(), (optind + 2).to_string());
                    self.getopts_char_idx = 0;
                } else {
                    // Missing argument
                    self.vars_mut().remove("OPTARG");
                    self.vars_mut()
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.getopts_char_idx = 0;
                    if silent {
                        self.insert_variable_checked(varname.clone(), ":".to_string());
                        self.vars_mut()
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
                self.vars_mut().remove("OPTARG");
                if char_idx + 1 < opt_chars.len() {
                    // More chars in this arg
                    self.getopts_char_idx = char_idx + 1;
                } else {
                    // Move to next arg
                    self.vars_mut()
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.getopts_char_idx = 0;
                }
            }
        } else {
            // Unknown option
            self.vars_mut().remove("OPTARG");
            if char_idx + 1 < opt_chars.len() {
                self.getopts_char_idx = char_idx + 1;
            } else {
                self.vars_mut()
                    .insert("OPTIND".to_string(), (optind + 1).to_string());
                self.getopts_char_idx = 0;
            }

            if silent {
                self.insert_variable_checked(varname.clone(), "?".to_string());
                self.vars_mut()
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
                // command -v: print name/path if it's a known command
                let output = if self.scoped.functions.contains_key(cmd_name.as_str())
                    || self.builtins.contains_key(cmd_name.as_str())
                    || self.has_host_builtin(cmd_name)
                    || is_keyword(cmd_name)
                {
                    Some(cmd_name.to_string())
                } else {
                    self.resolve_command_path(cmd_name).await
                };
                let mut result = if let Some(name) = output {
                    ExecResult::ok(format!("{}\n", name))
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
                let description = if self.scoped.functions.contains_key(cmd_name.as_str()) {
                    format!("{} is a function\n", cmd_name)
                } else if self.has_host_builtin(cmd_name)
                    || self.builtins.contains_key(cmd_name.as_str())
                {
                    format!("{} is a shell builtin\n", cmd_name)
                } else if is_keyword(cmd_name) {
                    format!("{} is a shell keyword\n", cmd_name)
                } else if let Some(path) = self.resolve_command_path(cmd_name).await {
                    format!("{} is {}\n", cmd_name, path)
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
                let target = remaining[0].as_str();
                let builtin_args = &remaining[1..];
                // Interpreter-native (special) builtins like `eval`, `source`,
                // `.`, `declare` are implemented in the interpreter, not as
                // trait builtins — some are only registered as unreachable
                // stubs. Route them through the special dispatch so
                // `command eval echo ok` behaves like `eval echo ok` rather
                // than hitting a stub. `command` already bypasses functions,
                // and specials outrank functions in normal dispatch anyway.
                if Self::is_special_builtin_name(target) {
                    // Box::pin: this can recurse (e.g. `command command eval ...`).
                    return Box::pin(self.execute_special_builtin_with_hooks(
                        target,
                        builtin_args,
                        _stdin,
                        redirects,
                    ))
                    .await;
                }
                // Resolve host-registered builtins first (same precedence as dispatch_command).
                if let Some(builtin) = self
                    .host_builtins
                    .as_ref()
                    .and_then(|reg| reg.lookup(target))
                {
                    return self
                        .execute_host_builtin(
                            target,
                            builtin,
                            builtin_args,
                            _stdin.as_deref(),
                            redirects,
                        )
                        .await;
                }
                if let Some(builtin) = self.builtins.get(target).cloned() {
                    return self
                        .execute_builtin_arc(
                            target,
                            builtin,
                            builtin_args,
                            _stdin.as_deref(),
                            redirects,
                        )
                        .await;
                }
                Ok(ExecResult::err(
                    format!("bash: {}: command not found\n", remaining[0]),
                    127,
                ))
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
            // declare with no args: print all variables, filtering hidden markers (TM-INF-017)
            let mut output = String::new();
            let mut entries: Vec<_> = self.scoped.variables.iter().collect();
            entries.sort_by_key(|(k, _)| (*k).clone());
            for (name, value) in entries {
                if is_hidden_variable(name) {
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
                let mut func_names: Vec<_> =
                    self.scoped.functions.keys().cloned().collect::<Vec<_>>();
                func_names.sort();
                for fname in &func_names {
                    output.push_str(&format!("{} ()\n{{\n    ...\n}}\n", fname));
                }
            } else {
                // Print specific functions — return 1 if any not found
                for name in &names {
                    if self.scoped.functions.contains_key(*name) {
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
                let mut entries: Vec<_> = self.scoped.variables.iter().collect();
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
                    if let Some(value) = self.scoped.variables.get(var_name) {
                        let mut attrs = String::from("--");
                        if self.is_var_readonly(var_name) {
                            attrs = String::from("-r");
                        }
                        output.push_str(&format!("declare {} {}=\"{}\"\n", attrs, var_name, value));
                    } else if let Some(arr) = self.scoped.assoc_arrays.get(var_name) {
                        let mut items: Vec<_> = arr.iter().collect();
                        items.sort_by_key(|(k, _)| (*k).clone());
                        let inner: String = items
                            .iter()
                            .map(|(k, v)| format!("[{}]=\"{}\"", k, v))
                            .collect::<Vec<_>>()
                            .join(" ");
                        output.push_str(&format!("declare -A {}=({})\n", var_name, inner));
                    } else if let Some(arr) = self.scoped.arrays.get(var_name) {
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

        let mut declare_stderr = String::new();
        let mut declare_exit_code: i32 = 0;

        // Set variables
        for name in &merged_names {
            if let Some(eq_pos) = name.find('=') {
                let var_name = &name[..eq_pos];
                let value = &name[eq_pos + 1..];

                // THREAT[TM-INJ-012]: Block internal variable prefix injection via declare
                if is_internal_variable(var_name) {
                    continue;
                }

                // THREAT[TM-INJ-020]: Refuse to overwrite readonly variables and
                // surface the error so callers cannot mistake a silent skip for success.
                if self.is_var_readonly(var_name) {
                    declare_stderr
                        .push_str(&format!("bash: declare: {var_name}: readonly variable\n"));
                    declare_exit_code = 1;
                    continue;
                }

                // Handle compound array assignment: declare -A m=([k]="v" ...)
                if (flags.assoc || flags.array) && value.starts_with('(') && value.ends_with(')') {
                    let inner = &value[1..value.len() - 1];
                    if flags.assoc {
                        let arr = self
                            .assoc_arrays_mut()
                            .entry(var_name.to_string())
                            .or_default();
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
                        let arr = self.arrays_mut().entry(var_name.to_string()).or_default();
                        arr.clear();
                        for (idx, val) in inner.split_whitespace().enumerate() {
                            arr.insert(idx, val.trim_matches('"').to_string());
                        }
                    }
                } else if flags.nameref {
                    // declare -n ref=target: create nameref
                    self.set_nameref(var_name, value.to_string());
                } else if flags.integer {
                    // Evaluate as arithmetic expression
                    let int_val = self.evaluate_arithmetic_with_assign(value);
                    self.insert_variable_checked(var_name.to_string(), int_val.to_string());
                    // Set persistent integer attribute marker
                    self.add_var_attr(var_name, VarAttrs::INTEGER);
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
                    self.add_var_attr(var_name, VarAttrs::LOWER);
                    self.remove_var_attr(var_name, VarAttrs::UPPER);
                }
                if is_uppercase {
                    self.add_var_attr(var_name, VarAttrs::UPPER);
                    self.remove_var_attr(var_name, VarAttrs::LOWER);
                }
                if is_readonly {
                    self.add_var_attr(var_name, VarAttrs::READONLY);
                }
                if is_export {
                    self.env.insert(
                        var_name.to_string(),
                        self.scoped
                            .variables
                            .get(var_name)
                            .cloned()
                            .unwrap_or_default(),
                    );
                }
            } else {
                // Declare without value
                if remove_nameref {
                    // typeset +n ref: remove nameref attribute
                    self.remove_nameref(name);
                } else if flags.nameref {
                    // typeset -n ref (without =value): use existing variable value as target
                    if let Some(existing) = self.scoped.variables.get(name.as_str()).cloned()
                        && !existing.is_empty()
                    {
                        self.set_nameref(name, existing);
                    }
                } else if flags.assoc {
                    // Initialize empty associative array
                    self.assoc_arrays_mut().entry(name.to_string()).or_default();
                } else if flags.array {
                    // Initialize empty indexed array
                    self.arrays_mut().entry(name.to_string()).or_default();
                } else if !self.scoped.variables.contains_key(name.as_str()) {
                    self.insert_variable_checked(name.to_string(), String::new());
                }
                // Set case conversion attribute markers
                if is_lowercase {
                    self.add_var_attr(name, VarAttrs::LOWER);
                    self.remove_var_attr(name, VarAttrs::UPPER);
                }
                if is_uppercase {
                    self.add_var_attr(name, VarAttrs::UPPER);
                    self.remove_var_attr(name, VarAttrs::LOWER);
                }
                if is_readonly {
                    self.add_var_attr(name, VarAttrs::READONLY);
                }
                if flags.integer {
                    self.add_var_attr(name, VarAttrs::INTEGER);
                }
                if is_export {
                    self.env.insert(
                        name.to_string(),
                        self.scoped
                            .variables
                            .get(name.as_str())
                            .cloned()
                            .unwrap_or_default(),
                    );
                }
            }
        }

        let mut result = ExecResult {
            stderr: declare_stderr,
            exit_code: declare_exit_code,
            ..Default::default()
        };
        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
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
                        fd_var: None,
                        kind: RedirectKind::HereString,
                        target: Word::literal(stdin_data.trim_end_matches('\n').to_string()),
                    }]
                } else {
                    Vec::new()
                };

                let inner_cmd = Command::Simple(SimpleCommand {
                    name: Word::quoted_literal(command.name),
                    args: command
                        .args
                        .iter()
                        .map(|s| Word::quoted_literal(s.clone()))
                        .collect(),
                    redirects: inner_redirects,
                    assignments: Vec::new(),
                    span: Span::new(),
                });

                let baseline_call_stack_len = self.call_stack.len();
                let baseline_bash_source_len = self.bash_source_stack.len();
                let baseline_function_depth = self.counters.function_depth;
                let baseline_pipeline_stdin = self.pipeline_stdin.clone();
                let exec_future = self.execute_command(&inner_cmd);
                match timeout(duration, exec_future).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        self.reconcile_cancelled_execution_state(
                            baseline_call_stack_len,
                            baseline_bash_source_len,
                            baseline_function_depth,
                            baseline_pipeline_stdin,
                        );
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
                            fd_var: None,
                            kind: RedirectKind::HereString,
                            target: Word::literal(stdin_data.trim_end_matches('\n').to_string()),
                        }]
                    } else {
                        Vec::new()
                    };

                    let inner_cmd = Command::Simple(SimpleCommand {
                        name: Word::quoted_literal(cmd.name),
                        args: cmd
                            .args
                            .iter()
                            .map(|s| Word::quoted_literal(s.clone()))
                            .collect(),
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
            builtins::ExecutionPlan::BatchWithStatus {
                commands,
                stderr_prefix,
                force_error_exit,
            } => {
                let mut combined_stdout = String::new();
                let mut combined_stderr = stderr_prefix;
                let mut last_exit_code = 0;

                for cmd in commands {
                    let cmd_redirects = if let Some(ref stdin_data) = cmd.stdin {
                        vec![Redirect {
                            fd: None,
                            fd_var: None,
                            kind: RedirectKind::HereString,
                            target: Word::literal(stdin_data.trim_end_matches('\n').to_string()),
                        }]
                    } else {
                        Vec::new()
                    };

                    let inner_cmd = Command::Simple(SimpleCommand {
                        name: Word::quoted_literal(cmd.name),
                        args: cmd
                            .args
                            .iter()
                            .map(|s| Word::quoted_literal(s.clone()))
                            .collect(),
                        redirects: cmd_redirects,
                        assignments: Vec::new(),
                        span: Span::new(),
                    });

                    let result = self.execute_command(&inner_cmd).await?;
                    combined_stdout.push_str(&result.stdout);
                    combined_stderr.push_str(&result.stderr);
                    last_exit_code = result.exit_code;
                }

                let exit_code = if force_error_exit && last_exit_code == 0 {
                    1
                } else {
                    last_exit_code
                };

                ExecResult {
                    stdout: combined_stdout,
                    stderr: combined_stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                    ..Default::default()
                }
            }
        };

        self.apply_redirections(result, redirects).await
    }

    /// Restore interpreter stacks/counters after an in-flight command future is cancelled.
    fn reconcile_cancelled_execution_state(
        &mut self,
        baseline_call_stack_len: usize,
        baseline_bash_source_len: usize,
        baseline_function_depth: usize,
        baseline_pipeline_stdin: Option<String>,
    ) {
        let leaked_call_frames = self
            .call_stack
            .len()
            .saturating_sub(baseline_call_stack_len);
        let leaked_bash_source_entries = self
            .bash_source_stack
            .len()
            .saturating_sub(baseline_bash_source_len);

        if leaked_call_frames > 0 {
            self.call_stack.truncate(baseline_call_stack_len);
        }
        if leaked_bash_source_entries > 0 {
            self.bash_source_stack.truncate(baseline_bash_source_len);
            self.update_bash_source();
        }

        // Some cancellable paths push call frames or BASH_SOURCE without pushing function depth.
        self.counters.function_depth = baseline_function_depth;
        self.pipeline_stdin = baseline_pipeline_stdin;

        if self.call_stack.is_empty() {
            self.arrays_mut().remove("FUNCNAME");
        } else {
            let funcname_arr: HashMap<usize, String> = self
                .call_stack
                .iter()
                .rev()
                .enumerate()
                .map(|(i, f)| (i, f.name.clone()))
                .collect();
            self.arrays_mut()
                .insert("FUNCNAME".to_string(), funcname_arr);
        }
    }

    /// Process structured side effects from builtin execution.
    async fn apply_builtin_side_effects(&mut self, result: &ExecResult) {
        // Builtins that mutate SHOPT_* directly via `ctx.variables` (e.g. the
        // `set -e` / `set +u` paths in the `set` builtin) don't update the
        // cached `flags` bitfield. Resync once after every builtin so the
        // bit cache can stay authoritative on the hot path. The scan covers
        // ~10 SHOPT_* entries — cheaper than threading a structured "shopt
        // changed" channel through every builtin.
        self.refresh_shopt_flags();
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
                    self.arrays_mut().remove(name);
                    if !arr.is_empty() {
                        self.insert_array_checked(name.clone(), arr);
                    }
                }
                builtins::BuiltinSideEffect::RemoveArray(name) => {
                    self.arrays_mut().remove(name);
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
                            local_arrays: HashMap::new(),
                            local_assoc_arrays: HashMap::new(),
                            positional: new_positional.clone(),
                        });
                    }
                }
                builtins::BuiltinSideEffect::ClearHistory => {
                    self.clear_history();
                    // Persist immediately so `history -c` is a same-exec sanitization boundary.
                    self.save_history().await;
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

    /// Resolve a path relative to cwd, normalizing `.` and `..` components.
    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        };
        crate::fs::normalize_path(&joined)
    }

    /// Expand a process substitution (`<(cmd)` or `>(cmd)`).
    async fn expand_process_substitution(
        &mut self,
        commands: &[Command],
        is_input: bool,
    ) -> Result<String> {
        if self.shell_profile.is_logic_only() {
            return Err(crate::error::Error::Execution(
                "bash: process substitution disabled in logic-only shell".to_string(),
            ));
        }

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
                self.proc_sub_paths.insert(path_str.clone());
                Ok(path_str)
            }
        } else {
            let _ = self.fs.write_file(path, b"").await;
            self.proc_sub_paths.insert(path_str.clone());
            self.deferred_proc_subs
                .push((path_str.clone(), commands.to_vec()));
            Ok(path_str)
        }
    }

    // THREAT[TM-DOS-089]: Command substitution body extracted into a Box::pin-ed
    // helper to cap per-level stack usage. Without this, each $(...) nesting level
    // adds the full expand_word state machine to the call stack, causing overflow
    // at moderate depths despite the logical depth limit.
    /// Snapshot the subshell-isolated portion of interpreter state.
    /// Used by `$(...)` and arithmetic substitution to undo any mutations the
    /// substituted command performed. Each `Arc<HashMap>` clones in O(1)
    /// (refcount bump); only a substitution that actually mutates state pays
    /// for a real HashMap clone, and only the maps it actually touched.
    fn snapshot_subshell_state(&self) -> SubshellSnapshot {
        SubshellSnapshot {
            scoped: self.scoped.clone(),
            flags: self.flags,
            cwd: self.cwd.clone(),
            memory_budget: self.memory_budget.clone(),
            exec_fd_table: self.exec_fd_table.clone(),
            random_state: self.random_state.load(Ordering::Relaxed),
            getopts_char_idx: self.getopts_char_idx,
            last_bg_pid: self.last_bg_pid.clone(),
        }
    }

    fn restore_subshell_state(&mut self, snap: SubshellSnapshot) {
        self.scoped = snap.scoped;
        self.flags = snap.flags;
        self.cwd = snap.cwd;
        self.memory_budget = snap.memory_budget;
        self.exec_fd_table = snap.exec_fd_table;
        self.random_state
            .store(snap.random_state, Ordering::Relaxed);
        self.getopts_char_idx = snap.getopts_char_idx;
        self.last_bg_pid = snap.last_bg_pid;
    }

    fn execute_cmd_subst<'a>(
        &'a mut self,
        commands: &'a [Command],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            // Command substitution runs in a subshell: snapshot all
            // mutable state so mutations don't leak to the parent.
            let snapshot = self.snapshot_subshell_state();
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
            if let Some(trap_cmd) = self.scoped.traps.get("EXIT").cloned()
                && snapshot.scoped.traps.get("EXIT") != Some(&trap_cmd)
                && let Ok(trap_script) = Parser::with_limits(
                    &trap_cmd,
                    self.limits.max_ast_depth,
                    self.limits.max_parser_operations,
                )
                .parse()
                && let Ok(trap_result) = self
                    .execute_capture_only_sequence(&trap_script.commands)
                    .await
            {
                stdout.push_str(&trap_result.stdout);
            }
            self.restore_subshell_state(snapshot);
            self.counters.pop_subst();
            self.subst_generation += 1;
            let trimmed = stdout.trim_end_matches('\n');
            Ok(trimmed.to_string())
        })
    }

    /// Maximum recursion depth for arithmetic expression evaluation.
    /// THREAT[TM-DOS-026]: Prevents stack overflow via deeply nested arithmetic like
    /// $(((((((...)))))))
    const MAX_ARITHMETIC_DEPTH: usize = 50;
    /// Shared recursion fuel for arithmetic variable expansion.
    /// THREAT[TM-DOS-026]: Bounds branching recursive variable expressions before they allocate exponentially.
    const MAX_ARITHMETIC_EXPANSION_FUEL: usize = 8192;
    /// Maximum expanded arithmetic expression size accepted before fallback to 0.
    /// THREAT[TM-DOS-026]: Prevents attacker-controlled multi-megabyte arithmetic strings.
    const MAX_ARITHMETIC_EXPANSION_BYTES: usize = 64 * 1024;

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

    /// Fully expand an associative array key using standard word expansion.
    /// This preserves literal bare names (e.g. `x` -> `x`) while correctly
    /// expanding embedded/multiple parameter references (e.g. `foo$bar`).
    async fn expand_assoc_key(&mut self, s: &str) -> Result<String> {
        let word = Parser::parse_word_string_with_limits(
            s,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        self.expand_word(&word).await
    }

    /// THREAT[TM-INJ-009]: Check if a variable name is an internal marker.
    fn is_internal_variable(name: &str) -> bool {
        is_internal_variable(name)
    }

    /// THREAT[TM-INF-017]: Check if a variable should be hidden from output.
    fn is_hidden_variable(name: &str) -> bool {
        is_hidden_variable(name)
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
        // Resolve nameref: if `name` is a nameref, assign to the target
        // instead. The common case (no nameref) reuses `name` without
        // allocating; only nameref hops allocate a fresh owned target.
        let resolved_string: String = {
            let resolved = self.resolve_nameref(&name);
            if std::ptr::eq(resolved.as_ptr(), name.as_ptr()) {
                name
            } else {
                resolved.to_string()
            }
        };
        let resolved: &str = resolved_string.as_str();
        // RANDOM=N reseeds the PRNG (matches bash behavior)
        if resolved == "RANDOM" {
            self.random_state
                .store(value.parse::<u32>().unwrap_or(0), Ordering::Relaxed);
            return;
        }
        // Attribute lookup is now a single map probe + bit test.
        let attrs = self.var_attrs_get(resolved);
        // THREAT[TM-INJ-019/020/021]: Block assignment to readonly variables
        if attrs.contains(VarAttrs::READONLY) {
            return;
        }
        // Apply integer attribute (declare -i): evaluate as arithmetic
        let value = if attrs.contains(VarAttrs::INTEGER) {
            self.evaluate_arithmetic_with_assign(&value).to_string()
        } else {
            value
        };
        // Apply case conversion attributes (declare -l / declare -u)
        let value = if attrs.contains(VarAttrs::LOWER) {
            value.to_lowercase()
        } else if attrs.contains(VarAttrs::UPPER) {
            value.to_uppercase()
        } else {
            value
        };
        // Check allexport (set -a): auto-export to env — now a bit test.
        let allexport = self.flags.contains(BashFlags::ALLEXPORT);

        // Walk the call stack top-down looking for an existing local binding.
        // The previous implementation cloned `resolved_string` for the
        // `entry()` API in every frame even when the key was absent — for a
        // tight `for i in {1..N}; do x=$((x+1)); done` inside a function this
        // is one clone per iteration. Using `get_mut` skips the clone unless
        // we actually have a local to update.
        for frame_idx in (0..self.call_stack.len()).rev() {
            if let Some(old_val_len) = self.call_stack[frame_idx]
                .locals
                .get(resolved)
                .map(String::len)
            {
                if self
                    .memory_budget
                    .check_variable_insert(
                        resolved.len(),
                        value.len(),
                        false,
                        resolved.len(),
                        old_val_len,
                        &self.memory_limits,
                    )
                    .is_err()
                {
                    return;
                }
                self.memory_budget.record_variable_insert(
                    resolved.len(),
                    value.len(),
                    false,
                    resolved.len(),
                    old_val_len,
                );
                if allexport {
                    let env_value = value.clone();
                    self.call_stack[frame_idx]
                        .locals
                        .insert(resolved_string.clone(), value);
                    self.insert_env_checked(resolved_string, env_value);
                    return;
                }
                self.call_stack[frame_idx]
                    .locals
                    .insert(resolved_string, value);
                return;
            }
        }
        // No local frame matched — insert at global scope. Only allexport
        // needs the extra clone for the env mirror; the common path moves
        // `value` straight into `variables`.
        if allexport {
            let env_value = value.clone();
            if self.insert_variable_checked(resolved_string.clone(), value) {
                self.insert_env_checked(resolved_string, env_value);
            }
        } else {
            self.insert_variable_checked(resolved_string, value);
        }
    }

    /// Resolve an indexed-array subscript the same way for read-before-write and write paths.
    fn resolve_indexed_array_subscript(&self, arr_name: &str, key: &str) -> usize {
        let raw_idx = self.evaluate_arithmetic(key);
        if raw_idx < 0 {
            let len = self
                .scoped
                .arrays
                .get(arr_name)
                .and_then(|a| a.keys().max().map(|m| m.saturating_add(1) as i128))
                .unwrap_or(0);
            (len + raw_idx as i128).max(0) as usize
        } else {
            raw_idx as usize
        }
    }

    /// Set a parameter expansion assignment target (`:=`), including array elements.
    fn set_parameter_expansion_target(&mut self, name: &str, value: String) {
        if let Some(bracket) = name.find('[')
            && name.ends_with(']')
        {
            let arr_name = &name[..bracket];
            let key = &name[bracket + 1..name.len() - 1];
            let resolved_name = self.resolve_nameref(arr_name).to_string();

            if self.scoped.assoc_arrays.contains_key(&resolved_name) {
                let expanded_key = self.expand_variable_or_literal(key);
                let is_new_entry = self
                    .scoped
                    .assoc_arrays
                    .get(&resolved_name)
                    .is_none_or(|a| !a.contains_key(&expanded_key));
                if is_new_entry
                    && self
                        .memory_budget
                        .check_array_entries(1, &self.memory_limits)
                        .is_err()
                {
                    return;
                }
                if is_new_entry {
                    self.memory_budget.record_array_insert(1);
                }
                self.assoc_arrays_mut()
                    .entry(resolved_name)
                    .or_default()
                    .insert(expanded_key, value);
                return;
            }

            let index = self.resolve_indexed_array_subscript(&resolved_name, key);
            let is_new_entry = self
                .scoped
                .arrays
                .get(&resolved_name)
                .is_none_or(|a| !a.contains_key(&index));
            if is_new_entry
                && self
                    .memory_budget
                    .check_array_entries(1, &self.memory_limits)
                    .is_err()
            {
                return;
            }
            if is_new_entry {
                self.memory_budget.record_array_insert(1);
            }
            self.arrays_mut()
                .entry(resolved_name)
                .or_default()
                .insert(index, value);
            return;
        }

        self.set_variable(name.to_string(), value);
    }

    /// Insert a variable into the global variables map with memory budget checking.
    /// Silently drops the insert if the budget would be exceeded.
    /// Internal marker variables (_READONLY_, _NAMEREF_, etc.) bypass budget checks.
    fn insert_variable_checked(&mut self, key: String, value: String) -> bool {
        let is_internal = Self::is_internal_variable(&key);
        if !is_internal {
            let is_new = !self.scoped.variables.contains_key(&key);
            let (old_key_len, old_value_len) = if is_new {
                (0, 0)
            } else {
                (
                    key.len(),
                    self.scoped.variables.get(&key).map_or(0, |v| v.len()),
                )
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
                return false; // silently reject — budget exceeded
            }
            self.memory_budget.record_variable_insert(
                key.len(),
                value.len(),
                is_new,
                old_key_len,
                old_value_len,
            );
        }
        // Keep the SHOPT flag cache in sync whenever SHOPT_* gets written.
        // Internal callers that bulk-insert variables (snapshot restore,
        // SHOPT bookkeeping in `execute_shell`) go through this routine, so
        // hooking it here is the single sync point.
        if let Some(bit) = BashFlags::from_shopt_name(&key) {
            if value == "1" {
                self.flags.insert(bit);
            } else {
                self.flags.remove(bit);
            }
        }
        self.vars_mut().insert(key, value);
        true
    }

    /// Insert a variable into the current local frame with memory budget checking.
    /// Silently drops the insert if the budget would be exceeded.
    fn insert_local_checked(&mut self, key: String, value: String) {
        let Some(frame) = self.call_stack.last() else {
            return;
        };
        let is_new = !frame.locals.contains_key(&key);
        let old_value_len = frame.locals.get(&key).map_or(0, String::len);
        let (old_key_len, old_value_len) = if is_new {
            (0, 0)
        } else {
            (key.len(), old_value_len)
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

        if let Some(frame) = self.call_stack.last_mut() {
            frame.locals.insert(key, value);
        }
    }

    /// Insert/update an environment variable with memory limit checks.
    /// Uses the variable limits to bound environment growth.
    fn insert_env_checked(&mut self, key: String, value: String) {
        let is_new = !self.env.contains_key(&key);
        if is_new && self.env.len() >= self.memory_limits.max_variable_count {
            return;
        }

        let old_value_len = self.env.get(&key).map_or(0, |v| v.len());
        let old_key_len = if is_new { 0 } else { key.len() };
        let current_env_bytes: usize = self.env.iter().map(|(k, v)| k.len() + v.len()).sum();
        let new_env_bytes = (current_env_bytes + key.len() + value.len())
            .saturating_sub(old_key_len + old_value_len);
        if new_env_bytes > self.memory_limits.max_total_variable_bytes {
            return;
        }

        self.env.insert(key, value);
    }

    /// Pop a call frame and restore any global array bindings shadowed by `local -a/-A`.
    fn pop_call_frame(&mut self) -> Option<CallFrame> {
        let frame = self.call_stack.pop()?;
        for (name, previous) in &frame.local_arrays {
            self.restore_array_binding(name, previous.clone());
        }
        for (name, previous) in &frame.local_assoc_arrays {
            self.restore_assoc_array_binding(name, previous.clone());
        }
        Some(frame)
    }

    /// Remember the array binding that a local indexed array declaration shadows.
    /// Snapshot the indexed-array binding a local declaration shadows.
    ///
    /// Returns `true` only when this call retained a *new* snapshot in the
    /// frame. A later shadow of the same name within the same frame keeps the
    /// first snapshot (`or_insert`) and returns `false`, signalling that the
    /// binding being replaced is a transient local — not retained anywhere —
    /// so its entries must be released from the array budget.
    fn remember_local_array_binding(&mut self, name: &str) -> bool {
        let previous = self.scoped.arrays.get(name).cloned();
        if let Some(frame) = self.call_stack.last_mut() {
            if frame.local_arrays.contains_key(name) {
                return false;
            }
            frame.local_arrays.insert(name.to_string(), previous);
            return true;
        }
        false
    }

    /// Snapshot the associative-array binding a local declaration shadows.
    /// See [`remember_local_array_binding`](Self::remember_local_array_binding)
    /// for the meaning of the return value.
    fn remember_local_assoc_array_binding(&mut self, name: &str) -> bool {
        let previous = self.scoped.assoc_arrays.get(name).cloned();
        if let Some(frame) = self.call_stack.last_mut() {
            if frame.local_assoc_arrays.contains_key(name) {
                return false;
            }
            frame.local_assoc_arrays.insert(name.to_string(), previous);
            return true;
        }
        false
    }

    fn restore_array_binding(&mut self, name: &str, previous: Option<HashMap<usize, String>>) {
        let old_entries = self.scoped.arrays.get(name).map_or(0, |a| a.len());
        // Saved bindings remain budgeted while shadowed; popping only releases
        // entries allocated by the local binding currently active in arrays.
        self.memory_budget.record_array_remove(old_entries);
        if let Some(arr) = previous {
            self.arrays_mut().insert(name.to_string(), arr);
        } else {
            self.arrays_mut().remove(name);
        }
    }

    fn restore_assoc_array_binding(
        &mut self,
        name: &str,
        previous: Option<HashMap<String, String>>,
    ) {
        let old_entries = self.scoped.assoc_arrays.get(name).map_or(0, |a| a.len());
        // Saved bindings remain budgeted while shadowed; popping only releases
        // entries allocated by the local binding currently active in assoc_arrays.
        self.memory_budget.record_array_remove(old_entries);
        if let Some(arr) = previous {
            self.assoc_arrays_mut().insert(name.to_string(), arr);
        } else {
            self.assoc_arrays_mut().remove(name);
        }
    }

    /// Insert an array with memory budget checking.
    /// Returns true if the insert succeeded.
    fn insert_array_checked(&mut self, name: String, arr: HashMap<usize, String>) -> bool {
        let new_entries = arr.len();
        let old_entries = self.scoped.arrays.get(&name).map_or(0, |a| a.len());
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
        self.arrays_mut().insert(name, arr);
        true
    }

    /// Insert an associative array with memory budget checking.
    /// Returns true if the insert succeeded.
    #[allow(dead_code)]
    fn insert_assoc_array_checked(&mut self, name: String, arr: HashMap<String, String>) -> bool {
        let new_entries = arr.len();
        let old_entries = self.scoped.assoc_arrays.get(&name).map_or(0, |a| a.len());
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
        self.assoc_arrays_mut().insert(name, arr);
        true
    }

    /// Resolve nameref chains: if `name` has a `_NAMEREF_<name>` marker,
    /// follow the chain (up to 10 levels to prevent infinite loops).
    fn resolve_nameref<'a>(&'a self, name: &'a str) -> &'a str {
        // Fast path: most variables aren't namerefs. One hashmap lookup decides.
        if self.scoped.namerefs.is_empty() {
            return name;
        }
        let mut current = name;
        let mut visited = std::collections::HashSet::new();
        visited.insert(name);
        for _ in 0..10 {
            if let Some(target) = self.scoped.namerefs.get(current) {
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
                // Execute the command and substitute in a subshell context:
                // save/restore mutable state so mutations don't leak.
                let parser = Parser::with_limits(
                    &cmd,
                    self.limits.max_ast_depth,
                    self.limits.max_parser_operations,
                );
                match parser.parse() {
                    Ok(script) => {
                        if self.counters.push_subst(&self.limits).is_err() {
                            result.push('0');
                        } else {
                            let snapshot = self.snapshot_subshell_state();
                            let cmd_result =
                                self.execute_command_sequence(&script.commands).await?;
                            self.restore_subshell_state(snapshot);
                            self.counters.pop_subst();
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
        // THREAT[TM-DOS-036]: IFS separator lookup must not use generic
        // expansion. Namerefs can point IFS at special parameters like `*`,
        // whose expansion re-enters this function. Resolve only regular
        // variable storage so local IFS and valid nameref targets still work.
        let name = self.resolve_nameref("IFS");
        if let Some(ifs) = self.lookup_regular_variable(name) {
            ifs.chars()
                .next()
                .map(|c| c.to_string())
                .unwrap_or_default()
        } else {
            // IFS unset: default separator is space
            " ".to_string()
        }
    }

    fn lookup_regular_variable(&self, name: &str) -> Option<String> {
        for frame in self.call_stack.iter().rev() {
            if let Some(value) = frame.locals.get(name) {
                return Some(value.clone());
            }
        }

        if let Some(value) = self.scoped.variables.get(name) {
            return Some(value.clone());
        }

        self.env.get(name).cloned()
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
            if let Some(arr) = self.scoped.assoc_arrays.get(arr_name) {
                return arr.get(idx_str).cloned().unwrap_or_default();
            } else if let Some(arr) = self.scoped.arrays.get(arr_name) {
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
                if let Some(last_bg_pid) = &self.last_bg_pid {
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
                        .scoped
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
                // $RANDOM - LCG matching bash behavior, seeded per-instance.
                // LCG: state = state * 1103515245 + 12345 (glibc constants)
                let prev = self.random_state.load(Ordering::Relaxed);
                let next = prev.wrapping_mul(1103515245).wrapping_add(12345);
                self.random_state.store(next, Ordering::Relaxed);
                return ((next >> 16) & 0x7fff).to_string();
            }
            "LINENO" => {
                // $LINENO - current line number from command span
                return self.current_line.to_string();
            }
            "PWD" => {
                return self.cwd.to_string_lossy().to_string();
            }
            "OLDPWD" => {
                if let Some(v) = self.scoped.variables.get("OLDPWD") {
                    return v.clone();
                }
                return self.cwd.to_string_lossy().to_string();
            }
            "HOSTNAME" => {
                if let Some(v) = self.scoped.variables.get("HOSTNAME") {
                    return v.clone();
                }
                return "localhost".to_string();
            }
            "BASH_VERSION" => {
                return COMPAT_BASH_VERSION.to_string();
            }
            "SECONDS" => {
                // Seconds since shell started - always 0 in stateless model
                if let Some(v) = self.scoped.variables.get("SECONDS") {
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

        self.lookup_regular_variable(name).unwrap_or_default()
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
        if self.scoped.variables.contains_key(name) {
            return true;
        }
        // Environment
        self.env.contains_key(name)
    }

    /// Check if nounset (`set -u`) is active.
    fn is_nounset(&self) -> bool {
        self.flags.contains(BashFlags::NOUNSET)
    }

    /// Check if pipefail (`set -o pipefail`) is active.
    fn is_pipefail(&self) -> bool {
        self.flags.contains(BashFlags::PIPEFAIL)
    }

    /// Run ERR trap if registered. Appends trap output to stdout/stderr.
    /// Run the DEBUG trap handler (fires before each simple command).
    /// Returns (stdout, stderr) from the trap handler.
    async fn run_debug_trap(&mut self) -> (String, String) {
        // THREAT[TM-DOS-035]: Suppress DEBUG trap inside trap handlers to prevent
        // recursive amplification (each trapped command firing more DEBUG traps).
        if self.in_trap {
            return (String::new(), String::new());
        }
        if let Some(trap_cmd) = self.scoped.traps.get("DEBUG").cloned() {
            // THREAT[TM-DOS-030]: Propagate interpreter parser limits
            if let Ok(trap_script) = Parser::with_limits(
                &trap_cmd,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            )
            .parse()
            {
                self.in_trap = true;
                let emit_before = self.output_emit_count;
                let result = self.execute_command_sequence(&trap_script.commands).await;
                self.in_trap = false;
                if let Ok(trap_result) = result {
                    self.maybe_emit_output(&trap_result.stdout, &trap_result.stderr, emit_before);
                    return (trap_result.stdout, trap_result.stderr);
                }
            }
        }
        (String::new(), String::new())
    }

    async fn run_err_trap(&mut self, stdout: &mut String, stderr: &mut String) {
        // THREAT[TM-DOS-035]: Suppress ERR trap re-entrancy while executing trap
        // handlers to prevent recursive ERR -> ERR amplification.
        if self.in_trap {
            return;
        }
        if let Some(trap_cmd) = self.scoped.traps.get("ERR").cloned() {
            // THREAT[TM-DOS-030]: Propagate interpreter parser limits
            if let Ok(trap_script) = Parser::with_limits(
                &trap_cmd,
                self.limits.max_ast_depth,
                self.limits.max_parser_operations,
            )
            .parse()
            {
                self.in_trap = true;
                let emit_before = self.output_emit_count;
                let result = self.execute_command_sequence(&trap_script.commands).await;
                self.in_trap = false;
                if let Ok(trap_result) = result {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bash;
    use crate::fs::InMemoryFs;
    use crate::parser::Parser;

    /// TM-DOS-042: comma-list brace expansion must not recurse one frame per
    /// brace group (stack overflow) nor accumulate unbounded memory. A long
    /// `{a,b}{a,b}...` sequence — far under the input cap — used to descend to
    /// full depth before any cap engaged.
    #[test]
    fn brace_expansion_comma_sequence_is_bounded() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let interp = Interpreter::new(Arc::clone(&fs));
        let s = "{a,b}".repeat(50_000);
        let out = interp.expand_braces(&s);
        // Must terminate without panic/overflow and stay bounded.
        let total: usize = out.iter().map(String::len).sum();
        assert!(
            total <= Interpreter::MAX_EXPANSION_RESULT_BYTES + 1024,
            "brace expansion produced {total} bytes — should be byte-capped"
        );
    }

    #[test]
    fn test_empty_anchored_replacement_respects_expansion_limit() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let interp = Interpreter::new(Arc::clone(&fs));
        let replacement = "a".repeat(Interpreter::MAX_EXPANSION_RESULT_BYTES + 1);

        assert_eq!(interp.replace_pattern("x", "#", &replacement, false), "x");
        assert_eq!(interp.replace_pattern("x", "%", &replacement, false), "x");
    }

    #[test]
    fn test_per_element_param_expansion_respects_aggregate_limit() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        let replacement = "a".repeat(2048);
        interp.set_variable("p".to_string(), replacement);
        interp.call_stack.push(CallFrame {
            name: "f".to_string(),
            locals: HashMap::new(),
            local_arrays: HashMap::new(),
            local_assoc_arrays: HashMap::new(),
            positional: vec!["x".to_string(); 6000],
        });

        let value = interp.resolve_param_expansion_name("@").1;
        let expanded = interp.apply_param_op_maybe_per_element(
            &value,
            "@",
            &ParameterOp::ReplaceFirst {
                pattern: "#".to_string(),
                replacement: "$p".to_string(),
            },
            "",
            false,
            true,
        );

        assert_eq!(expanded, value);
        assert!(expanded.len() < Interpreter::MAX_EXPANSION_RESULT_BYTES);
    }

    #[test]
    fn test_try_expand_range_alpha_large_step_does_not_loop() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let interp = Interpreter::new(Arc::clone(&fs));
        assert_eq!(
            interp.try_expand_range("a..z..256"),
            Some(vec!["a".to_string()])
        );
        assert_eq!(
            interp.try_expand_range("z..a..-256"),
            Some(vec!["z".to_string()])
        );
    }

    #[test]
    fn test_try_expand_range_numeric_large_step_does_not_overflow() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let interp = Interpreter::new(Arc::clone(&fs));

        assert_eq!(
            interp
                .try_expand_range("9223372036854775802..9223372036854775807..9223372036854775807"),
            Some(vec!["9223372036854775802".to_string()])
        );
        assert_eq!(
            interp.try_expand_range(
                "-9223372036854775803..-9223372036854775808..-9223372036854775808"
            ),
            Some(vec!["-9223372036854775803".to_string()])
        );
    }

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

    #[tokio::test(start_paused = true)]
    async fn test_timeout_does_not_leak_function_locals() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        let parser =
            Parser::new("f(){ local secret=shh; sleep 10; }\ntimeout 0.001 f\necho \"<$secret>\"");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(result.stdout.trim(), "<>");
    }

    #[tokio::test(start_paused = true)]
    async fn test_timeout_does_not_leak_bash_stdin_to_following_command() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        let parser = Parser::new("printf secret | timeout 0.001 bash -c 'sleep 10'; cat");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[test]
    fn test_cancelled_shell_frame_does_not_pop_function_depth() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.counters.function_depth = 1;
        interp.call_stack.push(CallFrame {
            name: "caller".to_string(),
            locals: HashMap::new(),
            local_arrays: HashMap::new(),
            local_assoc_arrays: HashMap::new(),
            positional: Vec::new(),
        });
        let baseline_call_stack_len = interp.call_stack.len();
        let baseline_bash_source_len = interp.bash_source_stack.len();
        let baseline_function_depth = interp.counters.function_depth;

        interp.call_stack.push(CallFrame {
            name: "bash".to_string(),
            locals: HashMap::new(),
            local_arrays: HashMap::new(),
            local_assoc_arrays: HashMap::new(),
            positional: Vec::new(),
        });
        interp.bash_source_stack.push("script.sh".to_string());

        interp.reconcile_cancelled_execution_state(
            baseline_call_stack_len,
            baseline_bash_source_len,
            baseline_function_depth,
            None,
        );

        assert_eq!(interp.call_stack.len(), baseline_call_stack_len);
        assert_eq!(interp.bash_source_stack.len(), baseline_bash_source_len);
        assert_eq!(interp.counters.function_depth, baseline_function_depth);
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

    /// Helper to run a script with custom limits and return result.
    async fn run_script_with_limits(
        script: &str,
        limits: ExecutionLimits,
        memory_limits: crate::limits::MemoryLimits,
    ) -> ExecResult {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_limits(limits);
        interp.set_memory_limits(memory_limits);
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        interp.execute(&ast).await.unwrap()
    }

    #[tokio::test]
    async fn test_ifs_split_field_limit_rejects_exploding_command_substitution() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_limits(ExecutionLimits::default().max_word_split_fields(3));
        let parser = Parser::new("IFS=,; for x in $(echo a,b,c,d); do :; done");
        let ast = parser.parse().unwrap();
        let err = interp.execute(&ast).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("word split field limit (3) exceeded")
        );
    }

    #[tokio::test]
    async fn test_ifs_split_byte_limit_rejects_large_materialized_field() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_limits(ExecutionLimits::default().max_word_split_bytes(5));
        let parser = Parser::new("v=abcdef; echo $v");
        let ast = parser.parse().unwrap();
        let err = interp.execute(&ast).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("word split byte limit (5) exceeded")
        );
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
    async fn test_allexport_respects_env_memory_limits() {
        let limits = ExecutionLimits::new();
        let memory_limits = crate::limits::MemoryLimits::new().max_variable_count(5);
        let mut script = String::from("set -a\n");
        for i in 0..20 {
            script.push_str(&format!("V{i}=x\n"));
        }
        script.push_str("export -p | grep -c '^declare -x V' || true\n");
        let result = run_script_with_limits(&script, limits, memory_limits).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "4");
    }

    #[test]
    fn test_allexport_rejected_global_update_does_not_mutate_env() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_memory_limits(crate::limits::MemoryLimits::new().max_total_variable_bytes(20));

        interp.set_variable("FILL".to_string(), "123456789012".to_string());
        interp.flags.insert(BashFlags::ALLEXPORT);
        interp.set_variable("A".to_string(), "1".to_string());
        interp.set_variable("A".to_string(), "1234567890".to_string());

        assert_eq!(
            interp.scoped.variables.get("A").map(String::as_str),
            Some("1")
        );
        assert_eq!(interp.env.get("A").map(String::as_str), Some("1"));
    }

    #[test]
    fn test_allexport_rejected_local_update_does_not_mutate_env() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_memory_limits(crate::limits::MemoryLimits::new().max_total_variable_bytes(20));
        interp.call_stack.push(CallFrame {
            name: "f".to_string(),
            locals: HashMap::from([("A".to_string(), "1".to_string())]),
            local_arrays: HashMap::new(),
            local_assoc_arrays: HashMap::new(),
            positional: Vec::new(),
        });
        interp
            .memory_budget
            .record_variable_insert(1, 1, true, 0, 0);
        interp.set_variable("FILL".to_string(), "123456789012".to_string());
        interp.flags.insert(BashFlags::ALLEXPORT);
        interp.insert_env_checked("A".to_string(), "1".to_string());

        interp.set_variable("A".to_string(), "1234567890".to_string());

        let frame = interp.call_stack.last().unwrap();
        assert_eq!(frame.locals.get("A").map(String::as_str), Some("1"));
        assert_eq!(interp.env.get("A").map(String::as_str), Some("1"));
    }

    #[tokio::test]
    async fn test_nested_loops_enforce_outer_loop_limit() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(2)
            .max_total_loop_iterations(100);
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_limits(limits);
        let parser = Parser::new("for i in 1 2 3; do for j in 1; do :; done; done; echo done");
        let ast = parser.parse().unwrap();
        let err = interp.execute(&ast).await.unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::ResourceLimit(crate::limits::LimitExceeded::MaxLoopIterations(2))
        ));
    }

    #[tokio::test]
    async fn test_nested_subshells_enforce_depth_limit() {
        let limits = ExecutionLimits::new().max_subshell_depth(2);
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_limits(limits);
        let parser = Parser::new("( ( ( echo too-deep ) ) )");
        let ast = parser.parse().unwrap();
        let err = interp.execute(&ast).await.unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::ResourceLimit(crate::limits::LimitExceeded::MaxSubshellDepth(2))
        ));
    }

    #[tokio::test]
    async fn test_pipeline_counts_each_stage_toward_command_limit() {
        let limits = ExecutionLimits::new().max_commands(2);
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.set_limits(limits);
        let parser = Parser::new("echo a | cat | cat");
        let ast = parser.parse().unwrap();
        let err = interp.execute(&ast).await.unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::ResourceLimit(crate::limits::LimitExceeded::MaxCommands(2))
        ));
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
    async fn test_exec_fd_in_subshell_does_not_leak_to_parent() {
        let result = run_script(
            "(exec 3>/tmp/subshell-fd.txt; echo child >&3); echo parent >&3; cat /tmp/subshell-fd.txt",
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("child"));
        assert!(!result.stdout.contains("parent"));
    }

    #[tokio::test]
    async fn test_exec_fd_in_command_substitution_does_not_leak_to_parent() {
        let result = run_script(
            "x=$(exec 3>/tmp/cmd-sub-fd.txt; echo child >&3); echo parent >&3; cat /tmp/cmd-sub-fd.txt",
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("child"));
        assert!(!result.stdout.contains("parent"));
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
    async fn test_bash_c_mutations_do_not_leak_to_parent() {
        // `bash -c` runs as a child process — variables it sets must not
        // become visible in the parent (real-bash semantics, see #1777).
        let result = run_script("bash -c 'X=inner'; echo \"[$X]\"").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "[]");
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
    async fn test_bash_script_file_enforces_max_input_bytes() {
        let fs = Arc::new(InMemoryFs::new());
        let large_script = "echo x\n".repeat(64);
        fs.write_file(
            std::path::Path::new("/tmp/large.sh"),
            large_script.as_bytes(),
        )
        .await
        .unwrap();

        let limits = ExecutionLimits::new().max_input_bytes(64);
        let mut interpreter = Interpreter::new(fs.clone());
        interpreter.set_limits(limits);
        let ast = Parser::new("bash /tmp/large.sh").parse().unwrap();
        let result = interpreter.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("input exceeds maximum size"));
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
        // Only *exported* variables are visible inside `bash -c` — a plain
        // assignment in the parent is not inherited (real-bash semantics, #1777).
        let unexported = run_script("X=test; bash -c 'echo \"[$X]\"'").await;
        assert_eq!(unexported.exit_code, 0);
        assert_eq!(unexported.stdout.trim(), "[]");

        let exported = run_script("export X=test; bash -c 'echo $X'").await;
        assert_eq!(exported.exit_code, 0);
        assert_eq!(exported.stdout.trim(), "test");
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

    #[tokio::test]
    async fn test_xargs_treats_stdin_as_literal_args() {
        // xargs should not glob-expand stdin-derived arguments.
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/workspace"), true)
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/workspace/a.txt"), b"A")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/workspace/b.txt"), b"B")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/workspace"));

        let parser = Parser::new("printf '*\\n' | xargs -I {} echo {}");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "*");
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
    async fn test_find_exec_preserves_literal_braces_in_path() {
        // Matched path must not undergo brace expansion when substituted into -exec args.
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(std::path::Path::new("/src"), true).await.unwrap();
        fs.write_file(std::path::Path::new("/src/{a,b}.txt"), b"literal")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/src/a.txt"), b"a")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/src/b.txt"), b"b")
            .await
            .unwrap();

        let mut interp = Interpreter::new(fs.clone());
        interp.set_cwd(std::path::PathBuf::from("/"));

        let script = r#"find /src -name "{a,b}.txt" -exec echo {} \;"#;
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "/src/{a,b}.txt");
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
    async fn test_arithmetic_base_suffix_pattern_with_double_percent() {
        let result = run_script("var='123foo%%bar'; echo $(( 10#${var%foo%%bar} ))").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "123");
    }

    #[tokio::test]
    async fn test_arithmetic_base_prefix_pattern_with_double_hash() {
        let result = run_script("var='foo##bar123'; echo $(( 10#${var#foo##bar} ))").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "123");
    }

    #[tokio::test]
    async fn test_arithmetic_nested_array_index_depth_guard() {
        let mut expr = "1".to_string();
        for _ in 0..(Interpreter::MAX_ARITHMETIC_DEPTH + 10) {
            expr = format!("arr[{expr}]");
        }
        let script = format!("arr[0]=0; arr[1]=1; echo $(({expr}))");
        let result = run_script(&script).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "0");
    }

    #[tokio::test]
    async fn test_arithmetic_self_referential_expression_is_bounded() {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            run_script("a='a+a'; echo $((a))"),
        )
        .await
        .expect("self-referential arithmetic expression should be bounded");

        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_arithmetic_self_referential_array_index_is_bounded() {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            run_script("arr[0]=1; i='arr[i]'; echo $((arr[i]))"),
        )
        .await
        .expect("self-referential arithmetic array index should be bounded");

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
    async fn test_eval_respects_max_input_bytes() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.limits.max_input_bytes = 8;
        let parser = Parser::new("eval 'echo 123456789'");
        let ast = parser.parse().unwrap();
        let err = interp.execute(&ast).await.unwrap_err();
        assert!(
            err.to_string().contains("input too large"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_source_respects_max_input_bytes() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        fs.write_file(
            std::path::Path::new("/tmp/large-source.sh"),
            b"echo 123456789",
        )
        .await
        .unwrap();
        let mut interp = Interpreter::new(Arc::clone(&fs));
        interp.limits.max_input_bytes = 8;
        let parser = Parser::new("source /tmp/large-source.sh");
        let ast = parser.parse().unwrap();
        let err = interp.execute(&ast).await.unwrap_err();
        assert!(
            err.to_string().contains("input too large"),
            "unexpected error: {err}"
        );
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
    async fn test_extglob_utf8_no_panic() {
        let result =
            run_script(r#"shopt -s extglob; v="é"; [[ "$v" == +(a) ]] && echo yes || echo no"#)
                .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "no");
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
    async fn test_regex_single_quoted_pattern_is_literal() {
        let mut bash = crate::Bash::new();
        let result = bash
            .exec(r#"re="200"; line="hello 200 world"; [[ $line =~ '$re' ]] && echo "match" || echo "no""#)
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "no");
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

    /// Issue #1277: glob `*` not expanded when adjacent to quoted variable expansion.
    /// In `"$var"*.ext`, the unquoted `*` must undergo glob expansion even though
    /// the word contains a quoted expansion (which suppresses IFS splitting).
    #[tokio::test]
    async fn test_glob_adjacent_to_quoted_variable() {
        let mut bash = crate::Bash::new();
        bash.fs()
            .mkdir(std::path::Path::new("/tmp/test"), true)
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/test/tag_hello.tmp.html"),
                b"hello",
            )
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/test/tag_world.tmp.html"),
                b"world",
            )
            .await
            .unwrap();

        // Test: ./"$p"*.tmp.html should expand the glob
        let result = bash
            .exec(r#"cd /tmp/test; p="tag_"; for f in ./"$p"*.tmp.html; do echo "$f"; done"#)
            .await
            .unwrap();
        let mut lines: Vec<&str> = result.stdout.trim().lines().collect();
        lines.sort();
        assert_eq!(
            lines,
            vec!["./tag_hello.tmp.html", "./tag_world.tmp.html"],
            "glob * adjacent to quoted var should expand"
        );

        // Test: ls ./"$p"*.tmp.html should also work
        let result = bash
            .exec(r#"cd /tmp/test; p="tag_"; ls ./"$p"*.tmp.html"#)
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "ls stderr: {}", result.stderr);
        assert!(
            result.stdout.contains("tag_hello.tmp.html"),
            "ls output: {}",
            result.stdout
        );
    }

    /// Quoted variable values must stay literal when an adjacent unquoted glob
    /// keeps pathname expansion enabled for the rest of the word.
    #[tokio::test]
    async fn test_quoted_variable_glob_chars_stay_literal_with_adjacent_glob() {
        let mut bash = crate::Bash::new();
        bash.fs()
            .mkdir(std::path::Path::new("/tmp/quoted_glob_literal"), true)
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/quoted_glob_literal/*literal.tmp"),
                b"literal",
            )
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/quoted_glob_literal/public.tmp"),
                b"public",
            )
            .await
            .unwrap();

        let result = bash
            .exec(r#"cd /tmp/quoted_glob_literal; p="*"; printf '%s\n' "$p"*.tmp"#)
            .await
            .unwrap();

        let mut lines: Vec<&str> = result.stdout.trim().lines().collect();
        lines.sort();
        assert_eq!(
            lines,
            vec!["*literal.tmp"],
            "glob chars from quoted variable must remain literal; stderr: {}",
            result.stderr
        );
    }

    /// Braces introduced by quoted parameter expansion must not undergo brace
    /// expansion when an adjacent unquoted glob remains active.
    #[tokio::test]
    async fn test_quoted_variable_braces_stay_literal_with_adjacent_glob() {
        let mut bash = crate::Bash::new();
        bash.fs()
            .mkdir(std::path::Path::new("/tmp/quoted_brace_literal"), true)
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/quoted_brace_literal/{secret,public}x.txt"),
                b"literal",
            )
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/quoted_brace_literal/secret.txt"),
                b"secret",
            )
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/quoted_brace_literal/public.txt"),
                b"public",
            )
            .await
            .unwrap();

        let result = bash
            .exec(r#"cd /tmp/quoted_brace_literal; p="{secret,public}"; printf '%s\n' "$p"*.txt"#)
            .await
            .unwrap();

        let mut lines: Vec<&str> = result.stdout.trim().lines().collect();
        lines.sort();
        assert_eq!(
            lines,
            vec!["{secret,public}x.txt"],
            "braces from quoted variable must remain literal; stderr: {}",
            result.stderr
        );
    }

    /// Issue #1333: glob `*` adjacent to quoted variable must also expand
    /// inside process substitution `<(...)`. The fix from #1287 applied at
    /// the top-level but not inside the subshell body of `<(cmd)`.
    #[tokio::test]
    async fn test_glob_adjacent_to_quoted_var_in_process_substitution() {
        let mut bash = crate::Bash::new();
        bash.fs()
            .mkdir(std::path::Path::new("/tmp/ps_glob"), true)
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/ps_glob/tag_foo.tmp.html"),
                b"foo",
            )
            .await
            .unwrap();
        bash.fs()
            .write_file(
                std::path::Path::new("/tmp/ps_glob/tag_bar.tmp.html"),
                b"bar",
            )
            .await
            .unwrap();

        // while-read over <(ls ./"$p"*.tmp.html) — real blocker case from bashblog.
        let result = bash
            .exec(
                r#"cd /tmp/ps_glob; p="tag_"; while read -r i; do echo "got:$i"; done < <(ls ./"$p"*.tmp.html)"#,
            )
            .await
            .unwrap();
        let mut lines: Vec<&str> = result.stdout.trim().lines().collect();
        lines.sort();
        assert_eq!(
            lines,
            vec!["got:./tag_bar.tmp.html", "got:./tag_foo.tmp.html"],
            "glob * inside <(...) should expand; stderr: {}",
            result.stderr
        );
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

    #[tokio::test]
    async fn test_quoted_remove_prefix_operand_keeps_glob_literal() {
        // Quoted pattern operand must keep wildcard chars literal:
        // bash: val="axxxb"; pat="a*"; echo "${val#"$pat"}" => "axxxb"
        let result = run_script(r#"val="axxxb"; pat="a*"; echo "${val#"$pat"}""#).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "axxxb");
    }

    #[tokio::test]
    async fn test_mixed_remove_prefix_operand_keeps_unquoted_glob_active() {
        // Mixed operand: quoted var part literalized, unquoted * stays wildcard.
        let result = run_script(r#"val="axxxb"; pat="a"; echo "${val#"$pat"*}""#).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "xxxb");
    }

    #[test]
    fn test_operand_quote_mark_uses_bounded_fallible_candidates() {
        let operand: String = OPERAND_QUOTE_MARK_CANDIDATES.iter().collect();
        assert_eq!(Interpreter::operand_quote_mark(&operand), None);
    }

    #[test]
    fn test_parse_marked_operand_no_quotes_is_unforced() {
        // No double quotes: nothing to preserve, no marker, not forced (and the
        // bounded candidate search is skipped via the fast path).
        let (_, mark, forced) = Interpreter::parse_marked_operand("a*b", 128, 1_000_000);
        assert_eq!(mark, None);
        assert!(!forced);
    }

    #[test]
    fn test_parse_marked_operand_escaped_quotes_do_not_force_quoting() {
        // All marker candidates appear in the source (no safe marker), and the
        // only double quote is escaped, so there are no unescaped boundaries to
        // preserve: quoted handling must NOT be forced.
        let mut operand: String = OPERAND_QUOTE_MARK_CANDIDATES.iter().collect();
        operand.push_str(r#"\""#);
        let (_, mark, forced) = Interpreter::parse_marked_operand(&operand, 128, 1_000_000);
        assert_eq!(mark, None);
        assert!(!forced, "escaped quotes must not force quoted expansion");
    }

    #[tokio::test]
    async fn test_quoted_remove_prefix_operand_with_all_mark_candidates_keeps_glob_literal() {
        let candidate_chars: String = OPERAND_QUOTE_MARK_CANDIDATES.iter().collect();
        let script = format!(
            r#"val="axxxb"; pat="a*"; echo "${{val#${{unset+{}}}"$pat"}}""#,
            candidate_chars
        );
        let result = run_script(&script).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "axxxb");
    }

    #[tokio::test]
    async fn test_quoted_remove_prefix_operand_rejects_colliding_source_marker() {
        let quote_mark = OPERAND_QUOTE_MARK_CANDIDATES[0];
        let dead_candidates: String = OPERAND_QUOTE_MARK_CANDIDATES[1..].iter().collect();
        let script = format!(
            "val=\"axxxb\"; pat=\"a*\"; echo \"${{val#${{unset+{dead_candidates}}}{quote_mark}\\\"$pat\\\"{quote_mark}${{unset+\\\"\\\"}}}}\""
        );
        let result = run_script(&script).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "axxxb");
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
    async fn test_indirect_expansion_with_default() {
        // Issue #937: ${!var:-default} should compose indirect + default
        let result =
            run_script(r#"name="TARGET"; TARGET="value"; echo "${!name:-fallback}""#).await;
        assert_eq!(result.stdout.trim(), "value");

        let result = run_script(r#"name="MISSING"; echo "${!name:-fallback}""#).await;
        assert_eq!(result.stdout.trim(), "fallback");

        let result = run_script(r#"name="EMPTY"; EMPTY=""; echo "${!name:-fallback}""#).await;
        assert_eq!(result.stdout.trim(), "fallback");

        let result = run_script(r#"name="UNSET"; echo "${!name:=assigned}""#).await;
        assert_eq!(result.stdout.trim(), "assigned");
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
    async fn test_debug_trap_no_recursive_amplification() {
        // THREAT[TM-DOS-035]: Commands inside the DEBUG trap handler must NOT
        // trigger further DEBUG trap invocations (prevents N*M amplification).
        let result = run_script(
            r#"trap_count=0; trap '((trap_count++))' DEBUG; echo a; echo b; echo c; trap - DEBUG; echo $trap_count"#,
        )
        .await;
        // DEBUG fires before: echo a (1), echo b (2), echo c (3), trap - DEBUG (4)
        // The ((trap_count++)) inside the trap must NOT fire another DEBUG trap.
        assert_eq!(result.stdout, "a\nb\nc\n4\n");
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
    async fn test_ifs_nameref_to_star_does_not_recurse() {
        // THREAT[TM-DOS-036]: IFS may be a nameref to a special parameter.
        // Separator lookup must not recursively expand `$*` through IFS.
        let result =
            run_script(r#"f() { local -n IFS='*'; local arr=(a b c); echo "${arr[*]}"; }; f"#)
                .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "a b c");
    }

    #[tokio::test]
    async fn test_ifs_nameref_to_regular_variable_array_join() {
        let result = run_script(
            r#"f() { local sep=:; local -n IFS=sep; local arr=(a b c); echo "${arr[*]}"; }; f"#,
        )
        .await;
        assert_eq!(result.stdout.trim(), "a:b:c");
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

    #[tokio::test]
    async fn test_process_sub_conditional_bracket() {
        // [[ ]] inside process substitution must be preserved during token reconstruction
        let result = run_script(r#"cat <( [[ 1 = 1 ]] && echo MATCH )"#).await;
        assert_eq!(result.stdout.trim(), "MATCH");
    }

    #[tokio::test]
    async fn test_process_sub_while_break_with_condition() {
        // while+break with conditional inside process substitution
        let result =
            run_script(r#"cat <( x=1; while true; do [[ $x -eq 1 ]] && break; done; echo OK )"#)
                .await;
        assert_eq!(result.stdout.trim(), "OK");
    }

    #[tokio::test]
    async fn test_process_sub_arithmetic() {
        // (( )) inside process substitution must be preserved
        let result = run_script(r#"cat <( x=5; (( x > 3 )) && echo YES )"#).await;
        assert_eq!(result.stdout.trim(), "YES");
    }

    #[tokio::test]
    async fn test_output_process_sub_cleared_after_failglob_in_same_exec() {
        let result =
            run_script(r#"shopt -s failglob; echo >(echo STALE) ./missing_*; echo VICTIM"#).await;
        assert!(
            !result.stdout.contains("STALE"),
            "deferred output process substitution leaked after failglob"
        );
        assert!(result.stdout.contains("VICTIM"));
    }

    #[tokio::test]
    async fn test_output_process_sub_cleared_between_bash_exec_calls() {
        let mut bash = crate::Bash::new();
        let first = bash
            .exec(r#"shopt -s failglob; echo >(cat /secret) ./missing_*"#)
            .await
            .unwrap();
        assert_eq!(first.exit_code, 1);

        let second = bash
            .exec("echo SECRET > /secret; echo VICTIM")
            .await
            .unwrap();
        assert_eq!(second.stdout, "VICTIM\n");
    }

    #[tokio::test]
    async fn test_stderr_redirect_devnull_simple_and_compound() {
        // Issue #1116: 2>/dev/null must suppress stderr from builtins
        let result = run_script("ls /nonexistent 2>/dev/null; echo exit:$?").await;
        assert_eq!(result.stderr, "", "simple: stderr should be suppressed");
        assert_eq!(result.stdout.trim(), "exit:2");

        // Compound command
        let result = run_script("{ ls /nonexistent; } 2>/dev/null; echo exit:$?").await;
        assert_eq!(result.stderr, "", "compound: stderr should be suppressed");
        assert_eq!(result.stdout.trim(), "exit:2");

        // &>/dev/null
        let result = run_script("ls /nonexistent &>/dev/null; echo exit:$?").await;
        assert_eq!(result.stderr, "", "&>: stderr should be suppressed");
        assert_eq!(result.stdout.trim(), "exit:2");

        // failglob + redirect
        let result = run_script("shopt -s failglob; ls ./*.html 2>/dev/null; echo exit:$?").await;
        assert_eq!(result.stderr, "", "failglob: stderr should be suppressed");
    }

    #[tokio::test]
    async fn test_fd3_redirect_pattern() {
        // Issue #1115: { echo "progress" 1>&3; echo "file content"; } 3>&1 >file
        let result = run_script(
            r#"{ echo "progress" 1>&3; echo "file content"; } 3>&1 > /tmp/test_fd.txt
cat /tmp/test_fd.txt"#,
        )
        .await;
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(
            lines,
            vec!["progress", "file content"],
            "fd3 → stdout, fd1 → file"
        );
    }

    #[tokio::test]
    async fn test_fd3_pending_output_not_leaked_across_commands() {
        // Regression: pending fd3+ buffer must not leak into later unrelated mixed redirects.
        let result = run_script(
            r#"echo "secret" 1>&3
echo "public" 2>&1 > /tmp/test_fd_leak.txt
cat /tmp/test_fd_leak.txt"#,
        )
        .await;
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines, vec!["public"]);
    }

    #[tokio::test]
    async fn test_fd3_pending_output_cleared_after_noclobber_error() {
        // Regression: failed outer fd-table redirects must not retain fd3+ data.
        let result = run_script(
            r#"echo existing > /tmp/test_fd_noclobber.txt
set -C
{ echo "secret" 1>&3; } 3>&1 > /tmp/test_fd_noclobber.txt
echo "public" 2>&1 > /tmp/test_fd_after_noclobber.txt
cat /tmp/test_fd_after_noclobber.txt"#,
        )
        .await;
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines, vec!["public"]);
        assert!(!result.stdout.contains("secret"));
    }

    #[tokio::test]
    async fn test_fd3_pending_output_not_leaked_across_exec_calls() {
        // Regression: Bash::exec reset clears stale fd3+ buffers in reused interpreters.
        let mut bash = Bash::new();
        let first = bash
            .exec(
                r#"echo existing > /tmp/test_fd_exec_leak.txt
set -C
{ echo "secret" 1>&3; } 3>&1 > /tmp/test_fd_exec_leak.txt"#,
            )
            .await
            .unwrap();
        assert_eq!(first.exit_code, 1);

        let second = bash
            .exec(
                r#"echo "public" 2>&1 > /tmp/test_fd_exec_public.txt
cat /tmp/test_fd_exec_public.txt"#,
            )
            .await
            .unwrap();
        assert_eq!(second.stdout, "public\n");
        assert!(!second.stdout.contains("secret"));
    }

    // Regression: date +"$var" must not word-split format when var contains spaces
    // https://github.com/everruns/bashkit/issues/1203
    #[tokio::test]
    async fn test_date_format_var_with_spaces_no_split() {
        // Use -u -d @0 for deterministic output (1970-01-01 UTC)
        let result = run_script(r#"fmt="%Y %m %d"; date -u -d @0 +"$fmt""#).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "1970 01 01");
    }

    // Mixed-quoting: prefix"$var" must stay one word (no IFS split)
    #[tokio::test]
    async fn test_mixed_quote_prefix_var_no_split() {
        // prefix"$var" should produce one argument, not be split at spaces
        let result = run_script(r#"v="a b c"; echo prefix"$v""#).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "prefixa b c");
    }

    // Mixed-quoting starting with quote: "$var"suffix must stay one word.
    #[tokio::test]
    async fn test_mixed_quote_starts_with_var_no_split() {
        let result = run_script(
            r#"v="a b c"; set -- "${v}"suffix; echo "count:$#"; echo "arg1:$1"; echo "arg2:${2:-<none>}""#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines, vec!["count:1", "arg1:a b csuffix", "arg2:<none>"]);
    }

    // Regression: only unquoted expansion parts in mixed words undergo IFS splitting.
    #[tokio::test]
    async fn test_mixed_quote_unquoted_prefix_var_still_splits() {
        let result = run_script(
            r#"a="x y"; b="q r"; set -- $a"$b"; echo "count:$#"; echo "arg1:$1"; echo "arg2:$2""#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines, vec!["count:2", "arg1:x", "arg2:yq r"]);
    }

    // Mixed-quoting: "$v"$u protects only the quoted segment; unquoted $u still splits.
    #[tokio::test]
    async fn test_mixed_quote_unquoted_suffix_var_splits() {
        let result = run_script(
            r#"v="x y"; u="a b"; set -- "$v"$u; echo "count:$#"; echo "arg1:$1"; echo "arg2:$2""#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines, vec!["count:2", "arg1:x ya", "arg2:b"]);
    }

    // Regression: unquoted IFS delimiters in a mixed word separate adjacent
    // literal/quoted segments even when the expansion contributes no field text.
    #[tokio::test]
    async fn test_mixed_quote_unquoted_ifs_boundary_before_quoted_suffix() {
        let result = run_script(
            r#"a=" "; b="q"; set -- p$a"$b"; echo "count:$#"; echo "arg1:$1"; echo "arg2:$2""#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines, vec!["count:2", "arg1:p", "arg2:q"]);
    }

    /// Issue #1184: input process substitution temp files must be cleaned up
    #[tokio::test]
    async fn test_proc_sub_input_cleanup() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));

        let parser = Parser::new(r#"for i in 1 2 3 4 5; do cat <(echo "hello $i"); done"#);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(result.exit_code, 0);
        interp.cleanup_proc_sub_files().await;

        if let Ok(entries) = fs.read_dir(Path::new("/dev/fd")).await {
            let leaked: Vec<_> = entries
                .iter()
                .filter(|e| e.name.starts_with("proc_sub_"))
                .collect();
            assert!(
                leaked.is_empty(),
                "proc_sub files leaked in /dev/fd: {:?}",
                leaked.iter().map(|e| &e.name).collect::<Vec<_>>()
            );
        }
    }

    /// Issue #1184: output process substitution temp files must be cleaned up
    #[tokio::test]
    async fn test_proc_sub_output_cleanup() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));

        let parser = Parser::new(r#"for i in 1 2 3; do echo "data $i" > >(cat); done"#);
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(result.exit_code, 0);
        interp.cleanup_proc_sub_files().await;

        if let Ok(entries) = fs.read_dir(Path::new("/dev/fd")).await {
            let leaked: Vec<_> = entries
                .iter()
                .filter(|e| e.name.starts_with("proc_sub_"))
                .collect();
            assert!(
                leaked.is_empty(),
                "proc_sub files leaked in /dev/fd: {:?}",
                leaked.iter().map(|e| &e.name).collect::<Vec<_>>()
            );
        }
    }

    /// Issue #1184: cleanup happens even when command fails
    #[tokio::test]
    async fn test_proc_sub_cleanup_on_failure() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));

        let parser = Parser::new(r#"cat <(echo "data") && false; true"#);
        let ast = parser.parse().unwrap();
        let _result = interp.execute(&ast).await.unwrap();
        interp.cleanup_proc_sub_files().await;

        if let Ok(entries) = fs.read_dir(Path::new("/dev/fd")).await {
            let leaked: Vec<_> = entries
                .iter()
                .filter(|e| e.name.starts_with("proc_sub_"))
                .collect();
            assert!(
                leaked.is_empty(),
                "proc_sub files leaked after failed command: {:?}",
                leaked.iter().map(|e| &e.name).collect::<Vec<_>>()
            );
        }
    }

    /// Regression: cleanup must not remove process substitution paths owned by other sessions.
    #[tokio::test]
    async fn test_proc_sub_cleanup_does_not_delete_other_session_files() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut owner = Interpreter::new(Arc::clone(&fs));
        let mut other = Interpreter::new(Arc::clone(&fs));

        let parser = Parser::new(r#"echo <(echo "data")"#);
        let ast = parser.parse().unwrap();
        let result = owner.execute(&ast).await.unwrap();
        let proc_sub_path = result.stdout.trim().to_string();
        assert!(proc_sub_path.starts_with("/dev/fd/proc_sub_"));
        assert!(fs.read_file(Path::new(&proc_sub_path)).await.is_ok());

        other.cleanup_proc_sub_files().await;

        assert!(
            fs.read_file(Path::new(&proc_sub_path)).await.is_ok(),
            "cleanup from another interpreter removed {}",
            proc_sub_path
        );
    }

    /// Regression: all known internal prefixes must be caught by is_internal_variable().
    #[test]
    fn test_is_internal_variable_covers_all_prefixes() {
        let internal_names = [
            "_NAMEREF_foo",
            "_READONLY_bar",
            "_UPPER_x",
            "_LOWER_y",
            "_INTEGER_n",
            "_ARRAY_READ_a",
            "_SHIFT_COUNT",
            "_SET_POSITIONAL",
            "SHOPT_e",
            "SHOPT_x",
            "SHOPT_expand_aliases",
            "SHOPT_pipefail",
        ];
        for name in &internal_names {
            assert!(
                is_internal_variable(name),
                "is_internal_variable() should return true for {name}"
            );
        }

        // _TTY_ is user-configurable but hidden from output
        let hidden_only = ["_TTY_0", "_TTY_1"];
        for name in &hidden_only {
            assert!(
                !is_internal_variable(name),
                "_TTY_ should NOT be blocked by is_internal_variable(): {name}"
            );
            assert!(
                is_hidden_variable(name),
                "_TTY_ should be hidden by is_hidden_variable(): {name}"
            );
        }

        let regular_vars = ["HOME", "PATH", "USER", "MY_VAR", "foo", "_"];
        for name in &regular_vars {
            assert!(
                !is_internal_variable(name),
                "is_internal_variable() should return false for regular variable {name}"
            );
        }
    }

    #[tokio::test]
    async fn test_shell_state_restore_preserves_readonly_attrs() {
        let mut interp = Interpreter::new(Arc::new(InMemoryFs::new()));
        let ast = Parser::new("readonly POLICY=safe").parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let state = interp.shell_state();
        let mut restored = Interpreter::new(Arc::new(InMemoryFs::new()));
        restored.restore_shell_state(&state);

        let assign = Parser::new("POLICY=unsafe; echo $POLICY").parse().unwrap();
        let out = restored.execute(&assign).await.unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout.trim(), "safe");
    }

    #[tokio::test]
    async fn test_shell_state_roundtrips_last_bg_pid() {
        let mut interp = Interpreter::new(Arc::new(InMemoryFs::new()));
        let ast = Parser::new("true &").parse().unwrap();
        interp.execute(&ast).await.unwrap();
        let bang = interp.last_bg_pid.clone();
        assert!(bang.is_some(), "$! should be set after backgrounding");

        let state = interp.shell_state();
        assert_eq!(state.last_bg_pid, bang);

        let mut restored = Interpreter::new(Arc::new(InMemoryFs::new()));
        restored.restore_shell_state(&state);
        let echo = Parser::new("echo $!").parse().unwrap();
        let out = restored.execute(&echo).await.unwrap();
        assert_eq!(out.stdout.trim(), bang.unwrap());
    }

    #[tokio::test]
    async fn test_shell_state_roundtrips_dir_stack() {
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(Path::new("/tmp"), true).await.unwrap();
        let mut interp = Interpreter::new(fs);
        let ast = Parser::new("cd /tmp; pushd /tmp >/dev/null")
            .parse()
            .unwrap();
        interp.execute(&ast).await.unwrap();
        assert_eq!(&*interp.scoped.dir_stack, &["/tmp".to_string()]);

        let state = interp.shell_state();
        assert_eq!(state.dir_stack, vec!["/tmp".to_string()]);

        let restored_fs = Arc::new(InMemoryFs::new());
        let mut restored = Interpreter::new(restored_fs);
        restored.restore_shell_state(&state);
        assert_eq!(&*restored.scoped.dir_stack, &["/tmp".to_string()]);
        let out = restored
            .execute(&Parser::new("dirs").parse().unwrap())
            .await
            .unwrap();
        assert!(out.stdout.contains("/tmp"));
    }

    #[tokio::test]
    async fn test_restore_shell_state_migrates_legacy_nameref_targets() {
        let state = ShellState {
            env: HashMap::new(),
            variables: HashMap::from([
                ("POLICY".to_string(), "safe".to_string()),
                ("_READONLY_POLICY".to_string(), String::new()),
                ("_NAMEREF_alias_var".to_string(), "POLICY".to_string()),
            ]),
            var_attrs: HashMap::new(),
            namerefs: HashMap::new(),
            arrays: HashMap::new(),
            assoc_arrays: HashMap::new(),
            cwd: PathBuf::from("/"),
            last_exit_code: 0,
            last_bg_pid: None,
            functions: HashMap::new(),
            aliases: HashMap::new(),
            traps: HashMap::new(),
            dir_stack: Vec::new(),
        };

        let mut restored = Interpreter::new(Arc::new(InMemoryFs::new()));
        restored.restore_shell_state(&state);

        assert_eq!(restored.resolve_nameref("alias_var"), "POLICY");
        assert!(!restored.scoped.variables.contains_key("_NAMEREF_alias_var"));

        let ast = Parser::new("alias_var=unsafe; echo $POLICY")
            .parse()
            .unwrap();
        let result = restored.execute(&ast).await.unwrap();
        assert_eq!(result.stdout.trim(), "safe");
        assert_eq!(
            restored.scoped.variables.get("POLICY").map(String::as_str),
            Some("safe")
        );
    }

    #[test]
    fn test_restore_shell_state_clears_stale_attrs_and_namerefs() {
        let mut interp = Interpreter::new(Arc::new(InMemoryFs::new()));
        interp.add_var_attr("POLICY", VarAttrs::READONLY);
        interp.set_nameref("alias_var", "POLICY".to_string());

        let clean_state = ShellState {
            env: HashMap::new(),
            variables: HashMap::from([("POLICY".to_string(), "safe".to_string())]),
            var_attrs: HashMap::new(),
            namerefs: HashMap::new(),
            arrays: HashMap::new(),
            assoc_arrays: HashMap::new(),
            cwd: PathBuf::from("/"),
            last_exit_code: 0,
            last_bg_pid: None,
            functions: HashMap::new(),
            aliases: HashMap::new(),
            traps: HashMap::new(),
            dir_stack: Vec::new(),
        };

        interp.restore_shell_state(&clean_state);

        assert!(!interp.is_var_readonly("POLICY"));
        assert!(interp.resolve_nameref("alias_var").eq("alias_var"));
    }
}
