//! Parser module for Bashkit
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
mod span;
mod tokens;

pub use ast::*;
pub use lexer::{Lexer, SpannedToken};
pub use span::{Position, Span};

use crate::error::{Error, Result};

/// Default maximum AST depth (matches ExecutionLimits default)
const DEFAULT_MAX_AST_DEPTH: usize = 100;

/// Hard cap on AST depth to prevent stack overflow even if caller misconfigures limits.
/// THREAT[TM-DOS-022]: Protects against deeply nested input attacks where
/// a large max_depth setting allows recursion deep enough to overflow the native stack.
/// This cap cannot be overridden by the caller.
///
/// Set conservatively to avoid stack overflow on tokio's blocking threads (default 2MB
/// stack in debug builds). Each parser recursion level uses ~4-8KB of stack in debug
/// mode. 100 levels × ~8KB = ~800KB, well within 2MB.
/// In release builds this could safely be higher, but we use one value for consistency.
const HARD_MAX_AST_DEPTH: usize = 100;

/// Default maximum parser operations (matches ExecutionLimits default)
const DEFAULT_MAX_PARSER_OPERATIONS: usize = 100_000;

/// Parser for bash scripts.
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<tokens::Token>,
    /// Span of the current token
    current_span: Span,
    /// Lookahead token for function parsing
    peeked_token: Option<SpannedToken>,
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
    ///
    /// THREAT[TM-DOS-022]: `max_depth` is clamped to `HARD_MAX_AST_DEPTH` (500)
    /// to prevent stack overflow from misconfiguration. Even if the caller passes
    /// `max_depth = 1_000_000`, the parser will cap it at 500.
    pub fn with_limits(input: &'a str, max_depth: usize, max_fuel: usize) -> Self {
        let mut lexer = Lexer::new(input);
        let spanned = lexer.next_spanned_token();
        let (current_token, current_span) = match spanned {
            Some(st) => (Some(st.token), st.span),
            None => (None, Span::new()),
        };
        Self {
            lexer,
            current_token,
            current_span,
            peeked_token: None,
            max_depth: max_depth.min(HARD_MAX_AST_DEPTH),
            current_depth: 0,
            fuel: max_fuel,
            max_fuel,
        }
    }

    /// Get the current token's span.
    pub fn current_span(&self) -> Span {
        self.current_span
    }

    /// Create a parse error with the current position.
    fn error(&self, message: impl Into<String>) -> Error {
        Error::parse_at(
            message,
            self.current_span.start.line,
            self.current_span.start.column,
        )
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
        let start_span = self.current_span;
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

        let end_span = self.current_span;
        Ok(Script {
            commands,
            span: start_span.merge(end_span),
        })
    }

    fn advance(&mut self) {
        if let Some(peeked) = self.peeked_token.take() {
            self.current_token = Some(peeked.token);
            self.current_span = peeked.span;
        } else {
            match self.lexer.next_spanned_token() {
                Some(st) => {
                    self.current_token = Some(st.token);
                    self.current_span = st.span;
                }
                None => {
                    self.current_token = None;
                    // Keep the last span for error reporting
                }
            }
        }
    }

    /// Peek at the next token without consuming the current one
    fn peek_next(&mut self) -> Option<&tokens::Token> {
        if self.peeked_token.is_none() {
            self.peeked_token = self.lexer.next_spanned_token();
        }
        self.peeked_token.as_ref().map(|st| &st.token)
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
        let start_span = self.current_span;
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
                                span: self.current_span,
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
                span: start_span.merge(self.current_span),
            })))
        }
    }

    /// Parse a pipeline (commands connected by |)
    fn parse_pipeline(&mut self) -> Result<Option<Command>> {
        let start_span = self.current_span;
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
                return Err(self.error("expected command after |"));
            }
        }

        if commands.len() == 1 {
            Ok(Some(commands.remove(0)))
        } else {
            Ok(Some(Command::Pipeline(Pipeline {
                negated: false,
                commands,
                span: start_span.merge(self.current_span),
            })))
        }
    }

    /// Parse redirections that follow a compound command (>, >>, 2>, etc.)
    fn parse_trailing_redirects(&mut self) -> Vec<Redirect> {
        let mut redirects = Vec::new();
        loop {
            match &self.current_token {
                Some(tokens::Token::RedirectOut) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            kind: RedirectKind::Output,
                            target,
                        });
                    }
                }
                Some(tokens::Token::RedirectAppend) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            kind: RedirectKind::Append,
                            target,
                        });
                    }
                }
                Some(tokens::Token::RedirectIn) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            kind: RedirectKind::Input,
                            target,
                        });
                    }
                }
                Some(tokens::Token::RedirectBoth) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            kind: RedirectKind::OutputBoth,
                            target,
                        });
                    }
                }
                Some(tokens::Token::DupOutput) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(1),
                            kind: RedirectKind::DupOutput,
                            target,
                        });
                    }
                }
                Some(tokens::Token::RedirectFd(fd)) => {
                    let fd = *fd;
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(fd),
                            kind: RedirectKind::Output,
                            target,
                        });
                    }
                }
                Some(tokens::Token::RedirectFdAppend(fd)) => {
                    let fd = *fd;
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(fd),
                            kind: RedirectKind::Append,
                            target,
                        });
                    }
                }
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
                _ => break,
            }
        }
        redirects
    }

    /// Parse a compound command and any trailing redirections
    fn parse_compound_with_redirects(
        &mut self,
        parser: impl FnOnce(&mut Self) -> Result<CompoundCommand>,
    ) -> Result<Option<Command>> {
        let compound = parser(self)?;
        let redirects = self.parse_trailing_redirects();
        Ok(Some(Command::Compound(compound, redirects)))
    }

    /// Parse a single command (simple or compound)
    fn parse_command(&mut self) -> Result<Option<Command>> {
        self.skip_newlines()?;

        // Check for compound commands and function keyword
        if let Some(tokens::Token::Word(w)) = &self.current_token {
            let word = w.clone();
            match word.as_str() {
                "if" => return self.parse_compound_with_redirects(|s| s.parse_if()),
                "for" => return self.parse_compound_with_redirects(|s| s.parse_for()),
                "while" => return self.parse_compound_with_redirects(|s| s.parse_while()),
                "until" => return self.parse_compound_with_redirects(|s| s.parse_until()),
                "case" => return self.parse_compound_with_redirects(|s| s.parse_case()),
                "time" => return self.parse_compound_with_redirects(|s| s.parse_time()),
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

        // Check for conditional expression [[ ... ]]
        if matches!(self.current_token, Some(tokens::Token::DoubleLeftBracket)) {
            return self.parse_compound_with_redirects(|s| s.parse_conditional());
        }

        // Check for arithmetic command ((expression))
        if matches!(self.current_token, Some(tokens::Token::DoubleLeftParen)) {
            return self.parse_compound_with_redirects(|s| s.parse_arithmetic_command());
        }

        // Check for subshell
        if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
            return self.parse_compound_with_redirects(|s| s.parse_subshell());
        }

        // Check for brace group
        if matches!(self.current_token, Some(tokens::Token::LeftBrace)) {
            return self.parse_compound_with_redirects(|s| s.parse_brace_group());
        }

        // Default to simple command
        match self.parse_simple_command()? {
            Some(cmd) => Ok(Some(Command::Simple(cmd))),
            None => Ok(None),
        }
    }

    /// Parse an if statement
    fn parse_if(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
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
            span: start_span.merge(self.current_span),
        }))
    }

    /// Parse a for loop
    fn parse_for(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
        self.push_depth()?;
        self.advance(); // consume 'for'
        self.skip_newlines()?;

        // Check for C-style for loop: for ((init; cond; step))
        if matches!(self.current_token, Some(tokens::Token::DoubleLeftParen)) {
            let result = self.parse_arithmetic_for_inner(start_span);
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
                        let is_quoted =
                            matches!(&self.current_token, Some(tokens::Token::QuotedWord(_)));
                        let mut word = self.parse_word(w.clone());
                        if is_quoted {
                            word.quoted = true;
                        }
                        words.push(word);
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
            // for var; do ... (iterates over positional params)
            // Consume optional semicolon before 'do'
            if matches!(self.current_token, Some(tokens::Token::Semicolon)) {
                self.advance();
            }
            None
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
            span: start_span.merge(self.current_span),
        }))
    }

    /// Parse C-style arithmetic for loop inner: for ((init; cond; step)); do body; done
    /// Note: depth tracking is done by parse_for which calls this
    fn parse_arithmetic_for_inner(&mut self, start_span: Span) -> Result<CompoundCommand> {
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
            span: start_span.merge(self.current_span),
        }))
    }

    /// Parse a while loop
    fn parse_while(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
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
        Ok(CompoundCommand::While(WhileCommand {
            condition,
            body,
            span: start_span.merge(self.current_span),
        }))
    }

    /// Parse an until loop
    fn parse_until(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
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
        Ok(CompoundCommand::Until(UntilCommand {
            condition,
            body,
            span: start_span.merge(self.current_span),
        }))
    }

    /// Parse a case statement: case WORD in pattern) commands ;; ... esac
    fn parse_case(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
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
                return Err(self.error("expected ')' after case pattern"));
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
        Ok(CompoundCommand::Case(CaseCommand {
            word,
            cases,
            span: start_span.merge(self.current_span),
        }))
    }

    /// Parse a time command: time [-p] [command]
    ///
    /// The time keyword measures execution time of the following command.
    /// Note: Bashkit only tracks wall-clock time, not CPU user/sys time.
    fn parse_time(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
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
            span: start_span.merge(self.current_span),
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
    /// Parse [[ conditional expression ]]
    fn parse_conditional(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume '[['

        let mut words = Vec::new();
        let mut saw_regex_op = false;

        loop {
            match &self.current_token {
                Some(tokens::Token::DoubleRightBracket) => {
                    self.advance(); // consume ']]'
                    break;
                }
                Some(tokens::Token::Word(w))
                | Some(tokens::Token::LiteralWord(w))
                | Some(tokens::Token::QuotedWord(w)) => {
                    let w_clone = w.clone();
                    let is_quoted =
                        matches!(self.current_token, Some(tokens::Token::QuotedWord(_)));
                    let is_literal =
                        matches!(self.current_token, Some(tokens::Token::LiteralWord(_)));

                    // After =~, collect the regex pattern (may contain parens)
                    if saw_regex_op {
                        let pattern = self.collect_conditional_regex_pattern(&w_clone);
                        words.push(Word::literal(&pattern));
                        saw_regex_op = false;
                        continue;
                    }

                    if w_clone == "=~" {
                        saw_regex_op = true;
                    }

                    let word = if is_literal {
                        Word {
                            parts: vec![WordPart::Literal(w_clone)],
                            quoted: true,
                        }
                    } else {
                        let mut parsed = self.parse_word(w_clone);
                        if is_quoted {
                            parsed.quoted = true;
                        }
                        parsed
                    };
                    words.push(word);
                    self.advance();
                }
                // Operators that the lexer tokenizes separately
                Some(tokens::Token::And) => {
                    words.push(Word::literal("&&"));
                    self.advance();
                }
                Some(tokens::Token::Or) => {
                    words.push(Word::literal("||"));
                    self.advance();
                }
                Some(tokens::Token::LeftParen) => {
                    words.push(Word::literal("("));
                    self.advance();
                }
                Some(tokens::Token::RightParen) => {
                    words.push(Word::literal(")"));
                    self.advance();
                }
                None => {
                    return Err(crate::error::Error::Parse(
                        "unexpected end of input in [[ ]]".to_string(),
                    ));
                }
                _ => {
                    // Skip unknown tokens
                    self.advance();
                }
            }
        }

        Ok(CompoundCommand::Conditional(words))
    }

    /// Collect a regex pattern after =~ in [[ ]], handling parens and special chars.
    fn collect_conditional_regex_pattern(&mut self, first_word: &str) -> String {
        let mut pattern = first_word.to_string();
        self.advance(); // consume the first word

        // Concatenate adjacent tokens that are part of the regex pattern
        loop {
            match &self.current_token {
                Some(tokens::Token::DoubleRightBracket) => break,
                Some(tokens::Token::And) | Some(tokens::Token::Or) => break,
                Some(tokens::Token::LeftParen) => {
                    pattern.push('(');
                    self.advance();
                }
                Some(tokens::Token::RightParen) => {
                    pattern.push(')');
                    self.advance();
                }
                Some(tokens::Token::Word(w))
                | Some(tokens::Token::LiteralWord(w))
                | Some(tokens::Token::QuotedWord(w)) => {
                    pattern.push_str(w);
                    self.advance();
                }
                _ => break,
            }
        }

        pattern
    }

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
        let start_span = self.current_span;
        self.advance(); // consume 'function'
        self.skip_newlines()?;

        // Get function name
        let name = match &self.current_token {
            Some(tokens::Token::Word(w)) => w.clone(),
            _ => return Err(self.error("expected function name")),
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
            body: Box::new(Command::Compound(body, Vec::new())),
            span: start_span.merge(self.current_span),
        }))
    }

    /// Parse POSIX-style function definition: name() { body }
    fn parse_function_posix(&mut self) -> Result<Command> {
        let start_span = self.current_span;
        // Get function name
        let name = match &self.current_token {
            Some(tokens::Token::Word(w)) => w.clone(),
            _ => return Err(self.error("expected function name")),
        };
        self.advance();

        // Consume ()
        if !matches!(self.current_token, Some(tokens::Token::LeftParen)) {
            return Err(self.error("expected '(' in function definition"));
        }
        self.advance(); // consume '('

        if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
            return Err(self.error("expected ')' in function definition"));
        }
        self.advance(); // consume ')'
        self.skip_newlines()?;

        // Expect { for body
        if !matches!(self.current_token, Some(tokens::Token::LeftBrace)) {
            return Err(self.error("expected '{' for function body"));
        }

        // Parse body as brace group
        let body = self.parse_brace_group()?;

        Ok(Command::Function(FunctionDef {
            name,
            body: Box::new(Command::Compound(body, Vec::new())),
            span: start_span.merge(self.current_span),
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
            Err(self.error(format!("expected '{}'", keyword)))
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
        let start_span = self.current_span;

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
                    let is_quoted =
                        matches!(&self.current_token, Some(tokens::Token::QuotedWord(_)));

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
                        let mut word = self.parse_word(w.clone());
                        if is_quoted {
                            word.quoted = true;
                        }
                        word
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

                    // Heredoc body consumed subsequent lines from input.
                    // Stop parsing this command - next tokens belong to new commands.
                    break;
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
                span: start_span.merge(self.current_span),
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
            span: start_span.merge(self.current_span),
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

                // THREAT[TM-DOS-021]: Propagate parent parser limits to child parser
                // to prevent depth limit bypass via nested process substitution.
                // Child inherits remaining depth budget and fuel from parent.
                let remaining_depth = self.max_depth.saturating_sub(self.current_depth);
                let inner_parser = Parser::with_limits(&cmd_str, remaining_depth, self.fuel);
                let commands = match inner_parser.parse() {
                    Ok(script) => script.commands,
                    Err(_) => Vec::new(),
                };

                Ok(Word {
                    parts: vec![WordPart::ProcessSubstitution { commands, is_input }],
                    quoted: false,
                })
            }
            _ => Err(self.error("expected word")),
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
                        // THREAT[TM-DOS-021]: Propagate parent parser limits to child parser
                        // to prevent depth limit bypass via nested command substitution.
                        let remaining_depth = self.max_depth.saturating_sub(self.current_depth);
                        let inner_parser =
                            Parser::with_limits(&cmd_str, remaining_depth, self.fuel);
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
                        // or ${!var} - indirect expansion
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
                        } else if chars.peek() == Some(&'}') {
                            // ${!var} - indirect expansion
                            chars.next(); // consume '}'
                            parts.push(WordPart::IndirectExpansion(var_name));
                        } else {
                            // ${!prefix*} or ${!prefix@} - prefix matching (not fully supported)
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

                        // Check for array access ${arr[index]} or ${arr[@]:offset:length}
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
                            // Check for slice ${arr[@]:offset:length}
                            if chars.peek() == Some(&':') && (index == "@" || index == "*") {
                                chars.next(); // consume ':'
                                              // Read offset (may include negative sign)
                                let mut offset = String::new();
                                while let Some(&c) = chars.peek() {
                                    if c == ':' || c == '}' {
                                        break;
                                    }
                                    offset.push(chars.next().unwrap());
                                }
                                // Check for length
                                let length = if chars.peek() == Some(&':') {
                                    chars.next(); // consume ':'
                                    let mut len = String::new();
                                    while let Some(&c) = chars.peek() {
                                        if c == '}' {
                                            break;
                                        }
                                        len.push(chars.next().unwrap());
                                    }
                                    Some(len)
                                } else {
                                    None
                                };
                                // Consume closing }
                                if chars.peek() == Some(&'}') {
                                    chars.next();
                                }
                                parts.push(WordPart::ArraySlice {
                                    name: var_name,
                                    offset,
                                    length,
                                });
                            } else {
                                // Consume closing }
                                if chars.peek() == Some(&'}') {
                                    chars.next();
                                }
                                parts.push(WordPart::ArrayAccess {
                                    name: var_name,
                                    index,
                                });
                            }
                        } else if let Some(&c) = chars.peek() {
                            // Check for operator
                            match c {
                                ':' => {
                                    chars.next(); // consume ':'
                                    match chars.peek() {
                                        Some(&'-') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            parts.push(WordPart::ParameterExpansion {
                                                name: var_name,
                                                operator: ParameterOp::UseDefault,
                                                operand: op,
                                            });
                                        }
                                        Some(&'=') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            parts.push(WordPart::ParameterExpansion {
                                                name: var_name,
                                                operator: ParameterOp::AssignDefault,
                                                operand: op,
                                            });
                                        }
                                        Some(&'+') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            parts.push(WordPart::ParameterExpansion {
                                                name: var_name,
                                                operator: ParameterOp::UseReplacement,
                                                operand: op,
                                            });
                                        }
                                        Some(&'?') => {
                                            chars.next();
                                            let op = self.read_brace_operand(&mut chars);
                                            parts.push(WordPart::ParameterExpansion {
                                                name: var_name,
                                                operator: ParameterOp::Error,
                                                operand: op,
                                            });
                                        }
                                        _ => {
                                            // Substring extraction ${var:offset} or ${var:offset:length}
                                            let mut offset = String::new();
                                            while let Some(&ch) = chars.peek() {
                                                if ch == ':' || ch == '}' {
                                                    break;
                                                }
                                                offset.push(chars.next().unwrap());
                                            }
                                            let length = if chars.peek() == Some(&':') {
                                                chars.next(); // consume ':'
                                                let mut len = String::new();
                                                while let Some(&ch) = chars.peek() {
                                                    if ch == '}' {
                                                        break;
                                                    }
                                                    len.push(chars.next().unwrap());
                                                }
                                                Some(len)
                                            } else {
                                                None
                                            };
                                            // Consume closing }
                                            if chars.peek() == Some(&'}') {
                                                chars.next();
                                            }
                                            parts.push(WordPart::Substring {
                                                name: var_name,
                                                offset,
                                                length,
                                            });
                                        }
                                    }
                                }
                                '#' => {
                                    chars.next();
                                    if chars.peek() == Some(&'#') {
                                        chars.next();
                                        let op = self.read_brace_operand(&mut chars);
                                        parts.push(WordPart::ParameterExpansion {
                                            name: var_name,
                                            operator: ParameterOp::RemovePrefixLong,
                                            operand: op,
                                        });
                                    } else {
                                        let op = self.read_brace_operand(&mut chars);
                                        parts.push(WordPart::ParameterExpansion {
                                            name: var_name,
                                            operator: ParameterOp::RemovePrefixShort,
                                            operand: op,
                                        });
                                    }
                                }
                                '%' => {
                                    chars.next();
                                    if chars.peek() == Some(&'%') {
                                        chars.next();
                                        let op = self.read_brace_operand(&mut chars);
                                        parts.push(WordPart::ParameterExpansion {
                                            name: var_name,
                                            operator: ParameterOp::RemoveSuffixLong,
                                            operand: op,
                                        });
                                    } else {
                                        let op = self.read_brace_operand(&mut chars);
                                        parts.push(WordPart::ParameterExpansion {
                                            name: var_name,
                                            operator: ParameterOp::RemoveSuffixShort,
                                            operand: op,
                                        });
                                    }
                                }
                                '/' => {
                                    // Pattern replacement ${var/pattern/replacement} or ${var//pattern/replacement}
                                    chars.next(); // consume '/'
                                    let replace_all = if chars.peek() == Some(&'/') {
                                        chars.next(); // consume second '/'
                                        true
                                    } else {
                                        false
                                    };
                                    // Read pattern until /
                                    let mut pattern = String::new();
                                    while let Some(&ch) = chars.peek() {
                                        if ch == '/' || ch == '}' {
                                            break;
                                        }
                                        pattern.push(chars.next().unwrap());
                                    }
                                    // Read replacement if present
                                    let replacement = if chars.peek() == Some(&'/') {
                                        chars.next(); // consume '/'
                                        let mut repl = String::new();
                                        while let Some(&ch) = chars.peek() {
                                            if ch == '}' {
                                                break;
                                            }
                                            repl.push(chars.next().unwrap());
                                        }
                                        repl
                                    } else {
                                        String::new()
                                    };
                                    // Consume closing }
                                    if chars.peek() == Some(&'}') {
                                        chars.next();
                                    }
                                    let op = if replace_all {
                                        ParameterOp::ReplaceAll {
                                            pattern,
                                            replacement,
                                        }
                                    } else {
                                        ParameterOp::ReplaceFirst {
                                            pattern,
                                            replacement,
                                        }
                                    };
                                    parts.push(WordPart::ParameterExpansion {
                                        name: var_name,
                                        operator: op,
                                        operand: String::new(),
                                    });
                                }
                                '^' => {
                                    // Case conversion uppercase ${var^} or ${var^^}
                                    chars.next(); // consume '^'
                                    let op = if chars.peek() == Some(&'^') {
                                        chars.next();
                                        ParameterOp::UpperAll
                                    } else {
                                        ParameterOp::UpperFirst
                                    };
                                    // Consume closing }
                                    if chars.peek() == Some(&'}') {
                                        chars.next();
                                    }
                                    parts.push(WordPart::ParameterExpansion {
                                        name: var_name,
                                        operator: op,
                                        operand: String::new(),
                                    });
                                }
                                ',' => {
                                    // Case conversion lowercase ${var,} or ${var,,}
                                    chars.next(); // consume ','
                                    let op = if chars.peek() == Some(&',') {
                                        chars.next();
                                        ParameterOp::LowerAll
                                    } else {
                                        ParameterOp::LowerFirst
                                    };
                                    // Consume closing }
                                    if chars.peek() == Some(&'}') {
                                        chars.next();
                                    }
                                    parts.push(WordPart::ParameterExpansion {
                                        name: var_name,
                                        operator: op,
                                        operand: String::new(),
                                    });
                                }
                                '}' => {
                                    chars.next();
                                    if !var_name.is_empty() {
                                        parts.push(WordPart::Variable(var_name));
                                    }
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
                                    if !var_name.is_empty() {
                                        parts.push(WordPart::Variable(var_name));
                                    }
                                }
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
