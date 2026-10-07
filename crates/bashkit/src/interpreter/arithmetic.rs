//! Arithmetic expansion and evaluation (`$(( ))`, `(( ))`, `let`).
//!
//! Important decisions:
//! - Two phases, as in bash: `$`-expansion is textual (`$x`, `${..}`, `$1`),
//!   then a tokenizer + precedence-climbing parser (bash `expr.c` grammar)
//!   evaluates the result. Bare identifiers are looked up while parsing and
//!   their values evaluated recursively as expressions.
//! - Evaluation never mutates the interpreter: assignments land in an overlay
//!   (so `x=5, x+1` sees 5) and are returned as an ordered write list. `&mut`
//!   callers apply the writes; read-only callers drop them.
//! - Errors (division by 0, syntax errors, bad subscripts, bad numbers) are
//!   real errors. `$((..))` turns them into a line abort (bash DISCARD),
//!   `((..))`/`let` into status 1. Read-only call sites record the error in
//!   `arith_error` and the command boundary aborts the line.
//! - Integers wrap at 64 bits; shift counts are masked to 0..63 (x86 / bash).
//! - THREAT[TM-DOS-026]: recursion depth (`MAX_ARITHMETIC_DEPTH`) and fuel
//!   (`MAX_ARITHMETIC_EXPANSION_FUEL`) bound nested and self-referential input.

use super::*;

/// One assignment performed while evaluating: `(name, key, value)`.
/// `key` is `None` for scalars, the decimal index for indexed arrays, and the
/// key for associative arrays.
pub(super) type ArithWrite = (String, Option<String>, String);

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(String),
    Ident(String, Option<String>),
    Op(&'static str),
    LParen,
    RParen,
    Eof,
}

const ARITH_OPS: &[&str] = &[
    "<<=", ">>=", "**", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=", "-=",
    "*=", "/=", "%=", "&=", "^=", "|=", "+", "-", "*", "/", "%", "<", ">", "!", "~", "&", "^", "|",
    "?", ":", "=", ",",
];

fn tokenize(src: &str) -> std::result::Result<Vec<(Tok, usize)>, (String, usize)> {
    let bytes = src.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_digit() {
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'#' | b'@' | b'_'))
            {
                i += 1;
            }
            toks.push((Tok::Num(src[start..i].to_string()), start));
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = src[start..i].to_string();
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let mut sub = None;
            if j < bytes.len() && bytes[j] == b'[' {
                let mut depth = 0usize;
                let mut k = j;
                let mut end = None;
                while k < bytes.len() {
                    match bytes[k] {
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(k);
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                let Some(end) = end else {
                    return Err(("bad array subscript".to_string(), start));
                };
                sub = Some(src[j + 1..end].to_string());
                i = end + 1;
            }
            toks.push((Tok::Ident(name, sub), start));
            continue;
        }
        if c == b'(' {
            toks.push((Tok::LParen, start));
            i += 1;
            continue;
        }
        if c == b')' {
            toks.push((Tok::RParen, start));
            i += 1;
            continue;
        }
        let rest = &src[i..];
        match ARITH_OPS.iter().find(|op| rest.starts_with(**op)) {
            Some(op) => {
                toks.push((Tok::Op(op), start));
                i += op.len();
            }
            None => {
                return Err((
                    "syntax error: invalid arithmetic operator".to_string(),
                    start,
                ));
            }
        }
    }
    toks.push((Tok::Eof, src.len()));
    Ok(toks)
}

/// Parse an integer constant the way bash does: `0x` hex, leading-0 octal,
/// `base#digits` (2..64), decimal otherwise. Overflow wraps.
pub(super) fn parse_arith_number(s: &str) -> std::result::Result<i64, String> {
    let (base, digits): (u32, &str) =
        if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            (16, rest)
        } else if let Some(hash) = s.find('#') {
            let base = s[..hash]
                .parse::<u32>()
                .ok()
                .filter(|b| (2..=64).contains(b))
                .ok_or_else(|| "invalid arithmetic base".to_string())?;
            (base, &s[hash + 1..])
        } else if s.len() > 1 && s.starts_with('0') {
            (8, &s[1..])
        } else {
            (10, s)
        };
    if digits.is_empty() && base != 8 {
        return Err("invalid integer constant".to_string());
    }
    let mut val: i64 = 0;
    for ch in digits.chars() {
        let d = match ch {
            '0'..='9' => ch as u32 - '0' as u32,
            'a'..='z' => ch as u32 - 'a' as u32 + 10,
            'A'..='Z' if base > 36 => ch as u32 - 'A' as u32 + 36,
            'A'..='Z' => ch as u32 - 'A' as u32 + 10,
            '@' => 62,
            '_' => 63,
            _ => return Err("invalid number".to_string()),
        };
        if d >= base {
            return Err("value too great for base".to_string());
        }
        val = val.wrapping_mul(base as i64).wrapping_add(d as i64);
    }
    Ok(val)
}

fn wrapping_ipow(mut base: i64, mut exp: i64) -> i64 {
    let mut acc: i64 = 1;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = acc.wrapping_mul(base);
        }
        base = base.wrapping_mul(base);
        exp >>= 1;
    }
    acc
}

/// A resolved assignment target.
struct LValue {
    name: String,
    key: Option<String>,
}

type ArithResult<T> = std::result::Result<T, String>;

/// Expression evaluator state shared across recursive variable evaluation.
pub(super) struct ArithEval<'a> {
    interp: &'a Interpreter,
    overlay: HashMap<(String, Option<String>), String>,
    pub(super) writes: Vec<ArithWrite>,
    noeval: u32,
    depth: usize,
    fuel: usize,
}

struct ArithParser<'s> {
    src: &'s str,
    toks: Vec<(Tok, usize)>,
    pos: usize,
}

impl ArithParser<'_> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }
    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].0
    }
    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos].0.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Tok::Op(o) if *o == op)
    }
    fn rest(&self) -> &str {
        &self.src[self.toks[self.pos].1..]
    }
    fn err(&self, msg: &str) -> String {
        let rest = self.rest().trim();
        if rest.is_empty() {
            msg.to_string()
        } else {
            format!("{msg} (error token is \"{rest}\")")
        }
    }
}

impl<'a> ArithEval<'a> {
    pub(super) fn new(interp: &'a Interpreter) -> Self {
        Self {
            interp,
            overlay: HashMap::new(),
            writes: Vec::new(),
            noeval: 0,
            depth: 0,
            fuel: Interpreter::MAX_ARITHMETIC_EXPANSION_FUEL,
        }
    }

    fn enter(&mut self) -> ArithResult<()> {
        self.depth += 1;
        if self.depth >= Interpreter::MAX_ARITHMETIC_DEPTH {
            return Err("expression recursion level exceeded".to_string());
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Evaluate a full expression (already `$`-expanded). Empty -> 0.
    pub(super) fn eval_str(&mut self, src: &str) -> ArithResult<i64> {
        if src.len() > Interpreter::MAX_ARITHMETIC_EXPANSION_BYTES {
            return Err("expression too long".to_string());
        }
        if self.fuel < src.len().max(1) {
            return Err("expression recursion level exceeded".to_string());
        }
        self.fuel -= src.len().max(1);
        if src.trim().is_empty() {
            return Ok(0);
        }
        let toks = tokenize(src).map_err(|(msg, at)| {
            let rest = src[at..].trim();
            format!("{msg} (error token is \"{rest}\")")
        })?;
        let mut p = ArithParser { src, toks, pos: 0 };
        self.enter()?;
        let r = self.comma(&mut p);
        self.leave();
        let v = r?;
        if !matches!(p.peek(), Tok::Eof) {
            return Err(p.err("syntax error in expression"));
        }
        Ok(v)
    }

    fn comma(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.assign(p)?;
        while p.is_op(",") {
            p.next();
            v = self.assign(p)?;
        }
        Ok(v)
    }

    fn assign(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        if let Tok::Ident(name, sub) = p.peek().clone()
            && let Tok::Op(op) = p.peek_at(1).clone()
            && matches!(
                op,
                "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "^=" | "|="
            )
        {
            p.next();
            p.next();
            let lv = self.lvalue(&name, sub.as_deref())?;
            let rhs = self.assign(p)?;
            let value = if op == "=" {
                rhs
            } else {
                let cur = self.read_lvalue_int(&lv)?;
                self.binop(&op[..op.len() - 1], cur, rhs, p)?
            };
            self.store(&lv, value);
            return Ok(value);
        }
        let v = self.cond(p)?;
        if let Tok::Op(op) = p.peek()
            && matches!(
                *op,
                "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "^=" | "|="
            )
        {
            return Err(p.err("attempted assignment to non-variable"));
        }
        Ok(v)
    }

    fn cond(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let c = self.lor(p)?;
        if !p.is_op("?") {
            return Ok(c);
        }
        p.next();
        if c == 0 {
            self.noeval += 1;
        }
        let t = self.comma(p);
        if c == 0 {
            self.noeval -= 1;
        }
        let t = t?;
        if !p.is_op(":") {
            return Err(p.err("`:' expected for conditional expression"));
        }
        p.next();
        if c != 0 {
            self.noeval += 1;
        }
        let f = self.cond(p);
        if c != 0 {
            self.noeval -= 1;
        }
        let f = f?;
        Ok(if c != 0 { t } else { f })
    }

    fn lor(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.land(p)?;
        while p.is_op("||") {
            p.next();
            let skip = v != 0;
            if skip {
                self.noeval += 1;
            }
            let r = self.land(p);
            if skip {
                self.noeval -= 1;
            }
            let r = r?;
            v = i64::from(v != 0 || r != 0);
        }
        Ok(v)
    }

    fn land(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.bor(p)?;
        while p.is_op("&&") {
            p.next();
            let skip = v == 0;
            if skip {
                self.noeval += 1;
            }
            let r = self.bor(p);
            if skip {
                self.noeval -= 1;
            }
            let r = r?;
            v = i64::from(v != 0 && r != 0);
        }
        Ok(v)
    }

    fn bor(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.bxor(p)?;
        while p.is_op("|") {
            p.next();
            v |= self.bxor(p)?;
        }
        Ok(v)
    }

    fn bxor(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.band(p)?;
        while p.is_op("^") {
            p.next();
            v ^= self.band(p)?;
        }
        Ok(v)
    }

    fn band(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.equality(p)?;
        while p.is_op("&") {
            p.next();
            v &= self.equality(p)?;
        }
        Ok(v)
    }

    fn equality(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.relational(p)?;
        loop {
            let op = match p.peek() {
                Tok::Op(o @ ("==" | "!=")) => *o,
                _ => return Ok(v),
            };
            p.next();
            let r = self.relational(p)?;
            v = i64::from(if op == "==" { v == r } else { v != r });
        }
    }

    fn relational(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.shift(p)?;
        loop {
            let op = match p.peek() {
                Tok::Op(o @ ("<" | ">" | "<=" | ">=")) => *o,
                _ => return Ok(v),
            };
            p.next();
            let r = self.shift(p)?;
            v = i64::from(match op {
                "<" => v < r,
                ">" => v > r,
                "<=" => v <= r,
                _ => v >= r,
            });
        }
    }

    fn shift(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.additive(p)?;
        loop {
            let op = match p.peek() {
                Tok::Op(o @ ("<<" | ">>")) => *o,
                _ => return Ok(v),
            };
            p.next();
            let r = self.additive(p)?;
            v = self.binop(op, v, r, p)?;
        }
    }

    fn additive(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.multiplicative(p)?;
        loop {
            let op = match p.peek() {
                Tok::Op(o @ ("+" | "-")) => *o,
                // `5--1` / `5++1`: binary operator followed by a unary one.
                Tok::Op("--") => {
                    p.toks[p.pos].0 = Tok::Op("-");
                    let at = p.toks[p.pos].1 + 1;
                    p.toks.insert(p.pos + 1, (Tok::Op("-"), at));
                    "-"
                }
                Tok::Op("++") => {
                    p.toks[p.pos].0 = Tok::Op("+");
                    let at = p.toks[p.pos].1 + 1;
                    p.toks.insert(p.pos + 1, (Tok::Op("+"), at));
                    "+"
                }
                _ => return Ok(v),
            };
            p.next();
            let r = self.multiplicative(p)?;
            v = if op == "+" {
                v.wrapping_add(r)
            } else {
                v.wrapping_sub(r)
            };
        }
    }

    fn multiplicative(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let mut v = self.power(p)?;
        loop {
            let op = match p.peek() {
                Tok::Op(o @ ("*" | "/" | "%")) => *o,
                _ => return Ok(v),
            };
            p.next();
            let r = self.power(p)?;
            v = self.binop(op, v, r, p)?;
        }
    }

    fn power(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let base = self.unary(p)?;
        if p.is_op("**") {
            p.next();
            self.enter()?;
            let exp = self.power(p);
            self.leave();
            return self.binop("**", base, exp?, p);
        }
        Ok(base)
    }

    fn unary(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        let op = match p.peek() {
            Tok::Op(o @ ("!" | "~" | "-" | "+")) => Some(*o),
            Tok::Op(o @ ("++" | "--")) => {
                if let Tok::Ident(name, sub) = p.peek_at(1).clone() {
                    let inc = *o == "++";
                    p.next();
                    p.next();
                    let lv = self.lvalue(&name, sub.as_deref())?;
                    let cur = self.read_lvalue_int(&lv)?;
                    let v = if inc {
                        cur.wrapping_add(1)
                    } else {
                        cur.wrapping_sub(1)
                    };
                    self.store(&lv, v);
                    return Ok(v);
                }
                // `++5` / `--5`: two unary signs.
                let single = if *o == "++" { "+" } else { "-" };
                p.toks[p.pos].0 = Tok::Op(single);
                let at = p.toks[p.pos].1 + 1;
                p.toks.insert(p.pos + 1, (Tok::Op(single), at));
                Some(single)
            }
            _ => None,
        };
        let Some(op) = op else {
            return self.primary(p);
        };
        p.next();
        self.enter()?;
        let v = self.unary(p);
        self.leave();
        let v = v?;
        Ok(match op {
            "!" => i64::from(v == 0),
            "~" => !v,
            "-" => v.wrapping_neg(),
            _ => v,
        })
    }

    fn primary(&mut self, p: &mut ArithParser) -> ArithResult<i64> {
        match p.peek().clone() {
            Tok::LParen => {
                p.next();
                self.enter()?;
                let v = self.comma(p);
                self.leave();
                let v = v?;
                if !matches!(p.peek(), Tok::RParen) {
                    return Err(p.err("missing `)'"));
                }
                p.next();
                Ok(v)
            }
            Tok::Num(s) => {
                let v =
                    parse_arith_number(&s).map_err(|m| format!("{m} (error token is \"{s}\")"))?;
                p.next();
                Ok(v)
            }
            Tok::Ident(name, sub) => {
                p.next();
                // `a[]` reads as 0 (bash reports "bad array subscript" but
                // keeps evaluating).
                if sub.as_deref().is_some_and(|s| s.trim().is_empty())
                    && !matches!(p.peek(), Tok::Op("++" | "--"))
                {
                    return Ok(0);
                }
                let lv = self.lvalue(&name, sub.as_deref())?;
                let cur = self.read_lvalue_int(&lv)?;
                match p.peek() {
                    Tok::Op(o @ ("++" | "--")) => {
                        let inc = *o == "++";
                        p.next();
                        let v = if inc {
                            cur.wrapping_add(1)
                        } else {
                            cur.wrapping_sub(1)
                        };
                        self.store(&lv, v);
                        Ok(cur)
                    }
                    _ => Ok(cur),
                }
            }
            _ => Err(p.err("syntax error: operand expected")),
        }
    }

    fn binop(&mut self, op: &str, l: i64, r: i64, p: &ArithParser) -> ArithResult<i64> {
        Ok(match op {
            "+" => l.wrapping_add(r),
            "-" => l.wrapping_sub(r),
            "*" => l.wrapping_mul(r),
            "/" | "%" => {
                if r == 0 {
                    if self.noeval > 0 {
                        return Ok(0);
                    }
                    return Err(p.err("division by 0"));
                }
                if op == "/" {
                    l.wrapping_div(r)
                } else {
                    l.wrapping_rem(r)
                }
            }
            "**" => {
                if r < 0 {
                    if self.noeval > 0 {
                        return Ok(0);
                    }
                    return Err("exponent less than 0".to_string());
                }
                wrapping_ipow(l, r)
            }
            "<<" => l.wrapping_shl((r & 63) as u32),
            ">>" => l.wrapping_shr((r & 63) as u32),
            "&" => l & r,
            "|" => l | r,
            "^" => l ^ r,
            _ => r,
        })
    }

    /// Resolve `name` / `name[sub]` to a concrete storage slot.
    fn lvalue(&mut self, name: &str, sub: Option<&str>) -> ArithResult<LValue> {
        let resolved = self.interp.resolve_nameref(name).to_string();
        let (name, sub): (String, Option<String>) = match sub {
            Some(s) => (resolved, Some(s.to_string())),
            None => match parse_embedded_array_ref(&resolved) {
                Some((n, s)) => (n.to_string(), Some(s.to_string())),
                None => (resolved, None),
            },
        };
        let Some(sub) = sub else {
            return Ok(LValue { name, key: None });
        };
        if self.interp.is_assoc_array(&name) {
            let key = strip_subscript_quotes(&sub).to_string();
            return Ok(LValue {
                name,
                key: Some(key),
            });
        }
        if sub.trim().is_empty() {
            return Err(format!("{name}[]: bad array subscript"));
        }
        let mut idx = self.eval_str(&sub)?;
        if idx < 0 {
            let len = self.interp.indexed_len_for_subscript(&name);
            idx += len;
            if idx < 0 && self.noeval == 0 {
                return Err(format!("{name}[{sub}]: bad array subscript"));
            }
        }
        Ok(LValue {
            name,
            key: Some(idx.max(0).to_string()),
        })
    }

    fn read_lvalue_str(&self, lv: &LValue) -> String {
        if let Some(v) = self.overlay.get(&(lv.name.clone(), lv.key.clone())) {
            return v.clone();
        }
        let interp = self.interp;
        match &lv.key {
            None => interp.expand_variable(&lv.name),
            Some(key) => {
                if let Some(arr) = interp.scoped.assoc_arrays.get(&lv.name) {
                    return arr.get(key).cloned().unwrap_or_default();
                }
                let idx: usize = key.parse().unwrap_or(0);
                if let Some(arr) = interp.scoped.arrays.get(&lv.name) {
                    return arr.get(&idx).cloned().unwrap_or_default();
                }
                if idx == 0 {
                    interp.expand_variable(&lv.name)
                } else {
                    String::new()
                }
            }
        }
    }

    fn read_lvalue_int(&mut self, lv: &LValue) -> ArithResult<i64> {
        let s = self.read_lvalue_str(lv);
        let t = s.trim();
        if t.is_empty() {
            return Ok(0);
        }
        if let Ok(v) = t.parse::<i64>() {
            return Ok(v);
        }
        if self.noeval > 0 {
            return Ok(0);
        }
        self.enter()?;
        let r = self.eval_str(t);
        self.leave();
        r
    }

    fn store(&mut self, lv: &LValue, value: i64) {
        if self.noeval > 0 {
            return;
        }
        let v = value.to_string();
        self.overlay
            .insert((lv.name.clone(), lv.key.clone()), v.clone());
        self.writes.push((lv.name.clone(), lv.key.clone(), v));
    }
}

impl Interpreter {
    /// True when `name` is (or is declared as) an associative array.
    pub(super) fn is_assoc_array(&self, name: &str) -> bool {
        self.scoped.assoc_arrays.contains_key(name)
    }

    /// Element count used to resolve negative subscripts: max index + 1 for
    /// arrays, 1 for a set scalar (element 0), 0 otherwise.
    pub(super) fn indexed_len_for_subscript(&self, name: &str) -> i64 {
        if let Some(arr) = self.scoped.arrays.get(name) {
            return arr.keys().max().map_or(0, |m| *m as i64 + 1);
        }
        i64::from(self.lookup_regular_variable(name).is_some())
    }

    /// Evaluate `expr`, returning the value or an error message, plus the
    /// assignments made (in order, including those before an error).
    pub(super) fn arith_eval(&self, expr: &str) -> (ArithResult<i64>, Vec<ArithWrite>) {
        let expanded = self.expand_arith_dollars(expr);
        let mut ev = ArithEval::new(self);
        let r = ev.eval_str(&expanded);
        (r, ev.writes)
    }

    /// Apply writes produced by [`arith_eval`](Self::arith_eval).
    pub(super) fn apply_arith_writes(&mut self, writes: Vec<ArithWrite>) {
        for (name, key, value) in writes {
            match key {
                None => self.set_variable(name, value),
                Some(key) => {
                    if self.scoped.assoc_arrays.contains_key(&name) {
                        self.set_assoc_element_checked(name, key, value);
                    } else {
                        self.set_parameter_expansion_target(&format!("{name}[{key}]"), value);
                    }
                }
            }
        }
    }

    /// Evaluate with side effects; errors are returned to the caller.
    pub(super) fn try_evaluate_arithmetic_with_assign(&mut self, expr: &str) -> ArithResult<i64> {
        let (r, writes) = self.arith_eval(expr);
        self.apply_arith_writes(writes);
        r.map_err(|m| format!("{}: {m}", expr.trim()))
    }

    /// Evaluate with side effects; an error is recorded for the command
    /// boundary (line abort) and the value reads as 0.
    pub(super) fn evaluate_arithmetic_with_assign(&mut self, expr: &str) -> i64 {
        match self.try_evaluate_arithmetic_with_assign(expr) {
            Ok(v) => v,
            Err(msg) => {
                self.record_arith_error(msg);
                0
            }
        }
    }

    /// Evaluate without side effects; an error is recorded for the command
    /// boundary (line abort) and the value reads as 0.
    pub(super) fn evaluate_arithmetic(&self, expr: &str) -> i64 {
        match self.arith_eval(expr).0 {
            Ok(v) => v,
            Err(msg) => {
                self.record_arith_error(format!("{}: {msg}", expr.trim()));
                0
            }
        }
    }

    pub(super) fn record_arith_error(&self, msg: String) {
        if let Ok(mut slot) = self.arith_error.lock()
            && slot.is_none()
        {
            *slot = Some(msg);
        }
    }

    pub(super) fn take_arith_error(&self) -> Option<String> {
        self.arith_error.lock().ok().and_then(|mut s| s.take())
    }

    /// Textual `$` expansion inside an arithmetic expression (`$x`, `${..}`,
    /// `$1`, `$#`, ...). Bare identifiers are left for the evaluator.
    /// Double quotes are dropped (`"$x"` is `$x`).
    pub(super) fn expand_arith_dollars(&self, expr: &str) -> String {
        if !expr.contains('$') && !expr.contains('"') {
            return expr.to_string();
        }
        let expr = expr.replace('"', "");
        let mut result = String::new();
        let mut chars = expr.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '$' {
                result.push(ch);
                continue;
            }
            if chars.peek() == Some(&'{') {
                chars.next();
                let mut brace_content = String::new();
                let mut brace_depth = 1i32;
                for c in chars.by_ref() {
                    if c == '{' {
                        brace_depth += 1;
                    } else if c == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            break;
                        }
                    }
                    brace_content.push(c);
                }
                result.push_str(&self.expand_brace_expr_in_arithmetic(&brace_content));
            } else if let Some(&c) = chars.peek()
                && matches!(c, '#' | '?' | '$' | '!' | '@' | '*' | '-' | '0'..='9')
            {
                chars.next();
                result.push_str(&self.expand_variable(&c.to_string()));
            } else {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if name.is_empty() {
                    result.push('$');
                } else {
                    result.push_str(&self.expand_variable(&name));
                }
            }
        }
        result
    }

    /// Expand a `${...}` expression encountered inside arithmetic context.
    /// Handles: `${#arr[@]}`, `${#arr[*]}`, `${#var}`, `${arr[idx]}`, `${var}`.
    pub(super) fn expand_brace_expr_in_arithmetic(&self, inner: &str) -> String {
        // ${#arr[@]} or ${#arr[*]} — array length
        if let Some(rest) = inner.strip_prefix('#') {
            if let Some(bracket) = rest.find('[') {
                if !rest.ends_with(']') {
                    return "0".to_string();
                }
                let end = rest.len() - 1;
                if bracket + 1 > end {
                    return "0".to_string();
                }
                let arr_name = &rest[..bracket];
                let idx = &rest[bracket + 1..end];
                if idx == "@" || idx == "*" {
                    if let Some(arr) = self.scoped.arrays.get(arr_name) {
                        return arr.len().to_string();
                    }
                    if let Some(arr) = self.scoped.assoc_arrays.get(arr_name) {
                        return arr.len().to_string();
                    }
                    return "0".to_string();
                }
                return self
                    .expand_name_or_array_element(&rest[..=end])
                    .chars()
                    .count()
                    .to_string();
            }
            let val = self.expand_variable(rest);
            return val.chars().count().to_string();
        }

        if let Some(bracket) = inner.find('[')
            && inner.ends_with(']')
            && is_valid_var_name(&inner[..bracket])
        {
            return self.expand_name_or_array_element(inner);
        }

        let has_operator = inner.contains("%%")
            || inner.contains('%')
            || (inner.contains('#') && !inner.starts_with('#'))
            || inner.contains(":-");
        if has_operator {
            return self.expand_param_op_in_arithmetic(inner);
        }

        self.expand_variable(inner)
    }

    /// Expand a parameter expansion with operators inside arithmetic context.
    /// Handles common cases like ${var%%-*}, ${var##prefix}, etc.
    pub(super) fn expand_param_op_in_arithmetic(&self, inner: &str) -> String {
        for (pos, ch) in inner.char_indices() {
            match ch {
                '%' => {
                    let name = &inner[..pos];
                    let value = self.expand_name_or_array_element(name);
                    if inner[pos..].starts_with("%%") {
                        let pattern = &inner[pos + 2..];
                        return self.remove_pattern(&value, pattern, false, true);
                    }
                    let pattern = &inner[pos + 1..];
                    return self.remove_pattern(&value, pattern, false, false);
                }
                '#' if pos > 0 => {
                    let name = &inner[..pos];
                    let value = self.expand_name_or_array_element(name);
                    if inner[pos..].starts_with("##") {
                        let pattern = &inner[pos + 2..];
                        return self.remove_pattern(&value, pattern, true, true);
                    }
                    let pattern = &inner[pos + 1..];
                    return self.remove_pattern(&value, pattern, true, false);
                }
                ':' if inner[pos..].starts_with(":-") => {
                    let name = &inner[..pos];
                    let default = &inner[pos + 2..];
                    let value = self.expand_name_or_array_element(name);
                    if value.is_empty() {
                        return default.to_string();
                    }
                    return value;
                }
                _ => {}
            }
        }
        self.expand_name_or_array_element(inner)
    }

    /// Resolve `name` or `arr[idx]` to its current string value.
    /// Used by parameter expansion inside arithmetic so `${arr[$key]:-N}` and
    /// friends can read associative/indexed array elements — `expand_variable`
    /// alone only handles scalar names. Fixes issue #1776.
    pub(super) fn expand_name_or_array_element(&self, name: &str) -> String {
        if let Some(bracket) = name.find('[')
            && name.ends_with(']')
        {
            let arr_name = &name[..bracket];
            let resolved = self.resolve_nameref(arr_name);
            let idx_str = strip_subscript_quotes(&name[bracket + 1..name.len() - 1]);
            if let Some(arr) = self.scoped.assoc_arrays.get(resolved) {
                let key = self.expand_variable_or_literal(idx_str);
                return arr.get(&key).cloned().unwrap_or_default();
            }
            if let Some(arr) = self.scoped.arrays.get(resolved) {
                let idx = self.resolve_indexed_array_subscript(resolved, idx_str);
                return arr.get(&idx).cloned().unwrap_or_default();
            }
            if self.resolve_indexed_array_subscript(resolved, idx_str) == 0 {
                return self.expand_variable(resolved);
            }
            return String::new();
        }
        self.expand_variable(name)
    }
}

/// Drop one layer of matching quotes around a subscript: `c["x y"]` -> key `x y`.
fn strip_subscript_quotes(sub: &str) -> &str {
    let t = sub.trim();
    if t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')))
    {
        &t[1..t.len() - 1]
    } else {
        sub
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_bash_bases() {
        assert_eq!(parse_arith_number("0x1f"), Ok(31));
        assert_eq!(parse_arith_number("017"), Ok(15));
        assert_eq!(parse_arith_number("2#101"), Ok(5));
        assert_eq!(parse_arith_number("64#@"), Ok(62));
        assert_eq!(parse_arith_number("36#Z"), Ok(35));
        assert!(parse_arith_number("08").is_err());
        assert!(parse_arith_number("65#1").is_err());
        assert_eq!(
            parse_arith_number("99999999999999999999"),
            Ok(7766279631452241919)
        );
    }

    #[test]
    fn tokenizer_rejects_unknown_operator() {
        assert!(tokenize("1 $ 2").is_err());
        assert!(tokenize("a[1 ").is_err());
        assert_eq!(tokenize("a[b[1]]+1").unwrap().len(), 4);
    }
}
