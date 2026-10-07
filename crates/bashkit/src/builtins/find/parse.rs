//! find command-line parser: starting points, global options, and the GNU
//! expression grammar.
//!
//! Precedence (high to low): `( )`, `!`/`-not`, `-a`/`-and`/juxtaposition,
//! `-o`/`-or`, `,`. Error texts follow GNU findutils 4.9 in the C locale.

use regex::{Regex, RegexBuilder};

/// Comparison sign of a numeric test argument (`+N`, `-N`, `N`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cmp {
    Lt,
    Eq,
    Gt,
}

impl Cmp {
    pub(super) fn test<T: PartialOrd>(self, value: T, n: T) -> bool {
        match self {
            Cmp::Lt => value < n,
            Cmp::Eq => value == n,
            Cmp::Gt => value > n,
        }
    }
}

/// Which timestamp a time test reads. The VFS keeps modification and
/// creation times; access and status-change times read the modification time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimeField {
    Modified,
    Created,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PermKind {
    Exact,
    All,
    Any,
}

/// Reference point of `-newer`-style tests, resolved before traversal.
#[derive(Debug)]
pub(super) enum NewerRef {
    File(String),
    /// `-newerXt`: a `date -d` string, parsed with the shared clock.
    Date(String),
    Nanos(i128),
}

#[derive(Debug)]
pub(super) enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Const(bool),
    Name {
        pat: String,
        nocase: bool,
    },
    Path {
        pat: String,
        nocase: bool,
    },
    Lname {
        pat: String,
        nocase: bool,
    },
    Regex(Regex),
    /// `-type` (`xtype == false`) or `-xtype` (`xtype == true`).
    Type {
        types: Vec<char>,
        xtype: bool,
    },
    Size {
        cmp: Cmp,
        n: u64,
        unit: u64,
    },
    Empty,
    /// `-mtime`-style (`minutes == false`, whole days, fraction ignored) or
    /// `-mmin`-style (`minutes == true`) age test.
    Age {
        field: TimeField,
        minutes: bool,
        cmp: Cmp,
        n: i64,
    },
    Newer {
        field: TimeField,
        reference: NewerRef,
    },
    Perm {
        mode: u32,
        kind: PermKind,
    },
    /// `-readable`/`-writable`/`-executable`: owner permission bit (4, 2, 1).
    Access(u32),
    Uid {
        cmp: Cmp,
        n: u64,
    },
    Gid {
        cmp: Cmp,
        n: u64,
    },
    Print {
        nul: bool,
    },
    Printf(String),
    Delete {
        id: usize,
    },
    Prune,
    Quit,
    Exec {
        id: usize,
        argv: Vec<String>,
        batch: bool,
        dir: bool,
    },
}

impl Expr {
    fn and(a: Expr, b: Expr) -> Expr {
        Expr::And(Box::new(a), Box::new(b))
    }

    /// Visit every node.
    pub(super) fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Expr)) {
        f(self);
        match self {
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Comma(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Expr::Not(a) => a.walk(f),
            _ => {}
        }
    }

    pub(super) fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Expr)) {
        f(self);
        match self {
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Comma(a, b) => {
                a.walk_mut(f);
                b.walk_mut(f);
            }
            Expr::Not(a) => a.walk_mut(f),
            _ => {}
        }
    }
}

/// Symlink handling selected by `-P` (default), `-H`, `-L`/`-follow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Follow {
    #[default]
    Never,
    Roots,
    Always,
}

#[derive(Debug, Default)]
pub(super) struct Options {
    pub max_depth: Option<usize>,
    pub min_depth: usize,
    pub depth_first: bool,
    pub follow: Follow,
    pub daystart: bool,
}

#[derive(Debug)]
pub(super) struct Parsed {
    pub paths: Vec<String>,
    pub expr: Expr,
    pub opts: Options,
    pub has_exec: bool,
}

/// Identity the ownership tests compare against (the single virtual user).
pub(super) struct Identity<'a> {
    pub username: &'a str,
    pub uid: u64,
    pub gid: u64,
}

#[derive(Clone, Copy)]
enum RegexType {
    Emacs,
    Basic,
    Extended,
}

struct Parser<'a> {
    toks: &'a [String],
    pos: usize,
    opts: Options,
    regex_type: RegexType,
    has_action: bool,
    has_exec: bool,
    next_id: usize,
    ident: &'a Identity<'a>,
}

type PResult<T> = std::result::Result<T, String>;

fn is_expr_start(arg: &str) -> bool {
    (arg.starts_with('-') && arg.len() > 1) || matches!(arg, "(" | ")" | "!" | ",")
}

/// Parse find's arguments. Errors are complete stderr lines.
pub(super) fn parse(args: &[String], ident: &Identity<'_>) -> PResult<Parsed> {
    let mut i = 0;
    let mut opts = Options::default();
    // Leading symlink/debug options.
    while i < args.len() {
        match args[i].as_str() {
            "-P" => opts.follow = Follow::Never,
            "-H" => opts.follow = Follow::Roots,
            "-L" => opts.follow = Follow::Always,
            "-D" => i += 1,
            a if a.starts_with("-O") && a[2..].chars().all(|c| c.is_ascii_digit()) => {}
            _ => break,
        }
        i += 1;
    }
    let mut paths = Vec::new();
    while i < args.len() && !is_expr_start(&args[i]) {
        paths.push(args[i].clone());
        i += 1;
    }
    if paths.is_empty() {
        paths.push(".".to_string());
    }
    let mut p = Parser {
        toks: &args[i..],
        pos: 0,
        opts,
        regex_type: RegexType::Emacs,
        has_action: false,
        has_exec: false,
        next_id: 0,
        ident,
    };
    let expr = if p.toks.is_empty() {
        None
    } else {
        let e = p.parse_comma()?;
        if let Some(tok) = p.peek() {
            return Err(if tok == ")" {
                "find: invalid expression; you have too many ')'\n".to_string()
            } else {
                format!("find: paths must precede expression: `{tok}'\n")
            });
        }
        Some(e)
    };
    let expr = match (expr, p.has_action) {
        (None, _) => Expr::Print { nul: false },
        (Some(e), true) => e,
        (Some(e), false) => Expr::and(e, Expr::Print { nul: false }),
    };
    Ok(Parsed {
        paths,
        expr,
        opts: p.opts,
        has_exec: p.has_exec,
    })
}

fn missing(pred: &str) -> String {
    format!("find: missing argument to `{pred}'\n")
}

fn invalid_arg(pred: &str, arg: &str) -> String {
    format!("find: invalid argument `{arg}' to `{pred}'\n")
}

/// Parse `[+-]N` into a comparison and the number text.
fn split_sign(arg: &str) -> (Cmp, &str) {
    if let Some(rest) = arg.strip_prefix('+') {
        (Cmp::Gt, rest)
    } else if let Some(rest) = arg.strip_prefix('-') {
        (Cmp::Lt, rest)
    } else {
        (Cmp::Eq, arg)
    }
}

fn parse_numeric<T: std::str::FromStr>(pred: &str, arg: &str) -> PResult<(Cmp, T)> {
    let (cmp, num) = split_sign(arg);
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid_arg(pred, arg));
    }
    num.parse()
        .map(|n| (cmp, n))
        .map_err(|_| invalid_arg(pred, arg))
}

fn parse_size(arg: &str) -> PResult<Expr> {
    let (cmp, rest) = split_sign(arg);
    let (num, unit) = match rest.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((idx, c)) => {
            if idx + c.len_utf8() != rest.len() {
                return Err(invalid_arg("-size", arg));
            }
            let unit = match c {
                'c' => 1,
                'w' => 2,
                'b' => 512,
                'k' => 1024,
                'M' => 1024 * 1024,
                'G' => 1024 * 1024 * 1024,
                _ => return Err(format!("find: invalid -size type `{c}'\n")),
            };
            (&rest[..idx], unit)
        }
        None => (rest, 512),
    };
    if num.is_empty() {
        return Err(invalid_arg("-size", arg));
    }
    let n = num.parse().map_err(|_| invalid_arg("-size", arg))?;
    Ok(Expr::Size { cmp, n, unit })
}

fn parse_perm(arg: &str) -> PResult<Expr> {
    let (kind, mode_str) = if let Some(rest) = arg.strip_prefix('-') {
        (PermKind::All, rest)
    } else if let Some(rest) = arg.strip_prefix('/') {
        (PermKind::Any, rest)
    } else {
        (PermKind::Exact, arg)
    };
    let mode = if !mode_str.is_empty() && mode_str.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        u32::from_str_radix(mode_str, 8)
            .ok()
            .filter(|m| *m <= 0o7777)
    } else {
        crate::builtins::fileops::apply_symbolic_mode(mode_str, 0)
    };
    match mode {
        Some(mode) => Ok(Expr::Perm { mode, kind }),
        None => Err(format!("find: invalid mode '{mode_str}'\n")),
    }
}

fn time_field(c: char) -> Option<TimeField> {
    match c {
        'a' | 'c' | 'm' => Some(TimeField::Modified),
        'B' => Some(TimeField::Created),
        _ => None,
    }
}

/// Translate an emacs/BRE/ERE pattern into Rust regex syntax.
fn translate_regex(pat: &str, ty: RegexType) -> String {
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::with_capacity(pat.len() + 8);
    let mut i = 0;
    let push_lit = |out: &mut String, c: char| {
        out.push_str(&regex::escape(&c.to_string()));
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                let n = chars[i + 1];
                i += 2;
                match (ty, n) {
                    (_, '<' | '>') => out.push_str("\\b"),
                    (_, '`') => out.push_str("\\A"),
                    (_, '\'') => out.push_str("\\z"),
                    (_, 'w' | 'W' | 'b' | 'B' | 's' | 'S') => {
                        out.push('\\');
                        out.push(n);
                    }
                    (RegexType::Extended, _) => push_lit(&mut out, n),
                    (_, '(' | ')' | '|' | '{' | '}') => out.push(n),
                    (RegexType::Basic, '+' | '?') => out.push(n),
                    (_, d) if d.is_ascii_digit() => {
                        // Back-references are unsupported by the regex engine;
                        // keep them so compilation reports an error.
                        out.push('\\');
                        out.push(d);
                    }
                    _ => push_lit(&mut out, n),
                }
                continue;
            }
            '(' | ')' | '|' | '{' | '}' if !matches!(ty, RegexType::Extended) => {
                push_lit(&mut out, c);
            }
            '+' | '?' if matches!(ty, RegexType::Basic) => push_lit(&mut out, c),
            '[' => {
                // Copy a bracket expression, escaping what Rust treats specially.
                out.push('[');
                i += 1;
                if chars.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                if chars.get(i) == Some(&']') {
                    out.push_str("\\]");
                    i += 1;
                }
                while i < chars.len() && chars[i] != ']' {
                    let b = chars[i];
                    if b == '[' && matches!(chars.get(i + 1), Some(':' | '.' | '=')) {
                        let delim = chars[i + 1];
                        let rest: String = chars[i + 2..].iter().collect();
                        let close: String = [delim, ']'].iter().collect();
                        if let Some(end) = rest.find(&close) {
                            let inner = &rest[..end];
                            if delim == ':' {
                                out.push_str(&format!("[:{inner}:]"));
                            } else {
                                for ic in inner.chars() {
                                    push_lit(&mut out, ic);
                                }
                            }
                            i += 2 + inner.chars().count() + 2;
                            continue;
                        }
                    }
                    match b {
                        '\\' | '[' | '&' | '~' => {
                            out.push('\\');
                            out.push(b);
                        }
                        _ => out.push(b),
                    }
                    i += 1;
                }
                if i < chars.len() {
                    out.push(']');
                }
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.pos).map(String::as_str)
    }

    fn take(&mut self) -> Option<&str> {
        let t = self.toks.get(self.pos).map(String::as_str);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn arg(&mut self, pred: &str) -> PResult<String> {
        self.take().map(str::to_string).ok_or_else(|| missing(pred))
    }

    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    fn at_operand_end(&self) -> bool {
        matches!(self.peek(), None | Some(")" | "," | "-o" | "-or"))
    }

    fn parse_comma(&mut self) -> PResult<Expr> {
        let mut left = self.parse_or()?;
        while self.peek() == Some(",") {
            self.pos += 1;
            if self.at_operand_end() {
                return Err(
                    "find: invalid expression; you have used a binary operator ',' with nothing after it.\n"
                        .to_string(),
                );
            }
            let right = self.parse_or()?;
            left = Expr::Comma(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut left = self.parse_and()?;
        while let Some(op @ ("-o" | "-or")) = self.peek() {
            let op = op.to_string();
            self.pos += 1;
            if self.at_operand_end() || matches!(self.peek(), Some("-a" | "-and")) {
                return Err(format!(
                    "find: invalid expression; you have used a binary operator '{op}' with nothing after it.\n"
                ));
            }
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(op @ ("-a" | "-and")) => {
                    let op = op.to_string();
                    self.pos += 1;
                    if self.at_operand_end() || matches!(self.peek(), Some("-a" | "-and")) {
                        return Err(format!(
                            "find: invalid expression; you have used a binary operator '{op}' with nothing after it.\n"
                        ));
                    }
                }
                None | Some(")" | "," | "-o" | "-or") => return Ok(left),
                Some(_) => {}
            }
            let right = self.parse_unary()?;
            left = Expr::and(left, right);
        }
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let Some(tok) = self.take().map(str::to_string) else {
            return Err("find: invalid expression\n".to_string());
        };
        match tok.as_str() {
            "!" | "-not" => {
                if self.at_operand_end() {
                    return Err(format!("find: expected an expression after '{tok}'\n"));
                }
                Ok(Expr::Not(Box::new(self.parse_unary()?)))
            }
            "(" => {
                if self.peek() == Some(")") {
                    return Err(
                        "find: invalid expression; empty parentheses are not allowed.\n"
                            .to_string(),
                    );
                }
                let inner = self.parse_comma()?;
                if self.take() != Some(")") {
                    return Err("find: invalid expression; I was expecting to find a ')' somewhere but did not see one.\n".to_string());
                }
                Ok(inner)
            }
            ")" => Err("find: invalid expression; you have too many ')'\n".to_string()),
            "-o" | "-or" | "-a" | "-and" | "," => Err(format!(
                "find: invalid expression; you have used a binary operator '{tok}' with nothing before it.\n"
            )),
            _ => self.primary(&tok),
        }
    }

    fn depth_arg(&mut self, pred: &str) -> PResult<usize> {
        let v = self.arg(pred)?;
        v.parse::<usize>()
            .ok()
            .filter(|_| !v.starts_with('+'))
            .ok_or_else(|| {
                format!(
                    "find: Expected a positive decimal integer argument to {pred}, but got '{v}'\n"
                )
            })
    }

    fn owner_arg(&mut self, pred: &str, group: bool) -> PResult<Expr> {
        let v = self.arg(pred)?;
        let (id, name_ok) = if v == self.ident.username {
            (
                if group {
                    self.ident.gid
                } else {
                    self.ident.uid
                },
                true,
            )
        } else if v == "root" {
            (0, true)
        } else if let Ok(n) = v.parse::<u64>() {
            (n, true)
        } else {
            (0, false)
        };
        if !name_ok {
            return Err(if group {
                format!("find: '{v}' is not the name of an existing group\n")
            } else {
                format!("find: '{v}' is not the name of a known user\n")
            });
        }
        Ok(if group {
            Expr::Gid {
                cmp: Cmp::Eq,
                n: id,
            }
        } else {
            Expr::Uid {
                cmp: Cmp::Eq,
                n: id,
            }
        })
    }

    fn primary(&mut self, tok: &str) -> PResult<Expr> {
        Ok(match tok {
            "-name" | "-iname" => Expr::Name {
                pat: self.arg(tok)?,
                nocase: tok == "-iname",
            },
            "-path" | "-wholename" | "-ipath" | "-iwholename" => Expr::Path {
                pat: self.arg(tok)?,
                nocase: tok.starts_with("-i"),
            },
            "-lname" | "-ilname" => Expr::Lname {
                pat: self.arg(tok)?,
                nocase: tok == "-ilname",
            },
            "-regex" | "-iregex" => {
                let pat = self.arg(tok)?;
                let translated = translate_regex(&pat, self.regex_type);
                let re = RegexBuilder::new(&format!("^(?:{translated})$"))
                    .case_insensitive(tok == "-iregex")
                    .dot_matches_new_line(true)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|_| {
                        format!(
                            "find: failed to compile regular expression '{pat}': Invalid regular expression\n"
                        )
                    })?;
                Expr::Regex(re)
            }
            "-regextype" => {
                let v = self.arg(tok)?;
                self.regex_type = match v.as_str() {
                    "emacs" | "findutils-default" => RegexType::Emacs,
                    "ed" | "grep" | "posix-basic" | "posix-minimal-basic" | "sed" => {
                        RegexType::Basic
                    }
                    "gnu-awk" | "posix-awk" | "awk" | "posix-egrep" | "egrep"
                    | "posix-extended" => RegexType::Extended,
                    _ => {
                        return Err(format!(
                            "find: Unknown regular expression type '{v}'; valid types are 'findutils-default', 'ed', 'emacs', 'gnu-awk', 'grep', 'posix-awk', 'awk', 'posix-basic', 'posix-egrep', 'egrep', 'posix-extended', 'posix-minimal-basic', 'sed'.\n"
                        ));
                    }
                };
                Expr::Const(true)
            }
            "-type" | "-xtype" => {
                let v = self.arg(tok)?;
                let mut types = Vec::new();
                for part in v.split(',') {
                    let mut chars = part.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c @ ('b' | 'c' | 'd' | 'p' | 'f' | 'l' | 's' | 'D')), None) => {
                            types.push(c)
                        }
                        _ => return Err(format!("find: Unknown argument to {tok}: {v}\n")),
                    }
                }
                Expr::Type {
                    types,
                    xtype: tok == "-xtype",
                }
            }
            "-size" => parse_size(&self.arg(tok)?)?,
            "-empty" => Expr::Empty,
            "-true" => Expr::Const(true),
            "-false" => Expr::Const(false),
            "-readable" => Expr::Access(4),
            "-writable" => Expr::Access(2),
            "-executable" => Expr::Access(1),
            "-mtime" | "-atime" | "-ctime" | "-mmin" | "-amin" | "-cmin" => {
                let v = self.arg(tok)?;
                let (cmp, n) = parse_numeric::<i64>(tok, &v)?;
                Expr::Age {
                    field: TimeField::Modified,
                    minutes: tok.ends_with("min"),
                    cmp,
                    n,
                }
            }
            "-newer" | "-anewer" | "-cnewer" => Expr::Newer {
                field: TimeField::Modified,
                reference: NewerRef::File(self.arg(tok)?),
            },
            _ if tok.starts_with("-newer") && tok.len() == 8 => {
                let mut cs = tok[6..].chars();
                let (x, y) = (cs.next().unwrap_or('?'), cs.next().unwrap_or('?'));
                let Some(field) = time_field(x) else {
                    return Err(format!("find: unknown predicate `{tok}'\n"));
                };
                let v = self.arg(tok)?;
                let reference = if y == 't' {
                    NewerRef::Date(v)
                } else if time_field(y).is_some() {
                    NewerRef::File(v)
                } else {
                    return Err(format!("find: unknown predicate `{tok}'\n"));
                };
                Expr::Newer { field, reference }
            }
            "-perm" => parse_perm(&self.arg(tok)?)?,
            "-user" => self.owner_arg(tok, false)?,
            "-group" => self.owner_arg(tok, true)?,
            "-uid" | "-gid" => {
                let v = self.arg(tok)?;
                let (cmp, n) = parse_numeric::<u64>(tok, &v)?;
                if tok == "-uid" {
                    Expr::Uid { cmp, n }
                } else {
                    Expr::Gid { cmp, n }
                }
            }
            "-nouser" | "-nogroup" => Expr::Const(false),
            "-maxdepth" => {
                self.opts.max_depth = Some(self.depth_arg(tok)?);
                Expr::Const(true)
            }
            "-mindepth" => {
                self.opts.min_depth = self.depth_arg(tok)?;
                Expr::Const(true)
            }
            "-depth" | "-d" => {
                self.opts.depth_first = true;
                Expr::Const(true)
            }
            "-follow" => {
                self.opts.follow = Follow::Always;
                Expr::Const(true)
            }
            "-daystart" => {
                self.opts.daystart = true;
                Expr::Const(true)
            }
            "-xdev"
            | "-mount"
            | "-noleaf"
            | "-ignore_readdir_race"
            | "-noignore_readdir_race"
            | "-warn"
            | "-nowarn" => Expr::Const(true),
            "-print" | "-print0" => {
                self.has_action = true;
                Expr::Print {
                    nul: tok == "-print0",
                }
            }
            "-printf" => {
                self.has_action = true;
                Expr::Printf(self.arg(tok)?)
            }
            "-delete" => {
                self.has_action = true;
                self.opts.depth_first = true;
                Expr::Delete { id: self.id() }
            }
            "-prune" => Expr::Prune,
            "-quit" => Expr::Quit,
            "-exec" | "-execdir" => {
                let mut argv = Vec::new();
                let mut batch = false;
                loop {
                    match self.take() {
                        None => return Err(missing(tok)),
                        Some(";" | "\\;") => break,
                        Some("+") if argv.last().is_some_and(|a: &String| a == "{}") => {
                            batch = true;
                            break;
                        }
                        Some(a) => argv.push(a.to_string()),
                    }
                }
                if argv.is_empty() {
                    return Err(missing(tok));
                }
                self.has_action = true;
                self.has_exec = true;
                Expr::Exec {
                    id: self.id(),
                    argv,
                    batch,
                    dir: tok == "-execdir",
                }
            }
            "-ok" | "-okdir" => {
                return Err(format!(
                    "find: {tok} needs an interactive terminal, which bashkit does not provide\n"
                ));
            }
            _ if tok.starts_with('-') => {
                return Err(format!("find: unknown predicate `{tok}'\n"));
            }
            _ => return Err(format!("find: paths must precede expression: `{tok}'\n")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: Identity<'static> = Identity {
        username: "sandbox",
        uid: 1000,
        gid: 1000,
    };

    fn p(args: &[&str]) -> PResult<Parsed> {
        let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&v, &ID)
    }

    #[test]
    fn defaults_and_implicit_print() {
        let r = p(&[]).unwrap();
        assert_eq!(r.paths, vec!["."]);
        assert!(matches!(r.expr, Expr::Print { nul: false }));
        let r = p(&["a", "b", "-name", "x"]).unwrap();
        assert_eq!(r.paths, vec!["a", "b"]);
        assert!(matches!(r.expr, Expr::And(_, _)));
    }

    #[test]
    fn precedence() {
        // -a binds tighter than -o: (a b) -o c, then implicit print wraps all.
        let r = p(&["-name", "a", "-type", "f", "-o", "-name", "c"]).unwrap();
        let Expr::And(lhs, print) = r.expr else {
            panic!("expected implicit print")
        };
        assert!(matches!(*print, Expr::Print { .. }));
        assert!(matches!(*lhs, Expr::Or(_, _)));
    }

    #[test]
    fn exec_forms() {
        let r = p(&["-exec", "echo", "{}", ";"]).unwrap();
        assert!(r.has_exec);
        assert!(matches!(r.expr, Expr::Exec { batch: false, .. }));
        let r = p(&["-exec", "echo", "{}", "+"]).unwrap();
        assert!(matches!(r.expr, Expr::Exec { batch: true, .. }));
        // `+` not after `{}` is an ordinary argument.
        assert_eq!(
            p(&["-exec", "echo", "+"]).unwrap_err(),
            "find: missing argument to `-exec'\n"
        );
    }

    #[test]
    fn gnu_error_texts() {
        let cases: &[(&[&str], &str)] = &[
            (&["-foo"], "find: unknown predicate `-foo'\n"),
            (&["-name"], "find: missing argument to `-name'\n"),
            (
                &["-o", "-name", "x"],
                "find: invalid expression; you have used a binary operator '-o' with nothing before it.\n",
            ),
            (
                &["(", "-name", "x"],
                "find: invalid expression; I was expecting to find a ')' somewhere but did not see one.\n",
            ),
            (
                &["-name", "x", "y"],
                "find: paths must precede expression: `y'\n",
            ),
            (&["-type", "q"], "find: Unknown argument to -type: q\n"),
            (
                &["-maxdepth", "x"],
                "find: Expected a positive decimal integer argument to -maxdepth, but got 'x'\n",
            ),
            (&["-size", "3q"], "find: invalid -size type `q'\n"),
            (&["!"], "find: expected an expression after '!'\n"),
            (&["-mtime", "x"], "find: invalid argument `x' to `-mtime'\n"),
            (&["-perm", "999"], "find: invalid mode '999'\n"),
            (
                &["-user", "nobodyx"],
                "find: 'nobodyx' is not the name of a known user\n",
            ),
            (
                &["-regex", "["],
                "find: failed to compile regular expression '[': Invalid regular expression\n",
            ),
        ];
        for (args, want) in cases {
            assert_eq!(p(args).unwrap_err(), *want, "args {}", args.join(" "));
        }
    }

    #[test]
    fn regex_translation() {
        assert_eq!(
            translate_regex(r".*\.\(c\|h\)", RegexType::Emacs),
            r".*\.(c|h)"
        );
        assert_eq!(translate_regex(r"a(b)", RegexType::Emacs), r"a\(b\)");
        assert_eq!(translate_regex(r"a+\+", RegexType::Basic), r"a\++");
        assert_eq!(
            translate_regex(r".*\.(c|h)", RegexType::Extended),
            r".*\.(c|h)"
        );
        assert_eq!(
            translate_regex(r"[[:digit:]]", RegexType::Emacs),
            r"[[:digit:]]"
        );
    }

    #[test]
    fn sizes() {
        assert!(matches!(
            parse_size("+2k").unwrap(),
            Expr::Size {
                cmp: Cmp::Gt,
                n: 2,
                unit: 1024
            }
        ));
        assert!(matches!(
            parse_size("10").unwrap(),
            Expr::Size {
                cmp: Cmp::Eq,
                n: 10,
                unit: 512
            }
        ));
    }
}
