//! Parser module for BashKit
//!
//! Implements a recursive descent parser for bash scripts.
//!
//! # Design Notes
//!
//! Reserved words (like `done`, `fi`, `then`) are only treated as special in command
//! position - when they would start a command. In argument position, they are regular
//! words. The termination of compound commands is handled by `parse_compound_list_until`
//! which checks for terminators BEFORE parsing each command.

// Parser uses chars().next().unwrap() after validating character presence.
// This is safe because we check bounds before accessing.
#![allow(clippy::unwrap_used)]

mod ast;
mod lexer;
mod tokens;

pub use ast::*;
pub use lexer::Lexer;

use crate::error::{Error, Result};

/// Default maximum AST depth (matches ExecutionLimits default)
const DEFAULT_MAX_AST_DEPTH: usize = 100;

/// Default maximum parser operations (matches ExecutionLimits default)
const DEFAULT_MAX_PARSER_OPERATIONS: usize = 100_000;

/// Parser for bash scripts.
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<tokens::Token>,
    /// Lookahead token for function parsing
    peeked_token: Option<tokens::Token>,
    /// Maximum allowed AST nesting depth
    max_depth: usize,
    /// Current nesting depth
    current_depth: usize,
    /// Remaining fuel for parsing operations
    fuel: usize,
    /// Maximum fuel (for error reporting)
    max_fuel: usize,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given input.
    pub fn new(input: &'a str) -> Self {
        Self::with_limits(input, DEFAULT_MAX_AST_DEPTH, DEFAULT_MAX_PARSER_OPERATIONS)
    }

    /// Create a new parser with a custom maximum AST depth.
    pub fn with_max_depth(input: &'a str, max_depth: usize) -> Self {
        Self::with_limits(input, max_depth, DEFAULT_MAX_PARSER_OPERATIONS)
    }

    /// Create a new parser with a custom fuel limit.
    pub fn with_fuel(input: &'a str, max_fuel: usize) -> Self {
        Self::with_limits(input, DEFAULT_MAX_AST_DEPTH, max_fuel)
    }

    /// Create a new parser with custom depth and fuel limits.
    pub fn with_limits(input: &'a str, max_depth: usize, max_fuel: usize) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self {
            lexer,
            current_token,
            peeked_token: None,
            max_depth,
            current_depth: 0,
            fuel: max_fuel,
            max_fuel,
        }
    }

    /// Consume one unit of fuel, returning an error if exhausted
    fn tick(&mut self) -> Result<()> {
        if self.fuel == 0 {
            let used = self.max_fuel;
            return Err(Error::Parse(format!(
                "parser fuel exhausted ({} operations, max {})",
                used, self.max_fuel
            )));
        }
        self.fuel -= 1;
        Ok(())
    }

    /// Push nesting depth and check limit
    fn push_depth(&mut self) -> Result<()> {
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            return Err(Error::Parse(format!(
                "AST nesting too deep ({} levels, max {})",
                self.current_depth, self.max_depth
            )));
        }
        Ok(())
    }

    /// Pop nesting depth
    fn pop_depth(&mut self) {
        if self.current_depth > 0 {
            self.current_depth -= 1;
        }
    }

    /// Parse the input and return the AST.
    pub fn parse(mut self) -> Result<Script> {
        let mut commands = Vec::new();

        while self.current_token.is_some() {
            self.tick()?;
            self.skip_newlines()?;
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
        if let Some(peeked) = self.peeked_token.take() {
            self.current_token = Some(peeked);
        } else {
            self.current_token = self.lexer.next_token();
        }
    }

    /// Peek at the next token without consuming the current one
    fn peek_next(&mut self) -> Option<&tokens::Token> {
        if self.peeked_token.is_none() {
            self.peeked_token = self.lexer.next_token();
        }
        self.peeked_token.as_ref()
    }

    fn skip_newlines(&mut self) -> Result<()> {
        while matches!(self.current_token, Some(tokens::Token::Newline)) {
            self.tick()?;
            self.advance();
        }
        Ok(())
    }

    /// Parse a command list (commands connected by && or ||)
    fn parse_command_list(&mut self) -> Result<Option<Command>> {
        self.tick()?;
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
                    self.skip_newlines()?;
                    // Check if there's more to parse
                    if self.current_token.is_none()
                        || matches!(self.current_token, Some(tokens::Token::Newline))
                    {
                        break;
                    }
                    ListOperator::Semicolon
                }
                Some(tokens::Token::Background) => {
                    self.advance();
                    self.skip_newlines()?;
                    // Check if there's more to parse after &
                    if self.current_token.is_none()
                        || matches!(self.current_token, Some(tokens::Token::Newline))
                    {
                        // Just & at end - return as background
                        rest.push((
                            ListOperator::Background,
                            Command::Simple(SimpleCommand {
                                name: Word::literal(""),
                                args: vec![],
                                redirects: vec![],
                                assignments: vec![],
                            }),
                        ));
                        break;
                    }
                    ListOperator::Background
                }
                _ => break,
            };

            self.skip_newlines()?;

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
        let first = match self.parse_command()? {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        let mut commands = vec![first];

        while matches!(self.current_token, Some(tokens::Token::Pipe)) {
            self.advance();
            self.skip_newlines()?;

            if let Some(cmd) = self.parse_command()? {
                commands.push(cmd);
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

    /// Parse a single command (simple or compound)
    fn parse_command(&mut self) -> Result<Option<Command>> {
        self.skip_newlines()?;

        // Check for compound commands and function keyword
        if let Some(tokens::Token::Word(w)) = &self.current_token {
            let word = w.clone();
            match word.as_str() {
                "if" => return self.parse_if().map(|c| Some(Command::Compound(c))),
                "for" => return self.parse_for().map(|c| Some(Command::Compound(c))),
                "while" => return self.parse_while().map(|c| Some(Command::Compound(c))),
                "until" => return self.parse_until().map(|c| Some(Command::Compound(c))),
                "case" => return self.parse_case().map(|c| Some(Command::Compound(c))),
                "time" => return self.parse_time().map(|c| Some(Command::Compound(c))),
                "function" => return self.parse_function_keyword().map(Some),
                _ => {
                    // Check for POSIX-style function: name() { body }
                    // Don't match if word contains '=' (that's an assignment like arr=(a b c))
                    if !word.contains('=')
                        && matches!(self.peek_next(), Some(tokens::Token::LeftParen))
                    {
                        return self.parse_function_posix().map(Some);
                    }
                }
            }
        }

        // Check for arithmetic command ((expression))
        if matches!(self.current_token, Some(tokens::Token::DoubleLeftParen)) {
            return self
                .parse_arithmetic_command()
                .map(|c| Some(Command::Compound(c)));
        }

        // Check for subshell
        if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
            return self.parse_subshell().map(|c| Some(Command::Compound(c)));
        }

        // Check for brace group
        if matches!(self.current_token, Some(tokens::Token::LeftBrace)) {
            return self.parse_brace_group().map(|c| Some(Command::Compound(c)));
        }

        // Default to simple command
        match self.parse_simple_command()? {
            Some(cmd) => Ok(Some(Command::Simple(cmd))),
            None => Ok(None),
        }
    }

    /// Parse an if statement
    fn parse_if(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume 'if'
        self.skip_newlines()?;

        // Parse condition
        let condition = self.parse_compound_list("then")?;

        // Expect 'then'
        self.expect_keyword("then")?;
        self.skip_newlines()?;

        // Parse then branch
        let then_branch = self.parse_compound_list_until(&["elif", "else", "fi"])?;

        // Parse elif branches
        let mut elif_branches = Vec::new();
        while self.is_keyword("elif") {
            self.advance(); // consume 'elif'
            self.skip_newlines()?;

            let elif_condition = self.parse_compound_list("then")?;
            self.expect_keyword("then")?;
            self.skip_newlines()?;

            let elif_body = self.parse_compound_list_until(&["elif", "else", "fi"])?;
            elif_branches.push((elif_condition, elif_body));
        }

        // Parse else branch
        let else_branch = if self.is_keyword("else") {
            self.advance(); // consume 'else'
            self.skip_newlines()?;
            Some(self.parse_compound_list("fi")?)
        } else {
            None
        };

        // Expect 'fi'
        self.expect_keyword("fi")?;

        self.pop_depth();
        Ok(CompoundCommand::If(IfCommand {
            condition,
            then_branch,
            elif_branches,
            else_branch,
        }))
    }

    /// Parse a for loop
    fn parse_for(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume 'for'
        self.skip_newlines()?;

        // Check for C-style for loop: for ((init; cond; step))
        if matches!(self.current_token, Some(tokens::Token::DoubleLeftParen)) {
            let result = self.parse_arithmetic_for_inner();
            self.pop_depth();
            return result;
        }

        // Expect variable name
        let variable = match &self.current_token {
            Some(tokens::Token::Word(w))
            | Some(tokens::Token::LiteralWord(w))
            | Some(tokens::Token::QuotedWord(w)) => w.clone(),
            _ => {
                self.pop_depth();
                return Err(Error::Parse(
                    "expected variable name in for loop".to_string(),
                ));
            }
        };
        self.advance();

        // Check for 'in' keyword
        let words = if self.is_keyword("in") {
            self.advance(); // consume 'in'

            // Parse word list until do/newline/;
            let mut words = Vec::new();
            loop {
                match &self.current_token {
                    Some(tokens::Token::Word(w)) if w == "do" => break,
                    Some(tokens::Token::Word(w)) | Some(tokens::Token::QuotedWord(w)) => {
                        words.push(self.parse_word(w.clone()));
                        self.advance();
                    }
                    Some(tokens::Token::LiteralWord(w)) => {
                        words.push(Word {
                            parts: vec![WordPart::Literal(w.clone())],
                            quoted: true,
                        });
                        self.advance();
                    }
                    Some(tokens::Token::Newline) | Some(tokens::Token::Semicolon) => {
                        self.advance();
                        break;
                    }
                    _ => break,
                }
            }
            Some(words)
        } else {
            None // for var; do ... (iterates over positional params)
        };

        self.skip_newlines()?;

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines()?;

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Expect 'done'
        self.expect_keyword("done")?;

        self.pop_depth();
        Ok(CompoundCommand::For(ForCommand {
            variable,
            words,
            body,
        }))
    }

    /// Parse C-style arithmetic for loop inner: for ((init; cond; step)); do body; done
    /// Note: depth tracking is done by parse_for which calls this
    fn parse_arithmetic_for_inner(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume '(('

        // Read the three expressions separated by semicolons
        let mut parts: Vec<String> = Vec::new();
        let mut current_expr = String::new();
        let mut paren_depth = 0;

        loop {
            match &self.current_token {
                Some(tokens::Token::DoubleRightParen) => {
                    // End of the (( )) section
                    parts.push(current_expr.trim().to_string());
                    self.advance();
                    break;
                }
                Some(tokens::Token::LeftParen) => {
                    paren_depth += 1;
                    current_expr.push('(');
                    self.advance();
                }
                Some(tokens::Token::RightParen) => {
                    if paren_depth > 0 {
                        paren_depth -= 1;
                        current_expr.push(')');
                        self.advance();
                    } else {
                        // Unexpected - probably error
                        self.advance();
                    }
                }
                Some(tokens::Token::Semicolon) => {
                    if paren_depth == 0 {
                        // Separator between init, cond, step
                        parts.push(current_expr.trim().to_string());
                        current_expr.clear();
                    } else {
                        current_expr.push(';');
                    }
                    self.advance();
                }
                Some(tokens::Token::Word(w))
                | Some(tokens::Token::LiteralWord(w))
                | Some(tokens::Token::QuotedWord(w)) => {
                    if !current_expr.is_empty()
                        && !current_expr.ends_with(' ')
                        && !current_expr.ends_with('(')
                    {
                        current_expr.push(' ');
                    }
                    current_expr.push_str(w);
                    self.advance();
                }
                Some(tokens::Token::Newline) => {
                    self.advance();
                }
                // Handle operators that are normally special tokens but valid in arithmetic
                Some(tokens::Token::RedirectIn) => {
                    current_expr.push('<');
                    self.advance();
                }
                Some(tokens::Token::RedirectOut) => {
                    current_expr.push('>');
                    self.advance();
                }
                Some(tokens::Token::And) => {
                    current_expr.push_str("&&");
                    self.advance();
                }
                Some(tokens::Token::Or) => {
                    current_expr.push_str("||");
                    self.advance();
                }
                Some(tokens::Token::Pipe) => {
                    current_expr.push('|');
                    self.advance();
                }
                Some(tokens::Token::Background) => {
                    current_expr.push('&');
                    self.advance();
                }
                None => {
                    return Err(Error::Parse(
                        "unexpected end of input in for loop".to_string(),
                    ));
                }
                _ => {
                    self.advance();
                }
            }
        }

        // Ensure we have exactly 3 parts
        while parts.len() < 3 {
            parts.push(String::new());
        }

        let init = parts.first().cloned().unwrap_or_default();
        let condition = parts.get(1).cloned().unwrap_or_default();
        let step = parts.get(2).cloned().unwrap_or_default();

        self.skip_newlines()?;

        // Skip optional semicolon after ))
        if matches!(self.current_token, Some(tokens::Token::Semicolon)) {
            self.advance();
        }
        self.skip_newlines()?;

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines()?;

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Expect 'done'
        self.expect_keyword("done")?;

        Ok(CompoundCommand::ArithmeticFor(ArithmeticForCommand {
            init,
            condition,
            step,
            body,
        }))
    }

    /// Parse a while loop
    fn parse_while(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume 'while'
        self.skip_newlines()?;

        // Parse condition
        let condition = self.parse_compound_list("do")?;

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines()?;

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Expect 'done'
        self.expect_keyword("done")?;

        self.pop_depth();
        Ok(CompoundCommand::While(WhileCommand { condition, body }))
    }

    /// Parse an until loop
    fn parse_until(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume 'until'
        self.skip_newlines()?;

        // Parse condition
        let condition = self.parse_compound_list("do")?;

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines()?;

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Expect 'done'
        self.expect_keyword("done")?;

        self.pop_depth();
        Ok(CompoundCommand::Until(UntilCommand { condition, body }))
    }

    /// Parse a case statement: case WORD in pattern) commands ;; ... esac
    fn parse_case(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume 'case'
        self.skip_newlines()?;

        // Get the word to match against
        let word = self.expect_word()?;
        self.skip_newlines()?;

        // Expect 'in'
        self.expect_keyword("in")?;
        self.skip_newlines()?;

        // Parse case items
        let mut cases = Vec::new();
        while !self.is_keyword("esac") && self.current_token.is_some() {
            self.skip_newlines()?;
            if self.is_keyword("esac") {
                break;
            }

            // Parse patterns (pattern1 | pattern2 | ...)
            // Optional leading (
            if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
                self.advance();
            }

            let mut patterns = Vec::new();
            while matches!(
                &self.current_token,
                Some(tokens::Token::Word(_))
                    | Some(tokens::Token::LiteralWord(_))
                    | Some(tokens::Token::QuotedWord(_))
            ) {
                let w = match &self.current_token {
                    Some(tokens::Token::Word(w))
                    | Some(tokens::Token::LiteralWord(w))
                    | Some(tokens::Token::QuotedWord(w)) => w.clone(),
                    _ => unreachable!(),
                };
                patterns.push(self.parse_word(w));
                self.advance();

                // Check for | between patterns
                if matches!(self.current_token, Some(tokens::Token::Pipe)) {
                    self.advance();
                } else {
                    break;
                }
            }

            // Expect )
            if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
                self.pop_depth();
                return Err(Error::Parse("expected ')' after case pattern".to_string()));
            }
            self.advance();
            self.skip_newlines()?;

            // Parse commands until ;; or esac
            let mut commands = Vec::new();
            while !self.is_case_terminator()
                && !self.is_keyword("esac")
                && self.current_token.is_some()
            {
                if let Some(cmd) = self.parse_command_list()? {
                    commands.push(cmd);
                }
                self.skip_newlines()?;
            }

            cases.push(CaseItem { patterns, commands });

            // Consume ;; if present
            if self.is_case_terminator() {
                self.advance_double_semicolon();
            }
            self.skip_newlines()?;
        }

        // Expect 'esac'
        self.expect_keyword("esac")?;

        self.pop_depth();
        Ok(CompoundCommand::Case(CaseCommand { word, cases }))
    }

    /// Parse a time command: time [-p] [command]
    ///
    /// The time keyword measures execution time of the following command.
    /// Note: BashKit only tracks wall-clock time, not CPU user/sys time.
    fn parse_time(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume 'time'
        self.skip_newlines()?;

        // Check for -p flag (POSIX format)
        let posix_format = if let Some(tokens::Token::Word(w)) = &self.current_token {
            if w == "-p" {
                self.advance();
                self.skip_newlines()?;
                true
            } else {
                false
            }
        } else {
            false
        };

        // Parse the command to time (if any)
        // time with no command is valid in bash (just outputs timing header)
        let command = self.parse_pipeline()?;

        Ok(CompoundCommand::Time(TimeCommand {
            posix_format,
            command: command.map(Box::new),
        }))
    }

    /// Check if current token is ;; (case terminator)
    fn is_case_terminator(&self) -> bool {
        // The lexer returns Semicolon for ; but we need ;;
        // For now, check for two semicolons
        matches!(self.current_token, Some(tokens::Token::Semicolon))
    }

    /// Advance past ;; (double semicolon)
    fn advance_double_semicolon(&mut self) {
        if matches!(self.current_token, Some(tokens::Token::Semicolon)) {
            self.advance();
            if matches!(self.current_token, Some(tokens::Token::Semicolon)) {
                self.advance();
            }
        }
    }

    /// Parse a subshell (commands in parentheses)
    fn parse_subshell(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume '('
        self.skip_newlines()?;

        let mut commands = Vec::new();
        while !matches!(self.current_token, Some(tokens::Token::RightParen) | None) {
            self.skip_newlines()?;
            if matches!(self.current_token, Some(tokens::Token::RightParen)) {
                break;
            }
            if let Some(cmd) = self.parse_command_list()? {
                commands.push(cmd);
            }
        }

        if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
            self.pop_depth();
            return Err(Error::Parse("expected ')' to close subshell".to_string()));
        }
        self.advance(); // consume ')'

        self.pop_depth();
        Ok(CompoundCommand::Subshell(commands))
    }

    /// Parse a brace group
    fn parse_brace_group(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume '{'
        self.skip_newlines()?;

        let mut commands = Vec::new();
        while !matches!(self.current_token, Some(tokens::Token::RightBrace) | None) {
            self.skip_newlines()?;
            if matches!(self.current_token, Some(tokens::Token::RightBrace)) {
                break;
            }
            if let Some(cmd) = self.parse_command_list()? {
                commands.push(cmd);
            }
        }

        if !matches!(self.current_token, Some(tokens::Token::RightBrace)) {
            self.pop_depth();
            return Err(Error::Parse(
                "expected '}' to close brace group".to_string(),
            ));
        }
        self.advance(); // consume '}'

        self.pop_depth();
        Ok(CompoundCommand::BraceGroup(commands))
    }

    /// Parse arithmetic command ((expression))
    fn parse_arithmetic_command(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume '(('

        // Read expression until we find ))
        let mut expr = String::new();
        let mut depth = 1;

        loop {
            match &self.current_token {
                Some(tokens::Token::DoubleLeftParen) => {
                    depth += 1;
                    expr.push_str("((");
                    self.advance();
                }
                Some(tokens::Token::DoubleRightParen) => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance(); // consume '))'
                        break;
                    }
                    expr.push_str("))");
                    self.advance();
                }
                Some(tokens::Token::LeftParen) => {
                    expr.push('(');
                    self.advance();
                }
                Some(tokens::Token::RightParen) => {
                    expr.push(')');
                    self.advance();
                }
                Some(tokens::Token::Word(w))
                | Some(tokens::Token::LiteralWord(w))
                | Some(tokens::Token::QuotedWord(w)) => {
                    if !expr.is_empty() && !expr.ends_with(' ') && !expr.ends_with('(') {
                        expr.push(' ');
                    }
                    expr.push_str(w);
                    self.advance();
                }
                Some(tokens::Token::Semicolon) => {
                    expr.push(';');
                    self.advance();
                }
                Some(tokens::Token::Newline) => {
                    self.advance();
                }
                // Handle operators that are normally special tokens but valid in arithmetic
                Some(tokens::Token::RedirectIn) => {
                    expr.push('<');
                    self.advance();
                }
                Some(tokens::Token::RedirectOut) => {
                    expr.push('>');
                    self.advance();
                }
                Some(tokens::Token::And) => {
                    expr.push_str("&&");
                    self.advance();
                }
                Some(tokens::Token::Or) => {
                    expr.push_str("||");
                    self.advance();
                }
                Some(tokens::Token::Pipe) => {
                    expr.push('|');
                    self.advance();
                }
                Some(tokens::Token::Background) => {
                    expr.push('&');
                    self.advance();
                }
                None => {
                    return Err(Error::Parse(
                        "unexpected end of input in arithmetic command".to_string(),
                    ));
                }
                _ => {
                    self.advance();
                }
            }
        }

        Ok(CompoundCommand::Arithmetic(expr.trim().to_string()))
    }

    /// Parse function definition with 'function' keyword: function name { body }
    fn parse_function_keyword(&mut self) -> Result<Command> {
        self.advance(); // consume 'function'
        self.skip_newlines()?;

        // Get function name
        let name = match &self.current_token {
            Some(tokens::Token::Word(w)) => w.clone(),
            _ => return Err(Error::Parse("expected function name".to_string())),
        };
        self.advance();
        self.skip_newlines()?;

        // Optional () after name
        if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
            self.advance(); // consume '('
            if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
                return Err(Error::Parse(
                    "expected ')' in function definition".to_string(),
                ));
            }
            self.advance(); // consume ')'
            self.skip_newlines()?;
        }

        // Expect { for body
        if !matches!(self.current_token, Some(tokens::Token::LeftBrace)) {
            return Err(Error::Parse("expected '{' for function body".to_string()));
        }

        // Parse body as brace group
        let body = self.parse_brace_group()?;

        Ok(Command::Function(FunctionDef {
            name,
            body: Box::new(Command::Compound(body)),
        }))
    }

    /// Parse POSIX-style function definition: name() { body }
    fn parse_function_posix(&mut self) -> Result<Command> {
        // Get function name
        let name = match &self.current_token {
            Some(tokens::Token::Word(w)) => w.clone(),
            _ => return Err(Error::Parse("expected function name".to_string())),
        };
        self.advance();

        // Consume ()
        if !matches!(self.current_token, Some(tokens::Token::LeftParen)) {
            return Err(Error::Parse(
                "expected '(' in function definition".to_string(),
            ));
        }
        self.advance(); // consume '('

        if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
            return Err(Error::Parse(
                "expected ')' in function definition".to_string(),
            ));
        }
        self.advance(); // consume ')'
        self.skip_newlines()?;

        // Expect { for body
        if !matches!(self.current_token, Some(tokens::Token::LeftBrace)) {
            return Err(Error::Parse("expected '{' for function body".to_string()));
        }

        // Parse body as brace group
        let body = self.parse_brace_group()?;

        Ok(Command::Function(FunctionDef {
            name,
            body: Box::new(Command::Compound(body)),
        }))
    }

    /// Parse commands until a terminating keyword
    fn parse_compound_list(&mut self, terminator: &str) -> Result<Vec<Command>> {
        self.parse_compound_list_until(&[terminator])
    }

    /// Parse commands until one of the terminating keywords
    fn parse_compound_list_until(&mut self, terminators: &[&str]) -> Result<Vec<Command>> {
        let mut commands = Vec::new();

        loop {
            self.skip_newlines()?;

            // Check for terminators
            if let Some(tokens::Token::Word(w)) = &self.current_token {
                if terminators.contains(&w.as_str()) {
                    break;
                }
            }

            if self.current_token.is_none() {
                break;
            }

            if let Some(cmd) = self.parse_command_list()? {
                commands.push(cmd);
            } else {
                break;
            }
        }

        Ok(commands)
    }

    /// Reserved words that cannot start a simple command.
    /// These words are only special in command position, not as arguments.
    const NON_COMMAND_WORDS: &'static [&'static str] =
        &["then", "else", "elif", "fi", "do", "done", "esac", "in"];

    /// Check if a word cannot start a command
    fn is_non_command_word(word: &str) -> bool {
        Self::NON_COMMAND_WORDS.contains(&word)
    }

    /// Check if current token is a specific keyword
    fn is_keyword(&self, keyword: &str) -> bool {
        matches!(&self.current_token, Some(tokens::Token::Word(w)) if w == keyword)
    }

    /// Expect a specific keyword
    fn expect_keyword(&mut self, keyword: &str) -> Result<()> {
        if self.is_keyword(keyword) {
            self.advance();
            Ok(())
        } else {
            Err(Error::Parse(format!("expected '{}'", keyword)))
        }
    }

    /// Strip surrounding quotes from a string value
    fn strip_quotes(s: &str) -> &str {
        if s.len() >= 2
            && ((s.starts_with('"') && s.ends_with('"'))
                || (s.starts_with('\'') && s.ends_with('\'')))
        {
            return &s[1..s.len() - 1];
        }
        s
    }

    /// Check if a word is an assignment (NAME=value, NAME+=value, or NAME[index]=value)
    /// Returns (name, optional_index, value, is_append)
    fn is_assignment(word: &str) -> Option<(&str, Option<&str>, &str, bool)> {
        // Check for += append operator first
        let (eq_pos, is_append) = if let Some(pos) = word.find("+=") {
            (pos, true)
        } else if let Some(pos) = word.find('=') {
            (pos, false)
        } else {
            return None;
        };

        let lhs = &word[..eq_pos];
        let value = &word[eq_pos + if is_append { 2 } else { 1 }..];

        // Check for array subscript: name[index]
        if let Some(bracket_pos) = lhs.find('[') {
            let name = &lhs[..bracket_pos];
            // Validate name
            if name.is_empty() {
                return None;
            }
            let mut chars = name.chars();
            let first = chars.next().unwrap();
            if !first.is_ascii_alphabetic() && first != '_' {
                return None;
            }
            for c in chars {
                if !c.is_ascii_alphanumeric() && c != '_' {
                    return None;
                }
            }
            // Extract index (everything between [ and ])
            if lhs.ends_with(']') {
                let index = &lhs[bracket_pos + 1..lhs.len() - 1];
                return Some((name, Some(index), value, is_append));
            }
        } else {
            // Name must be valid identifier: starts with letter or _, followed by alnum or _
            if lhs.is_empty() {
                return None;
            }
            let mut chars = lhs.chars();
            let first = chars.next().unwrap();
            if !first.is_ascii_alphabetic() && first != '_' {
                return None;
            }
            for c in chars {
                if !c.is_ascii_alphanumeric() && c != '_' {
                    return None;
                }
            }
            return Some((lhs, None, value, is_append));
        }
        None
    }

    /// Parse a simple command with redirections
    fn parse_simple_command(&mut self) -> Result<Option<SimpleCommand>> {
        self.tick()?;
        self.skip_newlines()?;

        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();

        loop {
            match &self.current_token {
                Some(tokens::Token::Word(w))
                | Some(tokens::Token::LiteralWord(w))
                | Some(tokens::Token::QuotedWord(w)) => {
                    let is_literal =
                        matches!(&self.current_token, Some(tokens::Token::LiteralWord(_)));

                    // Stop if this word cannot start a command (like 'then', 'fi', etc.)
                    // This check is only for command position - reserved words in argument
                    // position are handled as regular arguments. The termination of compound
                    // commands is handled by parse_compound_list_until which checks for
                    // terminators BEFORE calling parse_command_list.
                    if words.is_empty() && Self::is_non_command_word(w) {
                        break;
                    }

                    // Check for assignment (only before the command name, not for literal words)
                    if words.is_empty() && !is_literal {
                        let w_clone = w.clone();
                        if let Some((name, index, value, is_append)) = Self::is_assignment(&w_clone)
                        {
                            let name = name.to_string();
                            let index = index.map(|s| s.to_string());
                            let value_str = value.to_string();

                            // Check for array literal: arr=(a b c)
                            let assignment_value = if value_str.starts_with('(')
                                && value_str.ends_with(')')
                            {
                                let inner = &value_str[1..value_str.len() - 1];
                                let elements: Vec<Word> = inner
                                    .split_whitespace()
                                    .map(|s| self.parse_word(s.to_string()))
                                    .collect();
                                AssignmentValue::Array(elements)
                            } else if value_str.is_empty() {
                                // Check if next token is ( for arr=(...) syntax
                                self.advance();
                                if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
                                    self.advance(); // consume '('
                                    let mut elements = Vec::new();
                                    loop {
                                        match &self.current_token {
                                            Some(tokens::Token::RightParen) => {
                                                self.advance();
                                                break;
                                            }
                                            Some(tokens::Token::Word(elem))
                                            | Some(tokens::Token::LiteralWord(elem))
                                            | Some(tokens::Token::QuotedWord(elem)) => {
                                                let elem_clone = elem.clone();
                                                let word = if matches!(
                                                    &self.current_token,
                                                    Some(tokens::Token::LiteralWord(_))
                                                ) {
                                                    Word {
                                                        parts: vec![WordPart::Literal(elem_clone)],
                                                        quoted: true,
                                                    }
                                                } else {
                                                    self.parse_word(elem_clone)
                                                };
                                                elements.push(word);
                                                self.advance();
                                            }
                                            None => break,
                                            _ => {
                                                self.advance();
                                            }
                                        }
                                    }
                                    assignments.push(Assignment {
                                        name,
                                        index,
                                        value: AssignmentValue::Array(elements),
                                        append: is_append,
                                    });
                                    continue;
                                } else {
                                    // Empty assignment: VAR=
                                    assignments.push(Assignment {
                                        name,
                                        index,
                                        value: AssignmentValue::Scalar(Word::literal("")),
                                        append: is_append,
                                    });
                                    continue;
                                }
                            } else {
                                // Handle quoted values: strip quotes and handle appropriately
                                let value_word = if value_str.starts_with('"')
                                    && value_str.ends_with('"')
                                {
                                    // Double-quoted: strip quotes but allow variable expansion
                                    let inner = Self::strip_quotes(&value_str);
                                    self.parse_word(inner.to_string())
                                } else if value_str.starts_with('\'') && value_str.ends_with('\'') {
                                    // Single-quoted: literal, no expansion
                                    let inner = Self::strip_quotes(&value_str);
                                    Word {
                                        parts: vec![WordPart::Literal(inner.to_string())],
                                        quoted: true,
                                    }
                                } else {
                                    self.parse_word(value_str)
                                };
                                AssignmentValue::Scalar(value_word)
                            };
                            assignments.push(Assignment {
                                name,
                                index,
                                value: assignment_value,
                                append: is_append,
                            });
                            self.advance();
                            continue;
                        }
                    }

                    let word = if is_literal {
                        Word {
                            parts: vec![WordPart::Literal(w.clone())],
                            quoted: true,
                        }
                    } else {
                        self.parse_word(w.clone())
                    };
                    words.push(word);
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
                Some(tokens::Token::HereDoc) => {
                    self.advance();
                    // Get the delimiter word and track if it was quoted
                    // Quoted delimiters (single or double quotes) disable variable expansion
                    let (delimiter, quoted) = match &self.current_token {
                        Some(tokens::Token::Word(w)) => (w.clone(), false),
                        Some(tokens::Token::LiteralWord(w)) => (w.clone(), true),
                        Some(tokens::Token::QuotedWord(w)) => (w.clone(), true),
                        _ => return Err(Error::Parse("expected delimiter after <<".to_string())),
                    };
                    // Don't advance - let read_heredoc consume directly from lexer position

                    // Read the here document content (reads until delimiter line)
                    let content = self.lexer.read_heredoc(&delimiter);

                    // Now advance to get the next token after the heredoc
                    self.advance();

                    // If delimiter was quoted, content is literal (no expansion)
                    // Otherwise, parse for variable expansion
                    let target = if quoted {
                        Word::quoted_literal(content)
                    } else {
                        self.parse_word(content)
                    };

                    redirects.push(Redirect {
                        fd: None,
                        kind: RedirectKind::HereDoc,
                        target,
                    });
                }
                Some(tokens::Token::ProcessSubIn) | Some(tokens::Token::ProcessSubOut) => {
                    // Process substitution as argument
                    let word = self.expect_word()?;
                    words.push(word);
                }
                // &> - redirect both stdout and stderr to file
                Some(tokens::Token::RedirectBoth) => {
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        kind: RedirectKind::OutputBoth,
                        target,
                    });
                }
                // >& - duplicate output fd (used for >&2 etc.)
                Some(tokens::Token::DupOutput) => {
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: Some(1), // Default to stdout
                        kind: RedirectKind::DupOutput,
                        target,
                    });
                }
                // N> - redirect with specific file descriptor
                Some(tokens::Token::RedirectFd(fd)) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: Some(fd),
                        kind: RedirectKind::Output,
                        target,
                    });
                }
                // N>> - append with specific file descriptor
                Some(tokens::Token::RedirectFdAppend(fd)) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: Some(fd),
                        kind: RedirectKind::Append,
                        target,
                    });
                }
                // N>&M - duplicate fd N to M
                Some(tokens::Token::DupFd(src_fd, dst_fd)) => {
                    let src_fd = *src_fd;
                    let dst_fd = *dst_fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(src_fd),
                        kind: RedirectKind::DupOutput,
                        target: Word::literal(dst_fd.to_string()),
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

        // Handle assignment-only commands (VAR=value with no command)
        if words.is_empty() && !assignments.is_empty() {
            // Create a "noop" command that just does the assignments
            return Ok(Some(SimpleCommand {
                name: Word::literal(""),
                args: Vec::new(),
                redirects,
                assignments,
            }));
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
            assignments,
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
            Some(tokens::Token::LiteralWord(w)) => {
                // Single-quoted: no variable expansion
                let word = Word {
                    parts: vec![WordPart::Literal(w.clone())],
                    quoted: true,
                };
                self.advance();
                Ok(word)
            }
            Some(tokens::Token::QuotedWord(w)) => {
                // Double-quoted: parse for variable expansion
                let word = self.parse_word(w.clone());
                self.advance();
                Ok(word)
            }
            Some(tokens::Token::ProcessSubIn) | Some(tokens::Token::ProcessSubOut) => {
                // Process substitution <(cmd) or >(cmd)
                let is_input = matches!(self.current_token, Some(tokens::Token::ProcessSubIn));
                self.advance();

                // Parse commands until we hit a closing paren
                let mut cmd_str = String::new();
                let mut depth = 1;
                loop {
                    match &self.current_token {
                        Some(tokens::Token::LeftParen) => {
                            depth += 1;
                            cmd_str.push('(');
                            self.advance();
                        }
                        Some(tokens::Token::RightParen) => {
                            depth -= 1;
                            if depth == 0 {
                                self.advance();
                                break;
                            }
                            cmd_str.push(')');
                            self.advance();
                        }
                        Some(tokens::Token::Word(w)) | Some(tokens::Token::QuotedWord(w)) => {
                            if !cmd_str.is_empty() {
                                cmd_str.push(' ');
                            }
                            cmd_str.push_str(w);
                            self.advance();
                        }
                        Some(tokens::Token::LiteralWord(w)) => {
                            if !cmd_str.is_empty() {
                                cmd_str.push(' ');
                            }
                            cmd_str.push('\'');
                            cmd_str.push_str(w);
                            cmd_str.push('\'');
                            self.advance();
                        }
                        Some(tokens::Token::Pipe) => {
                            cmd_str.push_str(" | ");
                            self.advance();
                        }
                        Some(tokens::Token::Newline) => {
                            self.advance();
                        }
                        None => {
                            return Err(Error::Parse(
                                "unexpected end of input in process substitution".to_string(),
                            ));
                        }
                        _ => {
                            // Skip other tokens for now
                            self.advance();
                        }
                    }
                }

                // Parse the command inside
                let inner_parser = Parser::new(&cmd_str);
                let commands = match inner_parser.parse() {
                    Ok(script) => script.commands,
                    Err(_) => Vec::new(),
                };

                Ok(Word {
                    parts: vec![WordPart::ProcessSubstitution { commands, is_input }],
                    quoted: false,
                })
            }
            _ => Err(Error::Parse("expected word".to_string())),
        }
    }

    // Helper methods for word handling - kept for potential future use
    #[allow(dead_code)]
    /// Convert current word token to Word (handles Word, LiteralWord, QuotedWord)
    fn current_word_to_word(&self) -> Option<Word> {
        match &self.current_token {
            Some(tokens::Token::Word(w)) | Some(tokens::Token::QuotedWord(w)) => {
                Some(self.parse_word(w.clone()))
            }
            Some(tokens::Token::LiteralWord(w)) => Some(Word {
                parts: vec![WordPart::Literal(w.clone())],
                quoted: true,
            }),
            _ => None,
        }
    }

    #[allow(dead_code)]
    /// Check if current token is a word (Word, LiteralWord, or QuotedWord)
    fn is_current_word(&self) -> bool {
        matches!(
            &self.current_token,
            Some(tokens::Token::Word(_))
                | Some(tokens::Token::LiteralWord(_))
                | Some(tokens::Token::QuotedWord(_))
        )
    }

    #[allow(dead_code)]
    /// Get the string content if current token is a word
    fn current_word_str(&self) -> Option<String> {
        match &self.current_token {
            Some(tokens::Token::Word(w))
            | Some(tokens::Token::LiteralWord(w))
            | Some(tokens::Token::QuotedWord(w)) => Some(w.clone()),
            _ => None,
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

                // Check for $( - command substitution or arithmetic
                if chars.peek() == Some(&'(') {
                    chars.next(); // consume first '('

                    // Check for $(( - arithmetic expansion
                    if chars.peek() == Some(&'(') {
                        chars.next(); // consume second '('
                        let mut expr = String::new();
                        let mut depth = 2;
                        for c in chars.by_ref() {
                            if c == '(' {
                                depth += 1;
                                expr.push(c);
                            } else if c == ')' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                expr.push(c);
                            } else {
                                expr.push(c);
                            }
                        }
                        // Remove trailing ) if present
                        if expr.ends_with(')') {
                            expr.pop();
                        }
                        parts.push(WordPart::ArithmeticExpansion(expr));
                    } else {
                        // Command substitution $(...)
                        let mut cmd_str = String::new();
                        let mut depth = 1;
                        for c in chars.by_ref() {
                            if c == '(' {
                                depth += 1;
                                cmd_str.push(c);
                            } else if c == ')' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                cmd_str.push(c);
                            } else {
                                cmd_str.push(c);
                            }
                        }
                        // Parse the command inside
                        let inner_parser = Parser::new(&cmd_str);
                        if let Ok(script) = inner_parser.parse() {
                            parts.push(WordPart::CommandSubstitution(script.commands));
                        }
                    }
                } else if chars.peek() == Some(&'{') {
                    // ${VAR} format with possible parameter expansion
                    chars.next(); // consume '{'

                    // Check for ${#var} or ${#arr[@]} - length expansion
                    if chars.peek() == Some(&'#') {
                        chars.next(); // consume '#'
                        let mut var_name = String::new();
                        while let Some(&c) = chars.peek() {
                            if c == '}' || c == '[' {
                                break;
                            }
                            var_name.push(chars.next().unwrap());
                        }
                        // Check for array length ${#arr[@]} or ${#arr[*]}
                        if chars.peek() == Some(&'[') {
                            chars.next(); // consume '['
                            let mut index = String::new();
                            while let Some(&c) = chars.peek() {
                                if c == ']' {
                                    chars.next();
                                    break;
                                }
                                index.push(chars.next().unwrap());
                            }
                            // Consume closing }
                            if chars.peek() == Some(&'}') {
                                chars.next();
                            }
                            if index == "@" || index == "*" {
                                parts.push(WordPart::ArrayLength(var_name));
                            } else {
                                // ${#arr[n]} - length of element (same as ${#arr[n]})
                                parts.push(WordPart::Length(format!("{}[{}]", var_name, index)));
                            }
                        } else {
                            // Consume closing }
                            if chars.peek() == Some(&'}') {
                                chars.next();
                            }
                            parts.push(WordPart::Length(var_name));
                        }
                    } else if chars.peek() == Some(&'!') {
                        // Check for ${!arr[@]} or ${!arr[*]} - array indices
                        chars.next(); // consume '!'
                        let mut var_name = String::new();
                        while let Some(&c) = chars.peek() {
                            if c == '}' || c == '[' {
                                break;
                            }
                            var_name.push(chars.next().unwrap());
                        }
                        // Check for array indices ${!arr[@]} or ${!arr[*]}
                        if chars.peek() == Some(&'[') {
                            chars.next(); // consume '['
                            let mut index = String::new();
                            while let Some(&c) = chars.peek() {
                                if c == ']' {
                                    chars.next();
                                    break;
                                }
                                index.push(chars.next().unwrap());
                            }
                            // Consume closing }
                            if chars.peek() == Some(&'}') {
                                chars.next();
                            }
                            if index == "@" || index == "*" {
                                parts.push(WordPart::ArrayIndices(var_name));
                            } else {
                                // ${!arr[n]} - not standard, treat as variable
                                parts.push(WordPart::Variable(format!("!{}[{}]", var_name, index)));
                            }
                        } else {
                            // ${!prefix*} or ${!prefix@} - indirect expansion (not fully supported)
                            // For now, consume until } and treat as variable
                            while let Some(&c) = chars.peek() {
                                if c == '}' {
                                    chars.next();
                                    break;
                                }
                                var_name.push(chars.next().unwrap());
                            }
                            parts.push(WordPart::Variable(format!("!{}", var_name)));
                        }
                    } else {
                        // Read variable name
                        let mut var_name = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_alphanumeric() || c == '_' {
                                var_name.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }

                        // Check for array access ${arr[index]}
                        if chars.peek() == Some(&'[') {
                            chars.next(); // consume '['
                            let mut index = String::new();
                            while let Some(&c) = chars.peek() {
                                if c == ']' {
                                    chars.next();
                                    break;
                                }
                                index.push(chars.next().unwrap());
                            }
                            // Consume closing }
                            if chars.peek() == Some(&'}') {
                                chars.next();
                            }
                            parts.push(WordPart::ArrayAccess {
                                name: var_name,
                                index,
                            });
                        } else if let Some(&c) = chars.peek() {
                            // Check for operator
                            let (operator, operand) = match c {
                                ':' => {
                                    chars.next(); // consume ':'
                                    match chars.peek() {
                                        Some(&'-') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            (Some(ParameterOp::UseDefault), op)
                                        }
                                        Some(&'=') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            (Some(ParameterOp::AssignDefault), op)
                                        }
                                        Some(&'+') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            (Some(ParameterOp::UseReplacement), op)
                                        }
                                        Some(&'?') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            (Some(ParameterOp::Error), op)
                                        }
                                        _ => (None, String::new()),
                                    }
                                }
                                '#' => {
                                    chars.next();
                                    if chars.peek() == Some(&'#') {
                                        chars.next();
                                        let op = self.read_brace_operand(&mut chars);
                                        (Some(ParameterOp::RemovePrefixLong), op)
                                    } else {
                                        let op = self.read_brace_operand(&mut chars);
                                        (Some(ParameterOp::RemovePrefixShort), op)
                                    }
                                }
                                '%' => {
                                    chars.next();
                                    if chars.peek() == Some(&'%') {
                                        chars.next();
                                        let op = self.read_brace_operand(&mut chars);
                                        (Some(ParameterOp::RemoveSuffixLong), op)
                                    } else {
                                        let op = self.read_brace_operand(&mut chars);
                                        (Some(ParameterOp::RemoveSuffixShort), op)
                                    }
                                }
                                '}' => {
                                    chars.next();
                                    (None, String::new())
                                }
                                _ => {
                                    // Unknown, consume until }
                                    while let Some(&ch) = chars.peek() {
                                        if ch == '}' {
                                            chars.next();
                                            break;
                                        }
                                        chars.next();
                                    }
                                    (None, String::new())
                                }
                            };

                            if let Some(op) = operator {
                                parts.push(WordPart::ParameterExpansion {
                                    name: var_name,
                                    operator: op,
                                    operand,
                                });
                            } else if !var_name.is_empty() {
                                parts.push(WordPart::Variable(var_name));
                            }
                        } else if !var_name.is_empty() {
                            parts.push(WordPart::Variable(var_name));
                        }
                    }
                } else if let Some(&c) = chars.peek() {
                    // Check for special single-character variables ($?, $#, $@, $*, $!, $$, $-, $0-$9)
                    if matches!(c, '?' | '#' | '@' | '*' | '!' | '$' | '-') || c.is_ascii_digit() {
                        parts.push(WordPart::Variable(chars.next().unwrap().to_string()));
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
                    // Just a literal $ at end
                    current.push('$');
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

        Word {
            parts,
            quoted: false,
        }
    }

    /// Read operand for brace expansion (everything until closing brace)
    fn read_brace_operand(&self, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
        let mut operand = String::new();
        let mut depth = 1; // Track nested braces
        while let Some(&c) = chars.peek() {
            if c == '{' {
                depth += 1;
                operand.push(chars.next().unwrap());
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    chars.next(); // consume closing }
                    break;
                }
                operand.push(chars.next().unwrap());
            } else {
                operand.push(chars.next().unwrap());
            }
        }
        operand
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
