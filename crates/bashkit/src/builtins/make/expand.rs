//! Make variable and function expansion.
//!
//! Expansion is synchronous. Anything that needs the outside world
//! (`$(shell)`, `$(wildcard)`, `include`) is answered from an [`Oracle`]
//! cache; a miss returns [`Stop::Need`], the driver answers it (running a
//! command or touching the VFS) and expansion starts over. Results are
//! cached, so each query runs once and a restart is deterministic.

use std::collections::{HashMap, HashSet};

/// Expansion nesting cap (recursive variables, nested calls).
const MAX_DEPTH: usize = 200;
/// Cap on the text a single top-level expansion may produce.
pub(super) const MAX_EXPANSION_BYTES: usize = 4 * 1024 * 1024;

/// A query the expander cannot answer by itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Need {
    /// Run `sh -c CMD` and capture stdout.
    Shell(String),
    /// Expand a glob (or test a plain path for existence).
    Glob(String),
    /// Read a makefile (`include`).
    Read(String),
}

/// Why expansion or parsing stopped.
#[derive(Debug, Clone)]
pub(super) enum Stop {
    Need(Need),
    /// Fatal error: `file:line: *** msg.  Stop.` (or `make: *** ...` with no
    /// location).
    Fatal {
        loc: Option<(String, usize)>,
        msg: String,
    },
}

pub(super) type R<T> = std::result::Result<T, Stop>;

pub(super) fn fatal<T>(loc: Option<(String, usize)>, msg: impl Into<String>) -> R<T> {
    Err(Stop::Fatal {
        loc,
        msg: msg.into(),
    })
}

/// Answers to [`Need`] queries gathered so far.
#[derive(Default)]
pub(super) struct Oracle {
    pub shell: HashMap<String, String>,
    pub glob: HashMap<String, Vec<String>>,
    pub files: HashMap<String, Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Default,
    Environment,
    File,
    CommandLine,
    Override,
}

#[derive(Debug, Clone)]
pub(super) struct Var {
    pub value: String,
    pub recursive: bool,
    pub origin: Origin,
}

pub(super) type Vars = HashMap<String, Var>;

/// Output produced by `$(info)` / `$(warning)` during an expansion.
#[derive(Debug, Clone)]
pub(super) enum Message {
    Out(String),
    Err(String),
}

pub(super) struct Expander<'a> {
    vars: &'a Vars,
    target_vars: Option<&'a Vars>,
    oracle: &'a Oracle,
    /// Automatic variables, `$(call)` arguments, `$(foreach)` variables.
    locals: Vec<HashMap<String, String>>,
    active: HashSet<String>,
    depth: usize,
    produced: usize,
    loc: Option<(String, usize)>,
    curdir: &'a str,
    pub messages: Vec<Message>,
}

const FUNCTIONS: &[&str] = &[
    "subst",
    "patsubst",
    "strip",
    "findstring",
    "filter",
    "filter-out",
    "sort",
    "word",
    "wordlist",
    "words",
    "firstword",
    "lastword",
    "dir",
    "notdir",
    "suffix",
    "basename",
    "addsuffix",
    "addprefix",
    "join",
    "wildcard",
    "realpath",
    "abspath",
    "if",
    "or",
    "and",
    "foreach",
    "call",
    "value",
    "origin",
    "flavor",
    "error",
    "warning",
    "info",
    "shell",
    "eval",
    "file",
];

impl<'a> Expander<'a> {
    pub(super) fn new(
        vars: &'a Vars,
        oracle: &'a Oracle,
        curdir: &'a str,
        loc: Option<(String, usize)>,
    ) -> Self {
        Self {
            vars,
            target_vars: None,
            oracle,
            locals: Vec::new(),
            active: HashSet::new(),
            depth: 0,
            produced: 0,
            loc,
            curdir,
            messages: Vec::new(),
        }
    }

    pub(super) fn with_target_vars(mut self, tv: Option<&'a Vars>) -> Self {
        self.target_vars = tv;
        self
    }

    pub(super) fn with_locals(mut self, locals: HashMap<String, String>) -> Self {
        self.locals.push(locals);
        self
    }

    fn fatal<T>(&self, msg: impl Into<String>) -> R<T> {
        fatal(self.loc.clone(), msg)
    }

    fn charge(&mut self, n: usize) -> R<()> {
        self.produced = self.produced.saturating_add(n);
        if self.produced > MAX_EXPANSION_BYTES {
            return self.fatal("expansion exceeds 4 MiB");
        }
        Ok(())
    }

    /// Expand `$` references in `s`.
    pub(super) fn expand(&mut self, s: &str) -> R<String> {
        if !s.contains('$') {
            return Ok(s.to_string());
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fatal("variable expansion nested too deeply");
        }
        let mut out = String::new();
        let mut i = 0;
        while i < s.len() {
            let Some(off) = s[i..].find('$') else {
                out.push_str(&s[i..]);
                break;
            };
            out.push_str(&s[i..i + off]);
            i += off + 1;
            let Some(c) = s[i..].chars().next() else {
                break;
            };
            match c {
                '$' => {
                    out.push('$');
                    i += 1;
                }
                '(' | '{' => {
                    let close = if c == '(' { ')' } else { '}' };
                    let Some(end) = find_close(s, i + 1, c, close) else {
                        return self.fatal("unterminated variable reference");
                    };
                    let inner = &s[i + 1..end];
                    let v = self.reference(inner)?;
                    self.charge(v.len())?;
                    out.push_str(&v);
                    i = end + 1;
                }
                _ => {
                    let v = self.lookup(&c.to_string())?;
                    self.charge(v.len())?;
                    out.push_str(&v);
                    i += c.len_utf8();
                }
            }
        }
        self.depth -= 1;
        Ok(out)
    }

    fn reference(&mut self, inner: &str) -> R<String> {
        // Function call: `$(name args)`.
        if let Some(ws) = inner.find([' ', '\t']) {
            let name = &inner[..ws];
            if FUNCTIONS.contains(&name) {
                let rest = inner[ws..].trim_start_matches([' ', '\t']);
                return self.function(name, rest);
            }
        }
        // Substitution reference: `$(VAR:a=b)`.
        if let Some(colon) = top_level_find(inner, ':')
            && let Some(eq) = top_level_find(&inner[colon + 1..], '=')
        {
            let name = self.expand(&inner[..colon])?;
            let from = self.expand(&inner[colon + 1..colon + 1 + eq])?;
            let to = self.expand(&inner[colon + 2 + eq..])?;
            let value = self.lookup(name.trim())?;
            let (from, to) = if from.contains('%') {
                (from, to)
            } else {
                (format!("%{from}"), format!("%{to}"))
            };
            return Ok(words(&value)
                .map(|w| patsubst_word(&from, &to, w))
                .collect::<Vec<_>>()
                .join(" "));
        }
        let name = self.expand(inner)?;
        self.lookup(&name)
    }

    /// Value of variable `name`, expanded if recursive.
    pub(super) fn lookup(&mut self, name: &str) -> R<String> {
        for scope in self.locals.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Ok(v.clone());
            }
        }
        // `$(@D)`, `$(<F)`, ...
        let mut chars = name.chars();
        if let (Some(a), Some(b), None) = (chars.next(), chars.next(), chars.next())
            && "@<^+*?%|".contains(a)
            && (b == 'D' || b == 'F')
        {
            let base = self.lookup(&a.to_string())?;
            return Ok(words(&base)
                .map(|w| if b == 'D' { dir_of(w) } else { notdir(w) })
                .collect::<Vec<_>>()
                .join(" "));
        }
        let var = self
            .target_vars
            .and_then(|tv| tv.get(name))
            .or_else(|| self.vars.get(name));
        let Some(var) = var else {
            return Ok(String::new());
        };
        if !var.recursive {
            return Ok(var.value.clone());
        }
        if !self.active.insert(name.to_string()) {
            return self.fatal(format!(
                "Recursive variable '{name}' references itself (eventually)"
            ));
        }
        let value = var.value.clone();
        let r = self.expand(&value);
        self.active.remove(name);
        r
    }

    fn function(&mut self, name: &str, rest: &str) -> R<String> {
        match name {
            "subst" => {
                let a = self.args(rest, 3)?;
                let [from, to, text] = three(a);
                if from.is_empty() {
                    return Ok(text);
                }
                Ok(text.replace(&from, &to))
            }
            "patsubst" => {
                let [pat, rep, text] = three(self.args(rest, 3)?);
                Ok(words(&text)
                    .map(|w| patsubst_word(&pat, &rep, w))
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            "strip" => Ok(words(&self.expand(rest)?).collect::<Vec<_>>().join(" ")),
            "findstring" => {
                let [find, text, _] = three(self.args(rest, 2)?);
                Ok(if text.contains(&find) {
                    find
                } else {
                    String::new()
                })
            }
            "filter" | "filter-out" => {
                let [pats, text, _] = three(self.args(rest, 2)?);
                let pats: Vec<&str> = words(&pats).collect();
                let keep = name == "filter";
                Ok(words(&text)
                    .filter(|w| pats.iter().any(|p| pattern_match(p, w).is_some()) == keep)
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            "sort" => {
                let text = self.expand(rest)?;
                let mut v: Vec<&str> = words(&text).collect();
                v.sort_unstable();
                v.dedup();
                Ok(v.join(" "))
            }
            "word" => {
                let [n, text, _] = three(self.args(rest, 2)?);
                let n = self.number("word", &n)?;
                if n == 0 {
                    return self.fatal("first argument to 'word' function must be greater than 0");
                }
                Ok(words(&text).nth(n - 1).unwrap_or("").to_string())
            }
            "wordlist" => {
                let [s, e, text] = three(self.args(rest, 3)?);
                let s = self.number("wordlist", &s)?;
                let e = self.number("wordlist", &e)?;
                if s == 0 {
                    return self.fatal("invalid first argument to 'wordlist' function: '0'");
                }
                Ok(words(&text)
                    .skip(s - 1)
                    .take((e + 1).saturating_sub(s))
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            "words" => Ok(words(&self.expand(rest)?).count().to_string()),
            "firstword" => Ok(words(&self.expand(rest)?).next().unwrap_or("").to_string()),
            "lastword" => Ok(words(&self.expand(rest)?).last().unwrap_or("").to_string()),
            "dir" | "notdir" | "suffix" | "basename" | "abspath" => {
                let text = self.expand(rest)?;
                Ok(words(&text)
                    .filter_map(|w| match name {
                        "dir" => Some(dir_with_slash(w)),
                        "notdir" => Some(notdir(w)),
                        "suffix" => suffix(w).map(str::to_string),
                        "basename" => Some(basename(w).to_string()),
                        _ => Some(abspath(self.curdir, w)),
                    })
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            "addsuffix" | "addprefix" => {
                let [fix, text, _] = three(self.args(rest, 2)?);
                Ok(words(&text)
                    .map(|w| {
                        if name == "addsuffix" {
                            format!("{w}{fix}")
                        } else {
                            format!("{fix}{w}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            "join" => {
                let [a, b, _] = three(self.args(rest, 2)?);
                let a: Vec<&str> = words(&a).collect();
                let b: Vec<&str> = words(&b).collect();
                Ok((0..a.len().max(b.len()))
                    .map(|i| format!("{}{}", a.get(i).unwrap_or(&""), b.get(i).unwrap_or(&"")))
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            "wildcard" | "realpath" => {
                let text = self.expand(rest)?;
                let mut out = Vec::new();
                for w in words(&text) {
                    let q = if name == "realpath" {
                        abspath(self.curdir, w)
                    } else {
                        w.to_string()
                    };
                    match self.oracle.glob.get(&q) {
                        Some(found) => out.extend(found.iter().cloned()),
                        None => return Err(Stop::Need(Need::Glob(q))),
                    }
                }
                Ok(out.join(" "))
            }
            "if" => {
                let parts = split_args(rest, 3);
                let cond = self.expand(parts.first().copied().unwrap_or(""))?;
                if !cond.trim().is_empty() {
                    self.expand(parts.get(1).copied().unwrap_or(""))
                } else {
                    self.expand(parts.get(2).copied().unwrap_or(""))
                }
            }
            "or" => {
                for p in split_args(rest, usize::MAX) {
                    let v = self.expand(p)?;
                    if !v.trim().is_empty() {
                        return Ok(v.trim().to_string());
                    }
                }
                Ok(String::new())
            }
            "and" => {
                let mut last = String::new();
                for p in split_args(rest, usize::MAX) {
                    last = self.expand(p)?;
                    if last.trim().is_empty() {
                        return Ok(String::new());
                    }
                }
                Ok(last.trim().to_string())
            }
            "foreach" => {
                let parts = split_args(rest, 3);
                if parts.len() < 3 {
                    return self
                        .fatal("insufficient number of arguments (2) to function 'foreach'");
                }
                let var = self.expand(parts[0])?.trim().to_string();
                let list = self.expand(parts[1])?;
                let mut out = Vec::new();
                for w in words(&list) {
                    self.locals
                        .push(HashMap::from([(var.clone(), w.to_string())]));
                    let r = self.expand(parts[2]);
                    self.locals.pop();
                    out.push(r?);
                }
                Ok(out.join(" "))
            }
            "call" => {
                let parts = split_args(rest, usize::MAX);
                let fname = self.expand(parts.first().copied().unwrap_or(""))?;
                let fname = fname.trim().to_string();
                let mut scope = HashMap::from([("0".to_string(), fname.clone())]);
                for (i, p) in parts.iter().enumerate().skip(1) {
                    scope.insert(i.to_string(), self.expand(p)?);
                }
                // Unset higher-numbered arguments of an enclosing call.
                for i in parts.len().max(1)..10 {
                    scope.insert(i.to_string(), String::new());
                }
                let body = match self
                    .target_vars
                    .and_then(|tv| tv.get(&fname))
                    .or_else(|| self.vars.get(&fname))
                {
                    Some(v) => v.value.clone(),
                    None => return Ok(String::new()),
                };
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return self.fatal("variable expansion nested too deeply");
                }
                self.locals.push(scope);
                let r = self.expand(&body);
                self.locals.pop();
                self.depth -= 1;
                r
            }
            "value" => {
                let n = self.expand(rest)?;
                Ok(self
                    .vars
                    .get(n.trim())
                    .map(|v| v.value.clone())
                    .unwrap_or_default())
            }
            "origin" | "flavor" => {
                let n = self.expand(rest)?;
                let n = n.trim();
                let local = self.locals.iter().any(|s| s.contains_key(n));
                let v = self
                    .target_vars
                    .and_then(|tv| tv.get(n))
                    .or_else(|| self.vars.get(n));
                Ok(match (name, v) {
                    ("origin", _) if local => "automatic",
                    ("origin", None) | ("flavor", None) if !local => "undefined",
                    ("flavor", None) => "simple",
                    ("origin", Some(v)) => match v.origin {
                        Origin::Default => "default",
                        Origin::Environment => "environment",
                        Origin::File => "file",
                        Origin::CommandLine => "command line",
                        Origin::Override => "override",
                    },
                    (_, Some(v)) if v.recursive => "recursive",
                    _ => "simple",
                }
                .to_string())
            }
            "error" => {
                let msg = self.expand(rest)?;
                self.fatal(msg)
            }
            "warning" => {
                let msg = self.expand(rest)?;
                let prefix = match &self.loc {
                    Some((f, l)) => format!("{f}:{l}: "),
                    None => String::new(),
                };
                self.messages.push(Message::Err(format!("{prefix}{msg}\n")));
                Ok(String::new())
            }
            "info" => {
                let msg = self.expand(rest)?;
                self.messages.push(Message::Out(format!("{msg}\n")));
                Ok(String::new())
            }
            "shell" => {
                let cmd = self.expand(rest)?;
                match self.oracle.shell.get(&cmd) {
                    Some(out) => Ok(out.clone()),
                    None => Err(Stop::Need(Need::Shell(cmd))),
                }
            }
            // WTF: `$(eval)` would let expansion define rules mid-parse;
            // tracked as L-MAKE-001 rather than half-supported.
            _ => self.fatal(format!("function '{name}' is not supported (L-MAKE-001)")),
        }
    }

    fn args(&mut self, rest: &str, n: usize) -> R<Vec<String>> {
        split_args(rest, n)
            .into_iter()
            .map(|a| self.expand(a))
            .collect()
    }

    fn number(&self, func: &str, s: &str) -> R<usize> {
        s.trim().parse().or_else(|_| {
            self.fatal(format!(
                "non-numeric first argument to '{func}' function: '{}'",
                s.trim()
            ))
        })
    }
}

fn three(mut v: Vec<String>) -> [String; 3] {
    v.resize(3, String::new());
    let c = v.pop().unwrap_or_default();
    let b = v.pop().unwrap_or_default();
    let a = v.pop().unwrap_or_default();
    [a, b, c]
}

/// Index of the `close` matching an already-consumed `open`, starting at
/// `start`.
pub(super) fn find_close(s: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s[start..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return Some(start + i);
            }
            depth -= 1;
        }
    }
    None
}

/// First `needle` outside `$(...)`/`${...}`.
pub(super) fn top_level_find(s: &str, needle: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        match c {
            '(' | '{' if prev == '$' || depth > 0 => depth += 1,
            ')' | '}' if depth > 0 => depth -= 1,
            _ if c == needle && depth == 0 => return Some(i),
            _ => {}
        }
        prev = c;
    }
    None
}

/// Split function arguments on top-level commas into at most `n` parts.
pub(super) fn split_args(s: &str, n: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '{' => depth += 1,
            ')' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 && out.len() + 1 < n => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

pub(super) fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split_ascii_whitespace()
}

/// Match `word` against a `%` pattern; returns the stem (or `""` for an
/// exact match of a pattern without `%`).
pub(super) fn pattern_match<'w>(pat: &str, word: &'w str) -> Option<&'w str> {
    match pat.find('%') {
        None => (pat == word).then_some(""),
        Some(p) => {
            let (pre, suf) = (&pat[..p], &pat[p + 1..]);
            (word.len() >= pre.len() + suf.len() && word.starts_with(pre) && word.ends_with(suf))
                .then(|| &word[pre.len()..word.len() - suf.len()])
        }
    }
}

pub(super) fn patsubst_word(pat: &str, rep: &str, word: &str) -> String {
    match pattern_match(pat, word) {
        Some(stem) if pat.contains('%') => rep.replacen('%', stem, 1),
        Some(_) => rep.to_string(),
        None => word.to_string(),
    }
}

fn dir_with_slash(w: &str) -> String {
    match w.rfind('/') {
        Some(i) => w[..=i].to_string(),
        None => "./".to_string(),
    }
}

/// `$(@D)`: directory without the trailing slash.
fn dir_of(w: &str) -> String {
    match w.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => w[..i].to_string(),
        None => ".".to_string(),
    }
}

pub(super) fn notdir(w: &str) -> String {
    w.rsplit('/').next().unwrap_or(w).to_string()
}

fn suffix(w: &str) -> Option<&str> {
    let base_start = w.rfind('/').map_or(0, |i| i + 1);
    w[base_start..].rfind('.').map(|i| &w[base_start + i..])
}

fn basename(w: &str) -> &str {
    match suffix(w) {
        Some(s) => &w[..w.len() - s.len()],
        None => w,
    }
}

/// Lexical absolute path (no symlink resolution).
pub(super) fn abspath(curdir: &str, w: &str) -> String {
    let joined = if w.starts_with('/') {
        w.to_string()
    } else {
        format!("{curdir}/{w}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for p in joined.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(v: &str, recursive: bool) -> Var {
        Var {
            value: v.to_string(),
            recursive,
            origin: Origin::File,
        }
    }

    fn exp(vars: &Vars, s: &str) -> String {
        let oracle = Oracle::default();
        let mut e = Expander::new(vars, &oracle, "/w", None);
        match e.expand(s) {
            Ok(v) => v,
            Err(Stop::Fatal { msg, .. }) => format!("FATAL {msg}"),
            Err(Stop::Need(n)) => format!("NEED {n:?}"), // debug-ok: test-only
        }
    }

    #[test]
    fn functions_match_gnu_make() {
        let mut v = Vars::new();
        v.insert("SRC".into(), var("a.c b.c sub/c.c", false));
        v.insert("OBJ".into(), var("$(SRC:.c=.o)", true));
        v.insert("f".into(), var("<$(1)|$(2)>", true));
        assert_eq!(exp(&v, "$(OBJ)"), "a.o b.o sub/c.o");
        assert_eq!(exp(&v, "$(SRC:%.c=obj/%.o)"), "obj/a.o obj/b.o obj/sub/c.o");
        assert_eq!(exp(&v, "$(patsubst %.c,%.h,$(SRC))"), "a.h b.h sub/c.h");
        assert_eq!(exp(&v, "$(notdir $(SRC))"), "a.c b.c c.c");
        assert_eq!(exp(&v, "$(dir $(SRC))"), "./ ./ sub/");
        assert_eq!(exp(&v, "$(basename $(SRC))"), "a b sub/c");
        assert_eq!(exp(&v, "$(suffix $(SRC))"), ".c .c .c");
        assert_eq!(exp(&v, "$(words $(SRC))"), "3");
        assert_eq!(exp(&v, "$(word 2,$(SRC))"), "b.c");
        assert_eq!(exp(&v, "$(wordlist 2,3,$(SRC))"), "b.c sub/c.c");
        assert_eq!(exp(&v, "$(filter %.c,x.h a.c)"), "a.c");
        assert_eq!(exp(&v, "$(filter-out a.c,$(SRC))"), "b.c sub/c.c");
        assert_eq!(exp(&v, "$(sort c b a b)"), "a b c");
        assert_eq!(exp(&v, "$(subst .c,.o,$(SRC))"), "a.o b.o sub/c.o");
        assert_eq!(exp(&v, "$(addprefix -I,x y)"), "-Ix -Iy");
        assert_eq!(exp(&v, "$(addsuffix .o,x y)"), "x.o y.o");
        assert_eq!(exp(&v, "$(join a b,1 2 3)"), "a1 b2 3");
        assert_eq!(exp(&v, "$(strip  a   b  )"), "a b");
        assert_eq!(exp(&v, "$(if ,yes,no)$(if x,yes,no)"), "noyes");
        assert_eq!(exp(&v, "$(or ,b,c)|$(and a,,c)|$(and a,b)"), "b||b");
        assert_eq!(exp(&v, "$(foreach x,1 2,<$(x)>)"), "<1> <2>");
        assert_eq!(exp(&v, "$(call f,a,b)"), "<a|b>");
        assert_eq!(
            exp(&v, "$(origin SRC) $(origin NOPE) $(flavor OBJ)"),
            "file undefined recursive"
        );
        assert_eq!(exp(&v, "$(abspath ../x/./y)"), "/x/y");
        assert_eq!(exp(&v, "$$HOME"), "$HOME");
        assert_eq!(exp(&v, "$(firstword a b) $(lastword a b)"), "a b");
        assert_eq!(exp(&v, "${SRC}"), "a.c b.c sub/c.c");
    }

    #[test]
    fn recursion_and_errors_are_fatal() {
        let mut v = Vars::new();
        v.insert("A".into(), var("$(B)", true));
        v.insert("B".into(), var("$(A)", true));
        assert_eq!(
            exp(&v, "$(A)"),
            "FATAL Recursive variable 'A' references itself (eventually)"
        );
        assert_eq!(exp(&v, "$(error boom)"), "FATAL boom");
        assert_eq!(
            exp(&v, "$(word 0,a)"),
            "FATAL first argument to 'word' function must be greater than 0"
        );
        assert!(exp(&v, "$(eval x=1)").starts_with("FATAL function 'eval'"));
        assert_eq!(
            exp(&v, "$(unterminated"),
            "FATAL unterminated variable reference"
        );
    }

    #[test]
    fn external_queries_suspend() {
        let v = Vars::new();
        assert_eq!(exp(&v, "$(shell echo hi)"), "NEED Shell(\"echo hi\")");
        assert_eq!(exp(&v, "$(wildcard *.c)"), "NEED Glob(\"*.c\")");
        let mut oracle = Oracle::default();
        oracle.shell.insert("echo hi".into(), "hi".into());
        let mut e = Expander::new(&v, &oracle, "/w", None);
        assert_eq!(e.expand("[$(shell echo hi)]").ok().as_deref(), Some("[hi]"));
    }

    #[test]
    fn expansion_bomb_is_capped() {
        let mut v = Vars::new();
        v.insert("X0".into(), var(&"x".repeat(1024), false));
        for i in 1..20 {
            v.insert(
                format!("X{i}"),
                var(&format!("$(X{0})$(X{0})", i - 1), true),
            );
        }
        assert_eq!(exp(&v, "$(X19)"), "FATAL expansion exceeds 4 MiB");
    }
}
