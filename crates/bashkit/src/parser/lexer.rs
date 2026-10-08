//! Lexer for bash scripts
//!
//! Tokenizes input into a stream of tokens with source position tracking.

use std::collections::VecDeque;

use super::span::{Position, Span};
use super::tokens::Token;

/// A token with its source location span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
    /// Source text of a word token, only while raw capture is on (function
    /// bodies, for `type`/`declare -f` printing).
    pub raw: Option<String>,
}

/// Maximum nesting depth for command substitution in the lexer.
/// THREAT[TM-DOS-044]: Prevents stack overflow from deeply nested $() patterns.
const DEFAULT_MAX_SUBST_DEPTH: usize = 50;

// Important decision: a word that starts quoted and continues with an
// unquoted expansion marks its quoted spans with the parser's quote-boundary
// markers (`\u{1e}`/`\u{1f}`), which `parse_word` turns into per-part
// quotedness (`Word::part_quoted`), so later unquoted continuations split
// without splitting the quoted prefix. No in-band marker survives into
// expansion, so data bytes like `\x01` stay data. Marker insertion is
// one-pass, never repeated String::insert, to avoid parser DoS.
const QUOTED_SEGMENT_START: char = '\u{1e}';
const QUOTED_SEGMENT_END: char = '\u{1f}';

#[derive(Default)]
struct ContinuationFlags {
    has_unquoted_expansion: bool,
    has_unquoted_glob: bool,
    quoted_ranges: Vec<(usize, usize)>,
    error: Option<String>,
}

/// Bash parses a backquoted command when it runs, so a syntax error there
/// fails only that substitution (status 2), unlike `$(...)` which fails the
/// whole command at parse time. A body that does not parse becomes
/// `eval 'body'`, which reports the same error when (and only if) it runs.
fn defer_backtick_syntax_error(buf: &mut String, body_start: usize) {
    let Some(body) = buf.get(body_start..) else {
        return;
    };
    if body.trim().is_empty() || super::Parser::new(body).parse().is_ok() {
        return;
    }
    let deferred = format!("eval {}", super::raw::single_quote(body));
    buf.truncate(body_start);
    buf.push_str(&deferred);
}

/// Concrete (drop-free) type of [`Lexer::lookahead`], so borrows end early.
type Lookahead<'s, 'a> = std::iter::Peekable<
    std::iter::Chain<
        std::iter::Copied<std::collections::vec_deque::Iter<'s, char>>,
        std::iter::Peekable<std::str::Chars<'a>>,
    >,
>;

/// Lexer for bash scripts.
pub struct Lexer<'a> {
    #[allow(dead_code)] // Stored for error reporting in future
    input: &'a str,
    /// Current position in the input
    position: Position,
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    /// Buffer for re-injected characters (e.g., rest-of-line after heredoc delimiter).
    /// Consumed before `chars`.
    reinject_buf: VecDeque<char>,
    /// Maximum allowed nesting depth for command substitution
    max_subst_depth: usize,
    /// Record each word token's source text (see [`SpannedToken::raw`]).
    /// Important decision: captured from consumed chars, not span offsets,
    /// so text re-injected after a heredoc body still maps to its own token.
    capture_raw: bool,
    raw_buf: String,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input.
    pub fn new(input: &'a str) -> Self {
        Self::with_max_subst_depth(input, DEFAULT_MAX_SUBST_DEPTH)
    }

    /// Create a new lexer with a custom max substitution nesting depth.
    /// THREAT[TM-DOS-044]: Limits recursion in read_command_subst_into().
    pub fn with_max_subst_depth(input: &'a str, max_depth: usize) -> Self {
        Self {
            input,
            position: Position::new(),
            chars: input.chars().peekable(),
            reinject_buf: VecDeque::new(),
            max_subst_depth: max_depth,
            capture_raw: false,
            raw_buf: String::new(),
        }
    }

    /// Move the line count forward by `by` (for input embedded at a later line).
    pub(crate) fn shift_lines(&mut self, by: usize) {
        self.position.line += by;
    }

    /// Get the current position in the input.
    pub fn position(&self) -> Position {
        self.position
    }

    /// Get the next token from the input (without span info).
    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();
        self.next_token_inner()
    }

    fn peek_char(&mut self) -> Option<char> {
        if let Some(&ch) = self.reinject_buf.front() {
            Some(ch)
        } else {
            self.chars.peek().copied()
        }
    }

    fn advance(&mut self) -> Option<char> {
        let ch = if !self.reinject_buf.is_empty() {
            self.reinject_buf.pop_front()
        } else {
            self.chars.next()
        };
        if let Some(c) = ch {
            self.position.advance(c);
            if self.capture_raw {
                self.raw_buf.push(c);
            }
        }
        ch
    }

    /// Upcoming characters without consuming them: re-injected text first
    /// (the rest of a heredoc line), then the input.
    fn lookahead(&self) -> Lookahead<'_, 'a> {
        self.reinject_buf
            .iter()
            .copied()
            .chain(self.chars.clone())
            .peekable()
    }

    /// Whether word-token source capture is on.
    pub fn capture_raw(&self) -> bool {
        self.capture_raw
    }

    /// Turn word-token source capture on or off.
    pub fn set_capture_raw(&mut self, on: bool) {
        self.capture_raw = on;
    }

    /// Get the next token with its source span.
    pub fn next_spanned_token(&mut self) -> Option<SpannedToken> {
        self.skip_whitespace();
        let start = self.position;
        self.raw_buf.clear();
        let token = self.next_token_inner()?;
        let end = self.position;
        let raw = (self.capture_raw
            && matches!(
                token,
                Token::Word(_)
                    | Token::LiteralWord(_)
                    | Token::QuotedWord(_)
                    | Token::QuotedGlobWord(_)
            ))
        .then(|| std::mem::take(&mut self.raw_buf));
        Some(SpannedToken {
            token,
            span: Span::from_positions(start, end),
            raw,
        })
    }

    /// Internal: get next token without recording position (called after whitespace skip)
    fn next_token_inner(&mut self) -> Option<Token> {
        let ch = self.peek_char()?;

        match ch {
            '\n' => {
                self.advance();
                Some(Token::Newline)
            }
            ';' => {
                self.advance();
                if self.peek_char() == Some(';') {
                    self.advance();
                    if self.peek_char() == Some('&') {
                        self.advance();
                        Some(Token::DoubleSemiAmp) // ;;&
                    } else {
                        Some(Token::DoubleSemicolon) // ;;
                    }
                } else if self.peek_char() == Some('&') {
                    self.advance();
                    Some(Token::SemiAmp) // ;&
                } else {
                    Some(Token::Semicolon)
                }
            }
            '|' => {
                self.advance();
                if self.peek_char() == Some('|') {
                    self.advance();
                    Some(Token::Or)
                } else if self.peek_char() == Some('&') {
                    self.advance();
                    Some(Token::PipeBoth)
                } else {
                    Some(Token::Pipe)
                }
            }
            '&' => {
                self.advance();
                if self.peek_char() == Some('&') {
                    self.advance();
                    Some(Token::And)
                } else if self.peek_char() == Some('>') {
                    self.advance();
                    if self.peek_char() == Some('>') {
                        self.advance();
                        Some(Token::RedirectBothAppend)
                    } else {
                        Some(Token::RedirectBoth)
                    }
                } else {
                    Some(Token::Background)
                }
            }
            '>' => {
                self.advance();
                if self.peek_char() == Some('>') {
                    self.advance();
                    Some(Token::RedirectAppend)
                } else if self.peek_char() == Some('|') {
                    self.advance();
                    Some(Token::Clobber)
                } else if self.peek_char() == Some('(') {
                    self.advance();
                    Some(Token::ProcessSubOut)
                } else if self.peek_char() == Some('&') {
                    self.advance();
                    Some(Token::DupOutput)
                } else {
                    Some(Token::RedirectOut)
                }
            }
            '<' => {
                self.advance();
                if self.peek_char() == Some('<') {
                    self.advance();
                    if self.peek_char() == Some('<') {
                        self.advance();
                        Some(Token::HereString)
                    } else if self.peek_char() == Some('-') {
                        self.advance();
                        Some(Token::HereDocStrip)
                    } else {
                        Some(Token::HereDoc)
                    }
                } else if self.peek_char() == Some('(') {
                    self.advance();
                    Some(Token::ProcessSubIn)
                } else if self.peek_char() == Some('&') {
                    self.advance();
                    Some(Token::DupInput)
                } else if self.peek_char() == Some('>') {
                    self.advance();
                    Some(Token::RedirectReadWrite)
                } else {
                    Some(Token::RedirectIn)
                }
            }
            '(' => {
                self.advance();
                if self.peek_char() == Some('(') {
                    self.advance();
                    Some(Token::DoubleLeftParen)
                } else {
                    Some(Token::LeftParen)
                }
            }
            ')' => {
                self.advance();
                if self.peek_char() == Some(')') {
                    self.advance();
                    Some(Token::DoubleRightParen)
                } else {
                    Some(Token::RightParen)
                }
            }
            '{' => {
                // Look ahead to see if this is a brace expansion like {a,b,c} or {1..5}
                // vs a brace group like { cmd; }
                // Note: { must be followed by space/newline to be a brace group
                if self.is_brace_group_start() {
                    self.advance();
                    Some(Token::LeftBrace)
                } else {
                    // Any other `{` starts an ordinary word: braces are plain
                    // word characters to the lexer, and brace expansion runs
                    // on the parsed word (quotes and `$` handled as usual).
                    self.read_word()
                }
            }
            '}' => {
                // `}` closes a brace group only as a standalone word, the way
                // `{ cmd; }` writes it. Anything glued to it is an ordinary
                // word: bash runs `}b` as a command named `}b`, and echoes
                // `}b` as an argument.
                let stands_alone = self.right_brace_stands_alone();
                self.advance();
                if stands_alone {
                    Some(Token::RightBrace)
                } else {
                    // `read_word_starting_with` seeds the word with the prefix
                    // and reads from the cursor, so `}` must already be consumed.
                    self.read_word_starting_with("}")
                }
            }
            '[' => {
                self.advance();
                // `[[` is the keyword only as a whole word; `[[:digit:]]*` is
                // a glob bracket expression.
                let mut lookahead = self.lookahead();
                let keyword = lookahead.next() == Some('[')
                    && matches!(
                        lookahead.next(),
                        None | Some(' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')')
                    );
                if keyword {
                    self.advance();
                    Some(Token::DoubleLeftBracket)
                } else {
                    // [ could be the test command OR a glob bracket expression
                    // If followed by non-whitespace, treat as start of bracket expression
                    // e.g., [abc] is a glob pattern, [ -f file ] is test command
                    // But ["$*"] or ['text'] are NOT glob — they are literal [ + quoted word
                    match self.peek_char() {
                        Some(' ') | Some('\t') | Some('\n') | None => {
                            // Followed by whitespace or EOF - it's the test command
                            Some(Token::Word("[".to_string()))
                        }
                        Some('"') | Some('\'') | Some('$') => {
                            // [ followed by quote/expansion — treat as part of a regular word.
                            // Push [ back and read the entire word normally.
                            self.read_word_starting_with("[")
                        }
                        _ => {
                            // Part of a glob bracket expression [abc], read the whole thing
                            self.read_bracket_word()
                        }
                    }
                }
            }
            ']' => {
                // `]]` closes `[[` only as a whole word; `]]x` and `]x` are
                // ordinary words, like bash.
                let ends_word = |c: Option<char>| {
                    matches!(
                        c,
                        None | Some(' ' | '\t' | '\n' | ';' | '|' | '&' | '(' | ')' | '<' | '>')
                    )
                };
                self.advance();
                let mut ahead = self.lookahead();
                let double = ahead.next() == Some(']') && ends_word(ahead.next());
                if double {
                    self.advance();
                    Some(Token::DoubleRightBracket)
                } else if ends_word(self.peek_char()) {
                    Some(Token::Word("]".to_string()))
                } else {
                    self.read_word_starting_with("]")
                }
            }
            '\'' => self.read_single_quoted_string(),
            '"' => self.read_double_quoted_string(),
            '#' => {
                // Comment - skip to end of line
                self.skip_comment();
                self.next_token_inner()
            }
            // Handle file descriptor redirects like 2> or 2>&1
            '0'..='9' => self.read_word_or_fd_redirect(),
            _ => self.read_word(),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch == ' ' || ch == '\t' {
                self.advance();
            } else if ch == '\\' {
                // Check for backslash-newline (line continuation) between tokens
                let mut lookahead = self.lookahead();
                lookahead.next(); // skip backslash
                if lookahead.next() == Some('\n') {
                    self.advance(); // consume backslash
                    self.advance(); // consume newline
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    /// Check if this is a file descriptor redirect (e.g., 2>, 2>>, 2>&1,
    /// 10>file, 3<<<word, 4<>file) or just a regular word starting with a
    /// digit. Bash accepts any decimal fd number; more than
    /// `MAX_FD_PREFIX_DIGITS` digits (out of `int` range) is a plain word.
    fn read_word_or_fd_redirect(&mut self) -> Option<Token> {
        const MAX_FD_PREFIX_DIGITS: usize = 9;
        // Only the digits plus a 3-char redirect operator (e.g. "<<<", ">>")
        // matter, so bound the lookahead: collecting all remaining input here
        // made every digit-initial word O(n) and the whole lex O(n^2)
        // (TM-DOS-024).
        let input_remaining: String = self.lookahead().take(MAX_FD_PREFIX_DIGITS + 4).collect();
        let ndigits = input_remaining
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if ndigits == 0 || ndigits > MAX_FD_PREFIX_DIGITS {
            return self.read_word();
        }
        let Ok(fd) = input_remaining[..ndigits].parse::<i32>() else {
            return self.read_word();
        };
        let rest = &input_remaining[ndigits..];
        // Consume the fd digits plus `op_len` operator chars.
        let consume = |lexer: &mut Self, op_len: usize| {
            for _ in 0..ndigits + op_len {
                lexer.advance();
            }
        };

        if rest.starts_with(">>") {
            // N>> - append redirect with fd
            consume(self, 2);
            return Some(Token::RedirectFdAppend(fd));
        } else if rest.starts_with(">&") {
            // N>&M - duplicate fd
            consume(self, 2);

            // Read the target fd number or '-'
            let mut target_str = String::new();
            while let Some(c) = self.peek_char() {
                if c.is_ascii_digit() || c == '-' {
                    target_str.push(c);
                    self.advance();
                    if c == '-' {
                        break;
                    }
                } else {
                    break;
                }
            }

            if target_str == "-" {
                return Some(Token::DupFdCloseOut(fd));
            }
            if target_str.is_empty() {
                // Just N>& without target - treat as DupOutput with fd
                return Some(Token::RedirectFd(fd));
            }

            let target_fd: i32 = target_str.parse().unwrap_or(1);
            return Some(Token::DupFd(fd, target_fd));
        } else if rest.starts_with('>') {
            // N> - redirect with fd
            consume(self, 1);
            return Some(Token::RedirectFd(fd));
        } else if rest.starts_with("<&") {
            // N<&M or N<&- - duplicate input fd
            consume(self, 2);

            // Read the target fd number or '-'
            let mut target_str = String::new();
            while let Some(c) = self.peek_char() {
                if c.is_ascii_digit() || c == '-' {
                    target_str.push(c);
                    self.advance();
                    if c == '-' {
                        break;
                    }
                } else {
                    break;
                }
            }

            if target_str == "-" {
                return Some(Token::DupFdClose(fd));
            }
            let target_fd: i32 = target_str.parse().unwrap_or(0);
            return Some(Token::DupFdIn(fd, target_fd));
        } else if rest.starts_with("<<<") {
            // N<<<word - here string on fd N
            consume(self, 3);
            return Some(Token::HereStringFd(fd));
        } else if rest.starts_with("<<") {
            // N<<EOF / N<<-EOF - here document on fd N
            consume(self, 2);
            let strip = self.peek_char() == Some('-');
            if strip {
                self.advance();
            }
            return Some(Token::HereDocFd(fd, strip));
        } else if rest.starts_with("<>") {
            // N<>file - open file read-write on fd N
            consume(self, 2);
            return Some(Token::RedirectFdReadWrite(fd));
        } else if rest.starts_with('<') {
            // N< - input redirect with fd
            consume(self, 1);
            return Some(Token::RedirectFdIn(fd));
        }

        // Not a fd redirect pattern, read as regular word
        self.read_word()
    }

    /// Consume a backtick command substitution at the cursor and append it
    /// to `word` as `$(cmd)`.
    fn read_backtick_into(&mut self, word: &mut String) -> Result<(), String> {
        self.advance(); // consume opening `
        word.push_str("$(");
        let body_start = word.len();
        let mut closed = false;
        while let Some(c) = self.peek_char() {
            if c == '`' {
                self.advance(); // consume closing `
                closed = true;
                break;
            }
            if c == '\\' {
                // In backticks, backslash only escapes $, `, \, newline
                self.advance();
                if let Some(next) = self.peek_char() {
                    if matches!(next, '$' | '`' | '\\' | '\n') {
                        word.push(next);
                        self.advance();
                    } else {
                        word.push('\\');
                        word.push(next);
                        self.advance();
                    }
                }
            } else {
                word.push(c);
                self.advance();
            }
        }
        if !closed {
            return Err("unterminated backtick substitution".to_string());
        }
        defer_backtick_syntax_error(word, body_start);
        word.push(')');
        Ok(())
    }

    fn read_word_starting_with(&mut self, prefix: &str) -> Option<Token> {
        let mut word = prefix.to_string();
        let mut has_quoted_expansion = false;
        // Use the same logic as read_word but with pre-seeded content
        while let Some(ch) = self.peek_char() {
            if ch == '"' || ch == '\'' {
                // Word already has content (the prefix) — concatenate the quoted segment
                let quote_char = ch;
                self.advance();
                let mut closed = false;
                if quote_char == '"' {
                    word.push('\u{1e}');
                }
                while let Some(c) = self.peek_char() {
                    if c == quote_char {
                        self.advance();
                        closed = true;
                        break;
                    }
                    if c == '\\' && quote_char == '"' {
                        self.advance();
                        if let Some(next) = self.peek_char() {
                            match next {
                                '\n' => {
                                    self.advance();
                                }
                                '$' => {
                                    // Use NUL sentinel so parse_word() treats this
                                    // as a literal '$' rather than a variable expansion.
                                    word.push('\x00');
                                    word.push('$');
                                    self.advance();
                                }
                                '"' | '\\' | '`' => {
                                    word.push(next);
                                    self.advance();
                                }
                                _ => {
                                    word.push('\\');
                                    word.push(next);
                                    self.advance();
                                }
                            }
                            continue;
                        }
                    }
                    // Track quoted expansions for IFS-split suppression
                    if c == '$' && quote_char == '"' {
                        self.advance();
                        if self.take_dquote_dollar(&mut word) {
                            has_quoted_expansion = true;
                            continue;
                        }
                        if self.peek_char().is_some_and(|nc| {
                            nc.is_ascii_alphanumeric()
                                || nc == '_'
                                || matches!(nc, '{' | '(' | '?' | '#' | '@' | '*' | '!' | '$' | '-')
                        }) {
                            has_quoted_expansion = true;
                        }
                        // Read `$(...)`/`${...}` whole so quotes inside them
                        // keep their own meaning.
                        if self.peek_char() == Some('(') {
                            word.push('(');
                            self.advance();
                            self.read_command_subst_into(&mut word);
                        } else if self.peek_char() == Some('{') {
                            word.push('{');
                            self.advance();
                            if let Err(msg) = self.read_param_expansion_into(&mut word) {
                                return Some(Token::Error(msg));
                            }
                        }
                        continue;
                    }
                    if quote_char == '\'' && c == '$' {
                        // Preserve literal '$' semantics from single-quoted
                        // segments when concatenated into an existing word
                        // (e.g. foo'$(id)' or VAR='${HOME}').
                        word.push('\x00');
                    }
                    word.push(c);
                    self.advance();
                }
                if !closed {
                    return Some(Token::Error(format!(
                        "unterminated {} quote",
                        if quote_char == '\'' {
                            "single"
                        } else {
                            "double"
                        }
                    )));
                }
                if quote_char == '"' {
                    word.push('\u{1f}');
                }
                continue;
            } else if ch == '$' {
                word.push(ch);
                self.advance();
                // Read variable/expansion following $
                if let Some(nc) = self.peek_char() {
                    if nc == '{' {
                        word.push(nc);
                        self.advance();
                        if let Err(e) = self.read_unquoted_param_body(&mut word) {
                            return Some(Token::Error(e));
                        }
                    } else if nc == '(' {
                        word.push(nc);
                        self.advance();
                        let (open, close) = ('(', ')');
                        let mut depth = 1;
                        while let Some(bc) = self.peek_char() {
                            word.push(bc);
                            self.advance();
                            if bc == open {
                                depth += 1;
                            } else if bc == close {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                        }
                    } else if nc.is_ascii_alphanumeric()
                        || nc == '_'
                        || matches!(nc, '?' | '#' | '@' | '*' | '!' | '$' | '-')
                    {
                        word.push(nc);
                        self.advance();
                        if nc.is_ascii_alphabetic() || nc == '_' {
                            while let Some(vc) = self.peek_char() {
                                if vc.is_ascii_alphanumeric() || vc == '_' {
                                    word.push(vc);
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
                continue;
            } else if ch == '`' {
                if let Err(e) = self.read_backtick_into(&mut word) {
                    return Some(Token::Error(e));
                }
            } else if self.is_word_char(ch) || matches!(ch, ']' | '{' | '}') {
                // `}` included for the same reason as in `read_word`: it is a
                // reserved word, not a metacharacter, so it stays inside the
                // word unless it stands alone (`echo }}` prints `}}`). `{`
                // keeps a brace tail in the word (`[k]="v"{1,2}`).
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if has_quoted_expansion {
            Some(Token::QuotedWord(word))
        } else {
            Some(Token::Word(word))
        }
    }

    fn read_word(&mut self) -> Option<Token> {
        let mut word = String::new();
        // Track whether any double-quoted segment contained a variable/command
        // expansion.  When true the whole token is promoted to QuotedWord so
        // the interpreter suppresses IFS field splitting — matching POSIX
        // behaviour for words like  +"$fmt"  or  prefix"$var"suffix.
        let mut has_quoted_expansion = false;
        // Track whether any glob metacharacter (*, ?, [) appears in an
        // unquoted portion of the word.  When both `has_quoted_expansion` and
        // this flag are true, the word needs IFS-splitting suppression (quoted)
        // *and* glob expansion on the unquoted portion — e.g. `"$var"*.ext`.
        let mut has_unquoted_glob = false;
        // Byte ranges of quoted or backslash-escaped text, and whether an
        // unquoted `$`/backtick expansion appears. Together they decide how
        // quoted glob characters stay literal (see the end of this function).
        let mut quoted_ranges: Vec<(usize, usize)> = Vec::new();
        let mut has_unquoted_expansion = false;

        while let Some(ch) = self.peek_char() {
            // Handle quoted strings within words (e.g., a="Hello" or VAR="value")
            // This handles the case where a word like `a=` is followed by a quoted string
            if ch == '"' || ch == '\'' {
                if word.is_empty() {
                    // Start of a new token — let the main tokenizer handle quotes
                    break;
                }
                // Word already has content — concatenate the quoted segment
                // This handles: VAR="val", date +"%Y", echo foo"bar"
                let quote_char = ch;
                self.advance(); // consume opening quote
                let mut closed = false;
                if quote_char == '"' {
                    word.push('\u{1e}');
                }
                let seg_start = word.len();
                while let Some(c) = self.peek_char() {
                    if c == quote_char {
                        self.advance(); // consume closing quote
                        closed = true;
                        break;
                    }
                    if c == '\\' && quote_char == '"' {
                        self.advance();
                        if let Some(next) = self.peek_char() {
                            match next {
                                '\n' => {
                                    // \<newline> is line continuation: discard both
                                    self.advance();
                                }
                                '$' => {
                                    // Use NUL sentinel so parse_word() treats this
                                    // as a literal '$' rather than a variable expansion.
                                    word.push('\x00');
                                    word.push('$');
                                    self.advance();
                                }
                                '"' | '\\' | '`' => {
                                    word.push(next);
                                    self.advance();
                                }
                                _ => {
                                    word.push('\\');
                                    word.push(next);
                                    self.advance();
                                }
                            }
                            continue;
                        }
                    }
                    // Handle $(...) inside double-quoted word segments
                    // to preserve single-quoted strings within command substitutions
                    if c == '$' && quote_char == '"' {
                        self.advance();
                        if self.take_dquote_dollar(&mut word) {
                            has_quoted_expansion = true;
                            continue;
                        }
                        // Mark that this word contains a quoted expansion so IFS
                        // splitting is suppressed (e.g. +"$fmt" stays one field).
                        if self.peek_char().is_some_and(|nc| {
                            nc.is_ascii_alphanumeric()
                                || nc == '_'
                                || matches!(nc, '{' | '(' | '?' | '#' | '@' | '*' | '!' | '$' | '-')
                        }) {
                            has_quoted_expansion = true;
                        }
                        if self.peek_char() == Some('(') {
                            word.push('(');
                            self.advance();
                            self.read_command_subst_into(&mut word);
                            continue;
                        }
                        if self.peek_char() == Some('{') {
                            // `${...}` body in double-quote context, so a
                            // `$'...'` operand inside it stays ANSI-C quoting.
                            word.push('{');
                            self.advance();
                            if let Err(msg) = self.read_param_expansion_into(&mut word) {
                                return Some(Token::Error(msg));
                            }
                        }
                        continue;
                    }
                    if quote_char == '\'' && c == '$' {
                        // Preserve literal '$' semantics from single-quoted
                        // segments when concatenated into an existing word
                        // (e.g. foo'$(id)' or VAR='${HOME}').
                        word.push('\x00');
                    }
                    word.push(c);
                    self.advance();
                }
                if !closed {
                    return Some(Token::Error(format!(
                        "unterminated {} quote",
                        if quote_char == '\'' {
                            "single"
                        } else {
                            "double"
                        }
                    )));
                }
                quoted_ranges.push((seg_start, word.len()));
                if quote_char == '"' {
                    word.push('\u{1f}');
                }
                continue;
            } else if ch == '$' {
                // Handle variable references and command substitution
                self.advance();

                // $'...' — ANSI-C quoting: resolve escapes at parse time
                if self.peek_char() == Some('\'') {
                    self.advance(); // consume opening '
                    word.push('\u{1e}');
                    let (content, closed) = self.read_dollar_single_quoted_content();
                    if !closed {
                        return Some(Token::Error("unterminated single quote".to_string()));
                    }
                    let seg_start = word.len();
                    Self::push_literal_with_escaped_dollar(&mut word, &content);
                    quoted_ranges.push((seg_start, word.len()));
                    word.push('\u{1f}');
                    // ANSI-C quotes are single-quote semantics: quoted context.
                    has_quoted_expansion = true;
                    continue;
                }

                // $"..." — locale translation synonym, treated like "..."
                if self.peek_char() == Some('"') {
                    self.advance(); // consume opening "
                    // Locale quotes are double-quote semantics: quoted context.
                    has_quoted_expansion = true;
                    word.push('\u{1e}');
                    let seg_start = word.len();
                    let mut closed = false;
                    while let Some(c) = self.peek_char() {
                        if c == '"' {
                            self.advance();
                            closed = true;
                            break;
                        }
                        if c == '\\' {
                            self.advance();
                            if let Some(next) = self.peek_char() {
                                match next {
                                    '\n' => {
                                        self.advance();
                                    }
                                    '$' => {
                                        word.push('\x00');
                                        word.push('$');
                                        self.advance();
                                    }
                                    '"' | '\\' | '`' => {
                                        word.push(next);
                                        self.advance();
                                    }
                                    _ => {
                                        word.push('\\');
                                        word.push(next);
                                        self.advance();
                                    }
                                }
                                continue;
                            }
                        }
                        if c == '$' {
                            self.advance();
                            if self.take_dquote_dollar(&mut word) {
                                continue;
                            }
                            if let Some(nc) = self.peek_char() {
                                if nc == '{' {
                                    word.push(nc);
                                    self.advance();
                                    while let Some(bc) = self.peek_char() {
                                        word.push(bc);
                                        self.advance();
                                        if bc == '}' {
                                            break;
                                        }
                                    }
                                } else if nc == '(' {
                                    word.push(nc);
                                    self.advance();
                                    let mut depth = 1;
                                    while let Some(pc) = self.peek_char() {
                                        word.push(pc);
                                        self.advance();
                                        if pc == '(' {
                                            depth += 1;
                                        } else if pc == ')' {
                                            depth -= 1;
                                            if depth == 0 {
                                                break;
                                            }
                                        }
                                    }
                                } else if nc.is_ascii_alphanumeric()
                                    || nc == '_'
                                    || matches!(nc, '?' | '#' | '@' | '*' | '!' | '$' | '-')
                                {
                                    word.push(nc);
                                    self.advance();
                                    if nc.is_ascii_alphabetic() || nc == '_' {
                                        while let Some(vc) = self.peek_char() {
                                            if vc.is_ascii_alphanumeric() || vc == '_' {
                                                word.push(vc);
                                                self.advance();
                                            } else {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            continue;
                        }
                        word.push(c);
                        self.advance();
                    }
                    if !closed {
                        return Some(Token::Error("unterminated double quote".to_string()));
                    }
                    quoted_ranges.push((seg_start, word.len()));
                    word.push('\u{1f}');
                    continue;
                }

                has_unquoted_expansion = true;
                word.push(ch); // push the '$'

                // Check for $( - command substitution or arithmetic
                if self.peek_char() == Some('(') {
                    word.push('(');
                    self.advance();

                    // Check for $(( - arithmetic expansion
                    if self.peek_char() == Some('(') {
                        word.push('(');
                        self.advance();
                        // Read until ))
                        let mut depth = 2;
                        while let Some(c) = self.peek_char() {
                            word.push(c);
                            self.advance();
                            if c == '(' {
                                depth += 1;
                            } else if c == ')' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                        }
                    } else {
                        // Quote/heredoc-aware end of the body (see subst_scan).
                        // Nesting depth is bounded where bodies are parsed
                        // (parser max_depth) and run (max_subst_depth).
                        if !self.read_command_subst_body(&mut word) {
                            return Some(Token::Error(
                                "unterminated command substitution".to_string(),
                            ));
                        }
                    }
                } else if self.peek_char() == Some('{') {
                    // ${VAR} format — track nested braces so ${a[${#b[@]}]}
                    // doesn't stop at the inner }.
                    word.push('{');
                    self.advance();
                    if let Err(e) = self.read_unquoted_param_body(&mut word) {
                        return Some(Token::Error(e));
                    }
                } else {
                    // Check for special single-character variables ($?, $#, $@, $*, $!, $$, $-, $0-$9)
                    if let Some(c) = self.peek_char() {
                        if matches!(c, '?' | '#' | '@' | '*' | '!' | '$' | '-')
                            || c.is_ascii_digit()
                        {
                            word.push(c);
                            self.advance();
                        } else {
                            // Read variable name (alphanumeric + _)
                            while let Some(c) = self.peek_char() {
                                if c.is_ascii_alphanumeric() || c == '_' {
                                    word.push(c);
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
            } else if ch == '{' {
                // Brace expansion pattern - include entire {...} in word.
                // Counts as unquoted glob syntax: a word mixing it with quoted
                // text (`a{1,2}"*"`) must still reach brace expansion.
                has_unquoted_glob = true;
                word.push(ch);
                self.advance();
            } else if ch == '`' {
                // Backtick command substitution: convert `cmd` to $(cmd)
                has_unquoted_expansion = true;
                if let Err(e) = self.read_backtick_into(&mut word) {
                    return Some(Token::Error(e));
                }
            } else if ch == '\\' {
                self.advance();
                if let Some(next) = self.peek_char() {
                    if next == '\n' {
                        // Line continuation: skip backslash + newline
                        self.advance();
                    } else {
                        // Escaped character: backslash quotes the next char
                        // (quote removal — only the literal char survives).
                        // `\$` and `\`` must stay literal through parse_word, so
                        // they carry the NUL sentinel like quoted `\$` does.
                        if matches!(next, '$' | '`') {
                            word.push('\x00');
                        }
                        if Self::is_glob_escape_char(next) {
                            quoted_ranges.push((word.len(), word.len() + next.len_utf8()));
                        }
                        word.push(next);
                        self.advance();
                    }
                } else {
                    word.push('\\');
                }
            } else if ch == '(' && word.ends_with(['@', '?', '*', '+', '!']) {
                // Extglob: @(...), ?(...), *(...), +(...), !(...)
                // Consume through matching ) including nested parens
                word.push(ch);
                self.advance();
                let mut depth = 1;
                while let Some(c) = self.peek_char() {
                    word.push(c);
                    self.advance();
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        '\\' => {
                            if let Some(esc) = self.peek_char() {
                                if matches!(esc, '$' | '`') {
                                    // Keep `\$` literal through parse_word.
                                    word.pop();
                                    Self::push_extglob_literal(&mut word, esc);
                                } else {
                                    word.push(esc);
                                }
                                self.advance();
                            }
                        }
                        // Quotes inside a group make their text literal:
                        // `@(a|'b)')` has the alternative `b)`.
                        '\'' | '"' => {
                            word.pop();
                            let quote = c;
                            while let Some(q) = self.peek_char() {
                                self.advance();
                                if q == quote {
                                    break;
                                }
                                if quote == '"'
                                    && q == '\\'
                                    && let Some(esc) = self.peek_char()
                                    && matches!(esc, '"' | '\\' | '$' | '`')
                                {
                                    self.advance();
                                    Self::push_extglob_literal(&mut word, esc);
                                    continue;
                                }
                                if quote == '"' && matches!(q, '$' | '`') {
                                    // Expansions still run inside "...".
                                    word.push(q);
                                    continue;
                                }
                                Self::push_extglob_literal(&mut word, q);
                            }
                        }
                        _ => {}
                    }
                }
            } else if ch == '}' {
                // An unmatched `}` inside a word is a literal. `}` is a
                // reserved word, not a metacharacter, so it only terminates a
                // word when it stands alone — bash prints `a}b`, not `a } b`.
                // Brace expansion happens later, on the parsed word.
                word.push(ch);
                self.advance();
            } else if self.is_word_char(ch) {
                // Track glob metacharacters in unquoted portions
                if matches!(ch, '*' | '?' | '[') {
                    has_unquoted_glob = true;
                }
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Quoted or escaped glob characters are literal (`a"*"`, `a\*`,
        // `a'?'`). With an unquoted glob in the same word they are escaped
        // in place, the QuotedGlobWord convention also used for words that
        // start with a quote. Without one the word never globs, so it is
        // simply quoted. A word with an unquoted expansion keeps the old
        // classification: quoting it would suppress field splitting.
        let quoted_glob = quoted_ranges
            .iter()
            .any(|&(start, end)| word[start..end].contains(['*', '?', '[', '{', '}', ',']));
        if quoted_glob && has_unquoted_glob {
            return Some(Token::QuotedGlobWord(
                Self::escape_glob_metas_in_quoted_ranges(&word, &quoted_ranges),
            ));
        }
        if quoted_glob && !has_unquoted_expansion {
            return Some(Token::QuotedWord(word));
        }

        if word.is_empty() {
            None
        } else if has_quoted_expansion && has_unquoted_glob {
            // Mixed quoted/unquoted word with glob chars in the unquoted
            // portion — e.g. `"$var"*.ext`.  Suppress IFS splitting (quoted)
            // but glob expansion must still apply on the unquoted portions.
            Some(Token::QuotedGlobWord(word))
        } else if has_quoted_expansion {
            // A double-quoted segment contained a variable/command expansion.
            // Promote to QuotedWord so the interpreter suppresses IFS field
            // splitting, matching POSIX behaviour for  +"$fmt"  etc.
            Some(Token::QuotedWord(word))
        } else {
            Some(Token::Word(word))
        }
    }

    fn read_single_quoted_string(&mut self) -> Option<Token> {
        self.advance(); // consume opening '
        let mut content = String::new();
        let mut closed = false;

        while let Some(ch) = self.peek_char() {
            if ch == '\'' {
                self.advance(); // consume closing '
                closed = true;
                break;
            }
            content.push(ch);
            self.advance();
        }

        if !closed {
            return Some(Token::Error("unterminated single quote".to_string()));
        }

        // If next char is another quote or word char, concatenate (e.g., 'EOF'"2" -> EOF2).
        // An unquoted expansion or glob in the continuation keeps working
        // (`'a'$x`, `'a'*`); otherwise the whole token is literal. The quoted
        // prefix carries NUL sentinels so `parse_word` never expands its `$`.
        let mut content = {
            let mut escaped = String::with_capacity(content.len());
            Self::push_literal_with_escaped_dollar(&mut escaped, &content);
            escaped
        };
        let quoted_prefix_len = content.len();
        let flags = self.read_continuation_into(&mut content);
        if let Some(error) = flags.error {
            return Some(Token::Error(error));
        }
        if flags.has_unquoted_expansion {
            let mut ranges = flags.quoted_ranges;
            ranges.push((0, quoted_prefix_len));
            Self::apply_quote_markers(&mut content, ranges);
            return Some(Token::Word(content));
        }
        if flags.has_unquoted_glob {
            let mut ranges = flags.quoted_ranges;
            ranges.push((0, quoted_prefix_len));
            ranges.sort_unstable_by_key(|&(s, _)| s);
            return Some(Token::QuotedGlobWord(
                Self::escape_glob_metas_in_quoted_ranges(&content, &ranges),
            ));
        }

        // Single-quoted strings are literal - no variable expansion. Quote-boundary
        // markers are only for parsed mixed words; LiteralWord keeps contents raw.
        // Also decode any NUL escape sentinels from continued double-quoted segments.
        Some(Token::LiteralWord(Self::strip_markers_decode_sentinels(
            &content,
        )))
    }

    /// After a closing quote, read any adjacent quoted or unquoted word chars
    /// into `content`.  Handles concatenation like `'foo'"bar"baz` -> `foobarbaz`.
    fn read_continuation_into(&mut self, content: &mut String) -> ContinuationFlags {
        let mut flags = ContinuationFlags::default();
        loop {
            match self.peek_char() {
                Some('\'') => {
                    self.advance(); // opening '
                    let start = content.len();
                    let mut closed = false;
                    while let Some(ch) = self.peek_char() {
                        if ch == '\'' {
                            self.advance(); // closing '
                            closed = true;
                            break;
                        }
                        if matches!(ch, '\x00' | '$') {
                            content.push('\x00');
                        }
                        content.push(ch);
                        self.advance();
                    }
                    if !closed {
                        flags.error = Some("unterminated single quote".to_string());
                        break;
                    }
                    flags.quoted_ranges.push((start, content.len()));
                }
                Some('"') => {
                    self.advance(); // opening "
                    let start = content.len();
                    let mut closed = false;
                    while let Some(ch) = self.peek_char() {
                        if ch == '"' {
                            self.advance(); // closing "
                            closed = true;
                            break;
                        }
                        if ch == '\\' {
                            self.advance();
                            if let Some(next) = self.peek_char() {
                                match next {
                                    '$' => {
                                        content.push('\x00');
                                        content.push('$');
                                        self.advance();
                                    }
                                    '"' | '\\' | '`' => {
                                        content.push(next);
                                        self.advance();
                                    }
                                    _ => {
                                        content.push('\\');
                                        content.push(next);
                                        self.advance();
                                    }
                                }
                                continue;
                            }
                        }
                        content.push(ch);
                        self.advance();
                    }
                    if !closed {
                        flags.error = Some("unterminated double quote".to_string());
                        break;
                    }
                    flags.quoted_ranges.push((start, content.len()));
                }
                Some('$') => {
                    // Check for $'...' ANSI-C quoting in continuation
                    let mut lookahead = self.lookahead();
                    lookahead.next(); // skip $
                    if lookahead.next() == Some('\'') {
                        self.advance(); // consume $
                        self.advance(); // consume opening '
                        let start = content.len();
                        let (segment, closed) = self.read_dollar_single_quoted_content();
                        if !closed {
                            flags.error = Some("unterminated single quote".to_string());
                            break;
                        }
                        Self::push_literal_with_escaped_dollar(content, &segment);
                        flags.quoted_ranges.push((start, content.len()));
                    } else {
                        flags.has_unquoted_expansion = true;
                        content.push('$');
                        self.advance();
                    }
                }
                Some('\\') => {
                    // Backslash escape after a quoted segment: `'a'\''b'`.
                    self.advance();
                    match self.peek_char() {
                        // Line continuation.
                        Some('\n') => {
                            self.advance();
                        }
                        Some(next) => {
                            let start = content.len();
                            if next == '$' {
                                content.push('\x00');
                            }
                            content.push(next);
                            self.advance();
                            flags.quoted_ranges.push((start, content.len()));
                        }
                        None => content.push('\\'),
                    }
                }
                Some(ch) if self.is_word_char(ch) || matches!(ch, '{' | '}') => {
                    // `{` marks possible brace expansion, like a glob char.
                    if matches!(ch, '*' | '?' | '[' | '{') {
                        flags.has_unquoted_glob = true;
                    }
                    content.push(ch);
                    self.advance();
                }
                _ => break,
            }
        }
        flags
    }

    /// Add quote markers in one rebuild pass. Empty ranges carry no protected
    /// bytes, and adjacent quoted ranges are equivalent to one quoted span.
    fn apply_quote_markers(content: &mut String, mut ranges: Vec<(usize, usize)>) {
        ranges.retain(|(start, end)| start < end);
        if ranges.is_empty() {
            return;
        }
        ranges.sort_unstable();

        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            if let Some((_, last_end)) = merged.last_mut()
                && start <= *last_end
            {
                *last_end = (*last_end).max(end);
                continue;
            }
            merged.push((start, end));
        }

        let mut marked = String::with_capacity(content.len() + merged.len() * 2);
        let mut cursor = 0;
        for (start, end) in merged {
            marked.push_str(&content[cursor..start]);
            marked.push(QUOTED_SEGMENT_START);
            marked.push_str(&content[start..end]);
            marked.push(QUOTED_SEGMENT_END);
            cursor = end;
        }
        marked.push_str(&content[cursor..]);
        *content = marked;
    }

    /// Read ANSI-C quoted content ($'...').
    /// Opening $' already consumed. Returns the resolved string.
    /// Read the body of an unquoted `${...}` after the opening `${`, through
    /// the closing `}`.
    ///
    /// Important decision: quoting inside an unquoted expansion is resolved
    /// here, where the context is known. `'...'` and `$'...'` become a
    /// double-quoted span whose every char is NUL-escaped, so it stays literal
    /// and quoted through operand expansion (no `$`, no glob) and a `}` or `/`
    /// inside it never ends the operand. Inside `"..."` spans and after `\`,
    /// braces are NUL-escaped for the same reason. Bash keeps `'` literal in a
    /// double-quoted `${x:-'d'}`, which `read_param_expansion_into` handles.
    ///
    /// TODO: quoted operand text is still IFS-split when the expansion is
    /// unquoted (`${x:-'a  b'}` gives two fields, bash gives one). Fixing it
    /// needs quote markers carried out of `expand_operand` into field splitting.
    fn read_unquoted_param_body(&mut self, word: &mut String) -> Result<(), String> {
        fn push_escaped(word: &mut String, content: &str) {
            for ch in content.chars() {
                word.push('\x00');
                word.push(ch);
            }
        }
        let mut depth = 1usize;
        // Inside an array subscript (`${a['k']}`) quotes stay raw: subscript
        // evaluation removes them itself.
        let mut subscript = 0usize;
        while let Some(c) = self.peek_char() {
            self.advance();
            match c {
                '[' => {
                    subscript += 1;
                    word.push(c);
                }
                ']' => {
                    subscript = subscript.saturating_sub(1);
                    word.push(c);
                }
                '\'' if subscript > 0 => {
                    word.push(c);
                    while let Some(q) = self.peek_char() {
                        self.advance();
                        word.push(q);
                        if q == '\'' {
                            break;
                        }
                    }
                }
                '\\' => match self.peek_char() {
                    Some(n @ ('{' | '}')) => {
                        self.advance();
                        word.push('\x00');
                        word.push(n);
                    }
                    Some(n) => {
                        self.advance();
                        word.push('\\');
                        word.push(n);
                    }
                    None => word.push('\\'),
                },
                '\'' => {
                    let mut content = String::new();
                    let mut closed = false;
                    while let Some(q) = self.peek_char() {
                        self.advance();
                        if q == '\'' {
                            closed = true;
                            break;
                        }
                        content.push(q);
                    }
                    if !closed {
                        return Err("unterminated single quote".to_string());
                    }
                    word.push('"');
                    push_escaped(word, &content);
                    word.push('"');
                }
                '"' => {
                    word.push('"');
                    // Nested `${` depth inside this span; bare braces are literal.
                    let mut inner = 0usize;
                    let mut closed = false;
                    while let Some(q) = self.peek_char() {
                        self.advance();
                        match q {
                            '"' => {
                                closed = true;
                                break;
                            }
                            '\\' => {
                                word.push('\\');
                                if let Some(n) = self.peek_char() {
                                    self.advance();
                                    word.push(n);
                                }
                                continue;
                            }
                            '$' if self.peek_char() == Some('{') => {
                                self.advance();
                                word.push_str("${");
                                inner += 1;
                                continue;
                            }
                            '}' if inner > 0 => inner -= 1,
                            '{' | '}' => word.push('\x00'),
                            _ => {}
                        }
                        word.push(q);
                    }
                    if !closed {
                        return Err("unterminated double quote".to_string());
                    }
                    word.push('"');
                }
                '$' if self.peek_char() == Some('\'') => {
                    self.advance();
                    let (content, closed) = self.read_dollar_single_quoted_content();
                    if !closed {
                        return Err("unterminated single quote".to_string());
                    }
                    word.push('"');
                    push_escaped(word, &content);
                    word.push('"');
                }
                '$' if self.peek_char() == Some('{') => {
                    self.advance();
                    word.push_str("${");
                    if depth >= self.max_subst_depth {
                        return Err("parameter expansion nesting too deep".to_string());
                    }
                    depth += 1;
                }
                // `${x:-$(echo })}`: braces inside a command substitution
                // belong to it.
                '$' if self.peek_char() == Some('(') => {
                    self.advance();
                    word.push_str("$(");
                    if !self.read_command_subst_body(word) {
                        return Err("unterminated command substitution".to_string());
                    }
                }
                '}' => {
                    word.push('}');
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => word.push(c),
            }
        }
        Ok(())
    }

    fn read_dollar_single_quoted_content(&mut self) -> (String, bool) {
        let mut out = String::new();
        let mut closed = false;
        while let Some(ch) = self.peek_char() {
            if ch == '\'' {
                self.advance();
                closed = true;
                break;
            }
            if ch == '\\' {
                self.advance();
                if let Some(esc) = self.peek_char() {
                    self.advance();
                    match esc {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'a' => out.push('\x07'),
                        'b' => out.push('\x08'),
                        'f' => out.push('\x0C'),
                        'v' => out.push('\x0B'),
                        'e' | 'E' => out.push('\x1B'),
                        '\\' => out.push('\\'),
                        '\'' => out.push('\''),
                        '"' => out.push('"'),
                        '?' => out.push('?'),
                        'x' => {
                            let mut hex = String::new();
                            for _ in 0..2 {
                                if let Some(h) = self.peek_char() {
                                    if h.is_ascii_hexdigit() {
                                        hex.push(h);
                                        self.advance();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if let Ok(val) = u8::from_str_radix(&hex, 16) {
                                out.push(val as char);
                            }
                        }
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                if let Some(h) = self.peek_char() {
                                    if h.is_ascii_hexdigit() {
                                        hex.push(h);
                                        self.advance();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if let Ok(val) = u32::from_str_radix(&hex, 16)
                                && let Some(c) = char::from_u32(val)
                            {
                                out.push(c);
                            }
                        }
                        'U' => {
                            let mut hex = String::new();
                            for _ in 0..8 {
                                if let Some(h) = self.peek_char() {
                                    if h.is_ascii_hexdigit() {
                                        hex.push(h);
                                        self.advance();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if let Ok(val) = u32::from_str_radix(&hex, 16)
                                && let Some(c) = char::from_u32(val)
                            {
                                out.push(c);
                            }
                        }
                        '0'..='7' => {
                            let mut oct = String::new();
                            oct.push(esc);
                            for _ in 0..2 {
                                if let Some(o) = self.peek_char() {
                                    if o.is_ascii_digit() && o < '8' {
                                        oct.push(o);
                                        self.advance();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if let Ok(val) = u8::from_str_radix(&oct, 8) {
                                out.push(val as char);
                            }
                        }
                        _ => {
                            out.push('\\');
                            out.push(esc);
                        }
                    }
                } else {
                    out.push('\\');
                }
                continue;
            }
            out.push(ch);
            self.advance();
        }
        (out, closed)
    }

    /// Append a literal segment while protecting sentinel-sensitive bytes from parse_word expansion.
    fn push_literal_with_escaped_dollar(dst: &mut String, segment: &str) {
        for ch in segment.chars() {
            // `\x1e`/`\x1f` are this lexer's quote-boundary markers; a decoded
            // `$'\x1f'` byte must stay data (`SEP=$'\x1f'` is a common idiom).
            if matches!(ch, '\x00' | '$' | '\u{1e}' | '\u{1f}') {
                dst.push('\x00');
            }
            dst.push(ch);
        }
    }

    /// Characters `escape_glob_metas_in_quoted_ranges` backslash-escapes.
    /// Decode the body of `$'...'` at the start of `s` (just after `$'`):
    /// the resolved text and the bytes consumed through the closing `'`.
    pub(crate) fn decode_ansi_c_body(s: &str) -> Option<(String, usize)> {
        let mut lexer = Lexer::new(s);
        let (text, closed) = lexer.read_dollar_single_quoted_content();
        closed.then_some((text, lexer.position.offset))
    }

    /// Push a `$` just consumed inside double quotes. Before `'`
    /// (`"$'q'"`) or the closing `"` it starts no expansion, so it gets the
    /// NUL sentinel and stays a literal `$` even when quoted text follows
    /// (`"$"'q'`). `$$` is consumed whole (returns true) so its second `$`
    /// is not mistaken for a lone one before `"`.
    fn take_dquote_dollar(&mut self, word: &mut String) -> bool {
        let next = self.peek_char();
        if matches!(next, Some('\'' | '"') | None) {
            word.push('\x00');
        }
        word.push('$');
        if next == Some('$') {
            word.push('$');
            self.advance();
            return true;
        }
        false
    }

    /// Push one quoted character of an extglob group so it stays literal:
    /// pattern metacharacters get a backslash, `$`/`` ` `` the NUL sentinel.
    fn push_extglob_literal(word: &mut String, ch: char) {
        if Self::is_glob_escape_char(ch) || matches!(ch, '"' | '\'') {
            word.push('\\');
        } else if matches!(ch, '$' | '`') {
            word.push('\\');
            word.push('\x00');
        }
        word.push(ch);
    }

    fn is_glob_escape_char(ch: char) -> bool {
        matches!(
            ch,
            '\\' | '*' | '?' | '[' | ']' | '{' | '}' | ',' | '@' | '!' | '+' | '(' | ')' | '|'
        )
    }

    /// Drop quote-boundary markers and decode NUL escape sentinels in one
    /// pass, so a NUL-escaped marker byte survives as data.
    fn strip_markers_decode_sentinels(segment: &str) -> String {
        let mut out = String::with_capacity(segment.len());
        let mut chars = segment.chars();
        while let Some(ch) = chars.next() {
            match ch {
                '\x00' => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                '\u{1e}' | '\u{1f}' => {}
                c => out.push(c),
            }
        }
        out
    }

    fn read_double_quoted_string(&mut self) -> Option<Token> {
        self.advance(); // consume opening "
        let mut content = String::new();
        let mut closed = false;

        while let Some(ch) = self.peek_char() {
            match ch {
                '"' => {
                    self.advance(); // consume closing "
                    closed = true;
                    break;
                }
                '\\' => {
                    self.advance();
                    if let Some(next) = self.peek_char() {
                        // Handle escape sequences
                        match next {
                            '\n' => {
                                // \<newline> is line continuation: discard both
                                self.advance();
                            }
                            '$' => {
                                // Use NUL sentinel so parse_word() treats this
                                // as a literal '$' rather than a variable expansion.
                                content.push('\x00');
                                content.push('$');
                                self.advance();
                            }
                            '"' | '\\' | '`' => {
                                content.push(next);
                                self.advance();
                            }
                            _ => {
                                content.push('\\');
                                content.push(next);
                                self.advance();
                            }
                        }
                    }
                }
                '$' => {
                    self.advance();
                    if self.take_dquote_dollar(&mut content) {
                        continue;
                    }
                    if self.peek_char() == Some('(') {
                        // $(...) command substitution — track paren depth
                        content.push('(');
                        self.advance();
                        self.read_command_subst_into(&mut content);
                    } else if self.peek_char() == Some('{') {
                        // ${...} parameter expansion — track brace depth so
                        // inner quotes (e.g. ${arr["key"]}) don't end the string
                        content.push('{');
                        self.advance();
                        if let Err(msg) = self.read_param_expansion_into(&mut content) {
                            return Some(Token::Error(msg));
                        }
                    }
                }
                '`' => {
                    // Backtick command substitution inside double quotes
                    self.advance(); // consume opening `
                    content.push_str("$(");
                    let body_start = content.len();
                    while let Some(c) = self.peek_char() {
                        if c == '`' {
                            self.advance();
                            break;
                        }
                        if c == '\\' {
                            self.advance();
                            if let Some(next) = self.peek_char() {
                                if matches!(next, '$' | '`' | '\\' | '"') {
                                    content.push(next);
                                    self.advance();
                                } else {
                                    content.push('\\');
                                    content.push(next);
                                    self.advance();
                                }
                            }
                        } else {
                            content.push(c);
                            self.advance();
                        }
                    }
                    defer_backtick_syntax_error(&mut content, body_start);
                    content.push(')');
                }
                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        if !closed {
            return Some(Token::Error("unterminated double quote".to_string()));
        }

        // Check for continuation after closing quote: "foo"bar or "foo"/* etc.
        // If there's adjacent unquoted content (word chars, globs, more quotes),
        // concatenate so the word stays a single token.  When the continuation
        // contains glob metacharacters, return QuotedGlobWord so the interpreter
        // suppresses IFS splitting (the double-quoted segment) while still
        // performing glob expansion on the unquoted portion.
        if let Some(ch) = self.peek_char()
            && (self.is_word_char(ch) || matches!(ch, '\'' | '"' | '$' | '{' | '}'))
        {
            let quoted_prefix_len = content.len();
            let flags = self.read_continuation_into(&mut content);
            if let Some(error) = flags.error {
                return Some(Token::Error(error));
            }
            if flags.has_unquoted_expansion {
                let mut ranges = flags.quoted_ranges;
                ranges.push((0, quoted_prefix_len));
                // Build marker-delimited quoted spans in one pass so hostile
                // many-continuation words cannot trigger quadratic insertion work.
                Self::apply_quote_markers(&mut content, ranges);
                return Some(Token::Word(content));
            }
            if flags.has_unquoted_glob {
                // Escape glob metacharacters inside quoted ranges (initial double-quoted
                // prefix + any further quoted segments from read_continuation_into) so
                // the glob expander treats them as literals, not active patterns.
                let mut ranges = flags.quoted_ranges;
                if quoted_prefix_len > 0 {
                    ranges.push((0, quoted_prefix_len));
                }
                ranges.sort_unstable_by_key(|&(s, _)| s);
                return Some(Token::QuotedGlobWord(
                    Self::escape_glob_metas_in_quoted_ranges(&content, &ranges),
                ));
            }
            return Some(Token::QuotedWord(content));
        }

        Some(Token::QuotedWord(content))
    }

    /// Escape glob metacharacters within quoted byte ranges so that the glob
    /// expander treats them as literal characters rather than active patterns.
    /// Each range is also wrapped in quote-segment markers: `parse_word` then
    /// ends a variable name at the closing quote (`"$x"zz*` reads `$x`, not
    /// `$xzz`) and records which parts were quoted for field splitting.
    /// Ranges must be sorted and non-overlapping.
    fn escape_glob_metas_in_quoted_ranges(s: &str, quoted_ranges: &[(usize, usize)]) -> String {
        if quoted_ranges.is_empty() {
            return s.to_string();
        }
        // Collect chars with their byte positions for range-boundary checks.
        let char_vec: Vec<(usize, char)> = {
            let mut pos = 0usize;
            s.chars()
                .map(|c| {
                    let p = pos;
                    pos += c.len_utf8();
                    (p, c)
                })
                .collect()
        };

        let mut result = String::with_capacity(s.len() + 8);
        let mut range_idx = 0usize;
        // Stack of opening delimiters ('{'  or  '(') for active ${ } / $( ) constructs.
        // While non-empty we are inside an expansion and must NOT escape anything,
        // because the content is still unexpanded at parse time and characters like
        // { } [ ] are structural (e.g. ${arr[0]}, $(cmd)).
        let mut expansion_stack: Vec<char> = Vec::new();
        let n = char_vec.len();
        let mut i = 0usize;
        let mut marked_quoted = false;

        while i < n {
            let (byte_pos, ch) = char_vec[i];
            let ch_end = byte_pos + ch.len_utf8();

            // Advance past quoted-range entries that ended before this char.
            while range_idx < quoted_ranges.len() && quoted_ranges[range_idx].1 <= byte_pos {
                range_idx += 1;
            }
            let in_quoted = range_idx < quoted_ranges.len()
                && byte_pos >= quoted_ranges[range_idx].0
                && ch_end <= quoted_ranges[range_idx].1;
            if in_quoted != marked_quoted {
                result.push(if in_quoted { '\u{1e}' } else { '\u{1f}' });
                marked_quoted = in_quoted;
            }

            if in_quoted {
                if expansion_stack.is_empty() && ch == '$' {
                    // Peek at next char to detect ${ or $(
                    if let Some(&(_, next)) = char_vec.get(i + 1)
                        && (next == '{' || next == '(')
                    {
                        expansion_stack.push(next);
                        result.push(ch);
                        result.push(next);
                        i += 2;
                        continue;
                    }
                } else if !expansion_stack.is_empty() {
                    // Track nesting: ${ … { … } … } and $( … ( … ) … )
                    let top = *expansion_stack.last().unwrap();
                    match (top, ch) {
                        ('{', '{') | ('(', '(') => expansion_stack.push(ch),
                        ('{', '}') | ('(', ')') => {
                            expansion_stack.pop();
                        }
                        _ => {}
                    }
                    result.push(ch);
                    i += 1;
                    continue;
                }

                // Outside any ${ }/$( ) construct: escape glob / brace / extglob metas
                // so that runtime brace-expansion and glob-expansion treat them as literals,
                // matching how bash handles metacharacters inside double quotes.
                if expansion_stack.is_empty()
                    && matches!(
                        ch,
                        '\\' | '*'
                            | '?'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | ','
                            | '@'
                            | '!'
                            | '+'
                            | '('
                            | ')'
                            | '|'
                    )
                {
                    result.push('\\');
                }
            }

            result.push(ch);
            i += 1;
        }
        if marked_quoted {
            result.push('\u{1f}');
        }
        result
    }

    /// Read command substitution content after `$(`, handling nested parens,
    /// quotes, comments and heredoc bodies (see `subst_scan`). Appends chars
    /// to `content` and adds the closing `)`.
    fn read_command_subst_into(&mut self, content: &mut String) {
        self.read_command_subst_body(content);
    }

    /// Like `read_command_subst_into`; returns whether the closing `)` was
    /// found. The scanner is iterative, so deep nesting cannot overflow the
    /// host stack here (TM-DOS-044).
    fn read_command_subst_body(&mut self, content: &mut String) -> bool {
        let mut scanner = super::subst_scan::SubstScanner::new();
        while let Some(c) = self.advance() {
            content.push(c);
            if scanner.feed(c) == super::subst_scan::Step::Close {
                return true;
            }
        }
        false
    }

    /// Consume a `<(...)`/`>(...)` body after its opening token, through the
    /// closing `)`, with the same scanner as `$(...)`. Nothing is copied; the
    /// caller slices the source. Returns whether the `)` was found.
    pub fn skip_subst_body(&mut self) -> bool {
        let mut scanner = super::subst_scan::SubstScanner::new();
        while let Some(c) = self.advance() {
            if scanner.feed(c) == super::subst_scan::Step::Close {
                return true;
            }
        }
        false
    }

    /// Read parameter expansion content after `${`, handling nested braces and quotes.
    /// In bash, quotes inside `${...}` (e.g. `${arr["key"]}`) don't terminate the
    /// outer double-quoted string. Appends chars including closing `}` to `content`.
    /// THREAT[TM-DOS-045]: track nested `${...}` iteratively. This lexer runs
    /// before parser fuel is checked, so recursion here can overflow the host stack.
    fn read_param_expansion_into(&mut self, content: &mut String) -> Result<(), String> {
        let mut depth = 1usize;
        while let Some(c) = self.peek_char() {
            match c {
                '{' => {
                    if depth >= self.max_subst_depth {
                        return Err("parameter expansion nesting too deep".to_string());
                    }
                    depth += 1;
                    content.push(c);
                    self.advance();
                }
                '}' => {
                    depth -= 1;
                    self.advance();
                    content.push('}');
                    if depth == 0 {
                        break;
                    }
                }
                '"' => {
                    // Quotes inside ${...} are part of the expansion, not string delimiters
                    content.push('"');
                    self.advance();
                }
                '\'' => {
                    // Bash keeps `'` literal in "${x:-'d'}" but still matches
                    // braces past it: "${x:-'}'}" is `'}'`. NUL-escape braces in
                    // the span so the parser does not close the operand early.
                    let ansi_c = content.ends_with('$');
                    content.push('\'');
                    self.advance();
                    while let Some(q) = self.peek_char() {
                        self.advance();
                        if matches!(q, '{' | '}') {
                            content.push('\x00');
                        }
                        content.push(q);
                        if ansi_c && q == '\\' {
                            if let Some(n) = self.peek_char() {
                                self.advance();
                                content.push(n);
                            }
                        } else if q == '\'' {
                            break;
                        }
                    }
                }
                '\\' => {
                    // Inside ${...} within double quotes, same escape rules apply:
                    // \", \\, \$, \` produce the escaped char; others keep backslash
                    self.advance();
                    if let Some(esc) = self.peek_char() {
                        match esc {
                            '$' => {
                                content.push('\x00');
                                content.push('$');
                                self.advance();
                            }
                            '"' => {
                                // Use NUL sentinel so strip_operand_quotes()
                                // can distinguish literal " from quoting "
                                content.push('\x00');
                                content.push('"');
                                self.advance();
                            }
                            '\\' | '`' => {
                                content.push(esc);
                                self.advance();
                            }
                            '}' => {
                                // \} should be a literal } without closing the expansion
                                content.push('\\');
                                content.push('}');
                                self.advance();
                            }
                            _ => {
                                content.push('\\');
                                content.push(esc);
                                self.advance();
                            }
                        }
                    } else {
                        content.push('\\');
                    }
                }
                '$' => {
                    content.push('$');
                    self.advance();
                    if self.peek_char() == Some('(') {
                        content.push('(');
                        self.advance();
                        self.read_command_subst_into(content);
                    } else if self.peek_char() == Some('{') {
                        if depth >= self.max_subst_depth {
                            return Err("parameter expansion nesting too deep".to_string());
                        }
                        depth += 1;
                        content.push('{');
                        self.advance();
                    }
                }
                _ => {
                    content.push(c);
                    self.advance();
                }
            }
        }
        Ok(())
    }

    /// Check if { is followed by whitespace (brace group start)
    fn is_brace_group_start(&self) -> bool {
        let mut chars = self.lookahead();
        // Skip the opening {
        if chars.next() != Some('{') {
            return false;
        }
        // If next char is whitespace or newline, it's a brace group
        matches!(chars.next(), Some(' ') | Some('\t') | Some('\n') | None)
    }

    /// Whether the `}` at the cursor is a word of its own, and so the
    /// reserved word that closes a brace group.
    ///
    /// Mirrors [`Self::is_brace_group_start`] for the opening side. bash's
    /// metacharacters are space, tab, newline, `|`, `&`, `;`, `(`, `)`, `<`
    /// and `>`; `}` is not among them, so it delimits a word only when a
    /// metacharacter or EOF already follows it.
    fn right_brace_stands_alone(&self) -> bool {
        let mut chars = self.lookahead();
        if chars.next() != Some('}') {
            return false;
        }
        match chars.next() {
            None => true,
            Some(c) => matches!(
                c,
                ' ' | '\t' | '\n' | ';' | '|' | '&' | '(' | ')' | '<' | '>'
            ),
        }
    }

    /// Read a word starting with [ (glob bracket expression like [abc] or [a-z])
    /// The opening [ has already been consumed
    fn read_bracket_word(&mut self) -> Option<Token> {
        let mut word = String::from("[");

        // Read until we find the closing ] (handle nested correctly)
        while let Some(ch) = self.peek_char() {
            word.push(ch);
            self.advance();
            if ch == ']' {
                break;
            }
        }

        // Continue reading any remaining word characters (e.g., [abc]def)
        while let Some(ch) = self.peek_char() {
            if matches!(ch, '"' | '\'' | '$' | '`') {
                // A quoted or expanded tail stays in the same word:
                // `[k]="v w"` or `[k]=`cmd arg`` in a compound array is one
                // element.
                return self.read_word_starting_with(&word);
            } else if self.is_word_char(ch) || matches!(ch, '{' | '}') {
                // Braces are word text too (`[k]=v{1,2}`, `[ab]{x,y}`).
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        Some(Token::Word(word))
    }

    /// Read the body of `(( ... ))` as written, after the opening `((`,
    /// through the matching `))` (consumed, not returned). Parentheses nest;
    /// quoted text is kept verbatim. Bash reads the arithmetic command this
    /// way, so `<<` is a shift and spacing survives for `type`. Returns
    /// `None` when input ends first.
    pub fn read_dparen_body(&mut self) -> Option<String> {
        let mut body = String::new();
        let mut depth = 0usize;
        while let Some(c) = self.advance() {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 && self.peek_char() == Some(')') => {
                    self.advance();
                    return Some(body);
                }
                // An unbalanced `)` stays text.
                ')' if depth == 0 => {}
                ')' => depth -= 1,
                '\\' => {
                    body.push(c);
                    if let Some(n) = self.advance() {
                        body.push(n);
                    }
                    continue;
                }
                '\'' | '"' => {
                    body.push(c);
                    while let Some(n) = self.advance() {
                        body.push(n);
                        if n == c {
                            break;
                        }
                        if n == '\\'
                            && c == '"'
                            && let Some(e) = self.advance()
                        {
                            body.push(e);
                        }
                    }
                    continue;
                }
                _ => {}
            }
            body.push(c);
        }
        None
    }

    /// Read the raw source of a `[[ ... =~ REGEX ]]` operand, as bash does:
    /// one word that ends at unquoted whitespace or `;&<>` outside
    /// parentheses. Inside parentheses spaces belong to the regex; `|` and
    /// `#` are ordinary characters (`^(#+) (.+)$`, `x|y`). Quotes and
    /// backslashes are kept verbatim for the caller to interpret. Returns
    /// `None` when no operand follows.
    pub fn read_cond_regex(&mut self) -> Option<String> {
        while matches!(self.peek_char(), Some(' ' | '\t')) {
            self.advance();
        }
        let mut raw = String::new();
        let mut depth = 0usize;
        while let Some(ch) = self.peek_char() {
            if depth == 0 {
                if matches!(ch, ' ' | '\t' | '\n' | ';' | '&' | '<' | '>') {
                    break;
                }
                if ch == ')' {
                    break;
                }
                if ch == '|' {
                    // `||` ends the operand; a lone `|` is alternation.
                    let mut ahead = self.lookahead();
                    ahead.next();
                    if self.reinject_buf.is_empty() && ahead.peek() == Some(&'|') {
                        break;
                    }
                }
            }
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                '\n' => {} // only reachable inside parentheses
                _ => {}
            }
            raw.push(ch);
            self.advance();
            match ch {
                '\\' => {
                    if let Some(next) = self.peek_char() {
                        raw.push(next);
                        self.advance();
                    }
                }
                '\'' => {
                    while let Some(c) = self.advance() {
                        raw.push(c);
                        if c == '\'' {
                            break;
                        }
                    }
                }
                '"' => {
                    while let Some(c) = self.advance() {
                        raw.push(c);
                        if c == '\\' {
                            if let Some(n) = self.advance() {
                                raw.push(n);
                            }
                        } else if c == '"' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        (!raw.is_empty()).then_some(raw)
    }

    fn is_word_char(&self, ch: char) -> bool {
        !matches!(
            ch,
            ' ' | '\t' | '\n' | ';' | '|' | '&' | '>' | '<' | '(' | ')' | '{' | '}' | '\'' | '"'
        )
        // `#` is a word char: it starts a comment only at the start of a
        // word (`echo a#b` prints `a#b`), handled in `next_token_inner`.
    }

    /// Read here document content until the delimiter line is found.
    /// When `strip_tabs` is true (for `<<-`), leading tabs on the delimiter line
    /// are stripped before comparing.
    pub fn read_heredoc(&mut self, delimiter: &str) -> String {
        self.read_heredoc_with_strip(delimiter, false)
    }

    /// Read here document content with optional leading-tab stripping on the
    /// delimiter match (for `<<-`).
    pub fn read_heredoc_with_strip(&mut self, delimiter: &str, strip_tabs: bool) -> String {
        self.read_heredoc_with_strip_metered(delimiter, strip_tabs)
            .0
    }

    /// Read here document content and report rest-of-line work for parser fuel.
    /// THREAT[TM-DOS-064]: Long heredoc command-line suffixes are re-injected so
    /// list/pipeline tokens after `<<EOF` stay visible. Charging the suffix length
    /// to parser fuel prevents chained heredocs from repeatedly copying suffixes
    /// outside resource accounting.
    pub(crate) fn read_heredoc_with_strip_metered(
        &mut self,
        delimiter: &str,
        strip_tabs: bool,
    ) -> (String, usize) {
        let mut content = String::new();
        let mut current_line = String::new();

        // Save rest of current line (after the delimiter token on the command line).
        // For `cat <<EOF | sort`, this captures ` | sort` so the parser can
        // tokenize the pipe and subsequent command after the heredoc body.
        //
        // Quoted strings may span multiple lines (e.g., `cat <<EOF; echo "two\nthree"`),
        // so we track quoting state and continue across newlines until quotes close.
        let mut rest_of_line = String::new();
        let mut in_double_quote = false;
        let mut in_single_quote = false;
        while let Some(ch) = self.peek_char() {
            self.advance();
            if ch == '\n' && !in_double_quote && !in_single_quote {
                break;
            }
            if ch == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
            } else if ch == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
            } else if ch == '\\' && in_double_quote {
                // Escaped char inside double quotes — skip the next char too
                rest_of_line.push(ch);
                if let Some(next) = self.peek_char() {
                    rest_of_line.push(next);
                    self.advance();
                }
                continue;
            }
            rest_of_line.push(ch);
        }

        // Read lines until we find the delimiter
        loop {
            match self.peek_char() {
                Some('\n') => {
                    self.advance();
                    // Check if current line matches delimiter.
                    // For `<<-`, strip leading tabs from the delimiter line.
                    let line_for_match: &str = if strip_tabs {
                        current_line.trim_start_matches('\t')
                    } else {
                        &current_line
                    };
                    if line_for_match == delimiter {
                        break;
                    }
                    content.push_str(&current_line);
                    content.push('\n');
                    current_line.clear();
                }
                Some(ch) => {
                    current_line.push(ch);
                    self.advance();
                }
                None => {
                    // End of input - check last line (strip tabs for `<<-`)
                    let line_for_match: &str = if strip_tabs {
                        current_line.trim_start_matches('\t')
                    } else {
                        &current_line
                    };
                    if line_for_match == delimiter {
                        break;
                    }
                    if !current_line.is_empty() {
                        content.push_str(&current_line);
                    }
                    break;
                }
            }
        }

        // Re-inject saved rest-of-line so subsequent tokens (pipes, commands, etc.)
        // are visible to the parser. Add a newline so the tokenizer sees the line break.
        // The line break always comes back: it ends the command even when
        // nothing followed the delimiter (`cat <<A <<B`).
        let rest_of_line_chars = rest_of_line.chars().count();
        for ch in rest_of_line.chars() {
            self.reinject_buf.push_back(ch);
        }
        self.reinject_buf.push_back('\n');

        (content, rest_of_line_chars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_words() {
        let mut lexer = Lexer::new("echo hello world");

        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("hello".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("world".to_string())));
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_single_quoted_string() {
        let mut lexer = Lexer::new("echo 'hello world'");

        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        // Single-quoted strings return LiteralWord (no variable expansion)
        assert_eq!(
            lexer.next_token(),
            Some(Token::LiteralWord("hello world".to_string()))
        );
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_double_quoted_string() {
        let mut lexer = Lexer::new("echo \"hello world\"");

        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(
            lexer.next_token(),
            Some(Token::QuotedWord("hello world".to_string()))
        );
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_mixed_quote_empty_continuations_do_not_emit_quadratic_markers() {
        let mut script = String::from("\"a\"");
        for _ in 0..512 {
            script.push_str("\"\"");
        }
        script.push_str("$x");

        let mut lexer = Lexer::new(&script);
        assert_eq!(
            lexer.next_token(),
            Some(Token::Word("\u{1e}a\u{1f}$x".to_string()))
        );
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_double_quoted_nested_param_expansion_depth_limit() {
        let mut lexer = Lexer::with_max_subst_depth("\"${a:-${b:-${c}}}\"", 2);

        match lexer.next_token() {
            Some(Token::Error(msg)) => assert!(
                msg.contains("parameter expansion nesting too deep"),
                "expected parameter expansion depth error, got: {msg}"
            ),
            other => panic!("expected depth error token, got: {other:?}"),
        }
    }

    #[test]
    fn test_double_quoted_nested_param_expansion_at_limit() {
        let mut lexer = Lexer::with_max_subst_depth("\"${a:-${b}}\"", 2);

        assert_eq!(
            lexer.next_token(),
            Some(Token::QuotedWord("${a:-${b}}".to_string()))
        );
    }

    #[test]
    fn test_single_quoted_segment_in_word_escapes_dollar() {
        let mut lexer = Lexer::new("echo foo'$(id)'");
        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(
            lexer.next_token(),
            Some(Token::Word("foo\x00$(id)".to_string()))
        );
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_single_quoted_word_continuation_decodes_escaped_dollar() {
        let mut lexer = Lexer::new(r#"echo 'x'"\$HOME""#);
        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(
            lexer.next_token(),
            Some(Token::LiteralWord("x$HOME".to_string()))
        );
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_operators() {
        let mut lexer = Lexer::new("a | b && c || d; e &");

        assert_eq!(lexer.next_token(), Some(Token::Word("a".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Pipe));
        assert_eq!(lexer.next_token(), Some(Token::Word("b".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::And));
        assert_eq!(lexer.next_token(), Some(Token::Word("c".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Or));
        assert_eq!(lexer.next_token(), Some(Token::Word("d".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Semicolon));
        assert_eq!(lexer.next_token(), Some(Token::Word("e".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Background));
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_redirects() {
        let mut lexer = Lexer::new("a > b >> c < d << e <<< f");

        assert_eq!(lexer.next_token(), Some(Token::Word("a".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::RedirectOut));
        assert_eq!(lexer.next_token(), Some(Token::Word("b".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::RedirectAppend));
        assert_eq!(lexer.next_token(), Some(Token::Word("c".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::RedirectIn));
        assert_eq!(lexer.next_token(), Some(Token::Word("d".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::HereDoc));
        assert_eq!(lexer.next_token(), Some(Token::Word("e".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::HereString));
        assert_eq!(lexer.next_token(), Some(Token::Word("f".to_string())));
    }

    #[test]
    fn test_comment() {
        let mut lexer = Lexer::new("echo hello # this is a comment\necho world");

        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("hello".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Newline));
        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("world".to_string())));
    }

    #[test]
    fn test_variable_words() {
        let mut lexer = Lexer::new("echo $HOME $USER");

        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("$HOME".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("$USER".to_string())));
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_pipeline_tokens() {
        let mut lexer = Lexer::new("echo hello | cat");

        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("hello".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Pipe));
        assert_eq!(lexer.next_token(), Some(Token::Word("cat".to_string())));
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_read_heredoc() {
        // Simulate state after reading "cat <<EOF" - positioned at newline before content
        let mut lexer = Lexer::new("\nhello\nworld\nEOF");
        let content = lexer.read_heredoc("EOF");
        assert_eq!(content, "hello\nworld\n");
    }

    #[test]
    fn test_read_heredoc_single_line() {
        let mut lexer = Lexer::new("\ntest\nEOF");
        let content = lexer.read_heredoc("EOF");
        assert_eq!(content, "test\n");
    }

    #[test]
    fn test_read_heredoc_full_scenario() {
        // Full scenario: "cat <<EOF\nhello\nworld\nEOF"
        let mut lexer = Lexer::new("cat <<EOF\nhello\nworld\nEOF");

        // Parser would read these tokens
        assert_eq!(lexer.next_token(), Some(Token::Word("cat".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::HereDoc));
        assert_eq!(lexer.next_token(), Some(Token::Word("EOF".to_string())));

        // Now read heredoc content
        let content = lexer.read_heredoc("EOF");
        assert_eq!(content, "hello\nworld\n");
    }

    #[test]
    fn test_read_heredoc_with_redirect() {
        // Rest-of-line (> file.txt) is re-injected into the lexer buffer
        let mut lexer = Lexer::new("cat <<EOF > file.txt\nhello\nEOF");
        assert_eq!(lexer.next_token(), Some(Token::Word("cat".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::HereDoc));
        assert_eq!(lexer.next_token(), Some(Token::Word("EOF".to_string())));
        let content = lexer.read_heredoc("EOF");
        assert_eq!(content, "hello\n");
        // The redirect tokens are now available from the lexer
        assert_eq!(lexer.next_token(), Some(Token::RedirectOut));
        assert_eq!(
            lexer.next_token(),
            Some(Token::Word("file.txt".to_string()))
        );
    }

    #[test]
    fn test_read_heredoc_requires_exact_delimiter_match() {
        let mut lexer = Lexer::new("\nhello\n EOF\nEOF\n");
        let content = lexer.read_heredoc("EOF");
        assert_eq!(content, "hello\n EOF\n");
    }

    #[test]
    fn test_assoc_compound_assignment() {
        // m=([foo]="bar") lexes like any compound: the parser collects the
        // element tokens between the parentheses.
        let mut lexer = Lexer::new(r#"m=([foo]="bar")"#);
        assert_eq!(lexer.next_token(), Some(Token::Word("m=".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::LeftParen));
        assert!(matches!(lexer.next_token(), Some(Token::Word(_))));
        assert_eq!(lexer.next_token(), Some(Token::RightParen));
        assert_eq!(lexer.next_token(), None);
    }

    #[test]
    fn test_indexed_array_not_collapsed() {
        // arr=("hello world") should NOT be collapsed — parser handles
        // quoted elements token-by-token via the LeftParen path
        let mut lexer = Lexer::new(r#"arr=("hello world")"#);
        assert_eq!(lexer.next_token(), Some(Token::Word("arr=".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::LeftParen));
    }

    /// Regression test for fuzz crash: single digit at EOF should not panic
    /// (crash-13c5f6f887a11b2296d67f9857975d63b205ac4b)
    #[test]
    fn test_digit_at_eof_no_panic() {
        // A lone digit with no following redirect operator must not panic
        let mut lexer = Lexer::new("2");
        let token = lexer.next_token();
        assert!(token.is_some());
    }

    /// Issue #599: Nested ${...} inside unquoted ${...} must be a single token.
    #[test]
    fn test_nested_brace_expansion_single_token() {
        // ${arr[${#arr[@]} - 1]} should be ONE word token, not split at inner }
        let mut lexer = Lexer::new("${arr[${#arr[@]} - 1]}");
        let token = lexer.next_token();
        assert_eq!(
            token,
            Some(Token::Word("${arr[${#arr[@]} - 1]}".to_string()))
        );
        // No more tokens — everything was consumed
        assert_eq!(lexer.next_token(), None);
    }

    /// Simple ${var} still works after brace depth change.
    #[test]
    fn test_simple_brace_expansion_unchanged() {
        let mut lexer = Lexer::new("${foo}");
        assert_eq!(lexer.next_token(), Some(Token::Word("${foo}".to_string())));
        assert_eq!(lexer.next_token(), None);
    }

    /// `}` is a reserved word, not a metacharacter: it stays inside the word
    /// unless a metacharacter or EOF follows it.
    #[test]
    fn close_brace_does_not_split_a_word() {
        for src in ["a}b", "a}", "}b", "}}", "a}b}c", "a}{b"] {
            let mut lexer = Lexer::new(src);
            assert_eq!(
                lexer.next_token(),
                Some(Token::Word(src.to_string())),
                "{src:?} should lex as one word"
            );
            assert_eq!(lexer.next_token(), None, "{src:?} left trailing tokens");
        }
    }

    /// Standing alone, it is still the reserved word that closes a group.
    #[test]
    fn lone_close_brace_is_the_reserved_word() {
        for src in ["}", "} ", "};", "}\n", "})", "}|", "}&", "}<", "}>"] {
            let mut lexer = Lexer::new(src);
            assert_eq!(
                lexer.next_token(),
                Some(Token::RightBrace),
                "{src:?} should open with RightBrace"
            );
        }
    }

    /// The closing token of a brace group survives, whether or not a space
    /// precedes it.
    #[test]
    fn brace_group_still_lexes_as_a_group() {
        let mut lexer = Lexer::new("{ echo hi; }");
        assert_eq!(lexer.next_token(), Some(Token::LeftBrace));
        assert_eq!(lexer.next_token(), Some(Token::Word("echo".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Word("hi".to_string())));
        assert_eq!(lexer.next_token(), Some(Token::Semicolon));
        assert_eq!(lexer.next_token(), Some(Token::RightBrace));
        assert_eq!(lexer.next_token(), None);
    }
}
