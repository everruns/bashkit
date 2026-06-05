//! AWK interpreter - executes a parsed `AwkProgram`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::{AwkAction, AwkExpr, AwkFunctionDef, AwkOutputTarget, AwkPattern, AwkState, AwkValue};
use crate::builtins::MAX_FORMAT_WIDTH;
use crate::builtins::limits::{
    AWK_MAX_CALL_DEPTH as MAX_AWK_CALL_DEPTH,
    AWK_MAX_GETLINE_CACHE_BYTES as MAX_GETLINE_CACHE_BYTES,
    AWK_MAX_GETLINE_CACHED_FILES as MAX_GETLINE_CACHED_FILES,
    AWK_MAX_GETLINE_FILE_BYTES as MAX_GETLINE_FILE_BYTES,
    AWK_MAX_OUTPUT_BYTES as MAX_AWK_OUTPUT_BYTES, AWK_MAX_OUTPUT_TARGETS as MAX_AWK_OUTPUT_TARGETS,
};
use crate::builtins::search_common::build_regex;
use crate::fs::{FileSystem, normalize_path};
use crate::limits::ExecutionLimits;

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
    /// Buffered file outputs for `>` redirection (path -> content).
    /// Truncate mode: first write creates entry, subsequent writes append to buffer.
    /// Flushed to VFS after execution completes.
    pub(super) file_outputs: HashMap<String, String>,
    /// Buffered file outputs for `>>` redirection (path -> content).
    /// Append mode: content is appended to existing file on flush.
    pub(super) file_appends: HashMap<String, String>,
    /// Running byte total across stdout, stderr, and buffered file outputs.
    /// Keep this O(1): recomputing from redirect maps on every write is quadratic
    /// for attacker-controlled scripts that print tiny records to many paths.
    total_output_bytes: usize,
    /// Cached file inputs for `getline var < file` redirection.
    /// Maps normalized resolved path -> (lines, current_position).
    file_inputs: HashMap<String, (Vec<String>, usize)>,
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
            file_outputs: HashMap::new(),
            file_appends: HashMap::new(),
            total_output_bytes: 0,
            file_inputs: HashMap::new(),
            file_input_bytes: 0,
            call_depth: 0,
            fs: None,
            cwd: PathBuf::from("/"),
            range_active: HashMap::new(),
            max_loop_iterations: ExecutionLimits::default().max_loop_iterations,
        }
    }

    fn resolve_getline_path(&self, path_str: &str) -> String {
        let resolved = if path_str.starts_with('/') {
            PathBuf::from(path_str)
        } else {
            self.cwd.join(path_str)
        };
        normalize_path(&resolved).to_string_lossy().to_string()
    }

    /// Load a file into the `file_inputs` cache if not already present.
    /// Uses a separate thread + tokio runtime to bridge async VFS → sync context.
    /// Returns true on success, false on error.
    fn ensure_file_loaded(&mut self, resolved: &str) -> bool {
        if self.file_inputs.contains_key(resolved) {
            return true;
        }
        // Guard: cap cache entries and retained bytes before whole-file reads.
        if self.file_inputs.len() >= MAX_GETLINE_CACHED_FILES {
            return false;
        }
        let Some(fs) = &self.fs else {
            return false;
        };
        let fs = fs.clone();
        let p = PathBuf::from(resolved);
        // Spawn a thread with its own runtime to avoid blocking the current async runtime.
        let result = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let meta = runtime.block_on(fs.stat(&p))?;
            if meta.size > MAX_GETLINE_FILE_BYTES as u64 {
                return Err(std::io::Error::other("awk getline input too large").into());
            }
            runtime.block_on(fs.read_file(&p))
        })
        .join();
        match result {
            Ok(Ok(bytes)) => {
                if bytes.len() > MAX_GETLINE_FILE_BYTES {
                    return false;
                }
                let Some(total) = self.file_input_bytes.checked_add(bytes.len()) else {
                    return false;
                };
                if total > MAX_GETLINE_CACHE_BYTES {
                    return false;
                }
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                self.file_inputs.insert(resolved.to_string(), (lines, 0));
                self.file_input_bytes = total;
                true
            }
            _ => false,
        }
    }

    /// Evaluate an expression as a boolean, with special handling for regex
    /// literals: `/regex/` is matched against $0 in boolean context (e.g. && / ||).
    fn eval_expr_as_bool(&mut self, expr: &AwkExpr) -> bool {
        if let AwkExpr::Regex(pattern) = expr {
            let line = self.state.get_field(0).as_string();
            if let Ok(re) = build_regex(pattern) {
                return re.is_match(&line);
            }
            return false;
        }
        self.eval_expr(expr).as_bool()
    }

    fn eval_expr(&mut self, expr: &AwkExpr) -> AwkValue {
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
                        if let Ok(re) = build_regex(&r.as_string()) {
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
                        if let Ok(re) = build_regex(&r.as_string()) {
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
                        AwkValue::String(format!("{}{}{}", l.as_string(), subsep, r.as_string()))
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
                let s: String = parts
                    .iter()
                    .map(|p| self.eval_expr(p).as_string())
                    .collect();
                AwkValue::String(s)
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
                if let Ok(re) = build_regex(pattern) {
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
                let sep = if args.len() > 2 {
                    self.eval_expr(&args[2]).as_string()
                } else {
                    self.state.fs.clone()
                };

                let parts: Vec<&str> = if sep == " " {
                    s.split_whitespace().collect()
                } else {
                    s.split(&sep).collect()
                };

                // Store in array variable
                if let AwkExpr::Variable(arr_name) = &args[1] {
                    for (i, part) in parts.iter().enumerate() {
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
                    Err(e) => {
                        self.stderr_output.push_str(&e);
                        self.stderr_output.push('\n');
                        AwkValue::String(String::new())
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

                if let Ok(re) = build_regex(&pattern) {
                    let (result, count) = if name == "gsub" {
                        let count = re.find_iter(&target).count();
                        (
                            re.replace_all(&target, replacement.as_str()).to_string(),
                            count,
                        )
                    } else {
                        let count = if re.is_match(&target) { 1 } else { 0 };
                        (re.replace(&target, replacement.as_str()).to_string(), count)
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
                if let Ok(re) = build_regex(&pattern) {
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
                if let Ok(re) = build_regex(&pattern) {
                    if how == "g" || how == "G" {
                        AwkValue::String(re.replace_all(&target, replacement.as_str()).to_string())
                    } else {
                        // Replace nth occurrence (default 1st)
                        let n = how.parse::<usize>().unwrap_or(1);
                        let mut count = 0;
                        let result = re.replace_all(&target, |caps: &regex::Captures| -> String {
                            count += 1;
                            if count == n {
                                replacement.clone()
                            } else {
                                caps[0].to_string()
                            }
                        });
                        AwkValue::String(result.to_string())
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
            return AwkValue::Uninitialized;
        }
        self.call_depth += 1;

        // Save current local variables that will be shadowed
        let mut saved: Vec<(String, AwkValue)> = Vec::new();
        for param in &func.params {
            saved.push((param.clone(), self.state.get_variable(param)));
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
        for (name, val) in saved {
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
    ) -> std::result::Result<String, String> {
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
                        return Err(format!(
                            "awk: format width {} exceeds maximum ({})",
                            w_val,
                            Self::MAX_FORMAT_WIDTH
                        ));
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
                            return Err(format!(
                                "awk: format precision {} exceeds maximum ({})",
                                p_val,
                                Self::MAX_FORMAT_WIDTH
                            ));
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
        }

        Ok(result)
    }

    fn has_output_capacity(&self, len: usize) -> bool {
        self.total_output_bytes <= MAX_AWK_OUTPUT_BYTES.saturating_sub(len)
    }

    fn output_target_count(&self) -> usize {
        self.file_outputs.len() + self.file_appends.len()
    }

    fn is_new_output_target(&self, path: &str) -> bool {
        !self.file_outputs.contains_key(path) && !self.file_appends.contains_key(path)
    }

    /// Write text to stdout buffer or to a file output buffer based on the target.
    /// Returns `false` if the write would exceed output resource limits.
    fn write_output(&mut self, text: &str, target: &Option<AwkOutputTarget>) -> bool {
        if !self.has_output_capacity(text.len()) {
            self.stderr_output
                .push_str("awk: output limit exceeded (max 10MB)\n");
            return false;
        }
        match target {
            None => self.output.push_str(text),
            Some(AwkOutputTarget::Truncate(expr)) | Some(AwkOutputTarget::Append(expr)) => {
                let path = self.eval_expr(expr).as_string();
                // Intercept /dev/stderr and /dev/stdout — route to streams, not VFS
                if path == "/dev/stderr" {
                    self.stderr_output.push_str(text);
                } else if path == "/dev/stdout" {
                    self.output.push_str(text);
                } else {
                    if self.is_new_output_target(&path)
                        && self.output_target_count() >= MAX_AWK_OUTPUT_TARGETS
                    {
                        self.stderr_output
                            .push_str("awk: too many output redirection targets\n");
                        return false;
                    }
                    if matches!(target, Some(AwkOutputTarget::Append(_))) {
                        self.file_appends.entry(path).or_default().push_str(text);
                    } else {
                        self.file_outputs.entry(path).or_default().push_str(text);
                    }
                }
            }
        }
        self.total_output_bytes += text.len();
        true
    }

    /// Execute action. Returns flow control signal.
    pub(super) fn exec_action(&mut self, action: &AwkAction) -> AwkFlow {
        match action {
            AwkAction::Print(exprs, target) => {
                let parts: Vec<String> = exprs
                    .iter()
                    .map(|e| self.eval_expr(e).as_string())
                    .collect();
                let mut text = parts.join(&self.state.ofs);
                text.push_str(&self.state.ors);
                if !self.write_output(&text, target) {
                    return AwkFlow::Exit(Some(2));
                }
                AwkFlow::Continue
            }
            AwkAction::Printf(format_expr, args, target) => {
                let format_str = self.eval_expr(format_expr).as_string();
                let values: Vec<AwkValue> = args.iter().map(|a| self.eval_expr(a)).collect();
                match self.format_string(&format_str, &values) {
                    Ok(text) => {
                        if !self.write_output(&text, target) {
                            return AwkFlow::Exit(Some(2));
                        }
                        AwkFlow::Continue
                    }
                    Err(e) => {
                        self.stderr_output.push_str(&e);
                        self.stderr_output.push('\n');
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
                    if iters > self.max_loop_iterations {
                        break;
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
                    if iters > self.max_loop_iterations {
                        break;
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
                    if iters > self.max_loop_iterations {
                        break;
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
                    if iters > self.max_loop_iterations {
                        break;
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
                    // Delete all entries in the array
                    let prefix = format!("{}[", arr_name);
                    let keys: Vec<String> = self
                        .state
                        .variables
                        .keys()
                        .filter(|k| k.starts_with(&prefix))
                        .cloned()
                        .collect();
                    for key in keys {
                        self.state.variables.remove(&key);
                    }
                } else {
                    let full_key = format!("{}[{}]", arr_name, k);
                    self.state.variables.remove(&full_key);
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
