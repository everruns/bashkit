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
            if let Some(cmd) = self.parse_command()? {
                commands.push(cmd);
            }
            self.skip_newlines();
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

    fn parse_command(&mut self) -> Result<Option<Command>> {
        self.skip_newlines();

        match &self.current_token {
            None => Ok(None),
            Some(tokens::Token::Word(_)) => {
                let simple_cmd = self.parse_simple_command()?;
                Ok(Some(Command::Simple(simple_cmd)))
            }
            Some(token) => Err(Error::Parse(format!("unexpected token: {:?}", token))),
        }
    }

    fn parse_simple_command(&mut self) -> Result<SimpleCommand> {
        let mut words = Vec::new();

        while let Some(token) = &self.current_token {
            match token {
                tokens::Token::Word(w) => {
                    words.push(Word {
                        parts: vec![WordPart::Literal(w.clone())],
                    });
                    self.advance();
                }
                tokens::Token::Newline | tokens::Token::Semicolon => {
                    self.advance();
                    break;
                }
                _ => break,
            }
        }

        if words.is_empty() {
            return Err(Error::Parse("empty command".to_string()));
        }

        let name = words.remove(0);
        let args = words;

        Ok(SimpleCommand {
            name,
            args,
            redirects: Vec::new(),
            assignments: Vec::new(),
        })
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
}
