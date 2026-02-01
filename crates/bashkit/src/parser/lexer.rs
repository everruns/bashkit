//! Lexer for bash scripts
//!
//! Tokenizes input into a stream of tokens.

use super::tokens::Token;

/// Lexer for bash scripts.
pub struct Lexer<'a> {
    #[allow(dead_code)] // Stored for error reporting in future
    input: &'a str,
    #[allow(dead_code)] // Will be used for position tracking
    pos: usize,
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            chars: input.chars().peekable(),
        }
    }

    /// Get the next token from the input.
    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();

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
                Some(Token::LeftParen)
            }
            ')' => {
                self.advance();
                Some(Token::RightParen)
            }
            '{' => {
                self.advance();
                Some(Token::LeftBrace)
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
                    // Single [ is a command (test)
                    Some(Token::Word("[".to_string()))
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
                self.next_token()
            }
            _ => self.read_word(),
        }
    }

    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.next();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
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

    fn read_word(&mut self) -> Option<Token> {
        let mut word = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == '$' {
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
                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        Some(Token::Word(content))
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
            Some(Token::Word("hello world".to_string()))
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
