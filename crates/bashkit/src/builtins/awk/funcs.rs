//! awk builtin functions.
//!
//! Decisions:
//! - String positions and lengths count characters (gawk in a UTF-8
//!   locale), not bytes.
//! - `substr` rounds start and length like POSIX/gawk.
//! - `sub`/`gsub` replacement follows POSIX 2008 (gawk): `&` is the match,
//!   `\&` a literal `&`, `\\` a backslash. `gensub` adds `\0`..`\9`.
//! - Empty matches in `gsub` follow gawk: an empty match right after a
//!   previous match is skipped (`gsub(/b*/, "-", "abc")` is `-a-c-`).
//! - `rand()` reproduces gawk's generator (BSD `random()` with a 256-byte
//!   state seeded with 1, two draws per number), so seeded and unseeded
//!   sequences match gawk.
//! - Time functions use the sandbox clock and `TZ` (TM-INF-018).

use std::sync::Arc;

use super::ast::{Bi, Expr, VarRef, special};
use super::format::sprintf;
use super::interp::{
    Cell, Elem, Interp, Loc, R, Splitter, bool_val, fix_nan, next_char, split_text,
};
use super::regex::AwkRegex;
use super::value::{Value, str_to_num};

const RAND_DEG: usize = 63;
const RAND_SEP: usize = 1;

/// gawk's `random()` (BSD, TYPE_4 state).
pub(super) struct Rand {
    state: [u32; RAND_DEG],
    f: usize,
    r: usize,
    pub(super) seed: f64,
}

impl Rand {
    pub(super) fn new() -> Self {
        let mut r = Rand {
            state: [0; RAND_DEG],
            f: RAND_SEP,
            r: 0,
            seed: 0.0,
        };
        r.srandom(1);
        r
    }

    fn good_rand(x: i32) -> i32 {
        let mut x = x as i64;
        if x == 0 {
            x = 123_459_876;
        }
        let hi = x / 127_773;
        let lo = x % 127_773;
        x = 16_807 * lo - 2_836 * hi;
        if x < 0 {
            x += 0x7fff_ffff;
        }
        x as i32
    }

    pub(super) fn srandom(&mut self, x: u32) {
        self.state[0] = x;
        for i in 1..RAND_DEG {
            self.state[i] = Self::good_rand(self.state[i - 1] as i32) as u32;
        }
        self.f = RAND_SEP;
        self.r = 0;
        for _ in 0..10 * RAND_DEG {
            self.random();
        }
    }

    fn random(&mut self) -> u32 {
        self.state[self.f] = self.state[self.f].wrapping_add(self.state[self.r]);
        let v = (self.state[self.f] >> 1) & 0x7fff_ffff;
        self.f += 1;
        if self.f >= RAND_DEG {
            self.f = 0;
            self.r += 1;
        } else {
            self.r += 1;
            if self.r >= RAND_DEG {
                self.r = 0;
            }
        }
        v
    }

    pub(super) fn rand(&mut self) -> f64 {
        const DIV: f64 = 2_147_483_648.0;
        loop {
            let d1 = self.random() as f64;
            let d2 = self.random() as f64;
            let v = 0.5 + ((d1 / DIV + d2) / DIV) - 0.5;
            if v < 1.0 {
                return v;
            }
        }
    }
}

enum Which {
    All,
    Nth(usize),
}

impl Interp {
    async fn arg_val(&mut self, args: &[Expr], i: usize) -> R<Value> {
        match args.get(i) {
            Some(e) => self.eval(e).await,
            None => Ok(Value::Uninit),
        }
    }

    async fn arg_str(&mut self, args: &[Expr], i: usize) -> R<String> {
        let v = self.arg_val(args, i).await?;
        Ok(self.to_str(&v))
    }

    async fn arg_num(&mut self, args: &[Expr], i: usize) -> R<f64> {
        Ok(self.arg_val(args, i).await?.to_num())
    }

    pub(super) async fn call_builtin(&mut self, bi: Bi, args: &[Expr]) -> R<Value> {
        let num = |n: f64| Ok(Value::Num(n));
        match bi {
            Bi::Length => self.bi_length(args).await,
            Bi::Substr => {
                let s = self.arg_str(args, 0).await?;
                let m = self.arg_num(args, 1).await?;
                let n = if args.len() > 2 {
                    Some(self.arg_num(args, 2).await?)
                } else {
                    None
                };
                Ok(Value::Str(substr(&s, m, n)))
            }
            Bi::Index => {
                let s = self.arg_str(args, 0).await?;
                let t = self.arg_str(args, 1).await?;
                let (s, t) = if self.ignorecase {
                    (s.to_lowercase(), t.to_lowercase())
                } else {
                    (s, t)
                };
                num(match s.find(&t) {
                    Some(b) => (s[..b].chars().count() + 1) as f64,
                    None => 0.0,
                })
            }
            Bi::Split | Bi::Patsplit => self.bi_split(bi, args).await,
            Bi::Sub | Bi::Gsub => {
                let re = self.regex_operand(&args[0]).await?;
                let repl = self.arg_str(args, 1).await?;
                let loc = match args.get(2) {
                    Some(t) if is_lvalue(t) => Some(self.resolve(t).await?),
                    Some(_) => None,
                    None => Some(Loc::Field(0)),
                };
                let target = match (&loc, args.get(2)) {
                    (Some(l), _) => self.load(l)?,
                    (None, Some(e)) => self.eval(e).await?,
                    (None, None) => Value::Uninit,
                };
                let target = self.to_str(&target);
                let which = if bi == Bi::Sub {
                    Which::Nth(1)
                } else {
                    Which::All
                };
                let (out, n) = self.substitute(&re, &target, which, false, |s, caps, out| {
                    let (a, b) = caps[0].unwrap_or((0, 0));
                    expand_sub(&repl, &s[a..b], out);
                })?;
                if n > 0
                    && let Some(l) = loc
                {
                    self.store(l, Value::Str(out))?;
                }
                num(n as f64)
            }
            Bi::Gensub => {
                let re = self.regex_operand(&args[0]).await?;
                let repl = self.arg_str(args, 1).await?;
                let how = self.arg_val(args, 2).await?;
                let target = if args.len() > 3 {
                    self.arg_str(args, 3).await?
                } else {
                    let v = self.get_field(0)?;
                    self.to_str(&v)
                };
                let hs = self.to_str(&how);
                let which = if hs.starts_with('g') || hs.starts_with('G') {
                    Which::All
                } else {
                    let n = how.to_num();
                    Which::Nth(if n >= 1.0 { n as usize } else { 1 })
                };
                let (out, _) = self.substitute(&re, &target, which, true, |s, caps, out| {
                    expand_gensub(&repl, s, caps, out);
                })?;
                Ok(Value::Str(out))
            }
            Bi::Match => self.bi_match(args).await,
            Bi::Sprintf => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(a).await?);
                }
                let fmt = self.to_str(&vals[0]);
                match sprintf(&fmt, &vals[1..], &self.convfmt) {
                    Ok(s) => {
                        self.string_fits(s.len())?;
                        Ok(Value::Str(s))
                    }
                    Err(e) if e.located => Err(self.fatal_at(&e.msg)),
                    Err(e) => Err(self.fatal(&e.msg)),
                }
            }
            Bi::Sin => {
                let x = self.arg_num(args, 0).await?;
                num(fix_nan(x.sin(), &[x]))
            }
            Bi::Cos => {
                let x = self.arg_num(args, 0).await?;
                num(fix_nan(x.cos(), &[x]))
            }
            Bi::Atan2 => {
                let y = self.arg_num(args, 0).await?;
                let x = self.arg_num(args, 1).await?;
                num(y.atan2(x))
            }
            Bi::Exp => {
                let x = self.arg_num(args, 0).await?;
                num(x.exp())
            }
            Bi::Log => {
                let x = self.arg_num(args, 0).await?;
                num(fix_nan(x.ln(), &[x]))
            }
            Bi::Sqrt => {
                let x = self.arg_num(args, 0).await?;
                num(fix_nan(x.sqrt(), &[x]))
            }
            Bi::Int => {
                let x = self.arg_num(args, 0).await?;
                num(x.trunc())
            }
            Bi::Rand => num(self.rand.rand()),
            Bi::Srand => {
                let prev = self.rand.seed;
                let seed = if args.is_empty() {
                    self.io.host.clock.now_epoch().0 as f64
                } else {
                    self.arg_num(args, 0).await?
                };
                self.rand.seed = seed;
                self.rand.srandom(seed as i64 as u32);
                num(prev)
            }
            Bi::Tolower | Bi::Toupper => {
                let s = self.arg_str(args, 0).await?;
                let r = if bi == Bi::Tolower {
                    s.to_lowercase()
                } else {
                    s.to_uppercase()
                };
                self.string_fits(r.len())?;
                Ok(Value::Str(r))
            }
            Bi::System => {
                let cmd = self.arg_str(args, 0).await?;
                let r = self.run_command(false, &cmd, None).await?;
                num(r.exit_code as f64)
            }
            Bi::Close => {
                let name = self.arg_str(args, 0).await?;
                let r = self.close_stream(&name).await?;
                num(r)
            }
            Bi::Fflush => {
                self.flush_all().await?;
                num(0.0)
            }
            Bi::Strtonum => {
                let v = self.arg_val(args, 0).await?;
                num(match v {
                    Value::Num(n) => n,
                    other => strtonum(&self.to_str(&other)),
                })
            }
            Bi::Systime => num(self.io.host.clock.now_epoch().0 as f64),
            Bi::Strftime => self.bi_strftime(args).await,
            Bi::Mktime => {
                let spec = self.arg_str(args, 0).await?;
                let utc = if args.len() > 1 {
                    self.arg_val(args, 1).await?.to_bool()
                } else {
                    false
                };
                num(self.mktime(&spec, utc))
            }
            Bi::Asort | Bi::Asorti => self.bi_asort(bi, args).await,
            Bi::Typeof => {
                let t = self.type_of(&args[0]).await?;
                Ok(Value::Str(t.to_string()))
            }
            Bi::Isarray => {
                let t = self.type_of(&args[0]).await?;
                Ok(bool_val(t == "array"))
            }
            Bi::And | Bi::Or | Bi::Xor => {
                let mut acc = to_bits(self.arg_num(args, 0).await?);
                for i in 1..args.len() {
                    let v = to_bits(self.arg_num(args, i).await?);
                    acc = match bi {
                        Bi::And => acc & v,
                        Bi::Or => acc | v,
                        _ => acc ^ v,
                    };
                }
                num(acc as f64)
            }
            Bi::Lshift | Bi::Rshift => {
                let v = to_bits(self.arg_num(args, 0).await?);
                let n = to_bits(self.arg_num(args, 1).await?).min(63) as u32;
                num(if bi == Bi::Lshift {
                    v.wrapping_shl(n)
                } else {
                    v >> n
                } as f64)
            }
            Bi::Compl => {
                let v = to_bits(self.arg_num(args, 0).await?);
                num((!v & ((1u64 << 53) - 1)) as f64)
            }
        }
    }

    async fn bi_length(&mut self, args: &[Expr]) -> R<Value> {
        let n = match args.first() {
            None => {
                let v = self.get_field(0)?;
                self.to_str(&v).chars().count()
            }
            Some(Expr::Var(v)) if *v != VarRef::Global(special::NF) => {
                match &self.globals_or_local(*v) {
                    Some(Cell::Arr(id)) => self.array(*id).map_or(0, |a| a.len()),
                    _ => {
                        let v = self.get_var(*v)?;
                        self.to_str(&v).chars().count()
                    }
                }
            }
            Some(e @ Expr::Elem(..)) => {
                let loc = self.resolve(e).await?;
                if let Loc::Elem(id, key) = &loc
                    && let Some(Elem::Arr(sub)) = self.array(*id).and_then(|a| a.get(key))
                {
                    self.array(*sub).map_or(0, |a| a.len())
                } else {
                    let v = self.load(&loc)?;
                    self.to_str(&v).chars().count()
                }
            }
            Some(e) => {
                let v = self.eval(e).await?;
                self.to_str(&v).chars().count()
            }
        };
        Ok(Value::Num(n as f64))
    }

    fn globals_or_local(&self, v: VarRef) -> Option<Cell> {
        // Small helper so `length(arr)` can look without converting.
        Some(self.cell(v).clone())
    }

    async fn bi_split(&mut self, bi: Bi, args: &[Expr]) -> R<Value> {
        let s = self.arg_str(args, 0).await?;
        let splitter = match args.get(2) {
            None if bi == Bi::Patsplit => match &self.splitter {
                Splitter::Fpat(re) => Splitter::Fpat(re.clone()),
                _ => {
                    let fpat = match &self.globals[special::FPAT as usize] {
                        Cell::Val(v) => self.to_str(v),
                        Cell::Arr(_) => String::new(),
                    };
                    Splitter::Fpat(self.dyn_regex(&fpat)?)
                }
            },
            None => self.splitter.clone(),
            Some(e) => {
                let re_lit = matches!(e, Expr::Regex(_));
                if bi == Bi::Patsplit {
                    Splitter::Fpat(self.regex_operand(e).await?)
                } else if re_lit {
                    Splitter::Regex(self.regex_operand(e).await?)
                } else {
                    let sep = self.arg_str(args, 2).await?;
                    self.classify_sep(&sep)?
                }
            }
        };
        let mut seps = Vec::new();
        let max = self.split_budget();
        let parts =
            split_text(&splitter, &s, Some(&mut seps), max).map_err(|e| self.too_many(e))?;
        let n = parts.len();
        let arr = self.array_of_expr(&args[1]).await?;
        self.fill_array(arr, parts.into_iter().map(Value::from_input).collect())?;
        if let Some(sa) = args.get(3) {
            let sid = self.array_of_expr(sa).await?;
            // Default splitting: seps[0] is the leading blank run, and the
            // leading and trailing entries exist only when not empty.
            let (first, mut seps) = if matches!(splitter, Splitter::Space) {
                (0, seps)
            } else {
                (1, seps)
            };
            let lead_empty = first == 0 && seps.first().is_some_and(String::is_empty);
            if first == 0 && seps.len() > n && seps.last().is_some_and(String::is_empty) {
                seps.pop();
            }
            self.fill_array_from(sid, first, seps.into_iter().map(Value::Str).collect())?;
            if lead_empty {
                self.elem_delete(sid, "0");
            }
        }
        Ok(Value::Num(n as f64))
    }

    async fn bi_match(&mut self, args: &[Expr]) -> R<Value> {
        let s = self.arg_str(args, 0).await?;
        let re = self.regex_operand(&args[1]).await?;
        let caps = if args.len() > 2 {
            re.captures_at(&s, 0)
        } else {
            re.find_at(&s, 0).map(|m| vec![Some(m)])
        };
        let (rstart, rlength) = match &caps {
            Some(c) => {
                let (a, b) = c[0].unwrap_or((0, 0));
                (
                    (s[..a].chars().count() + 1) as f64,
                    s[a..b].chars().count() as f64,
                )
            }
            None => (0.0, -1.0),
        };
        self.set_var(VarRef::Global(special::RSTART), Value::Num(rstart))?;
        self.set_var(VarRef::Global(special::RLENGTH), Value::Num(rlength))?;
        if let Some(arr_e) = args.get(2) {
            let arr = self.array_of_expr(arr_e).await?;
            self.clear_array(arr);
            if let Some(c) = caps {
                let subsep = self.subsep.clone();
                for (i, g) in c.iter().enumerate() {
                    if let Some((a, b)) = g {
                        let text = s[*a..*b].to_string();
                        self.elem_store(arr, i.to_string(), Elem::Val(Value::from_input(text)));
                        let start = (s[..*a].chars().count() + 1) as f64;
                        let len = s[*a..*b].chars().count() as f64;
                        self.elem_store(
                            arr,
                            format!("{i}{subsep}start"),
                            Elem::Val(Value::Num(start)),
                        );
                        self.elem_store(
                            arr,
                            format!("{i}{subsep}length"),
                            Elem::Val(Value::Num(len)),
                        );
                    }
                }
            }
        }
        Ok(Value::Num(rstart))
    }

    async fn bi_strftime(&mut self, args: &[Expr]) -> R<Value> {
        let fmt = if args.is_empty() {
            self.procinfo_str("strftime")
                .unwrap_or_else(|| "%a %b %e %H:%M:%S %Z %Y".to_string())
        } else {
            self.arg_str(args, 0).await?
        };
        let ts = if args.len() > 1 {
            Some(self.arg_num(args, 1).await? as i64)
        } else {
            None
        };
        let utc = if args.len() > 2 {
            self.arg_val(args, 2).await?.to_bool()
        } else {
            false
        };
        let (tz, fmt) = if utc {
            // gmtime() names its zone "GMT".
            (Some("UTC".to_string()), utc_zone_name(&fmt))
        } else {
            (self.env_var("TZ"), fmt)
        };
        let clock = self.io.host.clock;
        let out = clock.strftime(tz.as_ref(), ts, &fmt).unwrap_or_default();
        self.string_fits(out.len())?;
        Ok(Value::Str(out))
    }

    fn mktime(&self, spec: &str, utc: bool) -> f64 {
        let parts: Vec<i64> = spec
            .split_whitespace()
            .map_while(|p| p.parse::<i64>().ok())
            .collect();
        if parts.len() < 6 || spec.split_whitespace().count() > 7 {
            return -1.0;
        }
        let tz = if utc {
            Some("UTC".to_string())
        } else {
            self.env_var("TZ")
        };
        let clock = self.io.host.clock;
        clock
            .mktime(
                tz.as_ref(),
                [parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]],
            )
            .map_or(-1.0, |t| t as f64)
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.io
            .host
            .env
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    fn procinfo_str(&self, key: &str) -> Option<String> {
        let Cell::Arr(id) = self.globals[special::PROCINFO as usize] else {
            return None;
        };
        match self.array(id)?.get(key)? {
            Elem::Val(v) => Some(self.to_str(v)),
            Elem::Arr(_) => None,
        }
    }

    async fn bi_asort(&mut self, bi: Bi, args: &[Expr]) -> R<Value> {
        let src = self.array_of_expr(&args[0]).await?;
        let how = if args.len() > 2 {
            self.arg_str(args, 2).await?
        } else {
            String::new()
        };
        let mut items: Vec<Value> = match self.array(src) {
            Some(a) => a
                .iter()
                .map(|(k, e)| {
                    if bi == Bi::Asorti {
                        Value::Str(k.clone())
                    } else {
                        match e {
                            Elem::Val(v) => v.clone(),
                            Elem::Arr(_) => Value::Uninit,
                        }
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        let convfmt = self.convfmt.clone();
        let desc = how.ends_with("_desc");
        let numeric_keys = how.contains("_num_");
        items.sort_by(|a, b| {
            let o = if bi == Bi::Asorti && !numeric_keys {
                a.to_str_fmt(&convfmt).cmp(&b.to_str_fmt(&convfmt))
            } else if bi == Bi::Asorti {
                str_to_num(&a.to_str_fmt(&convfmt)).total_cmp(&str_to_num(&b.to_str_fmt(&convfmt)))
            } else if how.contains("_str_") {
                a.to_str_fmt(&convfmt).cmp(&b.to_str_fmt(&convfmt))
            } else {
                // Default: numbers before strings, each in its own order.
                match (a.is_numeric(), b.is_numeric()) {
                    (true, true) => a.to_num().total_cmp(&b.to_num()),
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (false, false) => a.to_str_fmt(&convfmt).cmp(&b.to_str_fmt(&convfmt)),
                }
            };
            if desc { o.reverse() } else { o }
        });
        let n = items.len();
        let dest = match args.get(1) {
            Some(d) => self.array_of_expr(d).await?,
            None => src,
        };
        self.fill_array(dest, items)?;
        Ok(Value::Num(n as f64))
    }

    async fn type_of(&mut self, e: &Expr) -> R<&'static str> {
        Ok(match e {
            Expr::Var(v) if *v == VarRef::Global(special::NF) => "number",
            Expr::Var(v) => match self.globals_or_local(*v) {
                Some(Cell::Arr(_)) => "array",
                Some(Cell::Val(x)) => x.type_name(),
                None => "untyped",
            },
            Expr::Elem(v, groups) => {
                let id = match self.globals_or_local(*v) {
                    Some(Cell::Arr(id)) => id,
                    _ => return Ok("untyped"),
                };
                let mut id = id;
                let (last, path) = groups.split_last().ok_or(super::interp::Unwind::Fatal)?;
                for g in path {
                    let k = self.subscript(g).await?;
                    match self.array(id).and_then(|a| a.get(&k)) {
                        Some(Elem::Arr(sub)) => id = *sub,
                        _ => return Ok("untyped"),
                    }
                }
                let key = self.subscript(last).await?;
                match self.array(id).and_then(|a| a.get(&key)) {
                    Some(Elem::Arr(_)) => "array",
                    Some(Elem::Val(Value::Uninit)) => "untyped",
                    Some(Elem::Val(x)) => x.type_name(),
                    None => "untyped",
                }
            }
            Expr::Regex(_) => "regexp",
            other => {
                let v = self.eval(other).await?;
                v.type_name()
            }
        })
    }

    /// Core of sub/gsub/gensub with gawk's empty-match rules and the
    /// string cap checked as the result grows.
    fn substitute(
        &mut self,
        re: &Arc<AwkRegex>,
        s: &str,
        which: Which,
        need_caps: bool,
        mut rep: impl FnMut(&str, &[Option<(usize, usize)>], &mut String),
    ) -> R<(String, usize)> {
        let mut out = String::new();
        let mut pos = 0;
        let mut copied = 0;
        let mut count = 0;
        let mut replaced = 0;
        let mut last_end: Option<usize> = None;
        while pos <= s.len() {
            let caps = if need_caps {
                re.captures_at(s, pos)
            } else {
                re.find_at(s, pos).map(|m| vec![Some(m)])
            };
            let Some(caps) = caps else { break };
            let Some((ms, me)) = caps[0] else { break };
            if ms == me && last_end == Some(ms) {
                pos = next_char(s, ms);
                continue;
            }
            count += 1;
            let hit = match which {
                Which::All => true,
                Which::Nth(n) => count == n,
            };
            if hit {
                out.push_str(&s[copied..ms]);
                rep(s, &caps, &mut out);
                copied = me;
                replaced += 1;
                self.string_fits(out.len())?;
            }
            if ms == me {
                pos = next_char(s, ms);
            } else {
                pos = me;
                last_end = Some(me);
            }
            if let Which::Nth(n) = which
                && count >= n
            {
                break;
            }
        }
        if copied < s.len() {
            self.string_fits(out.len() + s.len() - copied)?;
            out.push_str(&s[copied..]);
        }
        Ok((out, replaced))
    }
}

fn is_lvalue(e: &Expr) -> bool {
    match e {
        Expr::Group(inner) => is_lvalue(inner),
        other => other.is_lvalue(),
    }
}

/// `substr(s, m[, n])` with POSIX rounding, counting characters.
/// Replace `%Z` with `GMT` in a strftime format (`%%` stays as is).
fn utc_zone_name(fmt: &str) -> String {
    let mut out = String::with_capacity(fmt.len());
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Z') => out.push_str("GMT"),
            Some(n) => {
                out.push('%');
                out.push(n);
            }
            None => out.push('%'),
        }
    }
    out
}

/// gawk's `substr`: a start below 1 counts from 1 without shortening
/// the length; fractional start and length are truncated; a length below
/// 1 (or NaN) gives "".
fn substr(s: &str, m: f64, n: Option<f64>) -> String {
    let length = match n {
        Some(n) if n.is_nan() || n < 1.0 => return String::new(),
        Some(n) => Some(if n < usize::MAX as f64 {
            n as usize
        } else {
            usize::MAX
        }),
        None => None,
    };
    let m = if m >= 1.0 { m } else { 1.0 };
    let indx = if m <= usize::MAX as f64 {
        (m - 1.0) as usize
    } else {
        usize::MAX
    };
    let len = s.chars().count();
    if indx >= len {
        return String::new();
    }
    let length = length.unwrap_or(len - indx).min(len - indx);
    s.chars().skip(indx).take(length).collect()
}

/// POSIX `sub`/`gsub` replacement text.
fn expand_sub(repl: &str, matched: &str, out: &mut String) {
    let mut chars = repl.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                }
                Some('&') => {
                    chars.next();
                    out.push('&');
                }
                _ => out.push('\\'),
            },
            '&' => out.push_str(matched),
            c => out.push(c),
        }
    }
}

/// `gensub` replacement: `&`/`\0` whole match, `\1`..`\9` groups.
fn expand_gensub(repl: &str, s: &str, caps: &[Option<(usize, usize)>], out: &mut String) {
    let group = |i: usize| -> &str { caps.get(i).copied().flatten().map_or("", |(a, b)| &s[a..b]) };
    let mut chars = repl.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek().copied() {
                Some(d @ '0'..='9') => {
                    chars.next();
                    out.push_str(group(d as usize - '0' as usize));
                }
                Some('&') => {
                    chars.next();
                    out.push('&');
                }
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                }
                _ => out.push('\\'),
            },
            '&' => out.push_str(group(0)),
            c => out.push(c),
        }
    }
}

/// gawk `strtonum`: `0x` hex, leading-zero octal, else decimal.
fn strtonum(s: &str) -> f64 {
    let t = s.trim_start();
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let v = if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        let digits: String = h.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        u64::from_str_radix(&digits, 16).map_or(0.0, |v| v as f64)
    } else if body.starts_with('0')
        && body.len() > 1
        && body[1..].starts_with(|c: char| c.is_ascii_digit())
    {
        let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.bytes().all(|b| b < b'8') {
            u64::from_str_radix(&digits, 8).map_or(0.0, |v| v as f64)
        } else {
            str_to_num(body)
        }
    } else {
        return str_to_num(s);
    };
    if neg { -v } else { v }
}

fn to_bits(v: f64) -> u64 {
    if v < 0.0 { (v as i64) as u64 } else { v as u64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substr_rounding() {
        assert_eq!(substr("hello", 2.0, Some(3.0)), "ell");
        // gawk 5.2: start clamps to 1, length kept; fractions truncate.
        assert_eq!(substr("hello", 0.0, Some(2.0)), "he");
        assert_eq!(substr("hello", -1.0, None), "hello");
        assert_eq!(substr("hello", 1.5, None), "hello");
        assert_eq!(substr("hello world", 1.5, Some(2.3)), "he");
        assert_eq!(substr("hello", 2.0, Some(0.5)), "");
        assert_eq!(substr("olá mundo", 1.0, Some(3.0)), "olá");
        assert_eq!(substr("abc", 5.0, None), "");
    }

    #[test]
    fn replacement_rules() {
        let mut out = String::new();
        expand_sub("<&>", "x", &mut out);
        assert_eq!(out, "<x>");
        out.clear();
        expand_sub("\\&", "x", &mut out);
        assert_eq!(out, "&");
        out.clear();
        expand_sub("\\\\&", "x", &mut out);
        assert_eq!(out, "\\x");
    }

    #[test]
    fn strtonum_forms() {
        assert_eq!(strtonum("0x1A"), 26.0);
        assert_eq!(strtonum("017"), 15.0);
        assert_eq!(strtonum("12abc"), 12.0);
        assert_eq!(strtonum("-0x10"), -16.0);
    }

    #[test]
    fn rand_matches_gawk_first_value() {
        let mut r = Rand::new();
        let v = r.rand();
        assert!((0.0..1.0).contains(&v));
    }
}
