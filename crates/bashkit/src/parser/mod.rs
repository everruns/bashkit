//! Parser module for BashKit
//!
//! Implements a recursive descent parser for bash scripts.

mod ast;
mod lexer;
mod tokens;

pub use ast::*;
pub use lexer::Lexer;

use crate::error::{Error, Result};

/// Parser for bash scripts.
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<tokens::Token>,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given input.
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self {
            lexer,
            current_token,
        }
    }

    /// Parse the input and return the AST.
    pub fn parse(mut self) -> Result<Script> {
        let mut commands = Vec::new();

        while self.current_token.is_some() {
            self.skip_newlines();
            if self.current_token.is_none() {
                break;
            }
            if let Some(cmd) = self.parse_command_list()? {
                commands.push(cmd);
            }
        }

        Ok(Script { commands })
    }

    fn advance(&mut self) {
        self.current_token = self.lexer.next_token();
    }

    fn skip_newlines(&mut self) {
        while matches!(self.current_token, Some(tokens::Token::Newline)) {
            self.advance();
        }
    }

    /// Parse a command list (commands connected by && or ||)
    fn parse_command_list(&mut self) -> Result<Option<Command>> {
        let first = match self.parse_pipeline()? {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        let mut rest = Vec::new();

        loop {
            let op = match &self.current_token {
                Some(tokens::Token::And) => {
                    self.advance();
                    ListOperator::And
                }
                Some(tokens::Token::Or) => {
                    self.advance();
                    ListOperator::Or
                }
                Some(tokens::Token::Semicolon) => {
                    self.advance();
                    self.skip_newlines();
                    // Check if there's more to parse
                    if self.current_token.is_none()
                        || matches!(self.current_token, Some(tokens::Token::Newline))
                    {
                        break;
                    }
                    ListOperator::Semicolon
                }
                _ => break,
            };

            self.skip_newlines();

            if let Some(cmd) = self.parse_pipeline()? {
                rest.push((op, cmd));
            } else {
                break;
            }
        }

        if rest.is_empty() {
            Ok(Some(first))
        } else {
            Ok(Some(Command::List(CommandList {
                first: Box::new(first),
                rest,
            })))
        }
    }

    /// Parse a pipeline (commands connected by |)
    fn parse_pipeline(&mut self) -> Result<Option<Command>> {
        let first = match self.parse_simple_command()? {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        let mut commands = vec![Command::Simple(first)];

        while matches!(self.current_token, Some(tokens::Token::Pipe)) {
            self.advance();
            self.skip_newlines();

            if let Some(cmd) = self.parse_simple_command()? {
                commands.push(Command::Simple(cmd));
            } else {
                return Err(Error::Parse("expected command after |".to_string()));
            }
        }

        if commands.len() == 1 {
            Ok(Some(commands.remove(0)))
        } else {
            Ok(Some(Command::Pipeline(Pipeline {
                negated: false,
                commands,
            })))
        }
    }

    /// Parse a simple command with redirections
    fn parse_simple_command(&mut self) -> Result<Option<SimpleCommand>> {
        self.skip_newlines();

        let mut words = Vec::new();
        let mut redirects = Vec::new();

        loop {
            match &self.current_token {
                Some(tokens::Token::Word(w)) => {
                    words.push(self.parse_word(w.clone()));
                    self.advance();
                }
                Some(tokens::Token::RedirectOut) => {
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        kind: RedirectKind::Output,
                        target,
                    });
                }
                Some(tokens::Token::RedirectAppend) => {
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        kind: RedirectKind::Append,
                        target,
                    });
                }
                Some(tokens::Token::RedirectIn) => {
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        kind: RedirectKind::Input,
                        target,
                    });
                }
                Some(tokens::Token::HereString) => {
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        kind: RedirectKind::HereString,
                        target,
                    });
                }
                Some(tokens::Token::Newline)
                | Some(tokens::Token::Semicolon)
                | Some(tokens::Token::Pipe)
                | Some(tokens::Token::And)
                | Some(tokens::Token::Or)
                | None => break,
                _ => break,
            }
        }

        if words.is_empty() {
            return Ok(None);
        }

        let name = words.remove(0);
        let args = words;

        Ok(Some(SimpleCommand {
            name,
            args,
            redirects,
            assignments: Vec::new(),
        }))
    }

    /// Expect a word token and return it as a Word
    fn expect_word(&mut self) -> Result<Word> {
        match &self.current_token {
            Some(tokens::Token::Word(w)) => {
                let word = self.parse_word(w.clone());
                self.advance();
                Ok(word)
            }
            _ => Err(Error::Parse("expected word".to_string())),
        }
    }

    /// Parse a word string into a Word with proper parts (variables, literals)
    fn parse_word(&self, s: String) -> Word {
        let mut parts = Vec::new();
        let mut chars = s.chars().peekable();
        let mut current = String::new();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                // Flush current literal
                if !current.is_empty() {
                    parts.push(WordPart::Literal(std::mem::take(&mut current)));
                }

                // Parse variable name
                if chars.peek() == Some(&'{') {
                    // ${VAR} format
                    chars.next(); // consume '{'
                    let mut var_name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == '}' {
                            chars.next(); // consume '}'
                            break;
                        }
                        var_name.push(chars.next().unwrap());
                    }
                    if !var_name.is_empty() {
                        parts.push(WordPart::Variable(var_name));
                    }
                } else {
                    // $VAR format
                    let mut var_name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_alphanumeric() || c == '_' {
                            var_name.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    if !var_name.is_empty() {
                        parts.push(WordPart::Variable(var_name));
                    } else {
                        // Just a literal $
                        current.push('$');
                    }
                }
            } else {
                current.push(ch);
            }
        }

        // Flush remaining literal
        if !current.is_empty() {
            parts.push(WordPart::Literal(current));
        }

        // If no parts, create an empty literal
        if parts.is_empty() {
            parts.push(WordPart::Literal(String::new()));
        }

        Word { parts }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_command() {
        let parser = Parser::new("echo hello");
        let script = parser.parse().unwrap();

        assert_eq!(script.commands.len(), 1);

        if let Command::Simple(cmd) = &script.commands[0] {
            assert_eq!(cmd.name.to_string(), "echo");
            assert_eq!(cmd.args.len(), 1);
            assert_eq!(cmd.args[0].to_string(), "hello");
        } else {
            panic!("expected simple command");
        }
    }

    #[test]
    fn test_parse_multiple_args() {
        let parser = Parser::new("echo hello world");
        let script = parser.parse().unwrap();

        if let Command::Simple(cmd) = &script.commands[0] {
            assert_eq!(cmd.name.to_string(), "echo");
            assert_eq!(cmd.args.len(), 2);
            assert_eq!(cmd.args[0].to_string(), "hello");
            assert_eq!(cmd.args[1].to_string(), "world");
        } else {
            panic!("expected simple command");
        }
    }

    #[test]
    fn test_parse_variable() {
        let parser = Parser::new("echo $HOME");
        let script = parser.parse().unwrap();

        if let Command::Simple(cmd) = &script.commands[0] {
            assert_eq!(cmd.args.len(), 1);
            assert_eq!(cmd.args[0].parts.len(), 1);
            assert!(matches!(&cmd.args[0].parts[0], WordPart::Variable(v) if v == "HOME"));
        } else {
            panic!("expected simple command");
        }
    }

    #[test]
    fn test_parse_pipeline() {
        let parser = Parser::new("echo hello | cat");
        let script = parser.parse().unwrap();

        assert_eq!(script.commands.len(), 1);
        assert!(matches!(&script.commands[0], Command::Pipeline(_)));

        if let Command::Pipeline(pipeline) = &script.commands[0] {
            assert_eq!(pipeline.commands.len(), 2);
        }
    }

    #[test]
    fn test_parse_redirect_out() {
        let parser = Parser::new("echo hello > /tmp/out");
        let script = parser.parse().unwrap();

        if let Command::Simple(cmd) = &script.commands[0] {
            assert_eq!(cmd.redirects.len(), 1);
            assert_eq!(cmd.redirects[0].kind, RedirectKind::Output);
            assert_eq!(cmd.redirects[0].target.to_string(), "/tmp/out");
        } else {
            panic!("expected simple command");
        }
    }

    #[test]
    fn test_parse_redirect_append() {
        let parser = Parser::new("echo hello >> /tmp/out");
        let script = parser.parse().unwrap();

        if let Command::Simple(cmd) = &script.commands[0] {
            assert_eq!(cmd.redirects.len(), 1);
            assert_eq!(cmd.redirects[0].kind, RedirectKind::Append);
        } else {
            panic!("expected simple command");
        }
    }

    #[test]
    fn test_parse_redirect_in() {
        let parser = Parser::new("cat < /tmp/in");
        let script = parser.parse().unwrap();

        if let Command::Simple(cmd) = &script.commands[0] {
            assert_eq!(cmd.redirects.len(), 1);
            assert_eq!(cmd.redirects[0].kind, RedirectKind::Input);
        } else {
            panic!("expected simple command");
        }
    }

    #[test]
    fn test_parse_command_list_and() {
        let parser = Parser::new("true && echo success");
        let script = parser.parse().unwrap();

        assert!(matches!(&script.commands[0], Command::List(_)));
    }

    #[test]
    fn test_parse_command_list_or() {
        let parser = Parser::new("false || echo fallback");
        let script = parser.parse().unwrap();

        assert!(matches!(&script.commands[0], Command::List(_)));
    }
}
