//! awk evaluator: variables, arrays, records, expressions and statements.
//!
//! Decisions:
//! - Async all the way down so file and command I/O await the VFS and the
//!   shell directly (no helper threads). Recursion goes through boxed
//!   futures; leaf expressions are evaluated without boxing.
//! - Arrays live in an arena and are referenced by id, which gives awk's
//!   by-reference array arguments for free. A function's untyped local that
//!   becomes an array is owned by its call frame and freed on return.
//! - `for (k in a)` (and `sorted_in` `@unsorted`) visits keys in gawk's
//!   storage order, modelled in `order`; arrays themselves keep insertion
//!   order. The other gawk `sorted_in` orders are supported.
//! - Runtime errors that gawk reports (`division by zero attempted`, ...)
//!   carry gawk's `awk: cmd. line:N: fatal:` prefix. Bashkit resource caps
//!   keep the plain `awk: fatal: ...` form (TM-DOS-109).
//! - THREAT[TM-DOS-110]: every string is checked against
//!   `AWK_MAX_STRING_BYTES` before it is built; variable, element and
//!   call-frame bytes are accounted against `max_live_intermediate_bytes`
//!   (checked before each statement, so growth overshoots by at most one
//!   statement's worth).
//! - THREAT[TM-DOS-033]: loops are counted per loop and per run;
//!   user-function recursion is capped by `AWK_MAX_CALL_DEPTH`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::ast::*;
use super::format::{format_number, sprintf};
use super::io::{Io, RsMode};
use super::regex::{AwkRegex, RegexCache};
use super::value::{Value, compare};
use crate::builtins::limits::{
    AWK_MAX_CALL_DEPTH, AWK_MAX_FIELD_INDEX, AWK_MAX_STRING_BYTES, AWK_VARIABLE_OVERHEAD_BYTES,
};

pub(super) type ArrId = usize;
pub(super) type R<T> = Result<T, Unwind>;
pub(super) type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Non-local exits out of expression/statement evaluation.
#[derive(Debug)]
pub(super) enum Unwind {
    Next,
    NextFile,
    Exit,
    Fatal,
}

#[derive(Debug)]
pub(super) enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
}

#[derive(Debug, Clone)]
pub(super) enum Cell {
    Val(Value),
    Arr(ArrId),
}

#[derive(Debug, Clone)]
pub(super) enum Elem {
    Val(Value),
    Arr(ArrId),
}

/// Insertion-ordered associative array with tombstones (O(1) delete).
#[derive(Debug, Default)]
pub(super) struct AwkArray {
    map: HashMap<String, usize>,
    entries: Vec<Option<(String, Elem)>>,
}

impl AwkArray {
    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    pub(super) fn get(&self, k: &str) -> Option<&Elem> {
        let i = *self.map.get(k)?;
        self.entries[i].as_ref().map(|(_, e)| e)
    }

    pub(super) fn contains(&self, k: &str) -> bool {
        self.map.contains_key(k)
    }

    /// Insert or replace; returns the previous element.
    fn insert(&mut self, k: String, e: Elem) -> Option<Elem> {
        if let Some(&i) = self.map.get(&k) {
            let slot = self.entries[i].as_mut()?;
            return Some(std::mem::replace(&mut slot.1, e));
        }
        self.map.insert(k.clone(), self.entries.len());
        self.entries.push(Some((k, e)));
        None
    }

    fn remove(&mut self, k: &str) -> Option<Elem> {
        let i = self.map.remove(k)?;
        let old = self.entries[i].take().map(|(_, e)| e);
        if self.entries.len() > 32 && self.map.len() * 2 < self.entries.len() {
            self.compact();
        }
        old
    }

    fn compact(&mut self) {
        let entries: Vec<_> = std::mem::take(&mut self.entries)
            .into_iter()
            .flatten()
            .collect();
        self.map.clear();
        for (i, (k, _)) in entries.iter().enumerate() {
            self.map.insert(k.clone(), i);
        }
        self.entries = entries.into_iter().map(Some).collect();
    }

    pub(super) fn keys(&self) -> Vec<String> {
        self.entries
            .iter()
            .flatten()
            .map(|(k, _)| k.clone())
            .collect()
    }

    fn drain(&mut self) -> Vec<(String, Elem)> {
        self.map.clear();
        std::mem::take(&mut self.entries)
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&String, &Elem)> {
        self.entries.iter().flatten().map(|(k, e)| (k, e))
    }
}

struct Frame {
    func: u32,
    base: usize,
    owned: Vec<ArrId>,
}

/// Where an lvalue lives once its subscripts / field index are evaluated.
pub(super) enum Loc {
    Var(VarRef),
    Elem(ArrId, String),
    Field(usize),
}

/// How records are split into fields.
#[derive(Clone)]
pub(super) enum Splitter {
    /// `FS = " "`: runs of blanks and newlines; edges ignored.
    Space,
    Char(char),
    /// `FS = ""`: one field per character.
    Chars,
    Regex(Arc<AwkRegex>),
    Csv,
    /// `FIELDWIDTHS`: (skip, width); width `None` takes the rest.
    Widths(Vec<(usize, Option<usize>)>),
    Fpat(Arc<AwkRegex>),
}

pub(super) struct Limits {
    pub(super) max_loop: usize,
    pub(super) max_total_loop: usize,
    pub(super) max_mem: usize,
}

pub(super) struct Interp {
    pub(super) prog: Arc<Program>,
    lit_regex: Vec<Arc<AwkRegex>>,
    lit_regex_icase: Vec<Option<Arc<AwkRegex>>>,
    pub(super) regex_cache: RegexCache,
    pub(super) globals: Vec<Cell>,
    locals: Vec<Cell>,
    frames: Vec<Frame>,
    pub(super) arrays: Vec<Option<AwkArray>>,
    free_arrays: Vec<ArrId>,
    // Current record.
    pub(super) record: String,
    fields: Vec<Value>,
    nf: usize,
    split_done: bool,
    record_stale: bool,
    // Cached special variables.
    pub(super) convfmt: String,
    pub(super) ofmt: String,
    ofs: String,
    pub(super) ors: String,
    pub(super) subsep: String,
    pub(super) splitter: Splitter,
    /// Splitter chosen by the last of FS/FIELDWIDTHS/FPAT assignments.
    pub(super) rs: RsMode,
    pub(super) csv: bool,
    pub(super) ignorecase: bool,
    pub(super) nr: f64,
    pub(super) fnr: f64,
    range_active: Vec<bool>,
    pub(super) io: Io,
    // Accounting and limits.
    pub(super) mem_bytes: usize,
    pub(super) limits: Limits,
    total_loop: usize,
    call_depth: usize,
    pub(super) exit_code: i32,
    pub(super) fatal: bool,
    pub(super) cur_loc: Option<u32>,
    pub(super) rand: super::funcs::Rand,
    /// Set when the shared request budget ran out; the builtin returns it
    /// as an error so the dispatcher reports it.
    pub(super) budget_error: Option<crate::limits::LimitExceeded>,
}

/// Number of the current record's fields that a read can address.
impl Interp {
    pub(super) fn new(prog: Arc<Program>, io: Io, limits: Limits) -> Result<Interp, String> {
        let mut lit_regex = Vec::with_capacity(prog.regexes.len());
        for re in &prog.regexes {
            match AwkRegex::new(re, false) {
                Ok(r) => lit_regex.push(Arc::new(r)),
                Err(e) => return Err(format!("{}: /{}/", e.0, truncate(re, 200))),
            }
        }
        let n_lit = lit_regex.len();
        let mut globals = vec![Cell::Val(Value::Uninit); prog.globals.len()];
        let s = |v: &str| Cell::Val(Value::Str(v.to_string()));
        globals[special::FS as usize] = s(" ");
        globals[special::OFS as usize] = s(" ");
        globals[special::ORS as usize] = s("\n");
        globals[special::RS as usize] = s("\n");
        globals[special::SUBSEP as usize] = s("\x1c");
        globals[special::CONVFMT as usize] = s("%.6g");
        globals[special::OFMT as usize] = s("%.6g");
        globals[special::NR as usize] = Cell::Val(Value::Num(0.0));
        globals[special::NF as usize] = Cell::Val(Value::Num(0.0));
        globals[special::FNR as usize] = Cell::Val(Value::Num(0.0));
        globals[special::RSTART as usize] = Cell::Val(Value::Num(0.0));
        globals[special::RLENGTH as usize] = Cell::Val(Value::Num(-1.0));
        globals[special::IGNORECASE as usize] = Cell::Val(Value::Num(0.0));
        globals[special::FILENAME as usize] = Cell::Val(Value::Str(String::new()));
        let ranges = prog.ranges;
        let mut interp = Interp {
            prog,
            lit_regex,
            lit_regex_icase: vec![None; n_lit],
            regex_cache: RegexCache::default(),
            globals,
            locals: Vec::new(),
            frames: Vec::new(),
            arrays: Vec::new(),
            free_arrays: Vec::new(),
            record: String::new(),
            fields: Vec::new(),
            nf: 0,
            split_done: true,
            record_stale: false,
            convfmt: "%.6g".to_string(),
            ofmt: "%.6g".to_string(),
            ofs: " ".to_string(),
            ors: "\n".to_string(),
            subsep: "\x1c".to_string(),
            splitter: Splitter::Space,
            rs: RsMode::Newline,
            csv: false,
            ignorecase: false,
            nr: 0.0,
            fnr: 0.0,
            range_active: vec![false; ranges],
            io,
            mem_bytes: 0,
            limits,
            total_loop: 0,
            call_depth: 0,
            exit_code: 0,
            fatal: false,
            cur_loc: None,
            rand: super::funcs::Rand::new(),
            budget_error: None,
        };
        let mem: usize = interp
            .globals
            .iter()
            .map(|c| match c {
                Cell::Val(v) => v.heap_bytes(),
                Cell::Arr(_) => 0,
            })
            .sum();
        interp.mem_bytes = mem;
        Ok(interp)
    }

    // ----- diagnostics -----

    /// Bashkit resource cap: `awk: fatal: msg`, exit 2 (TM-DOS-109).
    pub(super) fn fatal(&mut self, msg: &str) -> Unwind {
        if !self.fatal {
            self.fatal = true;
            self.err_line(&format!("awk: fatal: {}", truncate(msg, 1000)));
        }
        Unwind::Fatal
    }

    /// gawk runtime fatal error with its source location.
    pub(super) fn fatal_at(&mut self, msg: &str) -> Unwind {
        if !self.fatal {
            self.fatal = true;
            let loc = self.location();
            self.err_line(&format!("awk: {loc}fatal: {}", truncate(msg, 1000)));
        }
        Unwind::Fatal
    }

    /// Fatal error with a message that is printed as is after `awk: `.
    pub(super) fn fatal_plain(&mut self, msg: &str) -> Unwind {
        if !self.fatal {
            self.fatal = true;
            self.err_line(&format!("awk: {}", truncate(msg, 1000)));
        }
        Unwind::Fatal
    }

    /// Stop because the shared request budget is exhausted; the builtin
    /// dispatcher reports it.
    pub(super) fn budget_exhausted(&mut self, e: crate::limits::LimitExceeded) -> Unwind {
        self.fatal = true;
        self.budget_error.get_or_insert(e);
        Unwind::Fatal
    }

    pub(super) fn location(&self) -> String {
        match self.cur_loc.and_then(|l| self.prog.locs.get(l as usize)) {
            Some((name, line)) => format!("{name}:{line}: "),
            None => String::new(),
        }
    }

    pub(super) fn err_line(&mut self, s: &str) {
        self.io.stderr.push_str(s);
        self.io.stderr.push('\n');
    }

    // ----- budget and limits -----

    fn tick(&mut self) -> R<()> {
        if let Some(b) = &self.io.host.budget
            && let Err(e) = b.consume_work(1)
        {
            return Err(self.budget_exhausted(e));
        }
        self.check_mem()
    }

    fn tick_loop(&mut self, iters: usize) -> R<()> {
        self.total_loop += 1;
        if iters > self.limits.max_loop {
            let msg = format!("loop iteration limit ({}) exceeded", self.limits.max_loop);
            return Err(self.fatal(&msg));
        }
        if self.total_loop > self.limits.max_total_loop {
            let msg = format!(
                "total loop iteration limit ({}) exceeded",
                self.limits.max_total_loop
            );
            return Err(self.fatal(&msg));
        }
        Ok(())
    }

    pub(super) fn string_fits(&mut self, len: usize) -> R<()> {
        if len > AWK_MAX_STRING_BYTES {
            return Err(self.fatal(&format!(
                "string size limit ({AWK_MAX_STRING_BYTES} bytes) exceeded"
            )));
        }
        Ok(())
    }

    // ----- regex access -----

    pub(super) fn lit_regex(&mut self, id: u32) -> Arc<AwkRegex> {
        let i = id as usize;
        if !self.ignorecase {
            return self.lit_regex[i].clone();
        }
        if let Some(r) = &self.lit_regex_icase[i] {
            return r.clone();
        }
        let r = AwkRegex::new(&self.prog.regexes[i], true)
            .map(Arc::new)
            .unwrap_or_else(|_| self.lit_regex[i].clone());
        self.lit_regex_icase[i] = Some(r.clone());
        r
    }

    pub(super) fn dyn_regex(&mut self, pattern: &str) -> R<Arc<AwkRegex>> {
        match self.regex_cache.get(pattern, self.ignorecase) {
            Ok(r) => Ok(r),
            Err(e) => Err(self.fatal_at(&format!("{e}: /{}/", truncate(pattern, 200)))),
        }
    }

    /// The regex an operand denotes: a literal, or a string's value.
    pub(super) async fn regex_operand(&mut self, e: &Expr) -> R<Arc<AwkRegex>> {
        match e {
            Expr::Regex(id) => Ok(self.lit_regex(*id)),
            Expr::Group(inner) if matches!(**inner, Expr::Regex(_)) => {
                Box::pin(self.regex_operand(inner)).await
            }
            other => {
                let v = self.eval(other).await?;
                let s = self.to_str(&v);
                self.dyn_regex(&s)
            }
        }
    }

    // ----- conversions -----

    pub(super) fn to_str(&self, v: &Value) -> String {
        v.to_str_fmt(&self.convfmt)
    }

    /// Output form of a value (`print` uses OFMT for numbers).
    fn to_output(&self, v: &Value) -> String {
        match v {
            Value::Num(n) => format_number(*n, &self.ofmt),
            other => other.to_str_fmt(&self.convfmt),
        }
    }

    // ----- cells -----

    pub(super) fn cell(&self, v: VarRef) -> &Cell {
        match v {
            VarRef::Global(i) => &self.globals[i as usize],
            VarRef::Local(i) => {
                let base = self.frames.last().map_or(0, |f| f.base);
                &self.locals[base + i as usize]
            }
        }
    }

    fn cell_mut(&mut self, v: VarRef) -> &mut Cell {
        match v {
            VarRef::Global(i) => &mut self.globals[i as usize],
            VarRef::Local(i) => {
                let base = self.frames.last().map_or(0, |f| f.base);
                &mut self.locals[base + i as usize]
            }
        }
    }

    pub(super) fn var_name(&self, v: VarRef) -> String {
        match v {
            VarRef::Global(i) => self.prog.globals[i as usize].clone(),
            VarRef::Local(i) => self
                .frames
                .last()
                .and_then(|f| self.prog.funcs.get(f.func as usize))
                .and_then(|f| f.params.get(i as usize))
                .cloned()
                .unwrap_or_default(),
        }
    }

    pub(super) fn get_var(&mut self, v: VarRef) -> R<Value> {
        if v == VarRef::Global(special::NF) {
            self.ensure_split()?;
            return Ok(Value::Num(self.nf as f64));
        }
        match self.cell(v) {
            Cell::Val(x) => Ok(x.clone()),
            Cell::Arr(_) => {
                let name = self.var_name(v);
                Err(self.fatal_at(&format!(
                    "attempt to use array `{name}' in a scalar context"
                )))
            }
        }
    }

    pub(super) fn set_var(&mut self, v: VarRef, val: Value) -> R<()> {
        let new_bytes = val.heap_bytes();
        let old_bytes = match self.cell(v) {
            Cell::Val(x) => x.heap_bytes(),
            Cell::Arr(_) => {
                let name = self.var_name(v);
                return Err(self.fatal_at(&format!(
                    "attempt to use array `{name}' in a scalar context"
                )));
            }
        };
        self.mem_bytes = self.mem_bytes + new_bytes - old_bytes.min(self.mem_bytes + new_bytes);
        if let VarRef::Global(i) = v
            && i < special::COUNT
        {
            return self.set_special(i, val);
        }
        *self.cell_mut(v) = Cell::Val(val);
        Ok(())
    }

    fn set_special(&mut self, i: u32, val: Value) -> R<()> {
        match i {
            special::NF => {
                let n = val.to_num();
                self.globals[i as usize] = Cell::Val(val);
                return self.set_nf(n);
            }
            special::NR => self.nr = val.to_num(),
            special::FNR => self.fnr = val.to_num(),
            special::FS => {
                let s = self.to_str(&val);
                self.globals[i as usize] = Cell::Val(val);
                return self.set_fs(&s);
            }
            special::RS => {
                let s = self.to_str(&val);
                self.globals[i as usize] = Cell::Val(val);
                return self.set_rs(&s);
            }
            special::OFS => self.ofs = self.to_str(&val),
            special::ORS => self.ors = self.to_str(&val),
            special::SUBSEP => self.subsep = self.to_str(&val),
            special::CONVFMT => self.convfmt = val.to_str_fmt("%.6g"),
            special::OFMT => self.ofmt = self.to_str(&val),
            special::IGNORECASE => {
                let on = val.to_bool();
                self.globals[i as usize] = Cell::Val(val);
                if on != self.ignorecase {
                    self.ignorecase = on;
                    let fs = self.special_str(special::FS);
                    self.set_fs(&fs)?;
                    let rs = self.special_str(special::RS);
                    self.set_rs(&rs)?;
                }
                return Ok(());
            }
            special::FIELDWIDTHS => {
                let s = self.to_str(&val);
                self.globals[i as usize] = Cell::Val(val);
                self.splitter = Splitter::Widths(parse_widths(&s));
                self.split_done = false;
                return Ok(());
            }
            special::FPAT => {
                let s = self.to_str(&val);
                self.globals[i as usize] = Cell::Val(val);
                let re = self.dyn_regex(&s)?;
                self.splitter = Splitter::Fpat(re);
                self.split_done = false;
                return Ok(());
            }
            _ => {}
        }
        self.globals[i as usize] = Cell::Val(val);
        Ok(())
    }

    fn special_str(&self, i: u32) -> String {
        match &self.globals[i as usize] {
            Cell::Val(v) => self.to_str(v),
            Cell::Arr(_) => String::new(),
        }
    }

    pub(super) fn set_fs(&mut self, fs: &str) -> R<()> {
        self.splitter = if self.csv && fs == "," {
            Splitter::Csv
        } else {
            self.classify_sep(fs)?
        };
        Ok(())
    }

    /// Splitter for a separator string (FS rules, shared with `split()`).
    pub(super) fn classify_sep(&mut self, sep: &str) -> R<Splitter> {
        let mut chars = sep.chars();
        Ok(match (chars.next(), chars.next()) {
            (Some(' '), None) => Splitter::Space,
            (None, _) => Splitter::Chars,
            (Some(c), None) if c != '\\' && !(self.ignorecase && c.is_alphabetic()) => {
                Splitter::Char(c)
            }
            _ => Splitter::Regex(self.dyn_regex(sep)?),
        })
    }

    fn set_rs(&mut self, rs: &str) -> R<()> {
        let mut chars = rs.chars();
        self.rs = match (chars.next(), chars.next()) {
            (None, _) => RsMode::Paragraph,
            (Some('\n'), None) => RsMode::Newline,
            (Some(c), None) => RsMode::Char(c),
            _ => RsMode::Regex(self.dyn_regex(rs)?),
        };
        Ok(())
    }

    // ----- arrays -----

    fn new_array(&mut self) -> ArrId {
        if let Some(id) = self.free_arrays.pop() {
            self.arrays[id] = Some(AwkArray::default());
            return id;
        }
        self.arrays.push(Some(AwkArray::default()));
        self.arrays.len() - 1
    }

    /// Free an array and its subarrays, releasing their accounted bytes.
    fn free_array(&mut self, id: ArrId) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let Some(mut arr) = self.arrays.get_mut(id).and_then(Option::take) else {
                continue;
            };
            for (k, e) in arr.drain() {
                self.mem_bytes = self.mem_bytes.saturating_sub(elem_cost(&k, &e));
                if let Elem::Arr(sub) = e {
                    stack.push(sub);
                }
            }
            self.free_arrays.push(id);
        }
    }

    /// Remove every element of an array (the array itself stays).
    pub(super) fn clear_array(&mut self, id: ArrId) {
        let Some(arr) = self.arrays.get_mut(id).and_then(Option::as_mut) else {
            return;
        };
        let drained = arr.drain();
        for (k, e) in drained {
            self.mem_bytes = self.mem_bytes.saturating_sub(elem_cost(&k, &e));
            if let Elem::Arr(sub) = e {
                self.free_array(sub);
            }
        }
    }

    pub(super) fn array(&self, id: ArrId) -> Option<&AwkArray> {
        self.arrays.get(id).and_then(Option::as_ref)
    }

    /// Array behind a variable, creating it if the variable is untyped.
    pub(super) fn array_of_var(&mut self, v: VarRef) -> R<ArrId> {
        match self.cell(v) {
            Cell::Arr(id) => Ok(*id),
            Cell::Val(Value::Uninit) if !matches!(v, VarRef::Global(i) if i < special::COUNT && !matches!(i, special::ENVIRON | special::ARGV | special::PROCINFO | special::SYMTAB)) =>
            {
                let id = self.new_array();
                *self.cell_mut(v) = Cell::Arr(id);
                if let VarRef::Local(_) = v
                    && let Some(f) = self.frames.last_mut()
                {
                    f.owned.push(id);
                }
                Ok(id)
            }
            _ => {
                let name = self.var_name(v);
                Err(self.fatal_at(&format!("attempt to use scalar `{name}' as an array")))
            }
        }
    }

    /// Array denoted by `name` or `name[i]...` (a subarray), creating
    /// untyped parts.
    pub(super) async fn array_of_expr(&mut self, e: &Expr) -> R<ArrId> {
        match e {
            Expr::Var(v) => self.array_of_var(*v),
            Expr::Elem(v, groups) => {
                let mut id = self.array_of_var(*v)?;
                for g in groups {
                    let key = self.subscript(g).await?;
                    id = self.subarray(id, key)?;
                }
                Ok(id)
            }
            Expr::Group(inner) => Box::pin(self.array_of_expr(inner)).await,
            _ => Err(self.fatal_at("attempt to use a scalar value as array")),
        }
    }

    /// The subarray at `arr[key]`, creating it when missing or untyped.
    fn subarray(&mut self, arr: ArrId, key: String) -> R<ArrId> {
        match self.array(arr).and_then(|a| a.get(&key)) {
            Some(Elem::Arr(sub)) => return Ok(*sub),
            Some(Elem::Val(Value::Uninit)) | None => {}
            Some(Elem::Val(_)) => {
                return Err(self.fatal_at(&format!(
                    "attempt to use scalar `[\"{}\"]' as array",
                    truncate(&key, 100)
                )));
            }
        }
        let sub = self.new_array();
        self.elem_store(arr, key, Elem::Arr(sub));
        Ok(sub)
    }

    /// Join a subscript group with SUBSEP.
    pub(super) async fn subscript(&mut self, g: &[Expr]) -> R<String> {
        if g.len() == 1 {
            let v = self.eval(&g[0]).await?;
            return Ok(self.to_str(&v));
        }
        let mut key = String::new();
        for (i, e) in g.iter().enumerate() {
            if i > 0 {
                key.push_str(&self.subsep.clone());
            }
            let v = self.eval(e).await?;
            let s = self.to_str(&v);
            self.string_fits(key.len() + s.len())?;
            key.push_str(&s);
        }
        Ok(key)
    }

    pub(super) fn elem_store(&mut self, arr: ArrId, key: String, e: Elem) {
        let add = elem_cost(&key, &e);
        let Some(a) = self.arrays.get_mut(arr).and_then(Option::as_mut) else {
            return;
        };
        let k = key.clone();
        let old = a.insert(key, e);
        self.mem_bytes += add;
        if let Some(old) = old {
            self.mem_bytes = self.mem_bytes.saturating_sub(elem_cost(&k, &old));
            if let Elem::Arr(sub) = old {
                self.free_array(sub);
            }
        }
    }

    pub(super) fn elem_delete(&mut self, arr: ArrId, key: &str) {
        let Some(a) = self.arrays.get_mut(arr).and_then(Option::as_mut) else {
            return;
        };
        if let Some(old) = a.remove(key) {
            self.mem_bytes = self.mem_bytes.saturating_sub(elem_cost(key, &old));
            if let Elem::Arr(sub) = old {
                self.free_array(sub);
            }
        }
    }

    /// Value of `arr[key]`; a reference creates the element (POSIX).
    fn elem_load(&mut self, arr: ArrId, key: String) -> R<Value> {
        match self.array(arr).and_then(|a| a.get(&key)) {
            Some(Elem::Val(v)) => Ok(v.clone()),
            Some(Elem::Arr(_)) => Err(self.fatal_at(&format!(
                "attempt to use array `[\"{}\"]' in a scalar context",
                truncate(&key, 100)
            ))),
            None => {
                self.elem_store(arr, key, Elem::Val(Value::Uninit));
                Ok(Value::Uninit)
            }
        }
    }

    /// Set every element of `arr` from `vals` (1-based), replacing it.
    pub(super) fn fill_array(&mut self, arr: ArrId, vals: Vec<Value>) -> R<()> {
        self.fill_array_from(arr, 1, vals)
    }

    /// Like `fill_array`, with the first index `first`.
    pub(super) fn fill_array_from(&mut self, arr: ArrId, first: usize, vals: Vec<Value>) -> R<()> {
        self.clear_array(arr);
        for (i, v) in vals.into_iter().enumerate() {
            self.elem_store(arr, (i + first).to_string(), Elem::Val(v));
        }
        self.check_mem()
    }

    fn check_mem(&mut self) -> R<()> {
        if self.mem_bytes > self.limits.max_mem {
            let msg = format!("memory limit ({} bytes) exceeded", self.limits.max_mem);
            return Err(self.fatal(&msg));
        }
        Ok(())
    }

    // ----- records and fields -----

    pub(super) fn set_record(&mut self, rec: String) {
        self.mem_bytes = self.mem_bytes.saturating_sub(self.record.len()) + rec.len();
        self.record = rec;
        self.split_done = false;
        self.record_stale = false;
    }

    pub(super) fn ensure_split(&mut self) -> R<()> {
        if self.split_done {
            return Ok(());
        }
        let parts = self.split_record()?;
        let field_bytes: usize = self.fields.iter().map(Value::heap_bytes).sum();
        self.mem_bytes = self.mem_bytes.saturating_sub(field_bytes);
        let mut bytes = 0;
        self.fields = parts
            .into_iter()
            .map(|p| {
                bytes += p.len();
                Value::from_input(p)
            })
            .collect();
        self.mem_bytes += bytes;
        self.nf = self.fields.len();
        self.globals[special::NF as usize] = Cell::Val(Value::Num(self.nf as f64));
        self.split_done = true;
        Ok(())
    }

    /// Most array elements or fields the memory cap leaves room for.
    pub(super) fn split_budget(&self) -> usize {
        self.limits.max_mem.saturating_sub(self.mem_bytes) / AWK_VARIABLE_OVERHEAD_BYTES + 1
    }

    pub(super) fn too_many(&mut self, _: TooMany) -> Unwind {
        let msg = format!("memory limit ({} bytes) exceeded", self.limits.max_mem);
        self.fatal(&msg)
    }

    fn split_record(&mut self) -> R<Vec<String>> {
        let max = self.split_budget();
        let rec = std::mem::take(&mut self.record);
        let splitter = self.splitter.clone();
        let out = if matches!(self.rs, RsMode::Paragraph) && !matches!(splitter, Splitter::Space) {
            // POSIX: in paragraph mode newline always separates fields.
            let mut v = Vec::new();
            for line in rec.split('\n') {
                match split_text(&splitter, line, None, max.saturating_sub(v.len())) {
                    Ok(p) => v.extend(p),
                    Err(e) => {
                        self.record = rec;
                        return Err(self.too_many(e));
                    }
                }
            }
            Ok(v)
        } else {
            split_text(&splitter, &rec, None, max)
        };
        self.record = rec;
        out.map_err(|e| self.too_many(e))
    }

    fn rebuild_record(&mut self) -> R<()> {
        if !self.record_stale {
            return Ok(());
        }
        let mut total = 0usize;
        let strs: Vec<String> = self
            .fields
            .iter()
            .take(self.nf)
            .map(|v| v.to_str_fmt(&self.convfmt))
            .collect();
        for s in &strs {
            total += s.len() + self.ofs.len();
        }
        self.string_fits(total)?;
        let rec = strs.join(&self.ofs);
        self.mem_bytes = self.mem_bytes.saturating_sub(self.record.len()) + rec.len();
        self.record = rec;
        self.record_stale = false;
        Ok(())
    }

    pub(super) fn get_field(&mut self, i: usize) -> R<Value> {
        if i == 0 {
            self.rebuild_record()?;
            return Ok(Value::from_input(self.record.clone()));
        }
        self.ensure_split()?;
        Ok(self.fields.get(i - 1).cloned().unwrap_or(Value::Uninit))
    }

    pub(super) fn set_field(&mut self, i: usize, v: Value) -> R<()> {
        if i == 0 {
            let s = self.to_str(&v);
            self.set_record(s);
            return Ok(());
        }
        if i > AWK_MAX_FIELD_INDEX {
            return Err(self.fatal(&format!(
                "field index limit ({AWK_MAX_FIELD_INDEX}) exceeded"
            )));
        }
        self.ensure_split()?;
        // The rebuilt record must fit before any field is created.
        let est = self.record.len() + v.heap_bytes() + i.saturating_sub(self.nf) * self.ofs.len();
        self.string_fits(est)?;
        if i > self.fields.len() {
            self.fields.resize(i, Value::Str(String::new()));
        }
        if i > self.nf {
            for f in self.fields.iter_mut().take(i).skip(self.nf) {
                *f = Value::Str(String::new());
            }
            self.nf = i;
            self.globals[special::NF as usize] = Cell::Val(Value::Num(i as f64));
        }
        self.mem_bytes =
            self.mem_bytes + v.heap_bytes() - self.fields[i - 1].heap_bytes().min(self.mem_bytes);
        self.fields[i - 1] = v;
        self.record_stale = true;
        Ok(())
    }

    fn set_nf(&mut self, n: f64) -> R<()> {
        if n < 0.0 {
            return Err(self.fatal_at("NF set to negative value"));
        }
        let n = n as usize;
        if n > AWK_MAX_FIELD_INDEX {
            return Err(self.fatal(&format!(
                "field index limit ({AWK_MAX_FIELD_INDEX}) exceeded"
            )));
        }
        self.ensure_split()?;
        self.string_fits(self.record.len() + n.saturating_sub(self.nf) * self.ofs.len())?;
        if n > self.fields.len() {
            self.fields.resize(n, Value::Str(String::new()));
        }
        for f in self.fields.iter_mut().take(n).skip(self.nf) {
            *f = Value::Str(String::new());
        }
        self.nf = n;
        self.fields.truncate(n);
        self.globals[special::NF as usize] = Cell::Val(Value::Num(n as f64));
        self.record_stale = true;
        Ok(())
    }

    // ----- lvalues -----

    pub(super) async fn resolve(&mut self, lv: &Expr) -> R<Loc> {
        match lv {
            Expr::Var(v) => Ok(Loc::Var(*v)),
            Expr::Elem(v, groups) => {
                let mut id = self.array_of_var(*v)?;
                let (last, path) = groups.split_last().ok_or(Unwind::Fatal)?;
                for g in path {
                    let k = self.subscript(g).await?;
                    id = self.subarray(id, k)?;
                }
                let key = self.subscript(last).await?;
                Ok(Loc::Elem(id, key))
            }
            Expr::Field(e) => {
                let i = self.eval(e).await?.to_num();
                Ok(Loc::Field(self.field_index(i)?))
            }
            Expr::Group(inner) => Box::pin(self.resolve(inner)).await,
            _ => Err(self.fatal_at("assignment to a non-lvalue")),
        }
    }

    pub(super) fn field_index(&mut self, i: f64) -> R<usize> {
        if i < 0.0 || i.is_nan() {
            return Err(self.fatal_at(&format!(
                "attempt to access field {}",
                format_number(i.trunc(), "%.6g")
            )));
        }
        if i > AWK_MAX_FIELD_INDEX as f64 {
            return Err(self.fatal(&format!(
                "field index limit ({AWK_MAX_FIELD_INDEX}) exceeded"
            )));
        }
        Ok(i as usize)
    }

    pub(super) fn load(&mut self, loc: &Loc) -> R<Value> {
        match loc {
            Loc::Var(v) => self.get_var(*v),
            Loc::Elem(id, key) => self.elem_load(*id, key.clone()),
            Loc::Field(i) => self.get_field(*i),
        }
    }

    pub(super) fn store(&mut self, loc: Loc, v: Value) -> R<()> {
        self.string_fits(v.heap_bytes())?;
        match loc {
            Loc::Var(var) => self.set_var(var, v),
            Loc::Elem(id, key) => {
                if let Some(Elem::Arr(_)) = self.array(id).and_then(|a| a.get(&key)) {
                    return Err(self.fatal_at(&format!(
                        "attempt to use array `[\"{}\"]' in a scalar context",
                        truncate(&key, 100)
                    )));
                }
                self.elem_store(id, key, Elem::Val(v));
                Ok(())
            }
            Loc::Field(i) => self.set_field(i, v),
        }
    }

    pub(super) async fn assign(&mut self, lv: &Expr, v: Value) -> R<()> {
        let loc = self.resolve(lv).await?;
        self.store(loc, v)
    }

    // ----- expressions -----

    /// Evaluate; leaves are handled inline, the rest through a boxed future.
    pub(super) async fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Var(v) => self.get_var(*v),
            _ => self.eval_boxed(e).await,
        }
    }

    /// Each arm is its own boxed future, so a recursive call keeps only
    /// that arm's (small) poll frame on the stack, not one frame sized for
    /// every arm. This bounds stack use per awk call level.
    fn eval_boxed<'a>(&'a mut self, e: &'a Expr) -> BoxFut<'a, R<Value>> {
        match e {
            Expr::Num(n) => Box::pin(async move { Ok(Value::Num(*n)) }),
            Expr::Str(s) => Box::pin(async move { Ok(Value::Str(s.clone())) }),
            Expr::Var(v) => Box::pin(async move { self.get_var(*v) }),
            Expr::Regex(id) => Box::pin(async move {
                let re = self.lit_regex(*id);
                self.rebuild_record()?;
                Ok(bool_val(re.is_match(&self.record)))
            }),
            Expr::Elem(..) => Box::pin(async move {
                let loc = self.resolve(e).await?;
                self.load(&loc)
            }),
            Expr::Field(i) => Box::pin(async move {
                let n = self.eval(i).await?.to_num();
                let idx = self.field_index(n)?;
                self.get_field(idx)
            }),
            Expr::Group(inner) => Box::pin(async move { self.eval(inner).await }),
            Expr::Assign(lv, op, rhs) => Box::pin(async move {
                let loc = self.resolve(lv).await?;
                let r = self.eval(rhs).await?;
                let v = match op {
                    None => r,
                    Some(op) => {
                        let cur = self.load(&loc)?.to_num();
                        Value::Num(self.arith(*op, cur, r.to_num())?)
                    }
                };
                self.store(loc, v.clone())?;
                Ok(v)
            }),
            Expr::Neg(x) => Box::pin(async move { Ok(Value::Num(-self.eval(x).await?.to_num())) }),
            Expr::Plus(x) => Box::pin(async move { Ok(Value::Num(self.eval(x).await?.to_num())) }),
            Expr::Not(x) => Box::pin(async move { Ok(bool_val(!self.eval(x).await?.to_bool())) }),
            Expr::Binary(op, a, b) => Box::pin(async move {
                let x = self.eval(a).await?.to_num();
                let y = self.eval(b).await?.to_num();
                Ok(Value::Num(self.arith(*op, x, y)?))
            }),
            Expr::Cmp(op, a, b) => Box::pin(async move {
                let x = self.eval(a).await?;
                let y = self.eval(b).await?;
                let ord = if self.ignorecase && !(x.is_numeric() && y.is_numeric()) {
                    let (xs, ys) = (
                        self.to_str(&x).to_lowercase(),
                        self.to_str(&y).to_lowercase(),
                    );
                    xs.as_bytes().cmp(ys.as_bytes())
                } else {
                    compare(&x, &y, &self.convfmt)
                };
                use std::cmp::Ordering::*;
                let nan = (x.is_numeric() && y.is_numeric())
                    && (x.to_num().is_nan() || y.to_num().is_nan());
                let r = match op {
                    CmpOp::Lt => ord == Less,
                    CmpOp::Le => ord != Greater,
                    CmpOp::Gt => ord == Greater,
                    CmpOp::Ge => ord != Less,
                    CmpOp::Eq => ord == Equal,
                    CmpOp::Ne => ord != Equal,
                };
                Ok(bool_val(if nan { matches!(op, CmpOp::Ne) } else { r }))
            }),
            Expr::Match(neg, a, b) => Box::pin(async move {
                let s = self.eval(a).await?;
                let s = self.to_str(&s);
                let re = self.regex_operand(b).await?;
                Ok(bool_val(re.is_match(&s) != *neg))
            }),
            Expr::And(a, b) => {
                Box::pin(async move { Ok(bool_val(self.cond(a).await? && self.cond(b).await?)) })
            }
            Expr::Or(a, b) => {
                Box::pin(async move { Ok(bool_val(self.cond(a).await? || self.cond(b).await?)) })
            }
            Expr::Cond(c, a, b) => Box::pin(async move {
                if self.cond(c).await? {
                    self.eval(a).await
                } else {
                    self.eval(b).await
                }
            }),
            Expr::Concat(a, b) => Box::pin(async move {
                let x = self.eval(a).await?;
                let mut s = self.to_str(&x);
                let y = self.eval(b).await?;
                let t = self.to_str(&y);
                self.string_fits(s.len() + t.len())?;
                s.push_str(&t);
                Ok(Value::Str(s))
            }),
            Expr::In(subs, arr) => Box::pin(async move {
                let key = self.subscript(subs).await?;
                let id = self.array_of_expr(arr).await?;
                Ok(bool_val(self.array(id).is_some_and(|a| a.contains(&key))))
            }),
            Expr::IncDec(lv, delta, prefix) => Box::pin(async move {
                let loc = self.resolve(lv).await?;
                let old = self.load(&loc)?.to_num();
                let new = old + delta;
                self.store(loc, Value::Num(new))?;
                Ok(Value::Num(if *prefix { new } else { old }))
            }),
            Expr::Call(f, args) => {
                Box::pin(async move { Box::pin(self.call_user(*f, args)).await })
            }
            Expr::Indirect(name, args) => Box::pin(async move {
                let n = self.eval(name).await?;
                let n = self.to_str(&n);
                let prog = self.prog.clone();
                if let Some(i) = prog.funcs.iter().position(|f| f.name == n && f.defined) {
                    return Box::pin(self.call_user(i as u32, args)).await;
                }
                if let Some(bi) = Bi::from_name(&n) {
                    return Box::pin(self.call_builtin(bi, args)).await;
                }
                Err(self.fatal_at(&format!("function `{}' not defined", truncate(&n, 100))))
            }),
            Expr::Builtin(bi, args) => {
                Box::pin(async move { Box::pin(self.call_builtin(*bi, args)).await })
            }
            Expr::Getline(src, var) => {
                Box::pin(async move { Box::pin(self.getline(src, var.as_deref())).await })
            }
            Expr::List(items) => Box::pin(async move {
                let key = self.subscript(items).await?;
                Ok(Value::Str(key))
            }),
        }
    }

    pub(super) async fn cond(&mut self, e: &Expr) -> R<bool> {
        Ok(self.eval(e).await?.to_bool())
    }

    fn arith(&mut self, op: BinOp, x: f64, y: f64) -> R<f64> {
        Ok(match op {
            BinOp::Add => x + y,
            BinOp::Sub => x - y,
            BinOp::Mul => x * y,
            BinOp::Div => {
                if y == 0.0 {
                    return Err(self.fatal_at("division by zero attempted"));
                }
                x / y
            }
            BinOp::Mod => {
                if y == 0.0 {
                    return Err(self.fatal_at("division by zero attempted in `%'"));
                }
                x % y
            }
            BinOp::Pow => fix_nan(x.powf(y), &[x, y]),
        })
    }

    // ----- user functions -----

    async fn call_user(&mut self, fid: u32, args: &[Expr]) -> R<Value> {
        let prog = self.prog.clone();
        let f = &prog.funcs[fid as usize];
        if args.len() > f.params.len() {
            return Err(self.fatal_at(&format!(
                "function `{}' called with more arguments than declared",
                f.name
            )));
        }
        if self.call_depth >= AWK_MAX_CALL_DEPTH {
            return Err(self.fatal(&format!(
                "function call depth limit ({AWK_MAX_CALL_DEPTH}) exceeded"
            )));
        }
        let mut cells = Vec::with_capacity(f.params.len());
        for (j, a) in args.iter().enumerate() {
            let wants_array = f.array_params.get(j).copied().unwrap_or(false);
            let cell = match a {
                Expr::Var(v) if *v != VarRef::Global(special::NF) => match self.cell(*v) {
                    Cell::Arr(id) => Cell::Arr(*id),
                    Cell::Val(Value::Uninit) if wants_array => Cell::Arr(self.array_of_var(*v)?),
                    Cell::Val(x) => Cell::Val(x.clone()),
                },
                Expr::Elem(v, groups) => {
                    let mut id = self.array_of_var(*v)?;
                    let (last, path) = groups.split_last().ok_or(Unwind::Fatal)?;
                    for g in path {
                        let k = self.subscript(g).await?;
                        id = self.subarray(id, k)?;
                    }
                    let key = self.subscript(last).await?;
                    match self.array(id).and_then(|a| a.get(&key)) {
                        Some(Elem::Arr(sub)) => Cell::Arr(*sub),
                        _ if wants_array => Cell::Arr(self.subarray(id, key)?),
                        _ => Cell::Val(self.elem_load(id, key)?),
                    }
                }
                other => Cell::Val(self.eval(other).await?),
            };
            cells.push(cell);
        }
        cells.resize(f.params.len(), Cell::Val(Value::Uninit));
        let bytes: usize = cells
            .iter()
            .map(|c| match c {
                Cell::Val(v) => v.heap_bytes(),
                Cell::Arr(_) => 0,
            })
            .sum();
        self.mem_bytes += bytes;
        let base = self.locals.len();
        self.locals.extend(cells);
        self.frames.push(Frame {
            func: fid,
            base,
            owned: Vec::new(),
        });
        self.call_depth += 1;
        let saved_loc = self.cur_loc;
        let r = self.exec_block(&f.body).await;
        self.cur_loc = saved_loc;
        self.call_depth -= 1;
        if let Some(frame) = self.frames.pop() {
            for c in self.locals.drain(frame.base..) {
                if let Cell::Val(v) = c {
                    self.mem_bytes = self.mem_bytes.saturating_sub(v.heap_bytes());
                }
            }
            for id in frame.owned {
                self.free_array(id);
            }
        }
        match r? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Uninit),
        }
    }

    // ----- statements -----

    pub(super) async fn exec_block(&mut self, b: &[Stmt]) -> R<Flow> {
        for s in b {
            match self.exec_stmt(s).await? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    /// One boxed future per statement kind (see `eval_boxed`).
    fn exec_stmt<'a>(&'a mut self, s: &'a Stmt) -> BoxFut<'a, R<Flow>> {
        self.cur_loc = Some(s.loc);
        if let Err(e) = self.tick() {
            return Box::pin(std::future::ready(Err(e)));
        }
        match &s.kind {
            StmtKind::Expr(e) => Box::pin(async move {
                self.eval(e).await?;
                Ok(Flow::Normal)
            }),
            StmtKind::Print(args, dest) => Box::pin(async move {
                let mut line = String::new();
                if args.is_empty() {
                    self.rebuild_record()?;
                    line.push_str(&self.record);
                } else {
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            line.push_str(&self.ofs);
                        }
                        let v = self.eval(a).await?;
                        let t = self.to_output(&v);
                        self.string_fits(line.len() + t.len())?;
                        line.push_str(&t);
                    }
                }
                line.push_str(&self.ors);
                Box::pin(self.write_out(dest.as_ref(), line)).await?;
                Ok(Flow::Normal)
            }),
            StmtKind::Printf(args, dest) => Box::pin(async move {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(a).await?);
                }
                let fmt = self.to_str(&vals[0]);
                let out = match sprintf(&fmt, &vals[1..], &self.convfmt) {
                    Ok(s) => s,
                    Err(e) if e.located => return Err(self.fatal_at(&e.msg)),
                    Err(e) => return Err(self.fatal(&e.msg)),
                };
                self.string_fits(out.len())?;
                Box::pin(self.write_out(dest.as_ref(), out)).await?;
                Ok(Flow::Normal)
            }),
            StmtKind::If(c, a, b) => Box::pin(async move {
                if self.cond(c).await? {
                    return self.exec_block(a).await;
                } else if let Some(b) = b {
                    return self.exec_block(b).await;
                }
                Ok(Flow::Normal)
            }),
            StmtKind::While(c, body) => Box::pin(async move {
                let mut n = 0;
                while self.cond(c).await? {
                    n += 1;
                    self.tick_loop(n)?;
                    match self.exec_block(body).await? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                }
                Ok(Flow::Normal)
            }),
            StmtKind::Do(body, c) => Box::pin(async move {
                let mut n = 0;
                loop {
                    n += 1;
                    self.tick_loop(n)?;
                    match self.exec_block(body).await? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    if !self.cond(c).await? {
                        break;
                    }
                }
                Ok(Flow::Normal)
            }),
            StmtKind::For(init, c, incr, body) => Box::pin(async move {
                if let Some(i) = init {
                    self.exec_stmt(i).await?;
                }
                let mut n = 0;
                loop {
                    if let Some(c) = c
                        && !self.cond(c).await?
                    {
                        break;
                    }
                    n += 1;
                    self.tick_loop(n)?;
                    match self.exec_block(body).await? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    if let Some(i) = incr {
                        self.exec_stmt(i).await?;
                    }
                }
                Ok(Flow::Normal)
            }),
            StmtKind::ForIn(key, arr, body) => Box::pin(async move {
                let id = self.array_of_expr(arr).await?;
                let keys = self.for_in_keys(id);
                let mut n = 0;
                for k in keys {
                    if !self.array(id).is_some_and(|a| a.contains(&k)) {
                        continue;
                    }
                    n += 1;
                    self.tick_loop(n)?;
                    self.assign(key, Value::from_input(k)).await?;
                    match self.exec_block(body).await? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                }
                Ok(Flow::Normal)
            }),
            StmtKind::Block(b) => Box::pin(async move { self.exec_block(b).await }),
            StmtKind::Next => Box::pin(async move { Err(Unwind::Next) }),
            StmtKind::NextFile => Box::pin(async move { Err(Unwind::NextFile) }),
            StmtKind::Exit(e) => Box::pin(async move {
                if let Some(e) = e {
                    let v = self.eval(e).await?.to_num();
                    self.exit_code = if v.is_finite() {
                        (v as i64 & 0xff) as i32
                    } else {
                        0
                    };
                }
                Err(Unwind::Exit)
            }),
            StmtKind::Return(e) => Box::pin(async move {
                let v = match e {
                    Some(e) => self.eval(e).await?,
                    None => Value::Uninit,
                };
                Ok(Flow::Return(v))
            }),
            StmtKind::Break => Box::pin(async move { Ok(Flow::Break) }),
            StmtKind::Continue => Box::pin(async move { Ok(Flow::Continue) }),
            StmtKind::Delete(target) => Box::pin(async move {
                match target {
                    Expr::Var(v) => {
                        let id = self.array_of_var(*v)?;
                        self.clear_array(id);
                    }
                    Expr::Elem(v, groups) => {
                        let mut id = self.array_of_var(*v)?;
                        let (last, path) = groups.split_last().ok_or(Unwind::Fatal)?;
                        for g in path {
                            let k = self.subscript(g).await?;
                            match self.array(id).and_then(|a| a.get(&k)) {
                                Some(Elem::Arr(sub)) => id = *sub,
                                _ => return Ok(Flow::Normal),
                            }
                        }
                        let key = self.subscript(last).await?;
                        self.elem_delete(id, &key);
                    }
                    _ => {}
                };
                Ok(Flow::Normal)
            }),
            StmtKind::Switch(subject, cases) => Box::pin(async move {
                let v = self.eval(subject).await?;
                let mut matched = None;
                for (i, (label, _)) in cases.iter().enumerate() {
                    let hit = match label {
                        CaseLabel::Default => false,
                        CaseLabel::Regex(id) => {
                            let re = self.lit_regex(*id);
                            re.is_match(&self.to_str(&v))
                        }
                        CaseLabel::Value(e) => {
                            let c = self.eval(e).await?;
                            match c {
                                Value::Num(n) if v.is_numeric() => v.to_num() == n,
                                _ => self.to_str(&v) == self.to_str(&c),
                            }
                        }
                    };
                    if hit {
                        matched = Some(i);
                        break;
                    }
                }
                if matched.is_none() {
                    matched = cases
                        .iter()
                        .position(|(l, _)| matches!(l, CaseLabel::Default));
                }
                if let Some(start) = matched {
                    for (_, body) in &cases[start..] {
                        match self.exec_block(body).await? {
                            Flow::Normal => {}
                            Flow::Break => break,
                            other => return Ok(other),
                        }
                    }
                }
                Ok(Flow::Normal)
            }),
        }
    }

    /// Keys for `for (k in a)`, in `PROCINFO["sorted_in"]` order if set.
    fn for_in_keys(&self, id: ArrId) -> Vec<String> {
        let Some(arr) = self.array(id) else {
            return Vec::new();
        };
        let order = match &self.globals[special::PROCINFO as usize] {
            Cell::Arr(p) => match self.array(*p).and_then(|a| a.get("sorted_in")) {
                Some(Elem::Val(v)) => self.to_str(v),
                _ => String::new(),
            },
            Cell::Val(_) => String::new(),
        };
        let mut keys = arr.keys();
        if order.is_empty() || order == "@unsorted" {
            return super::order::gawk_order(keys);
        }
        let val = |k: &String| match arr.get(k) {
            Some(Elem::Val(v)) => v.clone(),
            _ => Value::Uninit,
        };
        match order.as_str() {
            "@ind_str_asc" => keys.sort(),
            "@ind_str_desc" => keys.sort_by(|a, b| b.cmp(a)),
            "@ind_num_asc" => keys.sort_by(|a, b| num_cmp(a, b)),
            "@ind_num_desc" => keys.sort_by(|a, b| num_cmp(b, a)),
            "@val_str_asc" | "@val_type_asc" => keys.sort_by(|a, b| {
                self.to_str(&val(a))
                    .cmp(&self.to_str(&val(b)))
                    .then_with(|| a.cmp(b))
            }),
            "@val_str_desc" | "@val_type_desc" => keys.sort_by(|a, b| {
                self.to_str(&val(b))
                    .cmp(&self.to_str(&val(a)))
                    .then_with(|| b.cmp(a))
            }),
            "@val_num_asc" => keys.sort_by(|a, b| {
                val(a)
                    .to_num()
                    .total_cmp(&val(b).to_num())
                    .then_with(|| a.cmp(b))
            }),
            "@val_num_desc" => keys.sort_by(|a, b| {
                val(b)
                    .to_num()
                    .total_cmp(&val(a).to_num())
                    .then_with(|| b.cmp(a))
            }),
            _ => {}
        }
        keys
    }

    // ----- program driving -----

    async fn pattern_matches(&mut self, rule: &Rule) -> R<bool> {
        Ok(match &rule.pattern {
            Pattern::All => true,
            Pattern::Expr(e) => self.cond(e).await?,
            Pattern::Range(a, b, id) => {
                if !self.range_active[*id] {
                    if self.cond(a).await? {
                        if !self.cond(b).await? {
                            self.range_active[*id] = true;
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    if self.cond(b).await? {
                        self.range_active[*id] = false;
                    }
                    true
                }
            }
        })
    }

    async fn run_rules(&mut self, prog: &Program) -> R<()> {
        for rule in &prog.rules {
            if !self.pattern_matches(rule).await? {
                continue;
            }
            match &rule.body {
                None => {
                    self.rebuild_record()?;
                    let mut line = self.record.clone();
                    line.push_str(&self.ors);
                    self.write_out(None, line).await?;
                }
                Some(b) => {
                    self.exec_block(b).await?;
                }
            }
        }
        Ok(())
    }

    pub(super) async fn run_blocks(&mut self, blocks: &[Block]) -> R<()> {
        for b in blocks {
            self.exec_block(b).await?;
        }
        Ok(())
    }

    /// Run the whole program; returns the exit status.
    pub(super) async fn run(&mut self) -> i32 {
        let prog = self.prog.clone();
        let mut exited = false;
        match self.run_blocks(&prog.begin).await {
            Ok(()) => {}
            Err(Unwind::Exit) => exited = true,
            Err(Unwind::Fatal) => return self.finish().await,
            Err(Unwind::Next | Unwind::NextFile) => {}
        }
        let reads_input = !prog.rules.is_empty()
            || !prog.end.is_empty()
            || !prog.beginfile.is_empty()
            || !prog.endfile.is_empty();
        if !exited && reads_input {
            loop {
                let rec = match self.next_main_record().await {
                    Ok(Some(r)) => r,
                    Ok(None) => break,
                    Err(Unwind::Exit) => {
                        exited = true;
                        break;
                    }
                    Err(Unwind::Fatal) => return self.finish().await,
                    Err(_) => continue,
                };
                self.set_record(rec);
                match self.run_rules(&prog).await {
                    Ok(()) | Err(Unwind::Next) => {}
                    Err(Unwind::NextFile) => {
                        if let Err(e) = self.skip_file().await {
                            match e {
                                Unwind::Exit => {
                                    exited = true;
                                    break;
                                }
                                Unwind::Fatal => return self.finish().await,
                                _ => {}
                            }
                        }
                    }
                    Err(Unwind::Exit) => {
                        exited = true;
                        break;
                    }
                    Err(Unwind::Fatal) => return self.finish().await,
                }
            }
        }
        let _ = exited;
        match self.run_blocks(&prog.end).await {
            Ok(()) | Err(Unwind::Exit) | Err(Unwind::Next) | Err(Unwind::NextFile) => {}
            Err(Unwind::Fatal) => return self.finish().await,
        }
        self.finish().await
    }

    async fn finish(&mut self) -> i32 {
        // Flush redirections and run pending output pipes, even after a
        // fatal error (gawk closes its files on exit too).
        self.close_all().await;
        if self.fatal { 2 } else { self.exit_code }
    }
}

pub(super) fn bool_val(b: bool) -> Value {
    Value::Num(if b { 1.0 } else { 0.0 })
}

/// A NaN produced from non-NaN operands prints as `-nan`, like gawk on
/// x86-64 (the hardware default NaN has its sign bit set).
pub(super) fn fix_nan(r: f64, inputs: &[f64]) -> f64 {
    if r.is_nan() && !inputs.iter().any(|x| x.is_nan()) {
        -f64::NAN
    } else {
        r
    }
}

fn elem_cost(key: &str, e: &Elem) -> usize {
    key.len()
        + AWK_VARIABLE_OVERHEAD_BYTES
        + match e {
            Elem::Val(v) => v.heap_bytes(),
            Elem::Arr(_) => 0,
        }
}

fn num_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (x, y) = (super::value::str_to_num(a), super::value::str_to_num(b));
    x.total_cmp(&y).then_with(|| a.cmp(b))
}

pub(super) fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn parse_widths(s: &str) -> Vec<(usize, Option<usize>)> {
    let mut out = Vec::new();
    for item in s.split_whitespace() {
        let (skip, w) = match item.split_once(':') {
            Some((a, b)) => (a.parse().unwrap_or(0), b),
            None => (0, item),
        };
        let width = if w == "*" { None } else { w.parse().ok() };
        if width.is_none() && w != "*" {
            continue;
        }
        out.push((skip, width));
    }
    out
}

/// Split `text` with `sp`; `seps` collects the separators when given.
pub(super) fn split_text(
    sp: &Splitter,
    text: &str,
    mut seps: Option<&mut Vec<String>>,
    max: usize,
) -> Result<Vec<String>, TooMany> {
    let mut out = Parts {
        v: Vec::new(),
        max,
        over: false,
    };
    match sp {
        Splitter::Space => {
            let is_blank = |c: char| c == ' ' || c == '\t' || c == '\n';
            let trimmed_start = text.trim_start_matches(is_blank);
            // seps[0] is the leading blank run (gawk), possibly empty.
            if let Some(s) = seps.as_deref_mut() {
                s.push(text[..text.len() - trimmed_start.len()].to_string());
            }
            let mut rest = trimmed_start;
            while !rest.is_empty() && !out.over {
                let end = rest.find(is_blank).unwrap_or(rest.len());
                out.push(rest[..end].to_string());
                let after = rest[end..].trim_start_matches(is_blank);
                if let Some(s) = seps.as_deref_mut() {
                    let sep = &rest[end..rest.len() - after.len()];
                    if !sep.is_empty() {
                        s.push(sep.to_string());
                    }
                }
                rest = after;
            }
        }
        _ if text.is_empty() => {}
        Splitter::Char(c) => {
            for part in text.split(*c) {
                out.push(part.to_string());
                if out.over {
                    break;
                }
            }
            if let Some(s) = seps.as_deref_mut() {
                for _ in 1..out.v.len() {
                    s.push(c.to_string());
                }
            }
        }
        Splitter::Chars => {
            for c in text.chars() {
                out.push(c.to_string());
                if out.over {
                    break;
                }
            }
        }
        Splitter::Regex(re) => {
            let mut last = 0;
            let mut pos = 0;
            while pos <= text.len() && !out.over {
                let Some((s, e)) = re.find_at(text, pos) else {
                    break;
                };
                if s == e {
                    // An empty match separates nothing.
                    pos = next_char(text, s);
                    continue;
                }
                out.push(text[last..s].to_string());
                if let Some(v) = seps.as_deref_mut() {
                    v.push(text[s..e].to_string());
                }
                last = e;
                pos = e;
            }
            out.push(text[last..].to_string());
        }
        Splitter::Csv => {
            for f in super::csv_split_fields(text) {
                out.push(f);
            }
        }
        Splitter::Widths(ws) => {
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            for (skip, w) in ws {
                if out.over {
                    break;
                }
                i += skip;
                if i >= chars.len() {
                    break;
                }
                let end = match w {
                    Some(w) => (i + w).min(chars.len()),
                    None => chars.len(),
                };
                out.push(chars[i..end].iter().collect());
                i = end;
            }
        }
        Splitter::Fpat(re) => {
            let mut pos = 0;
            let mut last_end: Option<usize> = None;
            while pos <= text.len() && !out.over {
                let Some((s, e)) = re.find_at(text, pos) else {
                    break;
                };
                if s == e {
                    if last_end != Some(s) {
                        out.push(String::new());
                        last_end = Some(s);
                    }
                    pos = next_char(text, s);
                    continue;
                }
                if let Some(v) = seps.as_deref_mut()
                    && let Some(l) = last_end
                {
                    v.push(text[l..s].to_string());
                }
                out.push(text[s..e].to_string());
                last_end = Some(e);
                pos = e;
            }
        }
    }
    if out.over { Err(TooMany) } else { Ok(out.v) }
}

/// More fields than the memory cap leaves room for.
pub(super) struct TooMany;

/// Split output that stops at `max` parts, so a huge string cannot build
/// a huge vector before the memory cap is checked (TM-DOS-110).
struct Parts {
    v: Vec<String>,
    max: usize,
    over: bool,
}

impl Parts {
    fn push(&mut self, s: String) {
        if self.v.len() >= self.max {
            self.over = true;
        } else {
            self.v.push(s);
        }
    }
}

/// Byte offset just past the character at `i` (or `len + 1` at the end).
pub(super) fn next_char(s: &str, i: usize) -> usize {
    match s[i..].chars().next() {
        Some(c) => i + c.len_utf8(),
        None => s.len() + 1,
    }
}
