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
    ofs: String,
    ors: String,
    nr: usize,
    nf: usize,
    fnr: usize,
    /// When true, fields are split per RFC 4180 CSV rules (--csv flag)
    csv_mode: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum AwkValue {
    Number(f64),
    String(String),
    Uninitialized,
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
        let mut variables = HashMap::new();
        // POSIX SUBSEP: subscript separator for multi-dimensional arrays
        variables.insert("SUBSEP".to_string(), AwkValue::String("\x1c".to_string()));
        Self {
            variables,
            fields: Vec::new(),
            fs: " ".to_string(),
            ofs: " ".to_string(),
            ors: "\n".to_string(),
            nr: 0,
            nf: 0,
            fnr: 0,
            csv_mode: false,
        }
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

impl AwkState {
    /// Split a line into fields based on current mode (CSV or FS)
    fn split_fields(&self, line: &str) -> Vec<String> {
        if self.csv_mode {
            csv_split_fields(line)
        } else if self.fs == " " {
            line.split_whitespace().map(String::from).collect()
        } else {
            line.split(&self.fs).map(String::from).collect()
        }
    }

    fn set_line(&mut self, line: &str) {
        self.nr += 1;
        self.fnr += 1;

        // Split by field separator
        self.fields = self.split_fields(line);

        self.nf = self.fields.len();

        // Set built-in variables
        self.variables
            .insert("NR".to_string(), AwkValue::Number(self.nr as f64));
        self.variables
            .insert("NF".to_string(), AwkValue::Number(self.nf as f64));
        self.variables
            .insert("FNR".to_string(), AwkValue::Number(self.fnr as f64));
        self.variables
            .insert("$0".to_string(), AwkValue::String(line.to_string()));
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
            "FS" => self.fs = value.as_string(),
            "OFS" => self.ofs = value.as_string(),
            "ORS" => self.ors = value.as_string(),
            "$0" => {
                let s = value.as_string();
                // Re-split fields when $0 is modified
                self.fields = self.split_fields(&s);
                self.nf = self.fields.len();
                self.variables
                    .insert("NF".to_string(), AwkValue::Number(self.nf as f64));
                self.variables.insert(name.to_string(), value);
            }
            _ => {
                self.variables.insert(name.to_string(), value);
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
                        ctx.cwd.join(&ctx.args[i])
                    };
                    program_str = match read_text_file(&*ctx.fs, &path, "awk").await {
                        Ok(t) => t,
                        Err(e) => return Ok(e),
                    };
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
        interp.max_loop_iterations = ctx
            .execution_extension::<ExecutionLimits>()
            .map(|limits| limits.max_loop_iterations)
            .unwrap_or_else(|| ExecutionLimits::default().max_loop_iterations);
        interp.functions = program.functions.clone();
        interp.state.fs = Self::process_escape_sequences(&field_sep);
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
                let mut result = ExecResult::with_code(interp.output, exit_code.unwrap_or(0));
                result.stderr = interp.stderr_output;
                return Ok(result);
            }
        }

        // Process input
        let inputs: Vec<String> = if files.is_empty() {
            vec![ctx.stdin.unwrap_or("").to_string()]
        } else {
            let mut inputs = Vec::new();
            for file in &files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                let text = match read_text_file(&*ctx.fs, &path, "awk").await {
                    Ok(t) => t,
                    Err(e) => return Ok(e),
                };
                inputs.push(text);
            }
            inputs
        };

        'files: for (file_idx, input) in inputs.iter().enumerate() {
            interp.state.fnr = 0;
            // Set FILENAME to current file path, or empty for stdin
            if !files.is_empty() {
                interp.state.variables.insert(
                    "FILENAME".to_string(),
                    AwkValue::String(files[file_idx].clone()),
                );
            } else {
                interp
                    .state
                    .variables
                    .insert("FILENAME".to_string(), AwkValue::String(String::new()));
            }
            // Index-based iteration so getline can advance the index
            interp.input_lines = input.lines().map(|l| l.to_string()).collect();
            interp.line_index = 0;

            while interp.line_index < interp.input_lines.len() {
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
        let mut result = ExecResult::with_code(interp.output, exit_code.unwrap_or(0));
        result.stderr = interp.stderr_output;
        Ok(result)
    }
}

impl Awk {
    /// AWK redirection streams through VFS as output is produced.
    async fn flush_file_outputs(_interp: &AwkInterpreter, _ctx: &Context<'_>) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
