//! Conservative syntax-nesting estimate for TypeScript source, checked before
//! the source reaches ZapCode.
//!
//! # Why
//!
//! ZapCode 1.5.1 parses with oxc and then lowers and compiles the AST with
//! plain recursion and no depth guard. A few KB of nested source (`[[[...`,
//! `1+1+...`, `!!!...`) overflows the native stack, which aborts the whole
//! host process. Monty's parser has a recursion limit ("Source is too deeply
//! nested"); ZapCode has none, so bashkit adds one in front of it.
//!
//! # Decisions
//!
//! - The estimate must never be lower than the real AST depth. It is an upper
//!   bound: every frame (`(`, `[`, `{`, `${`) counts one, plus every token since
//!   the last sibling boundary in each open frame. An AST level always consumes
//!   at least one token, so chains like `1+1+1`, `a.b.c`, `f()()`, `!!x`,
//!   `x=>x=>`, `a?b:c?d:` and `else if` are all counted.
//! - Sibling boundaries (`,` and `;`, and a newline between two complete
//!   statements in a block or at top level) reset the current frame's count,
//!   so long array literals, argument lists and programs stay small.
//! - A closing bracket merges the child frame's count into its parent instead
//!   of dropping it, so a mismatched or fake closer cannot lower the estimate.
//!   A closer that does not match the open frame is counted as a plain token.
//! - Regex-versus-division is ambiguous to a lexer. Regex bodies are charged one
//!   token per punctuation char and per word, so mistaking a division chain for
//!   a regex still counts the chain. `/` after the `)` of `if`/`for`/`while`/
//!   `with` is a regex, as in the JS grammar. After `}` a `/` is read as
//!   division; a regex statement right after a block cannot continue an
//!   expression or an `else if` chain, so misreading it cannot hide depth.
//! - On wasm32 there is no thread with a large stack; the same cap applies
//!   but ZapCode runs on the default wasm stack (see knowledge).
//! - The estimate may be too high for unusual code (a single statement with
//!   more than [`MAX_TS_NESTING`] tokens and no `,`/`;`); that is rejected with
//!   a clear error rather than risking a crash.

/// Highest accepted nesting estimate. ZapCode runs on a thread whose stack is
/// sized for this (see `TS_VM_STACK_BYTES` in `typescript.rs`), with margin
/// checked by `deep_nesting_at_the_limit_does_not_overflow` in debug builds.
pub(crate) const MAX_TS_NESTING: usize = 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Top,
    Paren { keyword: bool },
    Bracket,
    Brace,
    Template,
}

struct Frame {
    kind: FrameKind,
    count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// Identifier or keyword that ends an operand (`x`, `this`, `true`).
    Word,
    /// Keyword that expects an operand after it (`return`, `typeof`, `new`).
    PrefixKeyword,
    Literal,
    Open,
    /// `)` of `if (...)`, `for (...)`, `while (...)`, `with (...)`.
    KeywordClose,
    Close,
    Punct,
    Boundary,
}

/// Keywords after which an expression (and therefore a regex) may start, and
/// after which a newline does not end a statement.
const PREFIX_KEYWORDS: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
    "extends",
    "as",
    "satisfies",
    "keyof",
    "export",
    "default",
    "import",
    "let",
    "const",
    "var",
    "function",
    "class",
    "async",
    "static",
    "readonly",
    "type",
    "interface",
    "enum",
    "declare",
    "if",
    "while",
    "for",
    "with",
];

/// Words that continue the previous line's expression.
const CONTINUATION_WORDS: &[&str] = &["in", "instanceof", "of", "as", "satisfies", "else"];

struct Scanner<'a> {
    src: &'a [u8],
    i: usize,
    frames: Vec<Frame>,
    /// Sum of every frame's `count`, plus the number of frames.
    depth: usize,
    max: usize,
    prev: Tok,
    newline_pending: bool,
    /// The previous token was `if`/`for`/`while`/`with`.
    prev_paren_keyword: bool,
}

/// Return a conservative upper bound on the AST nesting depth of `src`.
pub(crate) fn estimate(src: &str) -> usize {
    let mut s = Scanner {
        src: src.as_bytes(),
        i: 0,
        frames: vec![Frame {
            kind: FrameKind::Top,
            count: 0,
        }],
        depth: 1,
        max: 1,
        prev: Tok::Boundary,
        newline_pending: false,
        prev_paren_keyword: false,
    };
    s.run();
    s.max
}

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

impl Scanner<'_> {
    fn peek(&self, off: usize) -> u8 {
        self.src.get(self.i + off).copied().unwrap_or(0)
    }

    fn top(&mut self) -> &mut Frame {
        // The Top frame is never popped (`close` checks the kind first).
        let last = self.frames.len() - 1;
        &mut self.frames[last]
    }

    fn add(&mut self, n: usize) {
        self.top().count += n;
        self.depth += n;
        self.max = self.max.max(self.depth);
    }

    fn reset(&mut self) {
        let c = std::mem::take(&mut self.top().count);
        self.depth -= c;
    }

    fn push(&mut self, kind: FrameKind) {
        self.frames.push(Frame { kind, count: 0 });
        self.depth += 1;
        self.max = self.max.max(self.depth);
    }

    /// Called before each real token; resolves a pending newline boundary.
    fn begin(&mut self, next: Tok, word: &[u8]) {
        if !self.newline_pending {
            return;
        }
        self.newline_pending = false;
        if !matches!(self.top().kind, FrameKind::Top | FrameKind::Brace) {
            return;
        }
        let prev_ends = matches!(self.prev, Tok::Word | Tok::Literal | Tok::Close);
        let next_starts = match next {
            Tok::Word | Tok::PrefixKeyword => {
                !CONTINUATION_WORDS.iter().any(|w| w.as_bytes() == word)
            }
            Tok::Close => true,
            _ => false,
        };
        if prev_ends && next_starts {
            self.reset();
        }
    }

    fn regex_allowed(&self) -> bool {
        matches!(
            self.prev,
            Tok::PrefixKeyword | Tok::Open | Tok::KeywordClose | Tok::Punct | Tok::Boundary
        )
    }

    fn run(&mut self) {
        while self.i < self.src.len() {
            let c = self.src[self.i];
            let paren_keyword = std::mem::take(&mut self.prev_paren_keyword);
            match c {
                b'\n' | b'\r' => {
                    self.newline_pending = true;
                    self.prev_paren_keyword = paren_keyword;
                    self.i += 1;
                }
                b' ' | b'\t' | 0x0b | 0x0c => {
                    self.prev_paren_keyword = paren_keyword;
                    self.i += 1;
                }
                b'/' if self.peek(1) == b'/' => {
                    self.prev_paren_keyword = paren_keyword;
                    self.line_comment();
                }
                b'/' if self.peek(1) == b'*' => {
                    self.prev_paren_keyword = paren_keyword;
                    self.block_comment();
                }
                b'/' if self.regex_allowed() => self.regex(),
                b'\'' | b'"' => {
                    self.begin(Tok::Literal, b"");
                    self.i += 1;
                    self.string(c);
                    self.add(1);
                    self.prev = Tok::Literal;
                }
                b'`' => {
                    self.begin(Tok::Literal, b"");
                    self.i += 1;
                    self.add(1);
                    self.template_text();
                }
                b'(' | b'[' | b'{' => {
                    self.begin(Tok::Open, b"");
                    self.i += 1;
                    self.add(1);
                    self.push(match c {
                        b'(' => FrameKind::Paren {
                            keyword: paren_keyword,
                        },
                        b'[' => FrameKind::Bracket,
                        _ => FrameKind::Brace,
                    });
                    self.prev = Tok::Open;
                }
                b')' | b']' | b'}' => {
                    self.begin(Tok::Close, b"");
                    self.i += 1;
                    self.close(c);
                }
                b',' | b';' => {
                    self.begin(Tok::Boundary, b"");
                    self.i += 1;
                    self.reset();
                    self.prev = Tok::Boundary;
                }
                c if is_word_byte(c) => self.word(),
                _ => {
                    self.begin(Tok::Punct, b"");
                    self.i += 1;
                    self.add(1);
                    self.prev = Tok::Punct;
                }
            }
        }
    }

    fn word(&mut self) {
        let start = self.i;
        while self.i < self.src.len() && is_word_byte(self.src[self.i]) {
            self.i += 1;
        }
        let word = &self.src[start..self.i];
        let numeric = word[0].is_ascii_digit();
        let kind = if numeric {
            Tok::Literal
        } else if PREFIX_KEYWORDS.iter().any(|k| k.as_bytes() == word) {
            Tok::PrefixKeyword
        } else {
            Tok::Word
        };
        self.begin(kind, word);
        // A number like `1.5e+3` lexes as several tokens; that only overcounts.
        self.add(1);
        self.prev = kind;
        self.prev_paren_keyword = matches!(word, b"if" | b"for" | b"while" | b"with") && !numeric;
    }

    fn close(&mut self, c: u8) {
        let top = self.frames.len() - 1;
        let matches = matches!(
            (self.frames[top].kind, c),
            (FrameKind::Paren { .. }, b')')
                | (FrameKind::Bracket, b']')
                | (FrameKind::Brace | FrameKind::Template, b'}')
        );
        if !matches {
            // Unbalanced source: count the closer, never lower the estimate.
            self.add(1);
            self.prev = Tok::Close;
            return;
        }
        let frame = self.frames.pop().expect("matched frame exists");
        // Fold the child into the parent (frame slot + its tokens stay counted).
        self.top().count += frame.count + 1;
        self.max = self.max.max(self.depth);
        match frame.kind {
            FrameKind::Template => self.template_text(),
            FrameKind::Paren { keyword: true } => self.prev = Tok::KeywordClose,
            _ => self.prev = Tok::Close,
        }
    }

    fn string(&mut self, quote: u8) {
        while self.i < self.src.len() {
            match self.src[self.i] {
                b'\\' => self.i += 2,
                c if c == quote => {
                    self.i += 1;
                    return;
                }
                _ => self.i += 1,
            }
        }
    }

    /// Scan template text after a backtick or after the `}` of `${...}`.
    fn template_text(&mut self) {
        while self.i < self.src.len() {
            match self.src[self.i] {
                b'\\' => self.i += 2,
                b'`' => {
                    self.i += 1;
                    self.prev = Tok::Literal;
                    return;
                }
                b'$' if self.peek(1) == b'{' => {
                    self.i += 2;
                    self.add(1);
                    self.push(FrameKind::Template);
                    self.prev = Tok::Open;
                    return;
                }
                _ => self.i += 1,
            }
        }
        self.prev = Tok::Literal;
    }

    fn line_comment(&mut self) {
        while self.i < self.src.len() && self.src[self.i] != b'\n' {
            self.i += 1;
        }
    }

    fn block_comment(&mut self) {
        self.i += 2;
        while self.i < self.src.len() {
            if self.src[self.i] == b'*' && self.peek(1) == b'/' {
                self.i += 2;
                return;
            }
            if self.src[self.i] == b'\n' {
                self.newline_pending = true;
            }
            self.i += 1;
        }
    }

    /// Scan a regex literal. Its body is charged like code (one token per
    /// punctuation char and per word), so a division chain mistaken for a
    /// regex is still counted. A newline before the closing `/` means it was
    /// not a regex: the `/` is then counted as an operator.
    fn regex(&mut self) {
        self.begin(Tok::Literal, b"");
        let start = self.i;
        self.i += 1;
        let mut tokens = 1;
        let mut in_class = false;
        let mut in_word = false;
        while self.i < self.src.len() {
            let c = self.src[self.i];
            if c == b'\n' || c == b'\r' {
                self.i = start + 1;
                self.add(1);
                self.prev = Tok::Punct;
                return;
            }
            let word = is_word_byte(c);
            if (word && !in_word) || (!word && !c.is_ascii_whitespace()) {
                tokens += 1;
            }
            in_word = word;
            match c {
                b'\\' => self.i += 2,
                b'[' => {
                    in_class = true;
                    self.i += 1;
                }
                b']' => {
                    in_class = false;
                    self.i += 1;
                }
                b'/' if !in_class => {
                    self.i += 1;
                    while self.i < self.src.len() && is_word_byte(self.src[self.i]) {
                        self.i += 1;
                    }
                    self.add(tokens);
                    self.prev = Tok::Literal;
                    return;
                }
                _ => self.i += 1,
            }
        }
        // Unterminated: treat as division from the `/` on.
        self.i = start + 1;
        self.add(1);
        self.prev = Tok::Punct;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nest(open: &str, inner: &str, close: &str, n: usize) -> String {
        format!("{}{}{}", open.repeat(n), inner, close.repeat(n))
    }

    #[test]
    fn ordinary_programs_stay_small() {
        let program = r#"
const nums = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
function total(xs: number[]): number {
  let s = 0
  for (const x of xs) {
    if (x % 2 === 0) { s += x } else { s -= x }
  }
  return s
}
const re = /a(b)c[)\]]/g
console.log(`sum=${total(nums)} ${"a" + "b"}`)
"#;
        assert!(estimate(program) < 100, "{}", estimate(program));
    }

    #[test]
    fn long_flat_lists_and_programs_stay_small() {
        let arr = format!("const a = [{}];", vec!["1"; 50_000].join(", "));
        assert!(estimate(&arr) < 10);
        let lines = "let x = 1\nx = x + 1\nconsole.log(x)\n".repeat(5_000);
        assert!(estimate(&lines) < 10);
        let semis = "x = x + 1;".repeat(5_000);
        assert!(estimate(&semis) < 10);
        let methods = format!("class A {{\n{}}}", "  m() { return 1 }\n".repeat(2_000));
        assert!(estimate(&methods) < 20, "methods {}", estimate(&methods));
    }

    #[test]
    fn every_nesting_shape_is_counted() {
        let n = 2_000;
        let cases = [
            nest("[", "1", "]", n),
            nest("(", "1", ")", n),
            nest("{", "", "}", n),
            nest("`${", "1", "}`", n),
            format!("x = {}1", "1+".repeat(n)),
            format!("x = {}1", "!".repeat(n)),
            format!("x = {}1", "- ".repeat(n)),
            format!("x = {}1", "typeof ".repeat(n)),
            format!("x = a{}", ".b".repeat(n)),
            format!("x = f{}", "()".repeat(n)),
            format!("x = {}1", "a=>".repeat(n)),
            format!("x = {}1", "a?b:".repeat(n)),
            format!("x = {}1", "new ".repeat(n)),
            format!("{}x", "if(1)".repeat(n)),
            format!("if(1){{}}{}", "else if(1){}".repeat(n)),
            format!("x = 1{}", "\n+1".repeat(n)),
            format!("x = a{}", "\n.b".repeat(n)),
            format!("x = {}1", "!\n".repeat(n)),
            // Division chain after `}` that a lexer could read as a regex.
            format!("x = {{}}/{}1/2", "1+".repeat(n)),
            // Regex after `if (...)` holding fake closers and separators.
            format!("{}x", "if(1)/\\),;\\]/\nelse ".repeat(n)),
        ];
        for (i, src) in cases.iter().enumerate() {
            assert!(estimate(src) >= n, "case {i}: {} < {n}", estimate(src));
        }
    }

    #[test]
    fn fake_closers_do_not_lower_the_estimate() {
        let n = 2_000;
        let src = format!("x = ({}1", "1+".repeat(n)) + &")".repeat(50) + &"+1".repeat(n);
        assert!(estimate(&src) >= 2 * n);
        let unbalanced = format!("{}{}", "[".repeat(n), "}".repeat(n));
        assert!(estimate(&unbalanced) >= n);
    }

    #[test]
    fn strings_comments_and_templates_hide_brackets() {
        let src = format!(
            "x = '{}' + \"{}\" // {}\n/* {} */ y = `{}`",
            "(".repeat(5_000),
            "[".repeat(5_000),
            "{".repeat(5_000),
            "(".repeat(5_000),
            "[".repeat(5_000)
        );
        assert!(estimate(&src) < 20, "{}", estimate(&src));
    }
}
