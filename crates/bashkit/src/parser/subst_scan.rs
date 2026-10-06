//! Finds the `)` that closes a `$(...)` body.
//!
//! Decision: one incremental scanner shared by the lexer and `parse_word`, fed
//! one char at a time, so both agree on where a command substitution ends.
//! Plain paren counting closed `$(cat <<'E'` at a `)` inside the heredoc body,
//! and `$(echo ")")` at the quoted `)`. Bash finds the end by parsing; this
//! tracks the contexts that can hide a `)`: single/double quotes, backslash
//! escapes, backticks, nested `$(...)`, comments and heredoc bodies.
//! Known gap: a `case` pattern's bare `)` inside `$(...)` still closes early,
//! as before this scanner (`$(case x in x) ...esac)`); bash accepts it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// Shell code inside `$(` (or `(` nesting within it).
    Code,
    Single,
    Double,
    Backtick,
}

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
        match c {
            '\\' => self.mode = Mode::Escape,
            '\'' => self.stack.push(Frame::Single),
            '"' => self.stack.push(Frame::Double),
            '`' => self.stack.push(Frame::Backtick),
            '#' if self
                .prev
                .is_none_or(|p| p.is_whitespace() || matches!(p, ';' | '|' | '&' | '(' | ')')) =>
            {
                self.mode = Mode::Comment;
            }
            '\n' => self.on_newline(),
            '(' => {
                if let Some(depth) = self.parens.last_mut() {
                    *depth += 1;
                    if self.prev == Some('(') && self.arith_at.is_none() {
                        self.arith_at = Some(depth.saturating_sub(2));
                    }
                }
            }
            ')' => {
                let depth = self.parens.last_mut().expect("code frame");
                if *depth > 0 {
                    *depth -= 1;
                    if self.arith_at.is_some_and(|at| *depth <= at) {
                        self.arith_at = None;
                    }
                } else {
                    // Closes this `$(` frame.
                    self.parens.pop();
                    self.stack.pop();
                    if self.stack.is_empty() {
                        return Step::Close;
                    }
                }
            }
            _ => {}
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
    fn unterminated() {
        assert_eq!(body("echo (hi"), None);
        assert_eq!(body("cat <<E\n)\n"), None);
    }
}
