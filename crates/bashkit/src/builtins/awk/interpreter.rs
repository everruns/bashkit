//! AWK interpreter - executes a parsed `AwkProgram`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use super::{
    AwkAction, AwkExpr, AwkFunctionDef, AwkOutputTarget, AwkPattern, AwkState, AwkValue, FieldSep,
};
use crate::builtins::MAX_FORMAT_WIDTH;
use crate::builtins::limits::{
    AWK_MAX_CALL_DEPTH as MAX_AWK_CALL_DEPTH, AWK_MAX_FIELD_INDEX,
    AWK_MAX_GETLINE_CACHE_BYTES as MAX_GETLINE_CACHE_BYTES,
    AWK_MAX_GETLINE_CACHED_FILES as MAX_GETLINE_CACHED_FILES,
    AWK_MAX_GETLINE_FILE_BYTES as MAX_GETLINE_FILE_BYTES,
    AWK_MAX_OUTPUT_BYTES as MAX_AWK_OUTPUT_BYTES, AWK_MAX_OUTPUT_TARGETS as MAX_AWK_OUTPUT_TARGETS,
    AWK_MAX_STRING_BYTES,
};
use crate::builtins::search_common::RuntimeRegexCache;
use crate::fs::{FileSystem, normalize_path};
use crate::limits::ExecutionLimits;
// On wasm32 there is no OS thread to bridge the sync AWK evaluator to the async
// VFS, so redirected reads/writes drive the VFS future inline with now_or_never.
use crate::fs::vfs_join;
#[cfg(target_family = "wasm")]
use futures_util::FutureExt;

/// Flow control signal from action execution
#[derive(Debug, PartialEq)]
pub(super) enum AwkFlow {
    Continue,          // Normal execution
    Next,              // Skip to next record
    Break,             // Break out of loop
    LoopContinue,      // Continue to next loop iteration
    Exit(Option<i32>), // Exit program with optional code
    Return(AwkValue),  // Return from user-defined function
}

// Awk runtime limits (TM-DOS-027, TM-DOS-028) live in `super::limits`:
// - AWK_MAX_CALL_DEPTH (user-function recursion)
// - AWK_MAX_OUTPUT_BYTES (total stdout+stderr+file redirects, 10 MB)
// - AWK_MAX_OUTPUT_TARGETS (distinct redirected output files).
// - AWK_MAX_GETLINE_CACHED_FILES (distinct files held open by `getline`).
// - AWK_MAX_GETLINE_FILE_BYTES / AWK_MAX_GETLINE_CACHE_BYTES (retained input bytes).
// THREAT[TM-DOS-023]: Runtime regex operands share one bounded cache. Invalid
// patterns are cached too, preventing repeated compilation failures from
// becoming a CPU sink.

/// One redirected VFS write, dispatched to the [`VfsWriter`] thread.
#[cfg(not(target_family = "wasm"))]
struct WriteJob {
    path: PathBuf,
    bytes: Vec<u8>,
    append: bool,
    resp: std::sync::mpsc::Sender<crate::error::Result<()>>,
}

/// A single long-lived OS thread owning one Tokio runtime that serializes
/// redirected VFS writes.
///
/// AWK executes synchronously inside the outer async runtime, so each
/// redirected write must hop to a thread with its own runtime to call the async
/// VFS. Spawning a fresh thread + runtime per write would let a tight
/// `print > file` loop create thousands of threads/runtimes (DoS). Reusing one
/// writer for the whole run keeps writes incremental (so VFS quotas are still
/// enforced as they happen) at O(1) threads.
#[cfg(not(target_family = "wasm"))]
struct VfsWriter {
    tx: Option<std::sync::mpsc::Sender<WriteJob>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_family = "wasm"))]
impl VfsWriter {
    fn new(fs: Arc<dyn FileSystem>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<WriteJob>();
        let handle = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("awk vfs writer runtime");
            runtime.block_on(async move {
                while let Ok(job) = rx.recv() {
                    let res = if job.append {
                        fs.append_file(&job.path, &job.bytes).await
                    } else {
                        fs.write_file(&job.path, &job.bytes).await
                    };
                    let _ = job.resp.send(res);
                }
            });
        });
        Self {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    /// Perform one write synchronously on the writer thread. Returns the VFS
    /// result, or `Err(())` if the writer thread is gone.
    fn write(
        &self,
        path: PathBuf,
        bytes: Vec<u8>,
        append: bool,
    ) -> Result<crate::error::Result<()>, ()> {
        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        let job = WriteJob {
            path,
            bytes,
            append,
            resp: resp_tx,
        };
        self.tx.as_ref().ok_or(())?.send(job).map_err(|_| ())?;
        resp_rx.recv().map_err(|_| ())
    }
}

#[cfg(not(target_family = "wasm"))]
impl Drop for VfsWriter {
    fn drop(&mut self) {
        // Drop the sender first so the writer loop's `rx.recv()` returns Err and
        // the thread exits, then join it to flush any in-flight write cleanly.
        drop(self.tx.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// wasm32 has no OS threads (`std::thread::spawn` returns `Unsupported`), so the
/// thread-hop writer above can't exist. The browser build only ever runs over
/// the in-memory VFS, whose writes resolve in a single poll, so we drive the
/// async VFS future to completion inline with `now_or_never` — same VFS, same
/// quota accounting, no thread.
#[cfg(target_family = "wasm")]
struct VfsWriter {
    fs: Arc<dyn FileSystem>,
}

#[cfg(target_family = "wasm")]
impl VfsWriter {
    fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    /// Perform one write by polling the VFS future once. Returns the VFS result,
    /// or `Err(())` if the write would suspend (an async-backed mount, which the
    /// browser build does not use).
    fn write(
        &self,
        path: PathBuf,
        bytes: Vec<u8>,
        append: bool,
    ) -> Result<crate::error::Result<()>, ()> {
        let fut = async {
            if append {
                self.fs.append_file(&path, &bytes).await
            } else {
                self.fs.write_file(&path, &bytes).await
            }
        };
        fut.now_or_never().ok_or(())
    }
}

pub(super) struct AwkInterpreter {
    pub(super) state: AwkState,
    pub(super) output: String,
    /// Stderr output buffer for `/dev/stderr` redirection
    pub(super) stderr_output: String,
    /// Lines of current input file (set before main loop)
    pub(super) input_lines: Vec<String>,
    /// Current line index within input_lines
    pub(super) line_index: usize,
    /// User-defined functions
    pub(super) functions: HashMap<String, AwkFunctionDef>,
    /// Current function call depth for recursion limiting
    call_depth: usize,
    /// Paths already seen as `>`/`>>` redirect targets in this AWK run.
    /// First write truncates (for `>`); later writes stream as appends. Also
    /// serves as the distinct-target set for the output-target DoS guard.
    pub(super) file_truncates: HashSet<String>,
    /// Total bytes streamed to real file redirects, excluding discarded devices.
    redirected_file_output_bytes: usize,
    /// Single reusable writer thread for redirected VFS writes (lazily started
    /// on the first redirect). Joined when the interpreter is dropped.
    vfs_writer: Option<VfsWriter>,
    /// Cached file inputs for `getline var < file` redirection.
    /// Maps normalized resolved path -> (lines, current_position).
    /// Maps normalized resolved path -> (lines, current_position, raw_bytes).
    file_inputs: HashMap<String, (Vec<String>, usize, usize)>,
    /// Approximate raw input bytes retained by `file_inputs`.
    file_input_bytes: usize,
    /// VFS reference for lazy file reads (getline < file).
    pub(super) fs: Option<Arc<dyn FileSystem>>,
    /// Working directory for resolving relative paths.
    pub(super) cwd: PathBuf,
    /// Tracks active state for range patterns (rule index -> is_active).
    /// When start pattern matches, range becomes active. When end pattern
    /// matches, that line is included but range becomes inactive.
    range_active: HashMap<usize, bool>,
    /// Max iterations for a single loop, inherited from execution limits.
    pub(super) max_loop_iterations: usize,
    /// Max iterations across all loops of one awk run (nested loops share it).
    pub(super) max_total_loop_iterations: usize,
    total_loop_iterations: usize,
    regex_cache: RuntimeRegexCache,
    /// Shared request budget; poisoning is surfaced by the builtin dispatcher.
    pub(super) execution_budget: Option<crate::limits::ExecutionBudget>,
    /// Set once a resource limit aborted the program (see `fatal`).
    fatal: bool,
    /// Cap on accounted variable bytes; the host's
    /// `max_live_intermediate_bytes`. THREAT[TM-DOS-110].
    pub(super) max_state_bytes: usize,
    /// Accounted bytes of values parked in user-function call frames.
    pinned_bytes: usize,
}

/// Which matches `replace_checked` substitutes.
#[derive(Clone, Copy)]
enum Replace {
    All,
    Nth(usize),
}

/// `format_string` failure: a user-facing diagnostic, or the string cap.
enum FormatError {
    Message(String),
    TooLarge,
}

impl AwkInterpreter {
    pub(super) fn new() -> Self {
        Self {
            state: AwkState::default(),
            output: String::new(),
            stderr_output: String::new(),
            input_lines: Vec::new(),
            line_index: 0,
            functions: HashMap::new(),
            file_truncates: HashSet::new(),
            redirected_file_output_bytes: 0,
            vfs_writer: None,
            file_inputs: HashMap::new(),
            file_input_bytes: 0,
            call_depth: 0,
            fs: None,
            cwd: PathBuf::from("/"),
            range_active: HashMap::new(),
            max_loop_iterations: ExecutionLimits::default().max_loop_iterations,
            max_total_loop_iterations: ExecutionLimits::default().max_total_loop_iterations,
            total_loop_iterations: 0,
            regex_cache: RuntimeRegexCache::default(),
            execution_budget: None,
            fatal: false,
            max_state_bytes: ExecutionLimits::default().max_live_intermediate_bytes as usize,
            pinned_bytes: 0,
        }
    }

    pub(super) fn is_fatal(&self) -> bool {
        self.fatal
    }

    /// THREAT[TM-DOS-110]: a single string may not exceed
    /// `AWK_MAX_STRING_BYTES`. Callers check a size before allocating it.
    fn string_fits(&mut self, len: usize) -> bool {
        if len > AWK_MAX_STRING_BYTES {
            self.fatal(&format!(
                "string size limit ({AWK_MAX_STRING_BYTES} bytes) exceeded"
            ));
            return false;
        }
        true
    }

    /// THREAT[TM-DOS-110]: variables, array elements and parked call-frame
    /// values together stay within the host's live-bytes limit. Checked at
    /// every expression, so growth overshoots by at most one assignment.
    ///
    /// Decision: a local cap, not an `ExecutionBudgetLease`. A lease that
    /// runs out poisons the whole request, while this fails only the awk
    /// command (fatal, exit 2) and the script carries on, like every other
    /// awk cap (TM-DOS-109).
    fn memory_fits(&mut self) -> bool {
        if self.fatal {
            return false;
        }
        if self.state.mem_bytes + self.pinned_bytes > self.max_state_bytes {
            let msg = format!("memory limit ({} bytes) exceeded", self.max_state_bytes);
            self.fatal(&msg);
            return false;
        }
        true
    }

    /// `sub`/`gsub`/`gensub` core: substitute matches of `re` in `target`,
    /// checking the result against the string cap as it grows (a 10 KB
    /// target and replacement would otherwise build 100 MB first).
    /// `expand` interprets `$N` group references in `rep`.
    fn replace_checked(
        &mut self,
        re: &regex::Regex,
        target: &str,
        rep: &str,
        which: Replace,
        expand: bool,
    ) -> Option<(String, usize)> {
        let mut out = String::new();
        let mut last = 0;
        let mut count = 0;
        for caps in re.captures_iter(target) {
            let Some(m) = caps.get(0) else { continue };
            count += 1;
            out.push_str(&target[last..m.start()]);
            let hit = match which {
                Replace::All => true,
                Replace::Nth(n) => count == n,
            };
            if !hit {
                out.push_str(m.as_str());
            } else if expand {
                caps.expand(rep, &mut out);
            } else {
                out.push_str(rep);
            }
            last = m.end();
            if !self.string_fits(out.len()) {
                return None;
            }
            if matches!(which, Replace::Nth(n) if count >= n) {
                break;
            }
        }
        if !self.string_fits(out.len() + target.len() - last) {
            return None;
        }
        out.push_str(&target[last..]);
        Some((out, count))
    }

    /// Abort the program because a resource limit was reached.
    /// THREAT[TM-DOS-109]: caps are reported, never silent.
    ///
    /// Decision (#2446): hitting a cap is never silent. Like a gawk fatal
    /// error, the message goes to stderr, no further actions (END included)
    /// run, and awk exits 2. Output produced so far is kept.
    fn fatal(&mut self, msg: &str) -> AwkFlow {
        if !self.fatal {
            self.fatal = true;
            self.stderr_output.push_str("awk: fatal: ");
            self.stderr_output.push_str(msg);
            self.stderr_output.push('\n');
        }
        AwkFlow::Exit(Some(2))
    }

    /// Count one iteration of a loop that has run `iters` times so far.
    /// THREAT[TM-DOS-033]: per-loop and whole-program caps, so nested loops
    /// cannot multiply past `max_total_loop_iterations`.
    fn tick_loop(&mut self, iters: usize) -> Option<AwkFlow> {
        self.total_loop_iterations += 1;
        if iters > self.max_loop_iterations {
            let msg = format!(
                "loop iteration limit ({}) exceeded",
                self.max_loop_iterations
            );
            return Some(self.fatal(&msg));
        }
        if self.total_loop_iterations > self.max_total_loop_iterations {
            let msg = format!(
                "total loop iteration limit ({}) exceeded",
                self.max_total_loop_iterations
            );
            return Some(self.fatal(&msg));
        }
        None
    }

    fn runtime_regex(&mut self, pattern: &str) -> Option<regex::Regex> {
        self.regex_cache.get_or_compile(pattern)
    }

    fn resolve_getline_path(&self, path_str: &str) -> String {
        let resolved = if path_str.starts_with('/') {
            PathBuf::from(path_str)
        } else {
            vfs_join(&self.cwd, path_str)
        };
        normalize_path(&resolved).to_string_lossy().to_string()
    }

    /// Load a file into the `file_inputs` cache if not already present.
    /// Uses a separate thread + tokio runtime to bridge async VFS → sync context.
    /// Returns true on success, false on error. An unreadable file is the
    /// normal awk `-1` case; exceeding a getline cap is fatal (#2446).
    fn ensure_file_loaded(&mut self, resolved: &str) -> bool {
        if self.file_inputs.contains_key(resolved) {
            return true;
        }
        // Guard: cap cache entries and retained bytes before whole-file reads.
        if self.file_inputs.len() >= MAX_GETLINE_CACHED_FILES {
            self.fatal(&format!(
                "getline open file limit ({MAX_GETLINE_CACHED_FILES}) exceeded"
            ));
            return false;
        }
        let Some(fs) = &self.fs else {
            return false;
        };
        let fs = fs.clone();
        let p = PathBuf::from(resolved);

        // `Ok(None)` means the file exceeds MAX_GETLINE_FILE_BYTES.
        // Native: spawn a thread with its own runtime to bridge the sync AWK
        // evaluator to the async VFS without blocking the outer runtime.
        #[cfg(not(target_family = "wasm"))]
        let result = std::thread::spawn(move || -> crate::error::Result<Option<Vec<u8>>> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let meta = runtime.block_on(fs.stat(&p))?;
            if meta.size > MAX_GETLINE_FILE_BYTES as u64 {
                return Ok(None);
            }
            runtime.block_on(fs.read_file(&p)).map(Some)
        })
        .join();

        // wasm32: no threads. Drive the in-memory VFS reads inline (they resolve
        // in one poll). The outer Ok mirrors a thread that didn't panic.
        #[cfg(target_family = "wasm")]
        let result: std::result::Result<crate::error::Result<Option<Vec<u8>>>, ()> = Ok((|| {
            let meta = match fs.stat(&p).now_or_never() {
                Some(m) => m?,
                None => return Err(std::io::Error::other("awk getline: vfs read suspended").into()),
            };
            if meta.size > MAX_GETLINE_FILE_BYTES as u64 {
                return Ok(None);
            }
            match fs.read_file(&p).now_or_never() {
                Some(bytes) => bytes.map(Some),
                None => Err(std::io::Error::other("awk getline: vfs read suspended").into()),
            }
        })(
        ));

        let bytes = match result {
            Ok(Ok(Some(bytes))) if bytes.len() <= MAX_GETLINE_FILE_BYTES => bytes,
            Ok(Ok(_)) => {
                self.fatal(&format!(
                    "getline input file size limit ({MAX_GETLINE_FILE_BYTES} bytes) exceeded"
                ));
                return false;
            }
            _ => return false,
        };
        let total = self.file_input_bytes.saturating_add(bytes.len());
        if total > MAX_GETLINE_CACHE_BYTES {
            self.fatal(&format!(
                "getline total input limit ({MAX_GETLINE_CACHE_BYTES} bytes) exceeded"
            ));
            return false;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        self.file_inputs
            .insert(resolved.to_string(), (lines, 0, bytes.len()));
        self.file_input_bytes = total;
        true
    }

    /// Evaluate an expression as a boolean, with special handling for regex
    /// literals: `/regex/` is matched against $0 in boolean context (e.g. && / ||).
    fn eval_expr_as_bool(&mut self, expr: &AwkExpr) -> bool {
        if let AwkExpr::Regex(pattern) = expr {
            let line = self.state.get_field(0).as_string();
            if let Some(re) = self.runtime_regex(pattern) {
                return re.is_match(&line);
            }
            return false;
        }
        self.eval_expr(expr).as_bool()
    }

    fn eval_expr(&mut self, expr: &AwkExpr) -> AwkValue {
        if !self.memory_fits() {
            return AwkValue::Uninitialized;
        }
        match expr {
            AwkExpr::Number(n) => AwkValue::Number(*n),
            AwkExpr::String(s) => AwkValue::String(s.clone()),
            AwkExpr::Field(index) => {
                let n = self.eval_expr(index).as_number() as usize;
                self.state.get_field(n)
            }
            AwkExpr::Variable(name) => self.state.get_variable(name),
            AwkExpr::Assign(name, val) => {
                let value = self.eval_expr(val);
                self.state.set_variable(name, value.clone());
                value
            }
            AwkExpr::BinOp(left, op, right) => {
                if op == "&&" {
                    let lb = self.eval_expr_as_bool(left);
                    if !lb {
                        return AwkValue::Number(0.0);
                    }
                    let rb = self.eval_expr_as_bool(right);
                    return AwkValue::Number(if rb { 1.0 } else { 0.0 });
                }
                if op == "||" {
                    let lb = self.eval_expr_as_bool(left);
                    if lb {
                        return AwkValue::Number(1.0);
                    }
                    let rb = self.eval_expr_as_bool(right);
                    return AwkValue::Number(if rb { 1.0 } else { 0.0 });
                }

                let l = self.eval_expr(left);
                let r = self.eval_expr(right);

                match op.as_str() {
                    "+" => AwkValue::Number(l.as_number() + r.as_number()),
                    "-" => AwkValue::Number(l.as_number() - r.as_number()),
                    "*" => AwkValue::Number(l.as_number() * r.as_number()),
                    "/" => AwkValue::Number(l.as_number() / r.as_number()),
                    "%" => AwkValue::Number(l.as_number() % r.as_number()),
                    "^" => AwkValue::Number(l.as_number().powf(r.as_number())),
                    "==" => AwkValue::Number(if l.as_string() == r.as_string() {
                        1.0
                    } else {
                        0.0
                    }),
                    "!=" => AwkValue::Number(if l.as_string() != r.as_string() {
                        1.0
                    } else {
                        0.0
                    }),
                    "<" => AwkValue::Number(if l.as_number() < r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    ">" => AwkValue::Number(if l.as_number() > r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    "<=" => AwkValue::Number(if l.as_number() <= r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    ">=" => AwkValue::Number(if l.as_number() >= r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    "~" => {
                        if let Some(re) = self.runtime_regex(&r.as_string()) {
                            AwkValue::Number(if re.is_match(&l.as_string()) {
                                1.0
                            } else {
                                0.0
                            })
                        } else {
                            AwkValue::Number(0.0)
                        }
                    }
                    "!~" => {
                        if let Some(re) = self.runtime_regex(&r.as_string()) {
                            AwkValue::Number(if !re.is_match(&l.as_string()) {
                                1.0
                            } else {
                                0.0
                            })
                        } else {
                            AwkValue::Number(1.0)
                        }
                    }
                    "SUBSEP_CONCAT" => {
                        let subsep = self.state.get_variable("SUBSEP").as_string();
                        let (l, r) = (l.as_string(), r.as_string());
                        if !self.string_fits(l.len() + subsep.len() + r.len()) {
                            return AwkValue::Uninitialized;
                        }
                        AwkValue::String(format!("{l}{subsep}{r}"))
                    }
                    _ => AwkValue::Uninitialized,
                }
            }
            AwkExpr::UnaryOp(op, expr) => match op.as_str() {
                "-" => {
                    let v = self.eval_expr(expr);
                    AwkValue::Number(-v.as_number())
                }
                "!" => {
                    let b = self.eval_expr_as_bool(expr);
                    AwkValue::Number(if b { 0.0 } else { 1.0 })
                }
                _ => self.eval_expr(expr),
            },
            AwkExpr::Concat(parts) => {
                let parts: Vec<String> = parts
                    .iter()
                    .map(|p| self.eval_expr(p).as_string())
                    .collect();
                if !self.string_fits(parts.iter().map(String::len).sum()) {
                    return AwkValue::Uninitialized;
                }
                AwkValue::String(parts.concat())
            }
            AwkExpr::ArrayAssign(name, key, val) => {
                let k = self.eval_expr(key).as_string();
                let v = self.eval_expr(val);
                let full_key = format!("{}[{}]", name, k);
                self.state.set_variable(&full_key, v.clone());
                v
            }
            AwkExpr::CompoundArrayAssign(name, key, op, val) => {
                let k = self.eval_expr(key).as_string();
                let full_key = format!("{}[{}]", name, k);
                let current = self.state.get_variable(&full_key).as_number();
                let rhs = self.eval_expr(val).as_number();
                let result = match op.as_str() {
                    "+" => current + rhs,
                    "-" => current - rhs,
                    "*" => current * rhs,
                    "/" => current / rhs,
                    "%" => current % rhs,
                    _ => rhs,
                };
                let v = AwkValue::Number(result);
                self.state.set_variable(&full_key, v.clone());
                v
            }
            AwkExpr::FieldAssign(index, val) => {
                let n = self.eval_expr(index).as_number() as usize;
                let v = self.eval_expr(val);
                if n == 0 {
                    self.state.set_variable("$0", v.clone());
                } else {
                    if n > AWK_MAX_FIELD_INDEX {
                        self.fatal(&format!(
                            "field index limit ({AWK_MAX_FIELD_INDEX}) exceeded"
                        ));
                        return AwkValue::Uninitialized;
                    }
                    // The rebuilt $0 must fit before any field is created.
                    let value_len = v.as_string().len();
                    let nf = n.max(self.state.fields.len());
                    let kept: usize = self
                        .state
                        .fields
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != n - 1)
                        .map(|(_, f)| f.len())
                        .sum();
                    let seps = (nf - 1).saturating_mul(self.state.ofs.len());
                    if !self.string_fits(kept + value_len + seps) {
                        return AwkValue::Uninitialized;
                    }
                    // Extend fields if needed
                    while self.state.fields.len() < n {
                        self.state.fields.push(String::new());
                    }
                    self.state.fields[n - 1] = v.as_string();
                    self.state.nf = self.state.fields.len();
                    // Rebuild $0
                    let new_line = self.state.fields.join(&self.state.ofs);
                    self.state.set_variable("$0", AwkValue::String(new_line));
                }
                v
            }
            AwkExpr::PostIncrement(name) => {
                let current = self.state.get_variable(name).as_number();
                self.state
                    .set_variable(name, AwkValue::Number(current + 1.0));
                AwkValue::Number(current) // Return old value
            }
            AwkExpr::PostDecrement(name) => {
                let current = self.state.get_variable(name).as_number();
                self.state
                    .set_variable(name, AwkValue::Number(current - 1.0));
                AwkValue::Number(current) // Return old value
            }
            AwkExpr::PreIncrement(name) => {
                let current = self.state.get_variable(name).as_number();
                let new_val = current + 1.0;
                self.state.set_variable(name, AwkValue::Number(new_val));
                AwkValue::Number(new_val) // Return new value
            }
            AwkExpr::PreDecrement(name) => {
                let current = self.state.get_variable(name).as_number();
                let new_val = current - 1.0;
                self.state.set_variable(name, AwkValue::Number(new_val));
                AwkValue::Number(new_val) // Return new value
            }
            AwkExpr::InArray(key, arr_name) => {
                let k = self.eval_expr(key).as_string();
                let full_key = format!("{}[{}]", arr_name, k);
                let exists = !matches!(self.state.get_variable(&full_key), AwkValue::Uninitialized);
                AwkValue::Number(if exists { 1.0 } else { 0.0 })
            }
            AwkExpr::FuncCall(name, args) => self.call_function(name, args),
            AwkExpr::Regex(pattern) => {
                // When used as a standalone expression, /regex/ matches against $0.
                // When used as a function argument (gsub, sub, match, split),
                // it's evaluated as a string pattern, so return the pattern string.
                AwkValue::String(pattern.clone())
            }
            AwkExpr::Match(expr, pattern) => {
                let s = self.eval_expr(expr).as_string();
                if let Some(re) = self.runtime_regex(pattern) {
                    AwkValue::Number(if re.is_match(&s) { 1.0 } else { 0.0 })
                } else {
                    AwkValue::Number(0.0)
                }
            }
            AwkExpr::GetlineFile { var, file } => {
                let path_str = self.eval_expr(file).as_string();
                let resolved = self.resolve_getline_path(&path_str);

                if !self.ensure_file_loaded(&resolved) {
                    return AwkValue::Number(-1.0);
                }

                let entry = self.file_inputs.get_mut(&resolved).unwrap();
                if entry.1 < entry.0.len() {
                    let line = entry.0[entry.1].clone();
                    entry.1 += 1;
                    match var {
                        Some(v) => {
                            self.state
                                .variables
                                .insert(v.clone(), AwkValue::String(line));
                        }
                        None => {
                            self.state.set_line(&line);
                        }
                    }
                    AwkValue::Number(1.0) // success
                } else {
                    AwkValue::Number(0.0) // EOF
                }
            }
        }
    }

    fn call_function(&mut self, name: &str, args: &[AwkExpr]) -> AwkValue {
        match name {
            "length" => {
                if args.is_empty() {
                    AwkValue::Number(self.state.get_field(0).as_string().len() as f64)
                } else {
                    // Check if the argument is an array name - if so, return element count
                    if let AwkExpr::Variable(ref arr_name) = args[0] {
                        let prefix = format!("{}[", arr_name);
                        let count = self
                            .state
                            .variables
                            .keys()
                            .filter(|k| k.starts_with(&prefix))
                            .count();
                        if count > 0 {
                            return AwkValue::Number(count as f64);
                        }
                    }
                    AwkValue::Number(self.eval_expr(&args[0]).as_string().len() as f64)
                }
            }
            "substr" => {
                if args.len() < 2 {
                    return AwkValue::Uninitialized;
                }
                let s = self.eval_expr(&args[0]).as_string();
                let start = (self.eval_expr(&args[1]).as_number() as usize).saturating_sub(1);
                let len = if args.len() > 2 {
                    self.eval_expr(&args[2]).as_number() as usize
                } else {
                    s.len()
                };
                let end = (start + len).min(s.len());
                AwkValue::String(s.chars().skip(start).take(end - start).collect())
            }
            "index" => {
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let s = self.eval_expr(&args[0]).as_string();
                let t = self.eval_expr(&args[1]).as_string();
                match s.find(&t) {
                    Some(i) => AwkValue::Number((i + 1) as f64),
                    None => AwkValue::Number(0.0),
                }
            }
            "split" => {
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let s = self.eval_expr(&args[0]).as_string();
                // A regex constant (`/re/`) is always an ERE; a string follows
                // the same rules as FS (see `FieldSep`).
                let (sep, force_regex) = match args.get(2) {
                    Some(AwkExpr::Regex(pattern)) => (pattern.clone(), true),
                    Some(expr) => (self.eval_expr(expr).as_string(), false),
                    None => (self.state.fs.clone(), false),
                };
                let regex = if (force_regex && !sep.is_empty()) || FieldSep::is_regex(&sep) {
                    self.runtime_regex(&sep)
                } else {
                    None
                };
                let parts = FieldSep::classify(&sep, regex.as_ref()).split(&s);

                // Store in array variable, replacing any previous contents.
                if let AwkExpr::Variable(arr_name) = &args[1] {
                    self.state.clear_array(arr_name);
                    for (i, part) in parts.iter().enumerate() {
                        // One call can create millions of elements.
                        if !self.memory_fits() {
                            return AwkValue::Uninitialized;
                        }
                        let key = format!("{}[{}]", arr_name, i + 1);
                        self.state
                            .set_variable(&key, AwkValue::String(part.to_string()));
                    }
                }

                AwkValue::Number(parts.len() as f64)
            }
            "sprintf" => {
                if args.is_empty() {
                    return AwkValue::String(String::new());
                }
                let format = self.eval_expr(&args[0]).as_string();
                let values: Vec<AwkValue> = args[1..].iter().map(|a| self.eval_expr(a)).collect();
                match self.format_string(&format, &values) {
                    Ok(s) => AwkValue::String(s),
                    Err(FormatError::Message(e)) => {
                        self.stderr_output.push_str(&e);
                        self.stderr_output.push('\n');
                        AwkValue::String(String::new())
                    }
                    Err(FormatError::TooLarge) => {
                        self.string_fits(usize::MAX);
                        AwkValue::Uninitialized
                    }
                }
            }
            "toupper" => {
                if args.is_empty() {
                    return AwkValue::Uninitialized;
                }
                AwkValue::String(self.eval_expr(&args[0]).as_string().to_uppercase())
            }
            "tolower" => {
                if args.is_empty() {
                    return AwkValue::Uninitialized;
                }
                AwkValue::String(self.eval_expr(&args[0]).as_string().to_lowercase())
            }
            "gsub" | "sub" => {
                // gsub(regexp, replacement, target)
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let pattern = self.eval_expr(&args[0]).as_string();
                let replacement = self.eval_expr(&args[1]).as_string();

                let target_expr = if args.len() > 2 {
                    args[2].clone()
                } else {
                    AwkExpr::Field(Box::new(AwkExpr::Number(0.0)))
                };

                let target = self.eval_expr(&target_expr).as_string();

                if let Some(re) = self.runtime_regex(&pattern) {
                    let which = if name == "gsub" {
                        Replace::All
                    } else {
                        Replace::Nth(1)
                    };
                    let Some((result, count)) =
                        self.replace_checked(&re, &target, &replacement, which, true)
                    else {
                        return AwkValue::Number(0.0);
                    };

                    // Update the target variable or field
                    match &target_expr {
                        AwkExpr::Variable(name) => {
                            self.state.set_variable(name, AwkValue::String(result));
                        }
                        AwkExpr::Field(index) => {
                            let n = self.eval_expr(index).as_number() as usize;
                            if n == 0 {
                                // $0 is stored as a variable
                                self.state.set_variable("$0", AwkValue::String(result));
                            }
                            // For other fields, we'd need to update the fields vec
                            // and rebuild $0, but for now we just support $0
                        }
                        _ => {}
                    }

                    AwkValue::Number(count as f64)
                } else {
                    AwkValue::Number(0.0)
                }
            }
            "int" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().trunc())
            }
            "sqrt" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().sqrt())
            }
            "sin" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().sin())
            }
            "cos" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().cos())
            }
            "log" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().ln())
            }
            "exp" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().exp())
            }
            "match" => {
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let s = self.eval_expr(&args[0]).as_string();
                let pattern = self.eval_expr(&args[1]).as_string();
                // Extract capture array name from 3rd arg (gawk extension)
                let arr_name = if args.len() >= 3 {
                    if let AwkExpr::Variable(name) = &args[2] {
                        Some(name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(re) = self.runtime_regex(&pattern) {
                    if let Some(caps) = re.captures(&s) {
                        let m = caps.get(0).unwrap();
                        let rstart = m.start() + 1; // awk is 1-indexed
                        let rlength = m.end() - m.start();
                        self.state
                            .set_variable("RSTART", AwkValue::Number(rstart as f64));
                        self.state
                            .set_variable("RLENGTH", AwkValue::Number(rlength as f64));
                        // Populate capture array if 3rd arg provided
                        if let Some(ref arr) = arr_name {
                            // arr[0] = entire match
                            let full_key = format!("{}[0]", arr);
                            self.state
                                .set_variable(&full_key, AwkValue::String(m.as_str().to_string()));
                            // arr[1..N] = capture groups
                            for i in 1..caps.len() {
                                let key = format!("{}[{}]", arr, i);
                                let val = caps
                                    .get(i)
                                    .map(|c| c.as_str().to_string())
                                    .unwrap_or_default();
                                self.state.set_variable(&key, AwkValue::String(val));
                            }
                        }
                        AwkValue::Number(rstart as f64)
                    } else {
                        self.state.set_variable("RSTART", AwkValue::Number(0.0));
                        self.state.set_variable("RLENGTH", AwkValue::Number(-1.0));
                        AwkValue::Number(0.0)
                    }
                } else {
                    AwkValue::Number(0.0)
                }
            }
            "gensub" => {
                // gensub(regexp, replacement, how [, target])
                if args.len() < 3 {
                    return AwkValue::Uninitialized;
                }
                let pattern = self.eval_expr(&args[0]).as_string();
                let replacement = self.eval_expr(&args[1]).as_string();
                let how = self.eval_expr(&args[2]).as_string();
                let target = if args.len() > 3 {
                    self.eval_expr(&args[3]).as_string()
                } else {
                    self.state.get_field(0).as_string()
                };
                if let Some(re) = self.runtime_regex(&pattern) {
                    let (which, expand) = if how == "g" || how == "G" {
                        (Replace::All, true)
                    } else {
                        // Replace nth occurrence (default 1st)
                        (Replace::Nth(how.parse::<usize>().unwrap_or(1)), false)
                    };
                    match self.replace_checked(&re, &target, &replacement, which, expand) {
                        Some((result, _)) => AwkValue::String(result),
                        None => AwkValue::Uninitialized,
                    }
                } else {
                    AwkValue::String(target)
                }
            }
            "__getline" => {
                // Plain getline as expression — advance to next input line, return 1/0
                self.line_index += 1;
                if self.line_index < self.input_lines.len() {
                    let line = self.input_lines[self.line_index].clone();
                    self.state.set_line(&line);
                    AwkValue::Number(1.0)
                } else {
                    AwkValue::Number(0.0)
                }
            }
            "__array_access" => {
                // Internal function for array indexing: arr[index]
                if args.len() < 2 {
                    return AwkValue::Uninitialized;
                }
                let arr_name = if let AwkExpr::Variable(name) = &args[0] {
                    name.clone()
                } else {
                    return AwkValue::Uninitialized;
                };
                let index = self.eval_expr(&args[1]);
                let key = format!("{}[{}]", arr_name, index.as_string());
                self.state.get_variable(&key)
            }
            "__ternary" => {
                // Ternary operator: cond ? then : else
                if args.len() < 3 {
                    return AwkValue::Uninitialized;
                }
                let cond = self.eval_expr(&args[0]);
                if cond.as_bool() {
                    self.eval_expr(&args[1])
                } else {
                    self.eval_expr(&args[2])
                }
            }
            "close" => {
                // close(name): release a `getline < file` input (freeing its
                // share of the getline caps) and make the next `>` write to
                // the same path truncate again. 0 if something was open.
                let Some(arg) = args.first() else {
                    return AwkValue::Number(-1.0);
                };
                let name = self.eval_expr(arg).as_string();
                let resolved = self.resolve_getline_path(&name);
                let mut closed = false;
                if let Some((_, _, bytes)) = self.file_inputs.remove(&resolved) {
                    self.file_input_bytes = self.file_input_bytes.saturating_sub(bytes);
                    closed = true;
                }
                let out_key = if name.starts_with('/') {
                    PathBuf::from(&name)
                } else {
                    vfs_join(&self.cwd, &name)
                }
                .to_string_lossy()
                .into_owned();
                closed |= self.file_truncates.remove(&out_key);
                AwkValue::Number(if closed { 0.0 } else { -1.0 })
            }
            _ => {
                // Check for user-defined function
                if let Some(func) = self.functions.get(name).cloned() {
                    self.call_user_function(&func, args)
                } else {
                    AwkValue::Uninitialized
                }
            }
        }
    }

    fn call_user_function(&mut self, func: &AwkFunctionDef, args: &[AwkExpr]) -> AwkValue {
        // THREAT[TM-DOS-027]: Limit recursion depth to prevent stack overflow
        if self.call_depth >= MAX_AWK_CALL_DEPTH {
            self.fatal(&format!(
                "function call depth limit ({MAX_AWK_CALL_DEPTH}) exceeded"
            ));
            return AwkValue::Uninitialized;
        }
        self.call_depth += 1;

        // Save current local variables that will be shadowed. Stored values
        // move out of the map and stay accounted as pinned bytes, so deep
        // recursion cannot duplicate memory past the cap (TM-DOS-110).
        let mut saved: Vec<(String, AwkValue, bool)> = Vec::new();
        for param in &func.params {
            match self.state.remove_var(param) {
                Some(val) => {
                    self.pinned_bytes += super::var_cost(param, &val);
                    saved.push((param.clone(), val, true));
                }
                None => saved.push((param.clone(), self.state.get_variable(param), false)),
            }
        }

        // Bind arguments to parameters
        for (i, param) in func.params.iter().enumerate() {
            let val = if i < args.len() {
                self.eval_expr(&args[i])
            } else {
                AwkValue::Uninitialized
            };
            self.state.set_variable(param, val);
        }

        // Execute function body, capture return value
        let mut return_value = AwkValue::Uninitialized;
        for action in &func.body.clone() {
            match self.exec_action(action) {
                AwkFlow::Return(val) => {
                    return_value = val;
                    break;
                }
                AwkFlow::Exit(_) => break,
                _ => {}
            }
        }

        // Restore saved variables
        for (name, val, pinned) in saved {
            if pinned {
                self.pinned_bytes -= super::var_cost(&name, &val);
            }
            self.state.set_variable(&name, val);
        }

        self.call_depth -= 1;
        return_value
    }

    /// Max width/precision for format specifiers to prevent memory exhaustion
    const MAX_FORMAT_WIDTH: usize = MAX_FORMAT_WIDTH;

    fn format_string(
        &self,
        format: &str,
        values: &[AwkValue],
    ) -> std::result::Result<String, FormatError> {
        let mut result = String::new();
        let mut chars = format.chars().peekable();
        let mut value_idx = 0;

        while let Some(c) = chars.next() {
            if c == '\\' {
                // Handle escape sequences in format strings
                match chars.peek() {
                    Some('n') => {
                        chars.next();
                        result.push('\n');
                    }
                    Some('t') => {
                        chars.next();
                        result.push('\t');
                    }
                    Some('r') => {
                        chars.next();
                        result.push('\r');
                    }
                    Some('\\') => {
                        chars.next();
                        result.push('\\');
                    }
                    _ => result.push('\\'),
                }
            } else if c == '%' {
                if chars.peek() == Some(&'%') {
                    chars.next();
                    result.push('%');
                    continue;
                }

                // Parse format specifier: %[flags][width][.precision]type
                let mut left_align = false;
                let mut zero_pad = false;
                let mut plus_sign = false;
                let mut width: Option<usize> = None;
                let mut precision: Option<usize> = None;
                let mut conversion = ' ';

                // Parse flags
                loop {
                    match chars.peek() {
                        Some(&'-') => {
                            left_align = true;
                            chars.next();
                        }
                        Some(&'0') if width.is_none() => {
                            zero_pad = true;
                            chars.next();
                        }
                        Some(&'+') => {
                            plus_sign = true;
                            chars.next();
                        }
                        _ => break,
                    }
                }

                // Parse width
                let mut w = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        w.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !w.is_empty()
                    && let Ok(w_val) = w.parse::<usize>()
                {
                    if w_val > Self::MAX_FORMAT_WIDTH {
                        return Err(FormatError::Message(format!(
                            "awk: format width {} exceeds maximum ({})",
                            w_val,
                            Self::MAX_FORMAT_WIDTH
                        )));
                    }
                    width = Some(w_val);
                }

                // Parse precision
                if chars.peek() == Some(&'.') {
                    chars.next();
                    let mut p = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() {
                            p.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    precision = if p.is_empty() {
                        Some(0)
                    } else if let Ok(p_val) = p.parse::<usize>() {
                        if p_val > Self::MAX_FORMAT_WIDTH {
                            return Err(FormatError::Message(format!(
                                "awk: format precision {} exceeds maximum ({})",
                                p_val,
                                Self::MAX_FORMAT_WIDTH
                            )));
                        }
                        Some(p_val)
                    } else {
                        None
                    };
                }

                // Parse conversion character
                if let Some(&c) = chars.peek()
                    && c.is_ascii_alphabetic()
                {
                    conversion = c;
                    chars.next();
                }

                if value_idx < values.len() {
                    let val = &values[value_idx];
                    value_idx += 1;

                    let formatted = match conversion {
                        'd' | 'i' => {
                            let n = val.as_number() as i64;
                            if plus_sign && n >= 0 {
                                format!("+{}", n)
                            } else {
                                format!("{}", n)
                            }
                        }
                        'f' => {
                            let n = val.as_number();
                            let prec = precision.unwrap_or(6);
                            format!("{:.prec$}", n)
                        }
                        'g' => {
                            let n = val.as_number();
                            let prec = precision.unwrap_or(6);
                            // %g: use shorter of %e or %f, strip trailing zeros
                            let s = format!("{:.prec$e}", n);
                            let f = format!("{:.prec$}", n);
                            if s.len() < f.len() { s } else { f }
                        }
                        'e' | 'E' => {
                            let n = val.as_number();
                            let prec = precision.unwrap_or(6);
                            format!("{:.prec$e}", n)
                        }
                        's' => {
                            let mut s = val.as_string();
                            if let Some(p) = precision {
                                s = s.chars().take(p).collect();
                            }
                            s
                        }
                        'c' => {
                            // %c: print character from ASCII code or first char of string
                            let n = val.as_number();
                            if n > 0.0 && n < 128.0 {
                                String::from(n as u8 as char)
                            } else {
                                let s = val.as_string();
                                s.chars().next().map(String::from).unwrap_or_default()
                            }
                        }
                        'x' | 'X' => {
                            let n = val.as_number() as i64;
                            if conversion == 'X' {
                                format!("{:X}", n)
                            } else {
                                format!("{:x}", n)
                            }
                        }
                        'o' => {
                            let n = val.as_number() as i64;
                            format!("{:o}", n)
                        }
                        _ => val.as_string(),
                    };

                    // Apply width and alignment
                    if let Some(w) = width {
                        if formatted.len() < w {
                            let padding = w - formatted.len();
                            if left_align {
                                result.push_str(&formatted);
                                for _ in 0..padding {
                                    result.push(' ');
                                }
                            } else if zero_pad
                                && matches!(conversion, 'd' | 'i' | 'f' | 'x' | 'X' | 'o')
                            {
                                for _ in 0..padding {
                                    result.push('0');
                                }
                                result.push_str(&formatted);
                            } else {
                                for _ in 0..padding {
                                    result.push(' ');
                                }
                                result.push_str(&formatted);
                            }
                        } else {
                            result.push_str(&formatted);
                        }
                    } else {
                        result.push_str(&formatted);
                    }
                }
            } else {
                result.push(c);
            }
            // THREAT[TM-DOS-110]: each conversion adds at most
            // MAX_FORMAT_WIDTH (or one value); stop before the next one.
            if result.len() > AWK_MAX_STRING_BYTES {
                return Err(FormatError::TooLarge);
            }
        }

        Ok(result)
    }

    /// Total bytes accounted across all output streams (O(1): stdout/stderr
    /// lengths plus the running streamed-redirect counter).
    fn total_output_bytes(&self) -> usize {
        self.output.len() + self.stderr_output.len() + self.redirected_file_output_bytes
    }

    fn has_output_capacity(&self, len: usize) -> bool {
        len <= MAX_AWK_OUTPUT_BYTES.saturating_sub(self.total_output_bytes())
    }

    /// Stream text to stdout, stderr, or the VFS-backed redirect target.
    /// Returns `false` if output limits or VFS validation reject the write.
    fn write_output(&mut self, text: &str, target: &Option<AwkOutputTarget>) -> bool {
        if !self.has_output_capacity(text.len()) {
            self.stderr_output
                .push_str("awk: output limit exceeded (max 10MB)\n");
            return false;
        }
        match target {
            None => {
                self.output.push_str(text);
                true
            }
            Some(AwkOutputTarget::Truncate(expr)) | Some(AwkOutputTarget::Append(expr)) => {
                let path = self.eval_expr(expr).as_string();
                // Intercept special devices before VFS work so /dev/null never buffers.
                if path == "/dev/null" {
                    true
                } else if path == "/dev/stderr" {
                    self.stderr_output.push_str(text);
                    true
                } else if path == "/dev/stdout" {
                    self.output.push_str(text);
                    true
                } else {
                    let append = matches!(target, Some(AwkOutputTarget::Append(_)));
                    self.write_file_output(&path, text, append)
                }
            }
        }
    }

    /// Write redirected output immediately through the VFS so sandbox quotas are
    /// enforced during AWK execution instead of after unbounded buffering.
    fn write_file_output(&mut self, path: &str, text: &str, append: bool) -> bool {
        let resolved = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            vfs_join(&self.cwd, path)
        };
        let key = resolved.to_string_lossy().into_owned();
        // Bound the number of distinct redirect targets (DoS guard, parity with
        // the previous buffered implementation). file_truncates doubles as the
        // distinct-target set, so reject new targets before inserting them.
        if !self.file_truncates.contains(&key)
            && self.file_truncates.len() >= MAX_AWK_OUTPUT_TARGETS
        {
            self.stderr_output
                .push_str("awk: too many output redirection targets\n");
            return false;
        }
        let fs = match &self.fs {
            Some(fs) => fs.clone(),
            None => {
                self.stderr_output
                    .push_str("awk: no filesystem available\n");
                return false;
            }
        };
        let bytes = text.as_bytes().to_vec();
        // First write to a path truncates (for `>`); later writes append. `>>`
        // always appends. insert() returns false when the path was already seen.
        let first_write = self.file_truncates.insert(key);
        let append = append || !first_write;

        // Start the single reusable writer thread on first use, then dispatch
        // every redirected write through it (one thread/runtime per AWK run, not
        // per write).
        let writer = self.vfs_writer.get_or_insert_with(|| VfsWriter::new(fs));
        match writer.write(resolved, bytes, append) {
            Ok(Ok(())) => {
                self.redirected_file_output_bytes += text.len();
                true
            }
            Ok(Err(e)) => {
                self.stderr_output
                    .push_str(&format!("awk: cannot write {path}: {e}\n"));
                false
            }
            Err(()) => {
                self.stderr_output
                    .push_str(&format!("awk: cannot write {path}: write worker failed\n"));
                false
            }
        }
    }

    /// Execute action. Returns flow control signal.
    pub(super) fn exec_action(&mut self, action: &AwkAction) -> AwkFlow {
        if self
            .execution_budget
            .as_ref()
            .is_some_and(|budget| budget.consume_work(1).is_err())
        {
            // The builtin dispatcher reports the poisoned budget.
            self.fatal = true;
        }
        if self.fatal || !self.memory_fits() {
            return AwkFlow::Exit(Some(2));
        }
        match action {
            AwkAction::Print(exprs, target) => {
                let parts: Vec<String> = exprs
                    .iter()
                    .map(|e| self.eval_expr(e).as_string())
                    .collect();
                if self.fatal {
                    // A limit fired while evaluating the arguments.
                    return AwkFlow::Exit(Some(2));
                }
                let len = parts.iter().map(String::len).sum::<usize>()
                    + parts.len().saturating_sub(1) * self.state.ofs.len();
                if !self.string_fits(len) {
                    return AwkFlow::Exit(Some(2));
                }
                let mut text = parts.join(&self.state.ofs);
                text.push_str(&self.state.ors);
                if !self.write_output(&text, target) {
                    // write_output already reported why; stop like a fatal error.
                    self.fatal = true;
                    return AwkFlow::Exit(Some(2));
                }
                AwkFlow::Continue
            }
            AwkAction::Printf(format_expr, args, target) => {
                let format_str = self.eval_expr(format_expr).as_string();
                let values: Vec<AwkValue> = args.iter().map(|a| self.eval_expr(a)).collect();
                if self.fatal {
                    return AwkFlow::Exit(Some(2));
                }
                match self.format_string(&format_str, &values) {
                    Ok(text) => {
                        if !self.write_output(&text, target) {
                            // write_output already reported why; stop like a fatal error.
                            self.fatal = true;
                            return AwkFlow::Exit(Some(2));
                        }
                        AwkFlow::Continue
                    }
                    Err(FormatError::Message(e)) => {
                        self.stderr_output.push_str(&e);
                        self.stderr_output.push('\n');
                        AwkFlow::Exit(Some(2))
                    }
                    Err(FormatError::TooLarge) => {
                        self.string_fits(usize::MAX);
                        AwkFlow::Exit(Some(2))
                    }
                }
            }
            AwkAction::Assign(name, expr) => {
                let value = self.eval_expr(expr);
                self.state.set_variable(name, value);
                AwkFlow::Continue
            }
            AwkAction::ArrayAssign(name, key, val) => {
                let k = self.eval_expr(key).as_string();
                let v = self.eval_expr(val);
                let full_key = format!("{}[{}]", name, k);
                self.state.set_variable(&full_key, v);
                AwkFlow::Continue
            }
            AwkAction::If(cond, then_actions, else_actions) => {
                let actions = if self.eval_expr(cond).as_bool() {
                    then_actions
                } else {
                    else_actions
                };
                for action in actions {
                    match self.exec_action(action) {
                        AwkFlow::Continue => {}
                        flow => return flow,
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::While(cond, actions) => {
                let mut iters = 0;
                while self.eval_expr(cond).as_bool() {
                    iters += 1;
                    if let Some(flow) = self.tick_loop(iters) {
                        return flow;
                    }
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break {
                        break;
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::DoWhile(cond, actions) => {
                let mut iters = 0;
                loop {
                    iters += 1;
                    if let Some(flow) = self.tick_loop(iters) {
                        return flow;
                    }
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break || !self.eval_expr(cond).as_bool() {
                        break;
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::For(init, cond, update, actions) => {
                self.exec_action(init);
                let mut iters = 0;
                while self.eval_expr(cond).as_bool() {
                    iters += 1;
                    if let Some(flow) = self.tick_loop(iters) {
                        return flow;
                    }
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break {
                        break;
                    }
                    self.exec_action(update);
                }
                AwkFlow::Continue
            }
            AwkAction::ForIn(var, arr_name, actions) => {
                // Collect array keys matching the pattern arr_name[*]
                let prefix = format!("{}[", arr_name);
                let mut keys: Vec<String> = self
                    .state
                    .variables
                    .keys()
                    .filter(|k| k.starts_with(&prefix) && k.ends_with(']'))
                    .map(|k| k[prefix.len()..k.len() - 1].to_string())
                    .collect();
                // Sort for deterministic iteration: numeric keys first, then lexical
                keys.sort_by(|a, b| match (a.parse::<f64>(), b.parse::<f64>()) {
                    (Ok(na), Ok(nb)) => na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal),
                    _ => a.cmp(b),
                });

                let mut iters = 0;
                for key in keys {
                    iters += 1;
                    if let Some(flow) = self.tick_loop(iters) {
                        return flow;
                    }
                    self.state.set_variable(var, AwkValue::String(key));
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break {
                        break;
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::Delete(arr_name, key) => {
                let k = self.eval_expr(key).as_string();
                if k == "*" {
                    self.state.clear_array(arr_name);
                } else {
                    let full_key = format!("{}[{}]", arr_name, k);
                    self.state.remove_var(&full_key);
                }
                AwkFlow::Continue
            }
            AwkAction::Next => AwkFlow::Next,
            AwkAction::Getline => {
                // Advance to next input line and update $0, NR, NF, FNR
                self.line_index += 1;
                if self.line_index < self.input_lines.len() {
                    let line = self.input_lines[self.line_index].clone();
                    self.state.set_line(&line);
                }
                AwkFlow::Continue
            }
            AwkAction::GetlineFile { var, file } => {
                // Read next line from file (action context — return value discarded).
                let path_str = self.eval_expr(file).as_string();
                let resolved = self.resolve_getline_path(&path_str);

                if !self.ensure_file_loaded(&resolved) {
                    return AwkFlow::Continue;
                }

                let entry = self.file_inputs.get_mut(&resolved).unwrap();
                if entry.1 < entry.0.len() {
                    let line = entry.0[entry.1].clone();
                    entry.1 += 1;
                    match var {
                        Some(v) => {
                            self.state
                                .variables
                                .insert(v.clone(), AwkValue::String(line));
                        }
                        None => {
                            self.state.set_line(&line);
                        }
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::Break => AwkFlow::Break,
            AwkAction::Continue => AwkFlow::LoopContinue,
            AwkAction::Exit(expr) => {
                let code = expr.as_ref().map(|e| self.eval_expr(e).as_number() as i32);
                AwkFlow::Exit(code)
            }
            AwkAction::Return(expr) => {
                let val = expr
                    .as_ref()
                    .map(|e| self.eval_expr(e))
                    .unwrap_or(AwkValue::Uninitialized);
                AwkFlow::Return(val)
            }
            AwkAction::Expression(expr) => {
                self.eval_expr(expr);
                AwkFlow::Continue
            }
        }
    }

    fn matches_pattern(&mut self, pattern: &AwkPattern) -> bool {
        match pattern {
            AwkPattern::Regex(re) => {
                let line = self.state.get_field(0).as_string();
                re.is_match(&line)
            }
            AwkPattern::Expression(expr) => self.eval_expr(expr).as_bool(),
            // Range patterns are handled specially via matches_pattern_with_index
            // which tracks state. This arm shouldn't normally be reached for ranges
            // in the main loop, but handle it defensively.
            AwkPattern::Range(_, _) => false,
        }
    }

    /// Check if a rule's pattern matches, with range state tracking by rule index.
    pub(super) fn matches_pattern_with_index(
        &mut self,
        pattern: &AwkPattern,
        rule_idx: usize,
    ) -> bool {
        match pattern {
            AwkPattern::Range(start, end) => {
                let active = *self.range_active.get(&rule_idx).unwrap_or(&false);
                if active {
                    // Already in range — check if end pattern matches
                    if self.matches_pattern(end) {
                        // End pattern matched: include this line, deactivate range
                        self.range_active.insert(rule_idx, false);
                    }
                    true
                } else {
                    // Not in range — check if start pattern matches
                    if self.matches_pattern(start) {
                        // If end also matches this same line, range closes immediately.
                        let end_matches = self.matches_pattern(end);
                        self.range_active.insert(rule_idx, !end_matches);
                        true
                    } else {
                        false
                    }
                }
            }
            other => self.matches_pattern(other),
        }
    }
}

#[cfg(test)]
mod regex_cache_tests {
    use super::*;

    #[test]
    fn repeated_match_expression_compiles_regex_once() {
        let mut interp = AwkInterpreter::new();
        interp.state.set_line("bytes=123");
        let expr = AwkExpr::BinOp(
            Box::new(AwkExpr::Field(Box::new(AwkExpr::Number(0.0)))),
            "~".to_string(),
            Box::new(AwkExpr::Regex("^bytes=".to_string())),
        );

        for _ in 0..300_000 {
            assert!(interp.eval_expr(&expr).as_bool());
        }

        assert_eq!(interp.regex_cache.compile_count(), 1);
    }

    #[test]
    fn repeated_invalid_regex_compiles_once() {
        let mut interp = AwkInterpreter::new();

        for _ in 0..100 {
            assert!(interp.runtime_regex("[").is_none());
        }

        assert_eq!(interp.regex_cache.compile_count(), 1);
    }
}
