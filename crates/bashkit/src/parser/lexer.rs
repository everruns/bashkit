//! Lexer for bash scripts
//!
//! Tokenizes input into a stream of tokens with source position tracking.

use super::span::{Position, Span};
use super::tokens::Token;

/// A token with its source location span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

/// Lexer for bash scripts.
pub struct Lexer<'a> {
    #[allow(dead_code)] // Stored for error reporting in future
    input: &'a str,
    /// Current position in the input
    position: Position,
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            position: Position::new(),
            chars: input.chars().peekable(),
        }
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
        self.chars.peek().copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.next();
        if let Some(c) = ch {
            self.position.advance(c);
        }
        ch
    }

    /// Get the next token with its source span.
    pub fn next_spanned_token(&mut self) -> Option<SpannedToken> {
        self.skip_whitespace();
        let start = self.position;
        let token = self.next_token_inner()?;
        let end = self.position;
        Some(SpannedToken {
            token,
            span: Span::from_positions(start, end),
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
                Some(Token::Semicolon)
            }
            '|' => {
                self.advance();
                if self.peek_char() == Some('|') {
                    self.advance();
                    Some(Token::Or)
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
                    Some(Token::RedirectBoth)
                } else {
                    Some(Token::Background)
                }
            }
            '>' => {
                self.advance();
                if self.peek_char() == Some('>') {
                    self.advance();
                    Some(Token::RedirectAppend)
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
                    } else {
                        Some(Token::HereDoc)
                    }
                } else if self.peek_char() == Some('(') {
                    self.advance();
                    Some(Token::ProcessSubIn)
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
                if self.looks_like_brace_expansion() {
                    self.read_brace_expansion_word()
                } else if self.is_brace_group_start() {
                    self.advance();
                    Some(Token::LeftBrace)
                } else {
                    // {single} without comma/dot-dot is kept as literal word
                    self.read_brace_literal_word()
                }
            }
            '}' => {
                self.advance();
                Some(Token::RightBrace)
            }
            '[' => {
                self.advance();
                if self.peek_char() == Some('[') {
                    self.advance();
                    Some(Token::DoubleLeftBracket)
                } else {
                    // [ could be the test command OR a glob bracket expression
                    // If followed by non-whitespace, treat as start of bracket expression
                    // e.g., [abc] is a glob pattern, [ -f file ] is test command
                    match self.peek_char() {
                        Some(' ') | Some('\t') | Some('\n') | None => {
                            // Followed by whitespace or EOF - it's the test command
                            Some(Token::Word("[".to_string()))
                        }
                        _ => {
                            // Part of a glob bracket expression [abc], read the whole thing
                            self.read_bracket_word()
                        }
                    }
                }
            }
            ']' => {
                self.advance();
                if self.peek_char() == Some(']') {
                    self.advance();
                    Some(Token::DoubleRightBracket)
                } else {
                    Some(Token::Word("]".to_string()))
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

    /// Check if this is a file descriptor redirect (e.g., 2>, 2>>, 2>&1)
    /// or just a regular word starting with a digit
    fn read_word_or_fd_redirect(&mut self) -> Option<Token> {
        // We need to look ahead to see if this is a fd redirect pattern
        // Collect the leading digits
        let mut fd_str = String::new();

        // Peek at the first digit - we know it's a digit from the match
        if let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                fd_str.push(ch);
            }
        }

        // Check if it's a single digit followed by > or <
        // We need to peek further without consuming
        let input_remaining: String = self.chars.clone().collect();

        // Check patterns: "N>" "N>>" "N>&" "N<" "N<&"
        if fd_str.len() == 1 {
            if let Some(first_digit) = fd_str.chars().next() {
                let rest = &input_remaining[1..]; // Skip the digit we already matched

                if rest.starts_with(">>") {
                    // N>> - append redirect with fd
                    let fd: i32 = first_digit.to_digit(10).unwrap() as i32;
                    self.advance(); // consume digit
                    self.advance(); // consume >
                    self.advance(); // consume >
                    return Some(Token::RedirectFdAppend(fd));
                } else if rest.starts_with(">&") {
                    // N>&M - duplicate fd
                    let fd: i32 = first_digit.to_digit(10).unwrap() as i32;
                    self.advance(); // consume digit
                    self.advance(); // consume >
                    self.advance(); // consume &

                    // Read the target fd number
                    let mut target_str = String::new();
                    while let Some(c) = self.peek_char() {
                        if c.is_ascii_digit() {
                            target_str.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }

                    if target_str.is_empty() {
                        // Just N>& without target - treat as DupOutput with fd
                        return Some(Token::RedirectFd(fd));
                    }

                    let target_fd: i32 = target_str.parse().unwrap_or(1);
                    return Some(Token::DupFd(fd, target_fd));
                } else if rest.starts_with('>') {
                    // N> - redirect with fd
                    let fd: i32 = first_digit.to_digit(10).unwrap() as i32;
                    self.advance(); // consume digit
                    self.advance(); // consume >
                    return Some(Token::RedirectFd(fd));
                }
            }
        }

        // Not a fd redirect pattern, read as regular word
        self.read_word()
    }

    fn read_word(&mut self) -> Option<Token> {
        let mut word = String::new();

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
                while let Some(c) = self.peek_char() {
                    if c == quote_char {
                        self.advance(); // consume closing quote
                        break;
                    }
                    if c == '\\' && quote_char == '"' {
                        self.advance();
                        if let Some(next) = self.peek_char() {
                            match next {
                                '"' | '\\' | '$' | '`' => {
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
                    word.push(c);
                    self.advance();
                }
                continue;
            } else if ch == '$' {
                // Handle variable references and command substitution
                word.push(ch);
                self.advance();

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
                        // Command substitution $(...) - track nested parens
                        let mut depth = 1;
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
                    }
                } else if self.peek_char() == Some('{') {
                    // ${VAR} format
                    word.push('{');
                    self.advance();
                    // Read until closing }
                    while let Some(c) = self.peek_char() {
                        word.push(c);
                        self.advance();
                        if c == '}' {
                            break;
                        }
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
                // Brace expansion pattern - include entire {...} in word
                word.push(ch);
                self.advance();
                let mut depth = 1;
                while let Some(c) = self.peek_char() {
                    word.push(c);
                    self.advance();
                    if c == '{' {
                        depth += 1;
                    } else if c == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
            } else if ch == '`' {
                // Backtick command substitution: convert `cmd` to $(cmd)
                self.advance(); // consume opening `
                word.push_str("$(");
                while let Some(c) = self.peek_char() {
                    if c == '`' {
                        self.advance(); // consume closing `
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
                word.push(')');
            } else if ch == '\\' {
                self.advance();
                if let Some(next) = self.peek_char() {
                    if next == '\n' {
                        // Line continuation: skip backslash + newline
                        self.advance();
                    } else {
                        // Escaped character: backslash quotes the next char
                        // (quote removal — only the literal char survives)
                        word.push(next);
                        self.advance();
                    }
                } else {
                    word.push('\\');
                }
            } else if self.is_word_char(ch) {
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if word.is_empty() {
            None
        } else {
            Some(Token::Word(word))
        }
    }

    fn read_single_quoted_string(&mut self) -> Option<Token> {
        self.advance(); // consume opening '
        let mut content = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == '\'' {
                self.advance(); // consume closing '
                break;
            }
            content.push(ch);
            self.advance();
        }

        // Single-quoted strings are literal - no variable expansion
        Some(Token::LiteralWord(content))
    }

    fn read_double_quoted_string(&mut self) -> Option<Token> {
        self.advance(); // consume opening "
        let mut content = String::new();

        while let Some(ch) = self.peek_char() {
            match ch {
                '"' => {
                    self.advance(); // consume closing "
                    break;
                }
                '\\' => {
                    self.advance();
                    if let Some(next) = self.peek_char() {
                        // Handle escape sequences
                        match next {
                            '"' | '\\' | '$' | '`' | '\n' => {
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
                '`' => {
                    // Backtick command substitution inside double quotes
                    self.advance(); // consume opening `
                    content.push_str("$(");
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
                    content.push(')');
                }
                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        Some(Token::QuotedWord(content))
    }

    /// Check if the content starting with { looks like a brace expansion
    /// Brace expansion: {a,b,c} or {1..5} (contains , or ..)
    /// Brace group: { cmd; } (contains spaces, semicolons, newlines)
    fn looks_like_brace_expansion(&self) -> bool {
        // Clone the iterator to peek ahead without consuming
        let mut chars = self.chars.clone();

        // Skip the opening {
        if chars.next() != Some('{') {
            return false;
        }

        let mut depth = 1;
        let mut has_comma = false;
        let mut has_dot_dot = false;
        let mut prev_char = None;

        for ch in chars {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        // Found matching }, check if we have brace expansion markers
                        return has_comma || has_dot_dot;
                    }
                }
                ',' if depth == 1 => has_comma = true,
                '.' if prev_char == Some('.') && depth == 1 => has_dot_dot = true,
                // Brace groups have whitespace/newlines/semicolons at depth 1
                ' ' | '\t' | '\n' | ';' if depth == 1 => return false,
                _ => {}
            }
            prev_char = Some(ch);
        }

        false
    }

    /// Check if { is followed by whitespace (brace group start)
    fn is_brace_group_start(&self) -> bool {
        let mut chars = self.chars.clone();
        // Skip the opening {
        if chars.next() != Some('{') {
            return false;
        }
        // If next char is whitespace or newline, it's a brace group
        matches!(chars.next(), Some(' ') | Some('\t') | Some('\n') | None)
    }

    /// Read a {literal} pattern without comma/dot-dot as a word
    fn read_brace_literal_word(&mut self) -> Option<Token> {
        let mut word = String::new();

        // Read the opening {
        if let Some('{') = self.peek_char() {
            word.push('{');
            self.advance();
        } else {
            return None;
        }

        // Read until matching }
        let mut depth = 1;
        while let Some(ch) = self.peek_char() {
            word.push(ch);
            self.advance();
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }

        // Continue reading any suffix
        while let Some(ch) = self.peek_char() {
            if self.is_word_char(ch) {
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        Some(Token::Word(word))
    }

    /// Read a brace expansion pattern as a word
    fn read_brace_expansion_word(&mut self) -> Option<Token> {
        let mut word = String::new();

        // Read the opening {
        if let Some('{') = self.peek_char() {
            word.push('{');
            self.advance();
        } else {
            return None;
        }

        // Read until matching }
        let mut depth = 1;
        while let Some(ch) = self.peek_char() {
            word.push(ch);
            self.advance();
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }

        // Continue reading any suffix after the brace pattern
        while let Some(ch) = self.peek_char() {
            if self.is_word_char(ch) || ch == '{' {
                if ch == '{' {
                    // Another brace pattern - include it
                    word.push(ch);
                    self.advance();
                    let mut inner_depth = 1;
                    while let Some(c) = self.peek_char() {
                        word.push(c);
                        self.advance();
                        match c {
                            '{' => inner_depth += 1,
                            '}' => {
                                inner_depth -= 1;
                                if inner_depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    word.push(ch);
                    self.advance();
                }
            } else {
                break;
            }
        }

        Some(Token::Word(word))
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
            if self.is_word_char(ch) {
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        Some(Token::Word(word))
    }

    fn is_word_char(&self, ch: char) -> bool {
        !matches!(
            ch,
            ' ' | '\t'
                | '\n'
                | ';'
                | '|'
                | '&'
                | '>'
                | '<'
                | '('
                | ')'
                | '{'
                | '}'
                | '\''
                | '"'
                | '#'
        )
    }

    /// Read here document content until the delimiter line is found
    pub fn read_heredoc(&mut self, delimiter: &str) -> String {
        let mut content = String::new();
        let mut current_line = String::new();

        // Skip to end of current line first (after the delimiter on command line)
        while let Some(ch) = self.peek_char() {
            self.advance();
            if ch == '\n' {
                break;
            }
        }

        // Read lines until we find the delimiter
        loop {
            match self.peek_char() {
                Some('\n') => {
                    self.advance();
                    // Check if current line matches delimiter
                    if current_line.trim() == delimiter {
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
                    // End of input - check last line
                    if current_line.trim() == delimiter {
                        break;
                    }
                    if !current_line.is_empty() {
                        content.push_str(&current_line);
                    }
                    break;
                }
            }
        }

        content
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
}
