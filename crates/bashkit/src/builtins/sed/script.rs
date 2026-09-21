//! sed script parser.
//!
//! Important decisions (issue #2427):
//!
//! - The whole script (every `-e`, every `-f`, the bare operand) is joined with
//!   newlines and parsed **once**, so multi-line `a\` text, `#` comments and the
//!   `#n` first-line directive work the way GNU sed defines them.
//! - The program is a **flat** instruction list: `{` compiles to a [`Kind::Block`]
//!   carrying the index of its `}`. Branch targets can therefore sit inside or
//!   outside a block, which a tree of nested groups cannot express.
//! - Every regex (substitution or address) goes through
//!   [`super::pattern::SedRegex`], so BRE/ERE translation happens in exactly one
//!   place.
//! - Parse errors are reported GNU-style (`-e expression #1, char N: ...`) and
//!   surface as exit status 1 with a stderr message, never as an interpreter
//!   error.

use std::sync::Arc;

use super::pattern::SedRegex;
use crate::builtins::limits::SED_MAX_GROUP_NESTING_DEPTH as MAX_GROUP_NESTING_DEPTH;

/// Longest diagnostic a parse error may produce (TM-INF-022 caps stderr).
const MAX_DIAGNOSTIC: usize = 512;

/// A script that failed to compile. GNU reports expression errors with the
/// offending character position and exit status 1, and whole-script problems
/// (an unresolvable branch target) with status 4.
pub(super) struct ParseError {
    pub(super) message: String,
    pub(super) code: i32,
}

pub(super) type ParseResult<T> = std::result::Result<T, ParseError>;

/// One piece of an `s///` replacement.
pub(super) enum RepPart {
    Lit(String),
    /// Capture group; `0` is the whole match (`&`).
    Group(usize),
    /// `\U`, `\L`, `\u`, `\l`, `\E`
    Case(CaseOp),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CaseOp {
    Upper,
    Lower,
    UpperOne,
    LowerOne,
    End,
}

pub(super) struct Subst {
    /// `None` means the empty regex `//`: reuse the last regex applied.
    pub(super) re: Option<Arc<SedRegex>>,
    pub(super) parts: Vec<RepPart>,
    /// 1-based index of the first occurrence to replace.
    pub(super) occurrence: usize,
    /// Replace `occurrence` and everything after it.
    pub(super) global: bool,
    pub(super) print: bool,
    pub(super) wfile: Option<String>,
}

pub(super) enum Kind {
    /// `{` — index of the matching `}`; skipped past when the address misses.
    Block(usize),
    BlockEnd,
    Label(String),
    Branch(Option<String>),
    BranchIfSub(Option<String>),
    BranchIfNoSub(Option<String>),
    Substitute(Box<Subst>),
    Transliterate(Vec<(char, char)>),
    Delete,
    DeleteFirstLine,
    Print,
    PrintFirstLine,
    Next,
    NextAppend,
    Quit(i32),
    QuitSilent(i32),
    Append(String),
    Insert(String),
    Change(String),
    HoldCopy,
    HoldAppend,
    GetCopy,
    GetAppend,
    Exchange,
    LineNumber,
    List(Option<usize>),
    ReadFile(String),
    ReadLine(String),
    WriteFile(String),
    WriteFirstLine(String),
    Zap,
    FileName,
    Nop,
}

pub(super) enum StartAddr {
    Line(usize),
    Last,
    Regex(Option<Arc<SedRegex>>),
    Step(usize, usize),
    /// `0` — only legal as the start of `0,/re/`.
    Zero,
}

pub(super) enum EndAddr {
    Line(usize),
    Last,
    Regex(Option<Arc<SedRegex>>),
    /// `addr,+N`
    Plus(usize),
    /// `addr,~N`
    Multiple(usize),
}

pub(super) struct Addr {
    pub(super) start: StartAddr,
    pub(super) end: Option<EndAddr>,
}

pub(super) struct Cmd {
    pub(super) addr: Option<Addr>,
    pub(super) negate: bool,
    pub(super) kind: Kind,
}

pub(super) struct Program {
    pub(super) cmds: Vec<Cmd>,
    /// `#n` on the very first line implies `-n`.
    pub(super) quiet: bool,
    /// Files named by `r`/`R`, which are read before execution starts.
    pub(super) read_files: Vec<String>,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    extended: bool,
    regex_seen: bool,
}

pub(super) fn parse(script: &str, extended: bool) -> ParseResult<Program> {
    let mut parser = Parser {
        chars: script.chars().collect(),
        pos: 0,
        extended,
        regex_seen: false,
    };
    parser.program()
}

impl Parser {
    /// GNU counts the characters consumed so far, so the position is
    /// `self.pos` (already one past the offending token), not `pos + 1`.
    fn err<T>(&self, msg: impl std::fmt::Display) -> ParseResult<T> {
        Err(ParseError {
            message: clamp(format!("-e expression #1, char {}: {msg}", self.pos)),
            code: 1,
        })
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t')) {
            self.pos += 1;
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.peek(),
            Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some(';')
        ) {
            self.pos += 1;
        }
    }

    fn rest_of_line(&mut self) -> String {
        let start = self.pos;
        while !matches!(self.peek(), None | Some('\n')) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn program(&mut self) -> ParseResult<Program> {
        let mut quiet = false;
        let mut read_files = Vec::new();

        // `#n` as the first two characters of the script implies -n.
        if self.chars.first() == Some(&'#') {
            if self.at(1) == Some('n') && matches!(self.at(2), None | Some('\n')) {
                quiet = true;
            }
            self.rest_of_line();
        }

        let mut cmds: Vec<Cmd> = Vec::new();
        let mut open_blocks: Vec<usize> = Vec::new();

        loop {
            self.skip_separators();
            let Some(ch) = self.peek() else { break };

            if ch == '#' {
                self.rest_of_line();
                continue;
            }

            if ch == '}' {
                self.pos += 1;
                let Some(open) = open_blocks.pop() else {
                    return self.err("unexpected `}'");
                };
                let end = cmds.len();
                if let Kind::Block(slot) = &mut cmds[open].kind {
                    *slot = end;
                }
                cmds.push(Cmd {
                    addr: None,
                    negate: false,
                    kind: Kind::BlockEnd,
                });
                continue;
            }

            let addr = self.address()?;
            self.skip_blanks();
            let mut negate = false;
            while self.eat('!') {
                negate = true;
                self.skip_blanks();
            }

            let Some(cmd_char) = self.peek() else {
                return self.err("missing command");
            };

            if cmd_char == '{' {
                self.pos += 1;
                if open_blocks.len() + 1 > MAX_GROUP_NESTING_DEPTH {
                    return self.err(format!(
                        "grouped command nesting exceeds max depth {MAX_GROUP_NESTING_DEPTH}"
                    ));
                }
                open_blocks.push(cmds.len());
                cmds.push(Cmd {
                    addr,
                    negate,
                    kind: Kind::Block(usize::MAX),
                });
                continue;
            }

            let kind = self.command(cmd_char, addr.is_some())?;
            // `q` and `Q` stop the stream, so a range makes no sense for them.
            if matches!(kind, Kind::Quit(_) | Kind::QuitSilent(_))
                && addr.as_ref().is_some_and(|a| a.end.is_some())
            {
                return self.err("command only uses one address");
            }
            match &kind {
                Kind::ReadFile(f) | Kind::ReadLine(f) => read_files.push(f.clone()),
                _ => {}
            }
            cmds.push(Cmd { addr, negate, kind });
        }

        if !open_blocks.is_empty() {
            return self.err("unmatched `{'");
        }

        // GNU resolves branch targets at compile time; an unknown label is an
        // error rather than a silent fall-through.
        for cmd in &cmds {
            let target = match &cmd.kind {
                Kind::Branch(Some(l))
                | Kind::BranchIfSub(Some(l))
                | Kind::BranchIfNoSub(Some(l)) => l,
                _ => continue,
            };
            let known = cmds
                .iter()
                .any(|c| matches!(&c.kind, Kind::Label(name) if name == target));
            if !known {
                return Err(ParseError {
                    message: clamp(format!("can't find label for jump to `{target}'")),
                    code: 4,
                });
            }
        }

        Ok(Program {
            cmds,
            quiet,
            read_files,
        })
    }

    // ----- addresses -------------------------------------------------------

    fn address(&mut self) -> ParseResult<Option<Addr>> {
        self.skip_blanks();
        let Some(ch) = self.peek() else {
            return Ok(None);
        };

        let start = match ch {
            '$' => {
                self.pos += 1;
                StartAddr::Last
            }
            '/' => {
                self.pos += 1;
                StartAddr::Regex(self.address_regex('/')?)
            }
            '\\' => {
                self.pos += 1;
                let Some(delim) = self.bump() else {
                    return self.err("unterminated address regex");
                };
                StartAddr::Regex(self.address_regex(delim)?)
            }
            c if c.is_ascii_digit() => {
                let n = self.number();
                if self.eat('~') {
                    let step = self.number();
                    StartAddr::Step(n, step)
                } else if n == 0 {
                    StartAddr::Zero
                } else {
                    StartAddr::Line(n)
                }
            }
            _ => return Ok(None),
        };

        self.skip_blanks();
        if !self.eat(',') {
            if matches!(start, StartAddr::Zero) {
                return self.err("invalid usage of line address 0");
            }
            return Ok(Some(Addr { start, end: None }));
        }

        self.skip_blanks();
        let end = match self.peek() {
            Some('$') => {
                self.pos += 1;
                EndAddr::Last
            }
            Some('/') => {
                self.pos += 1;
                EndAddr::Regex(self.address_regex('/')?)
            }
            Some('\\') => {
                self.pos += 1;
                let Some(delim) = self.bump() else {
                    return self.err("unterminated address regex");
                };
                EndAddr::Regex(self.address_regex(delim)?)
            }
            Some('+') => {
                self.pos += 1;
                EndAddr::Plus(self.number())
            }
            Some('~') => {
                self.pos += 1;
                EndAddr::Multiple(self.number())
            }
            Some(c) if c.is_ascii_digit() => EndAddr::Line(self.number()),
            _ => return self.err("unexpected `,'"),
        };

        if matches!(start, StartAddr::Zero) && !matches!(end, EndAddr::Regex(_)) {
            return self.err("invalid usage of line address 0");
        }

        Ok(Some(Addr {
            start,
            end: Some(end),
        }))
    }

    fn number(&mut self) -> usize {
        let mut n: usize = 0;
        while let Some(c) = self.peek() {
            let Some(d) = c.to_digit(10) else { break };
            n = n.saturating_mul(10).saturating_add(d as usize);
            self.pos += 1;
        }
        n
    }

    /// Read `<delim>regex<delim>` (the opening delimiter is already consumed)
    /// plus the trailing `I`/`M` flags, and compile it.
    fn address_regex(&mut self, delim: char) -> ParseResult<Option<Arc<SedRegex>>> {
        let body = self.delimited(delim, "address regex", true)?;
        let mut case_insensitive = false;
        let mut multi_line = false;
        loop {
            match self.peek() {
                Some('I') => {
                    case_insensitive = true;
                    self.pos += 1;
                }
                Some('M') => {
                    multi_line = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        self.compile(&body, case_insensitive, multi_line)
    }

    fn compile(
        &mut self,
        body: &str,
        case_insensitive: bool,
        multi_line: bool,
    ) -> ParseResult<Option<Arc<SedRegex>>> {
        if body.is_empty() {
            if !self.regex_seen {
                return self.err("no previous regular expression");
            }
            return Ok(None);
        }
        self.regex_seen = true;
        match SedRegex::new(body, self.extended, case_insensitive, multi_line) {
            Ok(re) => Ok(Some(Arc::new(re))),
            Err(e) => self.err(format!("invalid pattern: {}", flatten(&e))),
        }
    }

    /// Consume characters up to the next unescaped `delim`, keeping backslash
    /// escapes intact except `\<delim>`, which becomes a literal delimiter.
    ///
    /// `bracket_aware` is set while scanning a regex: inside a POSIX bracket
    /// expression the delimiter is an ordinary character, so `s/[/]/X/` matches
    /// a slash and `s/[//` is an unterminated command rather than an empty
    /// bracket expression.
    fn delimited(&mut self, delim: char, what: &str, bracket_aware: bool) -> ParseResult<String> {
        let mut out = String::new();
        while let Some(c) = self.bump() {
            if bracket_aware && c == '[' {
                out.push('[');
                self.copy_bracket(&mut out, what)?;
                continue;
            }
            if c == '\\' {
                match self.bump() {
                    // `\<delim>` is the delimiter as an ordinary character.
                    Some(n) if n == delim => {
                        if n.is_ascii_alphanumeric() {
                            out.push(n);
                        } else {
                            out.push('\\');
                            out.push(n);
                        }
                    }
                    Some('\n') => out.push('\n'),
                    Some(n) => {
                        out.push('\\');
                        out.push(n);
                    }
                    None => return self.err(format!("unterminated {what}")),
                }
                continue;
            }
            if c == delim {
                return Ok(out);
            }
            if c == '\n' && delim != '\n' {
                break;
            }
            out.push(c);
        }
        self.err(format!("unterminated {what}"))
    }

    /// Copy a bracket expression verbatim; the opening `[` is already emitted.
    fn copy_bracket(&mut self, out: &mut String, what: &str) -> ParseResult<()> {
        if self.peek() == Some('^') {
            out.push('^');
            self.pos += 1;
        }
        if self.peek() == Some(']') {
            out.push(']');
            self.pos += 1;
        }
        while let Some(c) = self.bump() {
            out.push(c);
            if c == ']' {
                return Ok(());
            }
            if c == '['
                && let Some(kind @ (':' | '.' | '=')) = self.peek()
            {
                out.push(kind);
                self.pos += 1;
                while let Some(inner) = self.bump() {
                    out.push(inner);
                    if inner == kind && self.peek() == Some(']') {
                        out.push(']');
                        self.pos += 1;
                        break;
                    }
                }
            }
        }
        self.err(format!("unterminated {what}"))
    }

    // ----- commands --------------------------------------------------------

    fn command(&mut self, ch: char, has_addr: bool) -> ParseResult<Kind> {
        self.pos += 1;
        match ch {
            's' => self.substitute(),
            'y' => self.transliterate(),
            'd' => Ok(Kind::Delete),
            'D' => Ok(Kind::DeleteFirstLine),
            'p' => Ok(Kind::Print),
            'P' => Ok(Kind::PrintFirstLine),
            'n' => Ok(Kind::Next),
            'N' => Ok(Kind::NextAppend),
            'h' => Ok(Kind::HoldCopy),
            'H' => Ok(Kind::HoldAppend),
            'g' => Ok(Kind::GetCopy),
            'G' => Ok(Kind::GetAppend),
            'x' => Ok(Kind::Exchange),
            'z' => Ok(Kind::Zap),
            'F' => Ok(Kind::FileName),
            '=' => Ok(Kind::LineNumber),
            'q' => Ok(Kind::Quit(self.exit_code())),
            'Q' => Ok(Kind::QuitSilent(self.exit_code())),
            'l' => {
                self.skip_blanks();
                let width = if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    Some(self.number())
                } else {
                    None
                };
                Ok(Kind::List(width))
            }
            'a' => Ok(Kind::Append(self.text())),
            'i' => Ok(Kind::Insert(self.text())),
            'c' => Ok(Kind::Change(self.text())),
            'r' => Ok(Kind::ReadFile(self.filename())),
            'R' => Ok(Kind::ReadLine(self.filename())),
            'w' => Ok(Kind::WriteFile(self.filename())),
            'W' => Ok(Kind::WriteFirstLine(self.filename())),
            ':' => {
                if has_addr {
                    return self.err(": doesn't want any addresses");
                }
                let label = self.label();
                if label.is_empty() {
                    return self.err("\":\" lacks a label");
                }
                Ok(Kind::Label(label))
            }
            'b' => Ok(Kind::Branch(self.optional_label())),
            't' => Ok(Kind::BranchIfSub(self.optional_label())),
            'T' => Ok(Kind::BranchIfNoSub(self.optional_label())),
            'v' => {
                self.rest_of_line();
                Ok(Kind::Nop)
            }
            'e' => self.err(
                "the `e' command is not available: Bashkit never executes host \
                 commands from a sed script",
            ),
            _ => self.err(format!("unknown command: `{ch}'")),
        }
    }

    fn exit_code(&mut self) -> i32 {
        self.skip_blanks();
        if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            (self.number() & 0xff) as i32
        } else {
            0
        }
    }

    fn label(&mut self) -> String {
        self.skip_blanks();
        let start = self.pos;
        while !matches!(self.peek(), None | Some('\n') | Some(';') | Some('}')) {
            self.pos += 1;
        }
        self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .trim()
            .to_string()
    }

    fn optional_label(&mut self) -> Option<String> {
        let label = self.label();
        if label.is_empty() { None } else { Some(label) }
    }

    /// `r`/`w` take the rest of the line, `;` included, as the file name.
    fn filename(&mut self) -> String {
        self.skip_blanks();
        self.rest_of_line().trim_end().to_string()
    }

    /// Text argument of `a`, `i` and `c`, in both the POSIX `a\` + newline form
    /// and the GNU one-liner form.
    fn text(&mut self) -> String {
        self.skip_blanks();
        if self.eat('\\') {
            self.eat('\n');
        }
        let mut out = String::new();
        while let Some(c) = self.bump() {
            if c == '\\' {
                match self.bump() {
                    Some('\n') => out.push('\n'),
                    Some(n) => out.push(n),
                    None => break,
                }
                continue;
            }
            if c == '\n' {
                break;
            }
            out.push(c);
        }
        out
    }

    fn transliterate(&mut self) -> ParseResult<Kind> {
        let Some(delim) = self.bump() else {
            return self.err("unterminated `y' command");
        };
        let src = unescape_y(&self.delimited(delim, "`y' command", false)?, delim);
        let dst = unescape_y(&self.delimited(delim, "`y' command", false)?, delim);
        if src.len() != dst.len() {
            return self.err("strings for `y' command are different lengths");
        }
        Ok(Kind::Transliterate(
            src.into_iter().zip(dst).collect::<Vec<_>>(),
        ))
    }

    fn substitute(&mut self) -> ParseResult<Kind> {
        let Some(delim) = self.bump() else {
            return self.err("unterminated `s' command");
        };
        if delim == '\\' || delim == '\n' {
            return self.err("unknown option to `s'");
        }
        let pattern = self.delimited(delim, "`s' command", true)?;
        let replacement = self.delimited(delim, "`s' command", false)?;

        let mut global = false;
        let mut print = false;
        let mut case_insensitive = false;
        let mut multi_line = false;
        let mut occurrence: Option<usize> = None;
        let mut wfile = None;

        loop {
            match self.peek() {
                Some('g') => {
                    global = true;
                    self.pos += 1;
                }
                Some('p') => {
                    print = true;
                    self.pos += 1;
                }
                Some('i') | Some('I') => {
                    case_insensitive = true;
                    self.pos += 1;
                }
                Some('m') | Some('M') => {
                    multi_line = true;
                    self.pos += 1;
                }
                Some('e') => {
                    return self.err(
                        "the `e' option to `s' is not available: Bashkit never \
                         executes host commands from a sed script",
                    );
                }
                Some(c) if c.is_ascii_digit() => {
                    if occurrence.is_some() {
                        return self.err("multiple number options to `s' command");
                    }
                    let n = self.number();
                    if n == 0 {
                        return self.err("number option to `s' command may not be zero");
                    }
                    occurrence = Some(n);
                }
                Some('w') => {
                    self.pos += 1;
                    wfile = Some(self.filename());
                    break;
                }
                Some(' ') | Some('\t') => {
                    self.pos += 1;
                }
                None | Some('\n') | Some(';') | Some('}') | Some('#') | Some('\r') => break,
                Some(c) => return self.err(format!("unknown option to `s': `{c}'")),
            }
        }

        let re = self.compile(&pattern, case_insensitive, multi_line)?;
        let parts = parse_replacement(&replacement);
        // GNU rejects `s/a/\1/` at compile time rather than substituting an
        // empty string, so a typo in a back-reference fails loudly.
        if let Some(re) = &re {
            let groups = re.group_count();
            for part in &parts {
                if let RepPart::Group(n) = part
                    && *n > groups
                {
                    return self.err(format!("invalid reference \\{n} on `s' command's RHS"));
                }
            }
        }
        Ok(Kind::Substitute(Box::new(Subst {
            re,
            parts,
            occurrence: occurrence.unwrap_or(1),
            global,
            print,
            wfile,
        })))
    }
}

fn clamp(mut text: String) -> String {
    if text.len() > MAX_DIAGNOSTIC {
        text.truncate(MAX_DIAGNOSTIC);
        text.push('…');
    }
    text
}

/// Collapse a multi-line engine diagnostic (the `regex` crate draws a caret
/// diagram) into one line, keeping stderr small per TM-INF-022.
fn flatten(message: &str) -> String {
    let compact = message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.chars().all(|c| c == '^'))
        .collect::<Vec<_>>()
        .join(" ");
    clamp(compact)
}

fn unescape_y(s: &str, delim: char) -> Vec<char> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            out.push(match n {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                c if c == delim => c,
                c => c,
            });
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Parse an `s///` replacement into literal / capture / case-fold parts.
///
/// Doing this ourselves (instead of translating to `regex`-crate replacement
/// syntax) is what keeps a literal `$` literal: `sed 's/a/$x/'` must emit `$x`,
/// not expand `$x` as a named capture group (issue #2427, finding A).
pub(super) fn parse_replacement(text: &str) -> Vec<RepPart> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0;

    macro_rules! flush {
        () => {
            if !lit.is_empty() {
                parts.push(RepPart::Lit(std::mem::take(&mut lit)));
            }
        };
    }

    while i < chars.len() {
        let c = chars[i];
        if c == '&' {
            flush!();
            parts.push(RepPart::Group(0));
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            i += 2;
            match n {
                '0'..='9' => {
                    flush!();
                    parts.push(RepPart::Group(n as usize - '0' as usize));
                }
                'n' => lit.push('\n'),
                't' => lit.push('\t'),
                'r' => lit.push('\r'),
                'f' => lit.push('\x0c'),
                'v' => lit.push('\x0b'),
                'a' => lit.push('\x07'),
                '\n' => lit.push('\n'),
                'L' => {
                    flush!();
                    parts.push(RepPart::Case(CaseOp::Lower));
                }
                'U' => {
                    flush!();
                    parts.push(RepPart::Case(CaseOp::Upper));
                }
                'l' => {
                    flush!();
                    parts.push(RepPart::Case(CaseOp::LowerOne));
                }
                'u' => {
                    flush!();
                    parts.push(RepPart::Case(CaseOp::UpperOne));
                }
                'E' => {
                    flush!();
                    parts.push(RepPart::Case(CaseOp::End));
                }
                // GNU drops the backslash before an ordinary character, so
                // `\$` is a literal `$` and `\q` is a literal `q`.
                other => lit.push(other),
            }
            continue;
        }
        lit.push(c);
        i += 1;
    }

    flush!();
    parts
}
