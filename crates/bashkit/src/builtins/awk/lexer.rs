//! awk lexer.
//!
//! Decisions:
//! - Lazy, parser-driven: `/` is lexed as division; the parser asks for a
//!   regex literal (`read_regex`) when it expects an operand and sees `/` or
//!   `/=`. That is the only way to tell `a / b / c` from `/re/`.
//! - Newlines are tokens (statement terminators); the parser skips them
//!   where the grammar allows (`opt_nls`). Comments and backslash-newline
//!   continuations are skipped here.
//! - An identifier immediately followed by `(` is a `FuncName` token: user
//!   function calls must not have a space before `(` (POSIX), which is what
//!   tells `f (x)` (concatenation) from `f(x)`.
//! - Numeric literals in source accept gawk's hex (`0x1A`) and octal (`011`)
//!   forms; strings converted at runtime never do (see `value.rs`).

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
    Num(f64),
    Str(String),
    /// Regex literal body (escapes kept for the regex translator).
    Regex(String),
    Name(String),
    FuncName(String),
    Builtin(String),
    // Keywords
    Begin,
    End,
    BeginFile,
    EndFile,
    Function,
    If,
    Else,
    While,
    For,
    Do,
    Break,
    Continue,
    Next,
    NextFile,
    Exit,
    Return,
    Delete,
    Getline,
    Print,
    Printf,
    In,
    Switch,
    Case,
    Default,
    // Punctuation
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Semi,
    Newline,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Not,
    Gt,
    Lt,
    Pipe,
    PipeAmp,
    Question,
    Colon,
    Tilde,
    NoMatch,
    Dollar,
    At,
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    Eq,
    Le,
    Ge,
    Ne,
    Incr,
    Decr,
    And,
    Or,
    Append,
    Eof,
}

pub(super) const BUILTIN_FUNCS: &[&str] = &[
    "length", "substr", "index", "split", "sub", "gsub", "gensub", "match", "sprintf", "sin",
    "cos", "atan2", "exp", "log", "sqrt", "int", "rand", "srand", "tolower", "toupper", "system",
    "close", "fflush", "strtonum", "systime", "strftime", "mktime", "asort", "asorti", "typeof",
    "isarray", "and", "or", "xor", "lshift", "rshift", "compl", "patsplit",
];

pub(super) struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

/// Lexing failure: message and byte offset.
pub(super) struct LexError {
    pub(super) msg: String,
    pub(super) pos: usize,
}

impl<'a> Lexer<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    pub(super) fn pos(&self) -> usize {
        self.pos
    }

    pub(super) fn reset(&mut self, pos: usize) {
        self.pos = pos;
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn byte_at(&self, i: usize) -> Option<u8> {
        self.src.as_bytes().get(i).copied()
    }

    /// Skip blanks, comments and backslash-newline continuations.
    fn skip_blank(&mut self) {
        while let Some(b) = self.peek_byte() {
            match b {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\\' if self.byte_at(self.pos + 1) == Some(b'\n') => self.pos += 2,
                b'\\'
                    if self.byte_at(self.pos + 1) == Some(b'\r')
                        && self.byte_at(self.pos + 2) == Some(b'\n') =>
                {
                    self.pos += 3
                }
                b'#' => {
                    while let Some(c) = self.peek_byte() {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    /// Next token and its start offset.
    pub(super) fn next(&mut self) -> Result<(Tok, usize), LexError> {
        self.skip_blank();
        let start = self.pos;
        let Some(b) = self.peek_byte() else {
            return Ok((Tok::Eof, start));
        };
        let two = |l: &Self, c: u8| l.byte_at(l.pos + 1) == Some(c);
        let tok = match b {
            b'\n' => {
                self.pos += 1;
                Tok::Newline
            }
            b'{' => self.one(Tok::LBrace),
            b'}' => self.one(Tok::RBrace),
            b'(' => self.one(Tok::LParen),
            b')' => self.one(Tok::RParen),
            b'[' => self.one(Tok::LBracket),
            b']' => self.one(Tok::RBracket),
            b';' => self.one(Tok::Semi),
            b',' => self.one(Tok::Comma),
            b'?' => self.one(Tok::Question),
            b':' => self.one(Tok::Colon),
            b'~' => self.one(Tok::Tilde),
            b'$' => self.one(Tok::Dollar),
            b'@' => self.one(Tok::At),
            b'+' if two(self, b'+') => self.two(Tok::Incr),
            b'+' if two(self, b'=') => self.two(Tok::AddAssign),
            b'+' => self.one(Tok::Plus),
            b'-' if two(self, b'-') => self.two(Tok::Decr),
            b'-' if two(self, b'=') => self.two(Tok::SubAssign),
            b'-' => self.one(Tok::Minus),
            b'*' if two(self, b'*') => {
                if self.byte_at(self.pos + 2) == Some(b'=') {
                    self.pos += 3;
                    Tok::PowAssign
                } else {
                    self.two(Tok::Caret)
                }
            }
            b'*' if two(self, b'=') => self.two(Tok::MulAssign),
            b'*' => self.one(Tok::Star),
            b'/' if two(self, b'=') => self.two(Tok::DivAssign),
            b'/' => self.one(Tok::Slash),
            b'%' if two(self, b'=') => self.two(Tok::ModAssign),
            b'%' => self.one(Tok::Percent),
            b'^' if two(self, b'=') => self.two(Tok::PowAssign),
            b'^' => self.one(Tok::Caret),
            b'!' if two(self, b'=') => self.two(Tok::Ne),
            b'!' if two(self, b'~') => self.two(Tok::NoMatch),
            b'!' => self.one(Tok::Not),
            b'>' if two(self, b'=') => self.two(Tok::Ge),
            b'>' if two(self, b'>') => self.two(Tok::Append),
            b'>' => self.one(Tok::Gt),
            b'<' if two(self, b'=') => self.two(Tok::Le),
            b'<' => self.one(Tok::Lt),
            b'=' if two(self, b'=') => self.two(Tok::Eq),
            b'=' => self.one(Tok::Assign),
            b'|' if two(self, b'|') => self.two(Tok::Or),
            b'|' if two(self, b'&') => self.two(Tok::PipeAmp),
            b'|' => self.one(Tok::Pipe),
            b'&' if two(self, b'&') => self.two(Tok::And),
            b'"' => self.string()?,
            b'0'..=b'9' | b'.' => self.number()?,
            c if c.is_ascii_alphabetic() || c == b'_' => self.word(),
            _ => {
                let c = self.src[self.pos..].chars().next().unwrap_or('?');
                return Err(LexError {
                    msg: format!("invalid char '{c}' in expression"),
                    pos: start,
                });
            }
        };
        Ok((tok, start))
    }

    fn one(&mut self, t: Tok) -> Tok {
        self.pos += 1;
        t
    }

    fn two(&mut self, t: Tok) -> Tok {
        self.pos += 2;
        t
    }

    fn word(&mut self) -> Tok {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let w = &self.src[start..self.pos];
        let kw = match w {
            "BEGIN" => Some(Tok::Begin),
            "END" => Some(Tok::End),
            "BEGINFILE" => Some(Tok::BeginFile),
            "ENDFILE" => Some(Tok::EndFile),
            "function" | "func" => Some(Tok::Function),
            "if" => Some(Tok::If),
            "else" => Some(Tok::Else),
            "while" => Some(Tok::While),
            "for" => Some(Tok::For),
            "do" => Some(Tok::Do),
            "break" => Some(Tok::Break),
            "continue" => Some(Tok::Continue),
            "next" => Some(Tok::Next),
            "nextfile" => Some(Tok::NextFile),
            "exit" => Some(Tok::Exit),
            "return" => Some(Tok::Return),
            "delete" => Some(Tok::Delete),
            "getline" => Some(Tok::Getline),
            "print" => Some(Tok::Print),
            "printf" => Some(Tok::Printf),
            "in" => Some(Tok::In),
            "switch" => Some(Tok::Switch),
            "case" => Some(Tok::Case),
            "default" => Some(Tok::Default),
            _ => None,
        };
        if let Some(k) = kw {
            return k;
        }
        if BUILTIN_FUNCS.contains(&w) {
            return Tok::Builtin(w.to_string());
        }
        if self.peek_byte() == Some(b'(') {
            Tok::FuncName(w.to_string())
        } else {
            Tok::Name(w.to_string())
        }
    }

    fn number(&mut self) -> Result<Tok, LexError> {
        let start = self.pos;
        let b = self.src.as_bytes();
        // Hex literal.
        if b[start] == b'0'
            && matches!(self.byte_at(start + 1), Some(b'x' | b'X'))
            && self
                .byte_at(start + 2)
                .is_some_and(|c| c.is_ascii_hexdigit())
        {
            let mut i = start + 2;
            while i < b.len() && b[i].is_ascii_hexdigit() {
                i += 1;
            }
            self.pos = i;
            let v = u64::from_str_radix(&self.src[start + 2..i], 16)
                .map(|v| v as f64)
                .unwrap_or(f64::INFINITY);
            return Ok(Tok::Num(v));
        }
        let mut i = start;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let int_end = i;
        let mut is_int = true;
        if i < b.len() && b[i] == b'.' {
            is_int = false;
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i == start + 1 && b[start] == b'.' {
            return Err(LexError {
                msg: "syntax error".to_string(),
                pos: start,
            });
        }
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            if j < b.len() && b[j].is_ascii_digit() {
                is_int = false;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
            }
        }
        self.pos = i;
        let text = &self.src[start..i];
        // gawk: a leading-zero integer of octal digits is octal.
        if is_int
            && int_end - start > 1
            && b[start] == b'0'
            && text.bytes().all(|c| (b'0'..=b'7').contains(&c))
        {
            let v = u64::from_str_radix(text, 8)
                .map(|v| v as f64)
                .unwrap_or(f64::INFINITY);
            return Ok(Tok::Num(v));
        }
        Ok(Tok::Num(text.parse::<f64>().unwrap_or(0.0)))
    }

    fn string(&mut self) -> Result<Tok, LexError> {
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.src[self.pos..].chars().next() else {
                return Err(LexError {
                    msg: "unterminated string".to_string(),
                    pos: start,
                });
            };
            self.pos += c.len_utf8();
            match c {
                '"' => break,
                '\n' => {
                    return Err(LexError {
                        msg: "unterminated string".to_string(),
                        pos: start,
                    });
                }
                '\\' => {
                    let Some(e) = self.src[self.pos..].chars().next() else {
                        out.push('\\');
                        continue;
                    };
                    self.pos += e.len_utf8();
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '\\' => out.push('\\'),
                        '"' => out.push('"'),
                        '/' => out.push('/'),
                        'a' => out.push('\x07'),
                        'b' => out.push('\x08'),
                        'f' => out.push('\x0c'),
                        'v' => out.push('\x0b'),
                        '\n' => {}
                        '0'..='7' => {
                            let mut v = e as u32 - '0' as u32;
                            for _ in 0..2 {
                                match self.peek_byte() {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            push_byte_char(&mut out, v);
                        }
                        'x' => {
                            let mut v = 0u32;
                            let mut n = 0;
                            while n < 2 {
                                match self.peek_byte() {
                                    Some(d) if d.is_ascii_hexdigit() => {
                                        v = v * 16 + (d as char).to_digit(16).unwrap_or(0);
                                        self.pos += 1;
                                        n += 1;
                                    }
                                    _ => break,
                                }
                            }
                            if n == 0 {
                                out.push_str("\\x");
                            } else {
                                push_byte_char(&mut out, v);
                            }
                        }
                        // gawk 5.3: `\u` takes up to 8 hex digits of a
                        // code point; with none it stays as written.
                        'u' => {
                            let mut v = 0u32;
                            let mut n = 0;
                            while n < 8 {
                                match self.peek_byte() {
                                    Some(d) if d.is_ascii_hexdigit() => {
                                        v = v
                                            .saturating_mul(16)
                                            .saturating_add((d as char).to_digit(16).unwrap_or(0));
                                        self.pos += 1;
                                        n += 1;
                                    }
                                    _ => break,
                                }
                            }
                            if n == 0 {
                                out.push_str("\\u");
                            } else {
                                push_byte_char(&mut out, v);
                            }
                        }
                        // gawk: an unknown escape keeps the character and
                        // drops the backslash (gawk also warns; we do not).
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
        Ok(Tok::Str(out))
    }

    /// Read a regex literal; `start` is the offset of the opening `/`.
    pub(super) fn read_regex(&mut self, start: usize) -> Result<Tok, LexError> {
        self.pos = start + 1;
        let mut out = String::new();
        let mut in_bracket = false;
        loop {
            let Some(c) = self.src[self.pos..].chars().next() else {
                return Err(LexError {
                    msg: "unterminated regexp".to_string(),
                    pos: start,
                });
            };
            self.pos += c.len_utf8();
            match c {
                '\n' => {
                    return Err(LexError {
                        msg: "unterminated regexp".to_string(),
                        pos: start,
                    });
                }
                '/' if !in_bracket => break,
                '\\' => {
                    let Some(e) = self.src[self.pos..].chars().next() else {
                        out.push('\\');
                        continue;
                    };
                    self.pos += e.len_utf8();
                    if e == '/' {
                        out.push('/');
                    } else if e == '\n' {
                        // Line continuation inside a regex.
                    } else {
                        out.push('\\');
                        out.push(e);
                    }
                }
                '[' if !in_bracket => {
                    in_bracket = true;
                    out.push('[');
                    // `]` right after `[` or `[^` is literal.
                    if self.peek_byte() == Some(b'^') {
                        out.push('^');
                        self.pos += 1;
                    }
                    if self.peek_byte() == Some(b']') {
                        out.push(']');
                        self.pos += 1;
                    }
                }
                '[' if in_bracket && matches!(self.peek_byte(), Some(b':' | b'.' | b'=')) => {
                    // Character class like [:alpha:] inside a bracket.
                    let delim = self.peek_byte().unwrap_or(b':') as char;
                    out.push('[');
                    let close = format!("{delim}]");
                    if let Some(end) = self.src[self.pos..].find(&close) {
                        out.push_str(&self.src[self.pos..self.pos + end + 2]);
                        self.pos += end + 2;
                    }
                }
                ']' if in_bracket => {
                    in_bracket = false;
                    out.push(']');
                }
                c => out.push(c),
            }
        }
        Ok(Tok::Regex(out))
    }
}

/// Process awk string escapes in command-line text (`-v`, `var=value`
/// operands, `-F`), as in a string literal.
pub(super) fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = chars.next() else {
            out.push('\\');
            break;
        };
        match e {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '/' => out.push('/'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            'v' => out.push('\x0b'),
            '0'..='7' => {
                let mut v = e as u32 - '0' as u32;
                for _ in 0..2 {
                    match chars.peek() {
                        Some(d @ '0'..='7') => {
                            v = v * 8 + (*d as u32 - '0' as u32);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                push_byte_char(&mut out, v);
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Push a byte-valued escape: ASCII as is, other values as the Unicode
/// character with that code (the input is handled as UTF-8 text).
fn push_byte_char(out: &mut String, v: u32) {
    if let Some(c) = char::from_u32(v) {
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        let mut l = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let (t, _) = l.next().ok().unwrap();
            if t == Tok::Eof {
                break;
            }
            out.push(t);
        }
        out
    }

    #[test]
    fn numbers() {
        assert_eq!(
            toks("0x1A 011 018 1.5e2 .5"),
            vec![
                Tok::Num(26.0),
                Tok::Num(9.0),
                Tok::Num(18.0),
                Tok::Num(150.0),
                Tok::Num(0.5)
            ]
        );
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(
            toks(r#""a\tb\101\x41\.""#),
            vec![Tok::Str("a\tbAA.".into())]
        );
    }

    #[test]
    fn operators_and_words() {
        assert_eq!(
            toks("x **= 2; f(1) g (2) length"),
            vec![
                Tok::Name("x".into()),
                Tok::PowAssign,
                Tok::Num(2.0),
                Tok::Semi,
                Tok::FuncName("f".into()),
                Tok::LParen,
                Tok::Num(1.0),
                Tok::RParen,
                Tok::Name("g".into()),
                Tok::LParen,
                Tok::Num(2.0),
                Tok::RParen,
                Tok::Builtin("length".into())
            ]
        );
    }

    #[test]
    fn regex_literal_with_slash_in_class() {
        let mut l = Lexer::new("/[/]x\\/y/ z");
        assert_eq!(l.read_regex(0).ok().unwrap(), Tok::Regex("[/]x/y".into()));
    }

    #[test]
    fn comments_and_continuations() {
        assert_eq!(
            toks("a # c\n\\\nb"),
            vec![Tok::Name("a".into()), Tok::Newline, Tok::Name("b".into())]
        );
    }
}
