//! Finds the `)` that closes a `$(...)` body.
//!
//! Decision: one incremental scanner shared by the lexer and `parse_word`, fed
//! one char at a time, so both agree on where a command substitution ends.
//! Plain paren counting closed `$(cat <<'E'` at a `)` inside the heredoc body,
//! and `$(echo ")")` at the quoted `)`. Bash finds the end by parsing; this
//! tracks the contexts that can hide a `)`: single/double quotes, backslash
//! escapes, backticks, nested `$(...)`, comments and heredoc bodies.
//! `case` is tracked too: the bare `)` that ends a case pattern
//! (`$(case x in x) ...;; esac)`) does not close the substitution. Words are
//! split at blanks and operators, and `case`/`in`/`esac` count only as
//! unquoted words in the right place (command position for `case`/`esac`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// Shell code inside `$(` (or `(` nesting within it).
    Code,
    Single,
    Double,
    Backtick,
}

/// Where an open `case` (inside one code frame) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CasePhase {
    /// After `case`, reading the subject word.
    Subject,
    /// Expecting `in`.
    In,
    /// Reading a pattern list; `started` once any pattern text was seen.
    Pattern { started: bool },
    /// In a branch body, until `;;`, `;&`, `;;&` or `esac`.
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CaseCtx {
    phase: CasePhase,
    /// Paren depth of the frame when `case` started.
    depth: usize,
}

/// Word and `case` tracking for one `Code` frame.
#[derive(Debug, Clone, Default)]
struct CodeState {
    /// Unquoted text of the current word, capped (only keywords matter).
    word: String,
    /// The current word has quoted or escaped parts, so it is no keyword.
    word_quoted: bool,
    /// The next word starts a command.
    cmd_pos: bool,
    cases: Vec<CaseCtx>,
}

/// Longest keyword we need to recognise (`case`, `esac`).
const MAX_KEYWORD: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Heredoc {
    delimiter: String,
    strip_tabs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    /// After a backslash: the next char is literal.
    Escape,
    /// `#` comment, until newline.
    Comment,
    /// Reading the word after `<<` / `<<-`.
    Delimiter {
        strip_tabs: bool,
        word: String,
        started: bool,
        quote: Option<char>,
    },
    /// Inside heredoc bodies; `line` accumulates the current line.
    Body {
        line: String,
    },
}

/// Result of feeding one char.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Char belongs to the body.
    Body,
    /// Char is the closing `)`; it is not part of the body.
    Close,
}

/// Incremental scanner for a `$(...)` body. Create it after consuming `$(`.
#[derive(Debug, Clone)]
pub(crate) struct SubstScanner {
    /// Context stack; the bottom `Code` frame is the substitution itself.
    stack: Vec<Frame>,
    /// Paren depth per `Code` frame (parallel to Code entries in `stack`).
    parens: Vec<usize>,
    /// Word/`case` state per `Code` frame (parallel to `parens`).
    code: Vec<CodeState>,
    mode: Mode,
    prev: Option<char>,
    /// `<` chars seen in a row in code context.
    lt_run: usize,
    pending_heredocs: Vec<Heredoc>,
    /// Paren depth at which an arithmetic `((` started in the top code frame;
    /// `<<` inside it is a shift, not a heredoc.
    arith_at: Option<usize>,
}

impl Default for SubstScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl SubstScanner {
    pub(crate) fn new() -> Self {
        Self {
            stack: vec![Frame::Code],
            parens: vec![0],
            code: vec![CodeState {
                cmd_pos: true,
                ..CodeState::default()
            }],
            mode: Mode::Normal,
            prev: None,
            lt_run: 0,
            pending_heredocs: Vec::new(),
            arith_at: None,
        }
    }

    pub(crate) fn feed(&mut self, c: char) -> Step {
        let step = self.feed_inner(c);
        self.prev = Some(c);
        step
    }

    fn top(&self) -> Frame {
        *self.stack.last().unwrap_or(&Frame::Code)
    }

    fn feed_inner(&mut self, c: char) -> Step {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Escape => Step::Body,
            Mode::Comment => {
                if c == '\n' {
                    self.on_newline();
                } else {
                    self.mode = Mode::Comment;
                }
                Step::Body
            }
            Mode::Body { mut line } => {
                if c != '\n' {
                    line.push(c);
                    self.mode = Mode::Body { line };
                    return Step::Body;
                }
                let doc = &self.pending_heredocs[0];
                let candidate = if doc.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if candidate == doc.delimiter {
                    self.pending_heredocs.remove(0);
                }
                if !self.pending_heredocs.is_empty() {
                    self.mode = Mode::Body {
                        line: String::new(),
                    };
                }
                Step::Body
            }
            Mode::Delimiter {
                strip_tabs,
                mut word,
                started,
                quote,
            } => {
                if let Some(q) = quote {
                    if c != q {
                        word.push(c);
                    }
                    let quote = if c == q { None } else { Some(q) };
                    self.mode = Mode::Delimiter {
                        strip_tabs,
                        word,
                        started: true,
                        quote,
                    };
                    return Step::Body;
                }
                if !started && c == '-' && word.is_empty() && !strip_tabs {
                    self.mode = Mode::Delimiter {
                        strip_tabs: true,
                        word,
                        started: false,
                        quote: None,
                    };
                    return Step::Body;
                }
                if !started && (c == ' ' || c == '\t') {
                    self.mode = Mode::Delimiter {
                        strip_tabs,
                        word,
                        started,
                        quote: None,
                    };
                    return Step::Body;
                }
                let ends =
                    c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')' | '<' | '>');
                if ends {
                    if !word.is_empty() {
                        self.pending_heredocs.push(Heredoc {
                            delimiter: word,
                            strip_tabs,
                        });
                    }
                    // The terminator is ordinary code: reprocess it.
                    return self.feed_inner(c);
                }
                if c == '\'' || c == '"' {
                    self.mode = Mode::Delimiter {
                        strip_tabs,
                        word,
                        started: true,
                        quote: Some(c),
                    };
                    return Step::Body;
                }
                if c != '\\' {
                    word.push(c);
                }
                self.mode = Mode::Delimiter {
                    strip_tabs,
                    word,
                    started: true,
                    quote: None,
                };
                Step::Body
            }
            Mode::Normal => self.feed_normal(c),
        }
    }

    fn on_newline(&mut self) {
        if !self.pending_heredocs.is_empty() {
            self.mode = Mode::Body {
                line: String::new(),
            };
        }
    }

    fn feed_normal(&mut self, c: char) -> Step {
        match self.top() {
            Frame::Single => {
                if c == '\'' {
                    self.stack.pop();
                }
                Step::Body
            }
            Frame::Double => {
                match c {
                    '\\' => self.mode = Mode::Escape,
                    '"' => {
                        self.stack.pop();
                    }
                    '(' if self.prev == Some('$') => self.push_code(),
                    '`' => self.stack.push(Frame::Backtick),
                    _ => {}
                }
                Step::Body
            }
            Frame::Backtick => {
                match c {
                    '\\' => self.mode = Mode::Escape,
                    '`' => {
                        self.stack.pop();
                    }
                    _ => {}
                }
                Step::Body
            }
            Frame::Code => self.feed_code(c),
        }
    }

    fn push_code(&mut self) {
        self.stack.push(Frame::Code);
        self.parens.push(0);
        self.code.push(CodeState {
            cmd_pos: true,
            ..CodeState::default()
        });
    }

    /// Mark the current word as quoted (a quote or escape starts in code).
    fn quote_word(&mut self) {
        if let Some(st) = self.code.last_mut() {
            st.word_quoted = true;
            st.word.push('"');
        }
    }

    /// End the current word in the top code frame and update `case` state.
    fn end_word(&mut self) {
        let Some(st) = self.code.last_mut() else {
            return;
        };
        if st.word.is_empty() {
            return;
        }
        let word = std::mem::take(&mut st.word);
        let keyword = !std::mem::take(&mut st.word_quoted);
        let cmd_pos = st.cmd_pos;
        let depth = self.parens.last().copied().unwrap_or(0);
        let kw = |k: &str| keyword && word == k;
        let mut next_cmd_pos = false;
        match st.cases.last_mut().map(|c| (c.phase, c.depth)) {
            Some((CasePhase::Subject, _)) => {
                if let Some(c) = st.cases.last_mut() {
                    c.phase = CasePhase::In;
                }
            }
            Some((CasePhase::In, _)) => {
                if let Some(c) = st.cases.last_mut() {
                    c.phase = if kw("in") {
                        CasePhase::Pattern { started: false }
                    } else {
                        // Not `in`: not a case we understand; stop tracking.
                        st.cases.pop();
                        return;
                    };
                }
            }
            Some((CasePhase::Pattern { started }, d)) if d == depth => {
                if !started && kw("esac") {
                    st.cases.pop();
                } else if let Some(c) = st.cases.last_mut() {
                    c.phase = CasePhase::Pattern { started: true };
                }
            }
            Some((CasePhase::Body, d)) if d == depth && cmd_pos && kw("esac") => {
                st.cases.pop();
            }
            _ => {
                if cmd_pos && kw("case") {
                    st.cases.push(CaseCtx {
                        phase: CasePhase::Subject,
                        depth,
                    });
                } else if cmd_pos
                    && keyword
                    && matches!(
                        word.as_str(),
                        "if" | "then"
                            | "else"
                            | "elif"
                            | "do"
                            | "while"
                            | "until"
                            | "{"
                            | "!"
                            | "time"
                    )
                {
                    next_cmd_pos = true;
                }
            }
        }
        st.cmd_pos = next_cmd_pos;
    }

    /// The open case in the top frame, if it is at the current paren depth.
    fn case_here(&self) -> Option<CasePhase> {
        let depth = self.parens.last().copied().unwrap_or(0);
        self.code
            .last()
            .and_then(|st| st.cases.last())
            .filter(|c| c.depth == depth)
            .map(|c| c.phase)
    }

    fn set_case_phase(&mut self, phase: CasePhase) {
        if let Some(c) = self.code.last_mut().and_then(|st| st.cases.last_mut()) {
            c.phase = phase;
        }
    }

    fn set_cmd_pos(&mut self, on: bool) {
        if let Some(st) = self.code.last_mut() {
            st.cmd_pos = on;
        }
    }

    fn feed_code(&mut self, c: char) -> Step {
        let lt_run = if c == '<' { self.lt_run + 1 } else { 0 };
        let heredoc_op = self.lt_run == 2 && c != '<' && self.arith_at.is_none();
        self.lt_run = lt_run;
        if heredoc_op {
            // `<<` followed by something other than a third `<`.
            self.mode = Mode::Delimiter {
                strip_tabs: false,
                word: String::new(),
                started: false,
                quote: None,
            };
            return self.feed_inner(c);
        }
        let word_char = !c.is_whitespace()
            && !matches!(
                c,
                ';' | '|' | '&' | '(' | ')' | '<' | '>' | '\\' | '\'' | '"' | '`'
            );
        if word_char {
            if c == '#'
                && self
                    .prev
                    .is_none_or(|p| p.is_whitespace() || matches!(p, ';' | '|' | '&' | '(' | ')'))
            {
                self.end_word();
                self.mode = Mode::Comment;
                return Step::Body;
            }
            if let Some(st) = self.code.last_mut()
                && st.word.len() < MAX_KEYWORD
            {
                st.word.push(c);
            } else if let Some(st) = self.code.last_mut() {
                st.word_quoted = true; // too long to be a keyword
            }
            return Step::Body;
        }
        match c {
            '\\' => {
                self.quote_word();
                self.mode = Mode::Escape;
            }
            '\'' => {
                self.quote_word();
                self.stack.push(Frame::Single);
            }
            '"' => {
                self.quote_word();
                self.stack.push(Frame::Double);
            }
            '`' => {
                self.quote_word();
                self.stack.push(Frame::Backtick);
            }
            '\n' => {
                self.end_word();
                self.set_cmd_pos(true);
                self.on_newline();
            }
            '(' => {
                self.end_word();
                // The optional `(` before a case pattern is not nesting.
                if let Some(CasePhase::Pattern { started: false }) = self.case_here() {
                    self.set_case_phase(CasePhase::Pattern { started: true });
                    return Step::Body;
                }
                if let Some(depth) = self.parens.last_mut() {
                    *depth += 1;
                    if self.prev == Some('(') && self.arith_at.is_none() {
                        self.arith_at = Some(depth.saturating_sub(2));
                    }
                }
                self.set_cmd_pos(true);
            }
            ')' => {
                self.end_word();
                // `)` ending a case pattern list starts the branch body.
                if let Some(CasePhase::Pattern { .. }) = self.case_here() {
                    self.set_case_phase(CasePhase::Body);
                    self.set_cmd_pos(true);
                    return Step::Body;
                }
                let depth = self.parens.last_mut().expect("code frame");
                if *depth > 0 {
                    *depth -= 1;
                    if self.arith_at.is_some_and(|at| *depth <= at) {
                        self.arith_at = None;
                    }
                    self.set_cmd_pos(false);
                } else {
                    // Closes this `$(` frame.
                    self.parens.pop();
                    self.code.pop();
                    self.stack.pop();
                    if self.stack.is_empty() {
                        return Step::Close;
                    }
                }
            }
            ';' | '&' => {
                self.end_word();
                // `;;`, `;&` and `;;&` end a case branch.
                if self.prev == Some(';') && self.case_here() == Some(CasePhase::Body) {
                    self.set_case_phase(CasePhase::Pattern { started: false });
                }
                self.set_cmd_pos(true);
            }
            '|' => {
                self.end_word();
                if !matches!(self.case_here(), Some(CasePhase::Pattern { .. })) {
                    self.set_cmd_pos(true);
                }
            }
            _ => self.end_word(),
        }
        Step::Body
    }
}

#[cfg(test)]
/// Split `s` (the text after `$(`) at the matching `)`: returns the body and
/// the byte offset just past the `)`, or `None` when unterminated.
fn split_subst_body(s: &str) -> Option<(&str, usize)> {
    let mut scanner = SubstScanner::new();
    for (i, c) in s.char_indices() {
        if scanner.feed(c) == Step::Close {
            return Some((&s[..i], i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(s: &str) -> Option<&str> {
        split_subst_body(s).map(|(b, _)| b)
    }

    #[test]
    fn plain_and_nested_parens() {
        assert_eq!(body("echo hi) rest"), Some("echo hi"));
        assert_eq!(body("(a) (b)) x"), Some("(a) (b)"));
        assert_eq!(body("echo $(date)) x"), Some("echo $(date)"));
    }

    #[test]
    fn quotes_hide_parens() {
        assert_eq!(body("echo \")\") x"), Some("echo \")\""));
        assert_eq!(body("echo ')') x"), Some("echo ')'"));
        assert_eq!(body("echo \\)) x"), Some("echo \\)"));
        assert_eq!(
            body("echo \"$(echo ')')\") x"),
            Some("echo \"$(echo ')')\"")
        );
        assert_eq!(body("echo `echo )`) x"), Some("echo `echo )`"));
    }

    #[test]
    fn comments_hide_parens() {
        assert_eq!(body("echo a # c)\n) x"), Some("echo a # c)\n"));
        assert_eq!(body("echo a#b) x"), Some("echo a#b"));
    }

    #[test]
    fn heredoc_bodies_hide_parens() {
        assert_eq!(
            body("cat <<'E'\nhello )\nE\n) x"),
            Some("cat <<'E'\nhello )\nE\n")
        );
        assert_eq!(
            body("cat <<-EOF\n\t( x\n\tEOF\n) x"),
            Some("cat <<-EOF\n\t( x\n\tEOF\n")
        );
        assert_eq!(
            body("cat <<A <<\"B\"\n)\nA\n)\nB\n) x"),
            Some("cat <<A <<\"B\"\n)\nA\n)\nB\n")
        );
        // here-string is not a heredoc
        assert_eq!(body("cat <<< x) y"), Some("cat <<< x"));
        // arithmetic shift is not a heredoc
        assert_eq!(body("echo $((1<<2))) y"), Some("echo $((1<<2))"));
    }

    #[test]
    fn case_patterns_do_not_close() {
        assert_eq!(
            body("case $x in a) echo A;; *) echo B;; esac) y"),
            Some("case $x in a) echo A;; *) echo B;; esac")
        );
        assert_eq!(
            body("case x in (a|b) echo;; esac) y"),
            Some("case x in (a|b) echo;; esac")
        );
        assert_eq!(
            body("case x in\n a)\n  echo;;\nesac\n) y"),
            Some("case x in\n a)\n  echo;;\nesac\n")
        );
        // nested case, fallthrough terminators
        assert_eq!(
            body("case a in a) case b in b) :;& *) :;;& esac;; esac) y"),
            Some("case a in a) case b in b) :;& *) :;;& esac;; esac")
        );
        // `case`/`esac` only count in command position
        assert_eq!(body("echo case) y"), Some("echo case"));
        assert_eq!(body("echo esac) y"), Some("echo esac"));
        // a `case` word inside a pattern body subshell
        assert_eq!(
            body("case x in x) (echo case);; esac) y"),
            Some("case x in x) (echo case);; esac")
        );
    }

    #[test]
    fn unterminated() {
        assert_eq!(body("echo (hi"), None);
        assert_eq!(body("cat <<E\n)\n"), None);
    }
}
