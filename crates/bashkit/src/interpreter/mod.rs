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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Monotonic counter for unique process substitution file paths
static PROC_SUB_COUNTER: AtomicU64 = AtomicU64::new(0);

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
/// Note: `_TTY_` is intentionally excluded — it is user-configurable (bashkit extension).
pub(crate) fn is_internal_variable(name: &str) -> bool {
    name.starts_with("SHOPT_")
        || name.starts_with("_NAMEREF_")
        || name.starts_with("_READONLY_")
        || name.starts_with("_UPPER_")
        || name.starts_with("_LOWER_")
        || name.starts_with("_INTEGER_")
        || name.starts_with("_ARRAY_READ_")
        || name.starts_with("_BG_EXIT_")
        || name.starts_with("_LAST_BG_")
        || name.starts_with("_DIRSTACK_")
        || name.starts_with("_OPTCHAR_")
        || name == "_EVAL_CMD"
        || name == "_SHIFT_COUNT"
        || name == "_SET_POSITIONAL"
}

/// THREAT[TM-INF-017]: Check if a variable should be hidden from user-visible output.
/// Superset of `is_internal_variable()` — also includes `_TTY_` which is user-settable
/// but should not appear in `set`, `declare -p`, or environment exports.
pub(crate) fn is_hidden_variable(name: &str) -> bool {
    is_internal_variable(name) || name.starts_with("_TTY_")
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