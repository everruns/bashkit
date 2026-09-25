//! awk - Pattern scanning and processing builtin
//!
//! Implements basic AWK functionality.
//!
//! Usage:
//!   awk '{print $1}' file
//!   awk -F: '{print $1}' /etc/passwd
//!   echo "a b c" | awk '{print $2}'
//!   awk 'BEGIN{print "start"} {print} END{print "end"}' file
//!   awk '/pattern/{print}' file
//!   awk 'NR==2{print}' file

// Parser invariant: `pos` is always a byte offset on a UTF-8 char boundary.
// Move across user-controlled text with `advance()`/`consume_while()`, and use
// raw `pos += N` only for known ASCII tokens and delimiters.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;

use super::limits::AWK_VARIABLE_OVERHEAD_BYTES;
use super::{Builtin, Context, read_text_file};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;
use crate::limits::ExecutionLimits;

/// awk command - pattern scanning and processing
pub struct Awk;

#[derive(Debug)]
struct AwkProgram {
    begin_actions: Vec<AwkAction>,
    main_rules: Vec<AwkRule>,
    end_actions: Vec<AwkAction>,
    functions: HashMap<String, AwkFunctionDef>,
}

#[derive(Debug, Clone)]
struct AwkFunctionDef {
    params: Vec<String>,
    body: Vec<AwkAction>,
}

#[derive(Debug)]
struct AwkRule {
    pattern: Option<AwkPattern>,
    actions: Vec<AwkAction>,
}

#[derive(Debug)]
enum AwkPattern {
    Regex(Regex),
    Expression(AwkExpr),
    /// Range pattern: /start/,/end/ — matches from start to end inclusive.
    /// Each sub-pattern can be a Regex or Expression.
    Range(Box<AwkPattern>, Box<AwkPattern>),
}

#[derive(Debug, Clone)]
enum AwkExpr {
    Number(f64),
    String(String),
    Field(Box<AwkExpr>), // $n
    Variable(String),    // var
    BinOp(Box<AwkExpr>, String, Box<AwkExpr>),
    UnaryOp(String, Box<AwkExpr>),
    Assign(String, Box<AwkExpr>),
    ArrayAssign(String, Box<AwkExpr>, Box<AwkExpr>), // arr[key] = val
    CompoundArrayAssign(String, Box<AwkExpr>, String, Box<AwkExpr>), // arr[key] += val
    Concat(Vec<AwkExpr>),
    FuncCall(String, Vec<AwkExpr>),
    Regex(String),
    #[allow(dead_code)] // matched in eval but construction deferred to pattern expansion
    Match(Box<AwkExpr>, String), // expr ~ /pattern/
    PostIncrement(String),                   // var++
    PostDecrement(String),                   // var--
    PreIncrement(String),                    // ++var
    PreDecrement(String),                    // --var
    InArray(Box<AwkExpr>, String),           // key in arr
    FieldAssign(Box<AwkExpr>, Box<AwkExpr>), // $n = val
    /// getline [var] < file as expression — returns 1 on success, 0 on EOF, -1 on error
    GetlineFile {
        var: Option<String>,
        file: Box<AwkExpr>,
    },
}

/// Output target for print/printf redirection (e.g., `> file`, `>> file`).
/// Pipe (`| cmd`) is not supported and returns a clear error.
#[derive(Debug, Clone)]
enum AwkOutputTarget {
    /// Truncate/create file: `> file`
    Truncate(AwkExpr),
    /// Append to file: `>> file`
    Append(AwkExpr),
}

#[derive(Debug, Clone)]
enum AwkAction {
    Print(Vec<AwkExpr>, Option<AwkOutputTarget>),
    Printf(AwkExpr, Vec<AwkExpr>, Option<AwkOutputTarget>),
    Assign(String, AwkExpr),
    ArrayAssign(String, AwkExpr, AwkExpr), // arr[key] = val
    If(AwkExpr, Vec<AwkAction>, Vec<AwkAction>),
    While(AwkExpr, Vec<AwkAction>),
    DoWhile(AwkExpr, Vec<AwkAction>),
    For(Box<AwkAction>, AwkExpr, Box<AwkAction>, Vec<AwkAction>),
    ForIn(String, String, Vec<AwkAction>), // for (key in arr) { body }
    Next,
    Break,
    Continue,
    Delete(String, AwkExpr), // delete arr[key]
    Getline,                 // getline — read next input record into $0
    /// getline [var] < file — read next line from file
    GetlineFile {
        var: Option<String>,
        file: AwkExpr,
    },
    Exit(Option<AwkExpr>),
    Return(Option<AwkExpr>),
    Expression(AwkExpr),
}

struct AwkState {
    variables: HashMap<String, AwkValue>,
    fields: Vec<String>,
    fs: String,
    /// Compiled ERE for `fs` when it is used as a regex (see `FieldSep`).
    fs_regex: Option<Regex>,
    ofs: String,
    ors: String,
    nr: usize,
    nf: usize,
    fnr: usize,
    /// When true, fields are split per RFC 4180 CSV rules (--csv flag)
    csv_mode: bool,
    /// Accounted bytes held by `variables` (keys, string values, and a fixed
    /// per-entry overhead). THREAT[TM-DOS-110]: checked by the interpreter
    /// against the live-bytes limit; mutate `variables` only through
    /// `insert_var` / `remove_var` so this stays exact.
    mem_bytes: usize,
}

fn var_cost(key: &str, value: &AwkValue) -> usize {
    key.len() + value.heap_bytes() + AWK_VARIABLE_OVERHEAD_BYTES
}

#[derive(Debug, Clone, PartialEq)]
enum AwkValue {
    Number(f64),
    String(String),
    Uninitialized,
}

impl AwkValue {
    fn heap_bytes(&self) -> usize {
        match self {
            AwkValue::String(s) => s.len(),
            _ => 0,
        }
    }
}

/// Format number using AWK's OFMT (%.6g): 6 significant digits, trim trailing zeros.
fn format_awk_number(n: f64) -> String {
    if n.is_nan() {
        return "nan".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    // Integers: no decimal point
    if n.fract() == 0.0 && n.abs() < 1e16 {
        return format!("{}", n as i64);
    }
    // %.6g: use 6 significant digits
    let abs = n.abs();
    let exp = abs.log10().floor() as i32;
    if !(-4..6).contains(&exp) {
        // Scientific notation: 5 decimal places = 6 sig digits
        let mut s = format!("{:.*e}", 5, n);
        // Trim trailing zeros in mantissa
        if let Some(e_pos) = s.find('e') {
            let (mantissa, exp_part) = s.split_at(e_pos);
            let trimmed = mantissa.trim_end_matches('0').trim_end_matches('.');
            s = format!("{}{}", trimmed, exp_part);
        }
        // Normalize exponent format: e1 -> e+01 etc. to match C printf
        // Actually AWK uses e+06 style. Rust uses e6. Fix:
        if let Some(e_pos) = s.find('e') {
            let exp_str = &s[e_pos + 1..];
            let exp_val: i32 = exp_str.parse().unwrap_or(0);
            let mantissa = &s[..e_pos];
            s = format!("{}e{:+03}", mantissa, exp_val);
        }
        s
    } else {
        // Fixed notation
        let decimal_places = (5 - exp).max(0) as usize;
        let mut s = format!("{:.*}", decimal_places, n);
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        s
    }
}

impl AwkValue {
    fn as_number(&self) -> f64 {
        match self {
            AwkValue::Number(n) => *n,
            AwkValue::String(s) => s.parse().unwrap_or(0.0),
            AwkValue::Uninitialized => 0.0,
        }
    }

    fn as_string(&self) -> String {
        match self {
            AwkValue::Number(n) => format_awk_number(*n),
            AwkValue::String(s) => s.clone(),
            AwkValue::Uninitialized => String::new(),
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            AwkValue::Number(n) => *n != 0.0,
            AwkValue::String(s) => {
                if s.is_empty() {
                    return false;
                }
                // In awk, numeric strings evaluate as numbers in boolean context
                if let Ok(n) = s.parse::<f64>() {
                    n != 0.0
                } else {
                    true
                }
            }
            AwkValue::Uninitialized => false,
        }
    }
}

impl Default for AwkState {
    fn default() -> Self {
        let mut state = Self {
            variables: HashMap::new(),
            fields: Vec::new(),
            fs: " ".to_string(),
            fs_regex: None,
            ofs: " ".to_string(),
            ors: "\n".to_string(),
            nr: 0,
            nf: 0,
            fnr: 0,
            csv_mode: false,
            mem_bytes: 0,
        };
        // POSIX SUBSEP: subscript separator for multi-dimensional arrays
        state.insert_var("SUBSEP".to_string(), AwkValue::String("\x1c".to_string()));
        state
    }
}

/// Parse a CSV line per RFC 4180: handle quoted fields, embedded commas,
/// and double-quote escaping.
fn csv_split_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    // Escaped quote
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(c);
        }
    }
    fields.push(field);
    fields
}

/// How a field separator string splits text (POSIX awk "Regular
/// Expressions as Field Separators").
///
/// Decision (#2445): one classification drives record splitting (`FS`) and
/// `split()`, so both follow the same rules:
/// - `" "`: runs of blanks/newlines, leading and trailing ones ignored
/// - `""`: every character is a field (gawk/mawk extension)
/// - any other single character: that character, literally
/// - anything longer: an ERE
enum FieldSep<'a> {
    Whitespace,
    Chars,
    Literal(&'a str),
    Regex(&'a Regex),
}

impl<'a> FieldSep<'a> {
    /// Returns `true` when `sep` must be matched as an ERE.
    fn is_regex(sep: &str) -> bool {
        sep != " " && sep.chars().nth(1).is_some()
    }

    /// Classify `sep`; `regex` is its compiled ERE when `is_regex(sep)`.
    /// An ERE that failed to compile falls back to a literal match.
    fn classify(sep: &'a str, regex: Option<&'a Regex>) -> Self {
        match sep {
            " " => Self::Whitespace,
            "" => Self::Chars,
            _ => match regex {
                Some(re) => Self::Regex(re),
                None => Self::Literal(sep),
            },
        }
    }

    fn split(&self, text: &str) -> Vec<String> {
        if text.is_empty() {
            // An empty record/string has no fields in every mode.
            return Vec::new();
        }
        match self {
            Self::Whitespace => text.split_whitespace().map(String::from).collect(),
            Self::Chars => text.chars().map(String::from).collect(),
            Self::Literal(sep) => text.split(sep).map(String::from).collect(),
            Self::Regex(re) => split_regex_nonempty(re, text),
        }
    }
}

/// Split on non-empty ERE matches. An empty match separates nothing, as in
/// gawk (`FS = "x*"` does not split between every character).
fn split_regex_nonempty(re: &Regex, text: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut last = 0;
    for m in re.find_iter(text).filter(|m| !m.as_str().is_empty()) {
        fields.push(text[last..m.start()].to_string());
        last = m.end();
    }
    fields.push(text[last..].to_string());
    fields
}

impl AwkState {
    /// Set `FS`, compiling it once when it is an ERE.
    fn set_fs(&mut self, fs: String) {
        // THREAT[TM-DOS-023]: `build_regex` enforces the shared regex size limits.
        self.fs_regex = if FieldSep::is_regex(&fs) {
            crate::builtins::search_common::build_regex(&fs).ok()
        } else {
            None
        };
        self.fs = fs;
    }

    /// Delete every element of array `name`.
    fn clear_array(&mut self, name: &str) {
        let prefix = format!("{name}[");
        let mut released = 0;
        self.variables.retain(|k, v| {
            let keep = !k.starts_with(&prefix);
            if !keep {
                released += var_cost(k, v);
            }
            keep
        });
        self.mem_bytes -= released;
    }

    /// Split a line into fields based on current mode (CSV or FS)
    fn split_fields(&self, line: &str) -> Vec<String> {
        if self.csv_mode {
            csv_split_fields(line)
        } else {
            FieldSep::classify(&self.fs, self.fs_regex.as_ref()).split(line)
        }
    }

    fn set_line(&mut self, line: &str) {
        self.nr += 1;
        self.fnr += 1;

        // Split by field separator
        self.fields = self.split_fields(line);

        self.nf = self.fields.len();

        // Set built-in variables
        self.insert_var("NR".to_string(), AwkValue::Number(self.nr as f64));
        self.insert_var("NF".to_string(), AwkValue::Number(self.nf as f64));
        self.insert_var("FNR".to_string(), AwkValue::Number(self.fnr as f64));
        self.insert_var("$0".to_string(), AwkValue::String(line.to_string()));
    }

    fn insert_var(&mut self, key: String, value: AwkValue) {
        let added = var_cost(&key, &value);
        if let Some(old) = self.variables.get(&key) {
            self.mem_bytes -= var_cost(&key, old);
        }
        self.mem_bytes += added;
        self.variables.insert(key, value);
    }

    fn remove_var(&mut self, key: &str) -> Option<AwkValue> {
        let old = self.variables.remove(key)?;
        self.mem_bytes -= var_cost(key, &old);
        Some(old)
    }

    fn get_field(&self, n: usize) -> AwkValue {
        if n == 0 {
            // $0 is the whole line
            self.variables
                .get("$0")
                .cloned()
                .unwrap_or(AwkValue::Uninitialized)
        } else if n <= self.fields.len() {
            AwkValue::String(self.fields[n - 1].clone())
        } else {
            AwkValue::Uninitialized
        }
    }

    fn get_variable(&self, name: &str) -> AwkValue {
        match name {
            "NR" => AwkValue::Number(self.nr as f64),
            "NF" => AwkValue::Number(self.nf as f64),
            "FNR" => AwkValue::Number(self.fnr as f64),
            "FS" => AwkValue::String(self.fs.clone()),
            "OFS" => AwkValue::String(self.ofs.clone()),
            "ORS" => AwkValue::String(self.ors.clone()),
            _ => self
                .variables
                .get(name)
                .cloned()
                .unwrap_or(AwkValue::Uninitialized),
        }
    }

    fn set_variable(&mut self, name: &str, value: AwkValue) {
        match name {
            "FS" => self.set_fs(value.as_string()),
            "OFS" => self.ofs = value.as_string(),
            "ORS" => self.ors = value.as_string(),
            "$0" => {
                let s = value.as_string();
                // Re-split fields when $0 is modified
                self.fields = self.split_fields(&s);
                self.nf = self.fields.len();
                self.insert_var("NF".to_string(), AwkValue::Number(self.nf as f64));
                self.insert_var(name.to_string(), value);
            }
            _ => {
                self.insert_var(name.to_string(), value);
            }
        }
    }
}

// THREAT[TM-DOS-027]: parser-depth limit lives in
// `super::limits::AWK_MAX_PARSER_DEPTH` (guards against deeply nested
// expressions).

/// Preprocess awk program: replace newlines with semicolons inside action blocks.
/// This makes newlines act as statement separators per POSIX awk spec.
/// Respects string literals, regex literals, and nested braces.
fn normalize_awk_newlines(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut brace_depth = 0;

    while i < chars.len() {
        match chars[i] {
            '{' => {
                brace_depth += 1;
                result.push('{');
                i += 1;
            }
            '}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                }
                result.push('}');
                i += 1;
            }
            '"' => {
                // String literal — pass through unchanged
                result.push('"');
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        result.push(chars[i]);
                        i += 1;
                    }
                    result.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    result.push(chars[i]); // closing "
                    i += 1;
                }
            }
            '/' => {
                // Regex literal — pass through unchanged (both pattern and expression context)
                result.push('/');
                i += 1;
                while i < chars.len() && chars[i] != '/' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        result.push(chars[i]);
                        i += 1;
                    }
                    result.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    result.push(chars[i]); // closing /
                    i += 1;
                }
            }
            '#' => {
                // Comment — skip to end of line, replace with newline/semicolon
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                if i < chars.len() {
                    if brace_depth > 0 {
                        result.push(';');
                    } else {
                        result.push('\n');
                    }
                    i += 1;
                }
            }
            '\\' if i + 1 < chars.len() && chars[i + 1] == '\n' => {
                // Backslash-newline: line continuation — join lines
                i += 2;
            }
            '\n' if brace_depth > 0 => {
                // Inside action block: replace newline with semicolon
                result.push(';');
                i += 1;
            }
            _ => {
                result.push(chars[i]);
                i += 1;
            }
        }
    }
    result
}

mod interpreter;
mod parser;
use crate::fs::vfs_join;
use interpreter::{AwkFlow, AwkInterpreter};
use parser::AwkParser;

impl Awk {
    /// Process C-style escape sequences in a string (e.g., \t → tab, \n → newline)
    fn process_escape_sequences(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('t') => result.push('\t'),
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('\\') => result.push('\\'),
                    Some('a') => result.push('\x07'),
                    Some('b') => result.push('\x08'),
                    Some('f') => result.push('\x0C'),
                    Some(other) => {
                        result.push('\\');
                        result.push(other);
                    }
                    None => result.push('\\'),
                }
            } else {
                result.push(c);
            }
        }
        result
    }
}

#[async_trait]
impl Builtin for Awk {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: awk [OPTION]... 'program' [FILE]...\nPattern scanning and processing language.\n\n  -F SEP\t\tuse SEP as field separator\n  -v var=val\tassign variable before execution\n  -f progfile\tread program from file\n  --csv, -k\tCSV mode (set field separator to comma)\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("awk (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut program_str = String::new();
        let mut files: Vec<String> = Vec::new();
        let mut field_sep = " ".to_string();
        let mut pre_vars: Vec<(String, String)> = Vec::new();
        let mut csv_mode = false;
        let mut i = 0;

        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "--csv" || arg == "-k" {
                csv_mode = true;
                field_sep = ",".to_string();
            } else if arg == "-F" {
                i += 1;
                if i < ctx.args.len() {
                    field_sep = ctx.args[i].clone();
                }
            } else if let Some(sep) = arg.strip_prefix("-F") {
                field_sep = sep.to_string();
            } else if arg == "-v" {
                // Variable assignment: -v var=value
                i += 1;
                if i < ctx.args.len()
                    && let Some(eq_pos) = ctx.args[i].find('=')
                {
                    let name = ctx.args[i][..eq_pos].to_string();
                    let mut value = ctx.args[i][eq_pos + 1..].to_string();
                    // Strip surrounding quotes if present (shell may pass them)
                    if (value.starts_with('"') && value.ends_with('"'))
                        || (value.starts_with('\'') && value.ends_with('\''))
                    {
                        value = value[1..value.len() - 1].to_string();
                    }
                    pre_vars.push((name, value));
                }
            } else if arg == "-f" {
                // Read program from file
                i += 1;
                if i < ctx.args.len() {
                    let path = if ctx.args[i].starts_with('/') {
                        std::path::PathBuf::from(&ctx.args[i])
                    } else {
                        vfs_join(ctx.cwd, &ctx.args[i])
                    };
                    program_str = match read_text_file(&*ctx.fs, &path, "awk").await {
                        Ok(t) => t,
                        Err(e) => return Ok(e),
                    };
                    ctx.consume_budget_input(program_str.len())?;
                }
            } else if arg.starts_with('-') {
                // Unknown option - ignore
            } else if program_str.is_empty() {
                program_str = arg.clone();
            } else {
                files.push(arg.clone());
            }
            i += 1;
        }

        if program_str.is_empty() {
            return Err(Error::Execution("awk: no program given".to_string()));
        }

        let program_str = normalize_awk_newlines(&program_str);
        let mut parser = AwkParser::new(&program_str);
        let program = parser.parse()?;

        let mut interp = AwkInterpreter::new();
        interp.execution_budget = ctx
            .execution_budget()
            .and_then(|budget| budget.try_with(Clone::clone).ok());
        let (max_loop, max_total_loop, max_live) = ctx
            .execution_extension::<ExecutionLimits>()
            .and_then(|limits| {
                limits
                    .try_with(|l| {
                        (
                            l.max_loop_iterations,
                            l.max_total_loop_iterations,
                            l.max_live_intermediate_bytes,
                        )
                    })
                    .ok()
            })
            .unwrap_or_else(|| {
                let d = ExecutionLimits::default();
                (
                    d.max_loop_iterations,
                    d.max_total_loop_iterations,
                    d.max_live_intermediate_bytes,
                )
            });
        interp.max_loop_iterations = max_loop;
        interp.max_total_loop_iterations = max_total_loop;
        interp.max_state_bytes = usize::try_from(max_live).unwrap_or(usize::MAX);
        interp.functions = program.functions.clone();
        interp
            .state
            .set_fs(Self::process_escape_sequences(&field_sep));
        interp.fs = Some(ctx.fs.clone());
        interp.cwd = ctx.cwd.clone();
        if csv_mode {
            interp.state.csv_mode = true;
            interp.state.ofs = ",".to_string();
        }

        // Set pre-assigned variables (-v)
        for (name, value) in &pre_vars {
            let awk_val = if let Ok(n) = value.parse::<f64>() {
                AwkValue::Number(n)
            } else {
                AwkValue::String(value.clone())
            };
            interp.state.set_variable(name, awk_val);
        }

        // Run BEGIN actions
        let mut exit_code: Option<i32> = None;
        for action in &program.begin_actions {
            if let AwkFlow::Exit(code) = interp.exec_action(action) {
                exit_code = code;
                // Run END actions even after exit
                for end_action in &program.end_actions {
                    if let AwkFlow::Exit(_) = interp.exec_action(end_action) {
                        break;
                    }
                }
                Self::flush_file_outputs(&interp, &ctx).await?;
                return Ok(Self::finish(interp, exit_code));
            }
        }
        if interp.is_fatal() {
            // A limit hit mid-action in BEGIN: no input, no END.
            return Ok(Self::finish(interp, exit_code));
        }

        // Process input
        let inputs: Vec<String> = if files.is_empty() {
            vec![ctx.stdin.map(ToString::to_string).unwrap_or_default()]
        } else {
            let mut inputs = Vec::new();
            for file in &files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    vfs_join(ctx.cwd, file)
                };

                let text = match read_text_file(&*ctx.fs, &path, "awk").await {
                    Ok(t) => t,
                    Err(e) => return Ok(e),
                };
                ctx.consume_budget_input(text.len())?;
                inputs.push(text);
            }
            inputs
        };

        'files: for (file_idx, input) in inputs.iter().enumerate() {
            interp.state.fnr = 0;
            // Set FILENAME to current file path, or empty for stdin
            if !files.is_empty() {
                interp.state.insert_var(
                    "FILENAME".to_string(),
                    AwkValue::String(files[file_idx].clone()),
                );
            } else {
                interp
                    .state
                    .insert_var("FILENAME".to_string(), AwkValue::String(String::new()));
            }
            // Index-based iteration so getline can advance the index
            interp.input_lines = input.lines().map(|l| l.to_string()).collect();
            interp.line_index = 0;

            while interp.line_index < interp.input_lines.len() {
                ctx.consume_budget_work(1)?;
                let line = interp.input_lines[interp.line_index].clone();
                interp.state.set_line(&line);

                for (rule_idx, rule) in program.main_rules.iter().enumerate() {
                    // Check pattern (with range state tracking)
                    let matches = match &rule.pattern {
                        Some(pattern) => interp.matches_pattern_with_index(pattern, rule_idx),
                        None => true,
                    };

                    if matches {
                        let mut next_record = false;
                        for action in &rule.actions {
                            match interp.exec_action(action) {
                                AwkFlow::Continue => {}
                                AwkFlow::Next => {
                                    next_record = true;
                                    break;
                                }
                                AwkFlow::Exit(code) => {
                                    exit_code = code;
                                    break 'files;
                                }
                                _ => {}
                            }
                        }
                        if next_record {
                            break;
                        }
                    }
                }
                interp.line_index += 1;
            }
        }

        // Run END actions (awk runs END even after exit in main body)
        for action in &program.end_actions {
            if let AwkFlow::Exit(code) = interp.exec_action(action) {
                if exit_code.is_none() {
                    exit_code = code;
                }
                break;
            }
        }

        Self::flush_file_outputs(&interp, &ctx).await?;
        Ok(Self::finish(interp, exit_code))
    }
}

impl Awk {
    /// A fatal limit error exits 2 whatever `exit` code the program chose,
    /// even when it fired inside an expression and the action ran on.
    fn finish(interp: AwkInterpreter, exit_code: Option<i32>) -> ExecResult {
        let code = if interp.is_fatal() {
            2
        } else {
            exit_code.unwrap_or(0)
        };
        let mut result = ExecResult::with_code(interp.output, code);
        result.stderr = interp.stderr_output.into();
        result
    }

    /// AWK redirection streams through VFS as output is produced.
    async fn flush_file_outputs(_interp: &AwkInterpreter, _ctx: &Context<'_>) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
