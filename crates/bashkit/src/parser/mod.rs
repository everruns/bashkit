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
pub mod budget;
mod lexer;
mod span;
mod tokens;

pub use ast::*;
pub use budget::{BudgetError, validate as validate_budget};
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
    input: &'a str,
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
        let mut lexer = Lexer::with_max_subst_depth(input, max_depth.min(HARD_MAX_AST_DEPTH));
        let spanned = lexer.next_spanned_token();
        let (current_token, current_span) = match spanned {
            Some(st) => (Some(st.token), st.span),
            None => (None, Span::new()),
        };
        Self {
            input,
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

    /// Parse a string as a word (handling $var, $((expr)), ${...}, etc.).
    /// Used by the interpreter to expand operands in parameter expansions lazily.
    pub fn parse_word_string(input: &str) -> Word {
        let parser = Parser::new(input);
        parser.parse_word(input.to_string())
    }

    /// THREAT[TM-DOS-050]: Parse a word string with caller-configured limits.
    /// Prevents bypass of parser limits in parameter expansion contexts.
    pub fn parse_word_string_with_limits(input: &str, max_depth: usize, max_fuel: usize) -> Word {
        let parser = Parser::with_limits(input, max_depth, max_fuel);
        parser.parse_word(input.to_string())
    }

    /// Create a parse error with the current position.
    fn error(&self, message: impl Into<String>) -> Error {
        Error::parse_at(
            message,
            self.current_span.start.line,
            self.current_span.start.column,
        )
    }

    fn current_command_end_offset(&self) -> usize {
        if self.current_token.is_some() {
            self.current_span.start.offset
        } else {
            self.input.len()
        }
    }

    fn source_slice(&self, start_offset: usize, end_offset: usize) -> Option<String> {
        self.input.get(start_offset..end_offset).map(str::to_owned)
    }

    /// Consume one unit of fuel, returning an error if exhausted
    fn tick(&mut self) -> Result<()> {
        if self.fuel == 0 {
            let used = self.max_fuel;
            return Err(Error::parse(format!(
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
            return Err(Error::parse(format!(
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

    /// Check if current token is an error token and return the error if so
    fn check_error_token(&self) -> Result<()> {
        if let Some(tokens::Token::Error(msg)) = &self.current_token {
            return Err(self.error(format!("syntax error: {}", msg)));
        }
        Ok(())
    }

    /// Parse the input and return the AST.
    pub fn parse(mut self) -> Result<Script> {
        // Check if the very first token is an error
        self.check_error_token()?;

        let start_span = self.current_span;
        let mut commands = Vec::new();

        while self.current_token.is_some() {
            self.tick()?;
            self.skip_newlines()?;
            self.check_error_token()?;
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
    ///
    /// Handles `!` pipeline negation: `! cmd | cmd2` negates the exit code.
    fn parse_pipeline(&mut self) -> Result<Option<Command>> {
        let start_span = self.current_span;

        // Check for pipeline negation: `! command`
        let negated = match &self.current_token {
            Some(tokens::Token::Word(w)) if w == "!" => {
                self.advance();
                true
            }
            _ => false,
        };

        let first = match self.parse_command()? {
            Some(cmd) => cmd,
            None => {
                if negated {
                    return Err(self.error("expected command after !"));
                }
                return Ok(None);
            }
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

        if commands.len() == 1 && !negated {
            Ok(Some(commands.remove(0)))
        } else {
            Ok(Some(Command::Pipeline(Pipeline {
                negated,
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
                Some(tokens::Token::RedirectOut) | Some(tokens::Token::Clobber) => {
                    let kind = if matches!(&self.current_token, Some(tokens::Token::Clobber)) {
                        RedirectKind::Clobber
                    } else {
                        RedirectKind::Output
                    };
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            fd_var: None,
                            kind,
                            target,
                        });
                    }
                }
                Some(tokens::Token::RedirectAppend) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            fd_var: None,
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
                            fd_var: None,
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
                            fd_var: None,
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
                            fd_var: None,
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
                            fd_var: None,
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
                            fd_var: None,
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
                        fd_var: None,
                        kind: RedirectKind::DupOutput,
                        target: Word::literal(dst_fd.to_string()),
                    });
                }
                Some(tokens::Token::DupInput) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(0),
                            fd_var: None,
                            kind: RedirectKind::DupInput,
                            target,
                        });
                    }
                }
                Some(tokens::Token::DupFdIn(src_fd, dst_fd)) => {
                    let src_fd = *src_fd;
                    let dst_fd = *dst_fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(src_fd),
                        fd_var: None,
                        kind: RedirectKind::DupInput,
                        target: Word::literal(dst_fd.to_string()),
                    });
                }
                Some(tokens::Token::DupFdClose(fd)) => {
                    let fd = *fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(fd),
                        fd_var: None,
                        kind: RedirectKind::DupInput,
                        target: Word::literal("-"),
                    });
                }
                Some(tokens::Token::RedirectFdIn(fd)) => {
                    let fd = *fd;
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(fd),
                            fd_var: None,
                            kind: RedirectKind::Input,
                            target,
                        });
                    }
                }
                Some(tokens::Token::HereString) => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            fd_var: None,
                            kind: RedirectKind::HereString,
                            target,
                        });
                    }
                }
                Some(tokens::Token::HereDoc) | Some(tokens::Token::HereDocStrip) => {
                    let strip_tabs =
                        matches!(self.current_token, Some(tokens::Token::HereDocStrip));
                    self.advance();
                    let (delimiter, quoted) = match &self.current_token {
                        Some(tokens::Token::Word(w)) => (w.clone(), false),
                        Some(tokens::Token::LiteralWord(w)) => (w.clone(), true),
                        Some(tokens::Token::QuotedWord(w))
                        | Some(tokens::Token::QuotedGlobWord(w)) => (w.clone(), true),
                        _ => break,
                    };
                    let content = self.lexer.read_heredoc_with_strip(&delimiter, strip_tabs);
                    let content = if strip_tabs {
                        let had_trailing_newline = content.ends_with('\n');
                        let mut stripped: String = content
                            .lines()
                            .map(|l| l.trim_start_matches('\t'))
                            .collect::<Vec<_>>()
                            .join("\n");
                        if had_trailing_newline {
                            stripped.push('\n');
                        }
                        stripped
                    } else {
                        content
                    };
                    self.advance();
                    let target = if quoted {
                        Word::quoted_literal(content)
                    } else {
                        self.parse_word(content)
                    };
                    let kind = if strip_tabs {
                        RedirectKind::HereDocStrip
                    } else {
                        RedirectKind::HereDoc
                    };
                    redirects.push(Redirect {
                        fd: None,
                        fd_var: None,
                        kind,
                        target,
                    });
                    // Rest-of-line tokens re-injected by lexer; break so callers
                    // can see pipes/semicolons.
                    break;
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
        self.check_error_token()?;

        // Check for compound commands and function keyword
        if let Some(tokens::Token::Word(w)) = &self.current_token {
            let word = w.clone();
            match word.as_str() {
                "if" => return self.parse_compound_with_redirects(|s| s.parse_if()),
                "for" => return self.parse_compound_with_redirects(|s| s.parse_for()),
                "while" => return self.parse_compound_with_redirects(|s| s.parse_while()),
                "until" => return self.parse_compound_with_redirects(|s| s.parse_until()),
                "case" => return self.parse_compound_with_redirects(|s| s.parse_case()),
                "select" => return self.parse_compound_with_redirects(|s| s.parse_select()),
                "time" => return self.parse_compound_with_redirects(|s| s.parse_time()),
                "coproc" => return self.parse_compound_with_redirects(|s| s.parse_coproc()),
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

        // Bash requires at least one command in then branch
        if then_branch.is_empty() {
            self.pop_depth();
            return Err(self.error("syntax error: empty then clause"));
        }

        // Parse elif branches
        let mut elif_branches = Vec::new();
        while self.is_keyword("elif") {
            self.advance(); // consume 'elif'
            self.skip_newlines()?;

            let elif_condition = self.parse_compound_list("then")?;
            self.expect_keyword("then")?;
            self.skip_newlines()?;

            let elif_body = self.parse_compound_list_until(&["elif", "else", "fi"])?;

            // Bash requires at least one command in elif branch
            if elif_body.is_empty() {
                self.pop_depth();
                return Err(self.error("syntax error: empty elif clause"));
            }

            elif_branches.push((elif_condition, elif_body));
        }

        // Parse else branch
        let else_branch = if self.is_keyword("else") {
            self.advance(); // consume 'else'
            self.skip_newlines()?;
            let branch = self.parse_compound_list("fi")?;

            // Bash requires at least one command in else branch
            if branch.is_empty() {
                self.pop_depth();
                return Err(self.error("syntax error: empty else clause"));
            }

            Some(branch)
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
            | Some(tokens::Token::QuotedWord(w))
            | Some(tokens::Token::QuotedGlobWord(w)) => w.clone(),
            _ => {
                self.pop_depth();
                return Err(Error::parse(
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
                    Some(tokens::Token::Word(w))
                    | Some(tokens::Token::QuotedWord(w))
                    | Some(tokens::Token::QuotedGlobWord(w)) => {
                        let is_quoted = matches!(
                            &self.current_token,
                            Some(tokens::Token::QuotedWord(_))
                                | Some(tokens::Token::QuotedGlobWord(_))
                        );
                        let mut word = self.parse_word(w.clone());
                        if is_quoted {
                            word.quoted = true;
                        }
                        if matches!(&self.current_token, Some(tokens::Token::QuotedGlobWord(_))) {
                            word.has_unquoted_glob = true;
                        }
                        words.push(word);
                        self.advance();
                    }
                    Some(tokens::Token::LiteralWord(w)) => {
                        words.push(Word {
                            parts: vec![WordPart::Literal(w.clone())],
                            quoted: true,
                            has_unquoted_glob: false,
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

        // Bash requires at least one command in loop body
        if body.is_empty() {
            self.pop_depth();
            return Err(self.error("syntax error: empty for loop body"));
        }

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

    /// Parse select loop: select var in list; do body; done
    fn parse_select(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
        self.push_depth()?;
        self.advance(); // consume 'select'
        self.skip_newlines()?;

        // Expect variable name
        let variable = match &self.current_token {
            Some(tokens::Token::Word(w))
            | Some(tokens::Token::LiteralWord(w))
            | Some(tokens::Token::QuotedWord(w))
            | Some(tokens::Token::QuotedGlobWord(w)) => w.clone(),
            _ => {
                self.pop_depth();
                return Err(Error::parse("expected variable name in select".to_string()));
            }
        };
        self.advance();

        // Expect 'in' keyword
        if !self.is_keyword("in") {
            self.pop_depth();
            return Err(Error::parse("expected 'in' in select".to_string()));
        }
        self.advance(); // consume 'in'

        // Parse word list until do/newline/;
        let mut words = Vec::new();
        loop {
            match &self.current_token {
                Some(tokens::Token::Word(w)) if w == "do" => break,
                Some(tokens::Token::Word(w))
                | Some(tokens::Token::QuotedWord(w))
                | Some(tokens::Token::QuotedGlobWord(w)) => {
                    let is_quoted = matches!(
                        &self.current_token,
                        Some(tokens::Token::QuotedWord(_)) | Some(tokens::Token::QuotedGlobWord(_))
                    );
                    let mut word = self.parse_word(w.clone());
                    if is_quoted {
                        word.quoted = true;
                    }
                    if matches!(&self.current_token, Some(tokens::Token::QuotedGlobWord(_))) {
                        word.has_unquoted_glob = true;
                    }
                    words.push(word);
                    self.advance();
                }
                Some(tokens::Token::LiteralWord(w)) => {
                    words.push(Word {
                        parts: vec![WordPart::Literal(w.clone())],
                        quoted: true,
                        has_unquoted_glob: false,
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

        self.skip_newlines()?;

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines()?;

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Bash requires at least one command in loop body
        if body.is_empty() {
            self.pop_depth();
            return Err(self.error("syntax error: empty select loop body"));
        }

        // Expect 'done'
        self.expect_keyword("done")?;

        self.pop_depth();
        Ok(CompoundCommand::Select(SelectCommand {
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
                | Some(tokens::Token::QuotedWord(w))
                | Some(tokens::Token::QuotedGlobWord(w)) => {
                    // Don't add space when joining operator pairs like < + =3 → <=3
                    let skip_space = current_expr.ends_with('<')
                        || current_expr.ends_with('>')
                        || current_expr.ends_with(' ')
                        || current_expr.ends_with('(')
                        || current_expr.is_empty();
                    if !skip_space {
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
                    return Err(Error::parse(
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

        // Bash requires at least one command in loop body
        if body.is_empty() {
            return Err(self.error("syntax error: empty for loop body"));
        }

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

        // Bash requires at least one command in loop body
        if body.is_empty() {
            self.pop_depth();
            return Err(self.error("syntax error: empty while loop body"));
        }

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

        // Bash requires at least one command in loop body
        if body.is_empty() {
            self.pop_depth();
            return Err(self.error("syntax error: empty until loop body"));
        }

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
                    | Some(tokens::Token::QuotedGlobWord(_))
            ) {
                let w = match &self.current_token {
                    Some(tokens::Token::Word(w))
                    | Some(tokens::Token::LiteralWord(w))
                    | Some(tokens::Token::QuotedWord(w))
                    | Some(tokens::Token::QuotedGlobWord(w)) => w.clone(),
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

            let terminator = self.parse_case_terminator();
            cases.push(CaseItem {
                patterns,
                commands,
                terminator,
            });
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

    /// Parse a coproc command: `coproc [NAME] command`
    ///
    /// If the token after `coproc` is a simple word followed by a compound
    /// command (`{`, `(`, `while`, `for`, etc.), it is treated as the coproc
    /// name. Otherwise the command starts immediately and the default name
    /// "COPROC" is used.
    fn parse_coproc(&mut self) -> Result<CompoundCommand> {
        let start_span = self.current_span;
        self.advance(); // consume 'coproc'
        self.skip_newlines()?;

        // Determine if next token is a NAME (simple word that is NOT a compound-
        // command keyword and is followed by a compound command start).
        let (name, consumed_name) = if let Some(tokens::Token::Word(w)) = &self.current_token {
            let word = w.clone();
            let is_compound_keyword = matches!(
                word.as_str(),
                "if" | "for" | "while" | "until" | "case" | "select" | "time" | "coproc"
            );
            let next_is_compound_start = matches!(
                self.peek_next(),
                Some(tokens::Token::LeftBrace) | Some(tokens::Token::LeftParen)
            );
            if !is_compound_keyword && next_is_compound_start {
                self.advance(); // consume the NAME
                self.skip_newlines()?;
                (word, true)
            } else {
                ("COPROC".to_string(), false)
            }
        } else {
            ("COPROC".to_string(), false)
        };

        let _ = consumed_name;

        // Parse the command body (could be simple, compound, or pipeline)
        let body = self.parse_pipeline()?;
        let body = body.ok_or_else(|| self.error("coproc: missing command"))?;

        Ok(CompoundCommand::Coproc(ast::CoprocCommand {
            name,
            body: Box::new(body),
            span: start_span.merge(self.current_span),
        }))
    }

    /// Check if current token is ;; (case terminator)
    fn is_case_terminator(&self) -> bool {
        matches!(
            self.current_token,
            Some(tokens::Token::DoubleSemicolon)
                | Some(tokens::Token::SemiAmp)
                | Some(tokens::Token::DoubleSemiAmp)
        )
    }

    /// Parse case terminator: `;;` (break), `;&` (fallthrough), `;;&` (continue matching)
    fn parse_case_terminator(&mut self) -> ast::CaseTerminator {
        match self.current_token {
            Some(tokens::Token::SemiAmp) => {
                self.advance();
                ast::CaseTerminator::FallThrough
            }
            Some(tokens::Token::DoubleSemiAmp) => {
                self.advance();
                ast::CaseTerminator::Continue
            }
            Some(tokens::Token::DoubleSemicolon) => {
                self.advance();
                ast::CaseTerminator::Break
            }
            _ => ast::CaseTerminator::Break,
        }
    }

    /// Parse a subshell (commands in parentheses)
    fn parse_subshell(&mut self) -> Result<CompoundCommand> {
        self.push_depth()?;
        self.advance(); // consume '('
        self.skip_newlines()?;

        let mut commands = Vec::new();
        while !matches!(
            self.current_token,
            Some(tokens::Token::RightParen) | Some(tokens::Token::DoubleRightParen) | None
        ) {
            self.skip_newlines()?;
            if matches!(
                self.current_token,
                Some(tokens::Token::RightParen) | Some(tokens::Token::DoubleRightParen)
            ) {
                break;
            }
            if let Some(cmd) = self.parse_command_list()? {
                commands.push(cmd);
            }
        }

        if matches!(self.current_token, Some(tokens::Token::DoubleRightParen)) {
            // `))` at end of nested subshells: consume as single `)`, leave `)` for parent
            self.current_token = Some(tokens::Token::RightParen);
        } else if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
            self.pop_depth();
            return Err(Error::parse("expected ')' to close subshell".to_string()));
        } else {
            self.advance(); // consume ')'
        }

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
            return Err(Error::parse(
                "expected '}' to close brace group".to_string(),
            ));
        }

        // Bash requires at least one command in a brace group
        if commands.is_empty() {
            self.pop_depth();
            return Err(self.error("syntax error: empty brace group"));
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
                | Some(tokens::Token::QuotedWord(w))
                | Some(tokens::Token::QuotedGlobWord(w)) => {
                    let w_clone = w.clone();
                    let is_quoted = matches!(
                        self.current_token,
                        Some(tokens::Token::QuotedWord(_)) | Some(tokens::Token::QuotedGlobWord(_))
                    );
                    let is_literal =
                        matches!(self.current_token, Some(tokens::Token::LiteralWord(_)));

                    // After =~, handle regex pattern.
                    // If the pattern contains $ (variable reference), parse it as a
                    // normal word so variables expand. Otherwise collect as literal
                    // regex to preserve parens, backslashes, etc.
                    if saw_regex_op {
                        if w_clone.contains('$') && !is_quoted {
                            // Variable reference — parse normally for expansion
                            let parsed = self.parse_word(w_clone);
                            words.push(parsed);
                            self.advance();
                        } else {
                            let pattern = self.collect_conditional_regex_pattern(&w_clone);
                            words.push(Word::literal(&pattern));
                        }
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
                            has_unquoted_glob: false,
                        }
                    } else {
                        let mut parsed = self.parse_word(w_clone);
                        if is_quoted {
                            parsed.quoted = true;
                        }
                        if matches!(self.current_token, Some(tokens::Token::QuotedGlobWord(_))) {
                            parsed.has_unquoted_glob = true;
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
                    if saw_regex_op {
                        // Regex pattern starts with '(' — collect it
                        let pattern = self.collect_conditional_regex_pattern("(");
                        words.push(Word::literal(&pattern));
                        saw_regex_op = false;
                        continue;
                    }
                    words.push(Word::literal("("));
                    self.advance();
                }
                Some(tokens::Token::RightParen) => {
                    words.push(Word::literal(")"));
                    self.advance();
                }
                None => {
                    return Err(crate::error::Error::parse(
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
                | Some(tokens::Token::QuotedWord(w))
                | Some(tokens::Token::QuotedGlobWord(w)) => {
                    pattern.push_str(w);
                    self.advance();
                }
                _ => break,
            }
        }

        pattern
    }

    /// Check if current token starts with `=` (e.g., Word("=5") from `>=5`).
    /// If so, return the rest of the word after `=`.
    fn current_token_starts_with_eq(&self) -> Option<String> {
        match &self.current_token {
            Some(tokens::Token::Assignment) => Some(String::new()),
            Some(tokens::Token::Word(w)) | Some(tokens::Token::LiteralWord(w)) => {
                w.strip_prefix('=').map(|rest| rest.to_string())
            }
            _ => None,
        }
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
                | Some(tokens::Token::QuotedWord(w))
                | Some(tokens::Token::QuotedGlobWord(w)) => {
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
                    self.advance();
                    // Check if next token starts with '=' to form '<='
                    if let Some(rest) = self.current_token_starts_with_eq() {
                        expr.push_str("<=");
                        if !rest.is_empty() {
                            expr.push_str(&rest);
                        }
                        self.advance();
                    } else {
                        expr.push('<');
                    }
                }
                Some(tokens::Token::RedirectOut) => {
                    self.advance();
                    // Check if next token starts with '=' to form '>='
                    if let Some(rest) = self.current_token_starts_with_eq() {
                        expr.push_str(">=");
                        if !rest.is_empty() {
                            expr.push_str(&rest);
                        }
                        self.advance();
                    } else {
                        expr.push('>');
                    }
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
                Some(tokens::Token::Assignment) => {
                    expr.push('=');
                    self.advance();
                }
                // In arithmetic context, N> is a number followed by >, not a fd redirect
                Some(tokens::Token::RedirectFd(fd)) => {
                    let fd = *fd;
                    self.advance();
                    if let Some(rest) = self.current_token_starts_with_eq() {
                        // N>= → number >= ...
                        expr.push_str(&format!("{}>=", fd));
                        if !rest.is_empty() {
                            expr.push_str(&rest);
                        }
                        self.advance();
                    } else {
                        expr.push_str(&format!("{}>", fd));
                    }
                }
                Some(tokens::Token::RedirectFdAppend(fd)) => {
                    // N>> in arithmetic is N >> (right shift)
                    let fd = *fd;
                    expr.push_str(&format!("{}>>", fd));
                    self.advance();
                }
                Some(tokens::Token::RedirectFdIn(fd)) => {
                    let fd = *fd;
                    self.advance();
                    if let Some(rest) = self.current_token_starts_with_eq() {
                        expr.push_str(&format!("{}<=", fd));
                        if !rest.is_empty() {
                            expr.push_str(&rest);
                        }
                        self.advance();
                    } else {
                        expr.push_str(&format!("{}<", fd));
                    }
                }
                Some(tokens::Token::RedirectAppend) => {
                    // >> in arithmetic is right shift
                    expr.push_str(">>");
                    self.advance();
                }
                None => {
                    return Err(Error::parse(
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
                return Err(Error::parse(
                    "expected ')' in function definition".to_string(),
                ));
            }
            self.advance(); // consume ')'
            self.skip_newlines()?;
        }

        // Expect { for body
        if !matches!(self.current_token, Some(tokens::Token::LeftBrace)) {
            return Err(Error::parse("expected '{' for function body".to_string()));
        }

        // Parse body as brace group
        let body = self.parse_brace_group()?;
        let end_offset = self.current_command_end_offset();

        Ok(Command::Function(FunctionDef {
            name,
            body: Box::new(Command::Compound(body, Vec::new())),
            source: self.source_slice(start_span.start.offset, end_offset),
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
        let end_offset = self.current_command_end_offset();

        Ok(Command::Function(FunctionDef {
            name,
            body: Box::new(Command::Compound(body, Vec::new())),
            source: self.source_slice(start_span.start.offset, end_offset),
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
            if let Some(tokens::Token::Word(w)) = &self.current_token
                && terminators.contains(&w.as_str())
            {
                break;
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
    /// Split array element text respecting single and double quotes.
    /// Returns Vec of (element_text, was_quoted).
    /// Quoted elements have their outer quotes stripped.
    fn split_array_elements(s: &str) -> Vec<(String, bool)> {
        let mut result = Vec::new();
        let mut current = String::new();
        let mut chars = s.chars().peekable();
        let mut in_double_quote = false;
        let mut in_single_quote = false;
        let mut is_quoted = false;

        while let Some(c) = chars.next() {
            match c {
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                    is_quoted = true;
                    // Don't include the quote character in output
                }
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                    is_quoted = true;
                    // Don't include the quote character in output
                }
                '\\' if in_double_quote => {
                    // In double quotes, backslash escapes certain chars
                    if let Some(&next) = chars.peek() {
                        if matches!(next, '$' | '`' | '"' | '\\' | '\n') {
                            current.push(chars.next().unwrap());
                        } else {
                            current.push(c);
                        }
                    } else {
                        current.push(c);
                    }
                }
                c if c.is_ascii_whitespace() && !in_double_quote && !in_single_quote => {
                    if !current.is_empty() {
                        result.push((current.clone(), is_quoted));
                        current.clear();
                        is_quoted = false;
                    }
                }
                _ => {
                    current.push(c);
                }
            }
        }
        if !current.is_empty() {
            result.push((current, is_quoted));
        }
        result
    }

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
    /// Collect array elements between `(` and `)` tokens into a `Vec<Word>`.
    fn collect_array_elements(&mut self) -> Vec<Word> {
        let mut elements = Vec::new();
        loop {
            match &self.current_token {
                Some(tokens::Token::RightParen) => {
                    self.advance();
                    break;
                }
                Some(tokens::Token::Word(elem))
                | Some(tokens::Token::LiteralWord(elem))
                | Some(tokens::Token::QuotedWord(elem))
                | Some(tokens::Token::QuotedGlobWord(elem)) => {
                    let elem_clone = elem.clone();
                    let word = if matches!(&self.current_token, Some(tokens::Token::LiteralWord(_)))
                    {
                        Word {
                            parts: vec![WordPart::Literal(elem_clone)],
                            quoted: true,
                            has_unquoted_glob: false,
                        }
                    } else if matches!(
                        &self.current_token,
                        Some(tokens::Token::QuotedWord(_)) | Some(tokens::Token::QuotedGlobWord(_))
                    ) {
                        let mut w = self.parse_word(elem_clone);
                        w.quoted = true;
                        w
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
        elements
    }

    /// Parse the value side of an assignment (`VAR=value`).
    /// Returns `Some((Assignment, needs_advance))` if the current word is an assignment.
    /// The bool indicates whether the caller must call `self.advance()` afterward.
    fn try_parse_assignment(&mut self, w: &str) -> Option<(Assignment, bool)> {
        let (name, index, value, is_append) = Self::is_assignment(w)?;
        let name = name.to_string();
        let index = index.map(|s| s.to_string());
        let value_str = value.to_string();

        // Array literal in the token itself: arr=(a b c)
        if value_str.starts_with('(') && value_str.ends_with(')') {
            let inner = &value_str[1..value_str.len() - 1];
            let elements: Vec<Word> = Self::split_array_elements(inner)
                .into_iter()
                .map(|(s, quoted)| {
                    if quoted {
                        let mut w = self.parse_word(s);
                        w.quoted = true;
                        w
                    } else {
                        self.parse_word(s)
                    }
                })
                .collect();
            return Some((
                Assignment {
                    name,
                    index,
                    value: AssignmentValue::Array(elements),
                    append: is_append,
                },
                true,
            ));
        }

        // Empty value — check for arr=(...) syntax with separate tokens
        if value_str.is_empty() {
            self.advance();
            if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
                self.advance(); // consume '('
                let elements = self.collect_array_elements();
                return Some((
                    Assignment {
                        name,
                        index,
                        value: AssignmentValue::Array(elements),
                        append: is_append,
                    },
                    false,
                ));
            }
            // Empty assignment: VAR=
            return Some((
                Assignment {
                    name,
                    index,
                    value: AssignmentValue::Scalar(Word::literal("")),
                    append: is_append,
                },
                false,
            ));
        }

        // Quoted or plain scalar value
        let value_word = if value_str.starts_with('"') && value_str.ends_with('"') {
            let inner = Self::strip_quotes(&value_str);
            let mut w = self.parse_word(inner.to_string());
            w.quoted = true;
            w
        } else if value_str.starts_with('\'') && value_str.ends_with('\'') {
            let inner = Self::strip_quotes(&value_str);
            Word {
                parts: vec![WordPart::Literal(inner.to_string())],
                quoted: true,
                has_unquoted_glob: false,
            }
        } else {
            self.parse_word(value_str)
        };
        Some((
            Assignment {
                name,
                index,
                value: AssignmentValue::Scalar(value_word),
                append: is_append,
            },
            true,
        ))
    }

    /// Parse a compound array argument in arg position (e.g. `declare -a arr=(x y z)`).
    /// Called when the current word ends with `=` and the next token is `(`.
    /// Returns the compound word if successful, or `None` if not a compound assignment.
    fn try_parse_compound_array_arg(&mut self, saved_w: String) -> Option<Word> {
        if !matches!(self.current_token, Some(tokens::Token::LeftParen)) {
            return None;
        }
        self.advance(); // consume '('
        let mut compound = saved_w;
        compound.push('(');
        loop {
            match &self.current_token {
                Some(tokens::Token::RightParen) => {
                    compound.push(')');
                    self.advance();
                    break;
                }
                Some(tokens::Token::Word(elem))
                | Some(tokens::Token::LiteralWord(elem))
                | Some(tokens::Token::QuotedWord(elem))
                | Some(tokens::Token::QuotedGlobWord(elem)) => {
                    if !compound.ends_with('(') {
                        compound.push(' ');
                    }
                    compound.push_str(elem);
                    self.advance();
                }
                None => break,
                _ => {
                    self.advance();
                }
            }
        }
        Some(self.parse_word(compound))
    }

    /// Parse a heredoc redirect (`<<` or `<<-`) and any trailing redirects on the same line.
    fn parse_heredoc_redirect(
        &mut self,
        strip_tabs: bool,
        redirects: &mut Vec<Redirect>,
    ) -> Result<()> {
        self.advance();
        // Get the delimiter word and track if it was quoted
        let (delimiter, quoted) = match &self.current_token {
            Some(tokens::Token::Word(w)) => (w.clone(), false),
            Some(tokens::Token::LiteralWord(w)) => (w.clone(), true),
            Some(tokens::Token::QuotedWord(w)) | Some(tokens::Token::QuotedGlobWord(w)) => {
                (w.clone(), true)
            }
            _ => return Err(Error::parse("expected delimiter after <<".to_string())),
        };

        let content = self.lexer.read_heredoc_with_strip(&delimiter, strip_tabs);

        // Strip leading tabs for <<-
        let content = if strip_tabs {
            let had_trailing_newline = content.ends_with('\n');
            let mut stripped: String = content
                .lines()
                .map(|l: &str| l.trim_start_matches('\t'))
                .collect::<Vec<_>>()
                .join("\n");
            if had_trailing_newline {
                stripped.push('\n');
            }
            stripped
        } else {
            content
        };

        let target = if quoted {
            Word::quoted_literal(content)
        } else {
            self.parse_word(content)
        };

        let kind = if strip_tabs {
            RedirectKind::HereDocStrip
        } else {
            RedirectKind::HereDoc
        };

        redirects.push(Redirect {
            fd: None,
            fd_var: None,
            kind,
            target,
        });

        // Advance so re-injected rest-of-line tokens are picked up
        self.advance();

        // Consume any trailing redirects on the same line (e.g. `cat <<EOF > file`)
        self.collect_trailing_redirects(redirects);
        Ok(())
    }

    /// Consume redirect tokens that follow a heredoc on the same line.
    fn collect_trailing_redirects(&mut self, redirects: &mut Vec<Redirect>) {
        while let Some(tok) = &self.current_token {
            match tok {
                tokens::Token::RedirectOut | tokens::Token::Clobber => {
                    let kind = if matches!(&self.current_token, Some(tokens::Token::Clobber)) {
                        RedirectKind::Clobber
                    } else {
                        RedirectKind::Output
                    };
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            fd_var: None,
                            kind,
                            target,
                        });
                    }
                }
                tokens::Token::RedirectAppend => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: None,
                            fd_var: None,
                            kind: RedirectKind::Append,
                            target,
                        });
                    }
                }
                tokens::Token::RedirectFd(fd) => {
                    let fd = *fd;
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(fd),
                            fd_var: None,
                            kind: RedirectKind::Output,
                            target,
                        });
                    }
                }
                tokens::Token::DupInput => {
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(0),
                            fd_var: None,
                            kind: RedirectKind::DupInput,
                            target,
                        });
                    }
                }
                tokens::Token::DupFdIn(src_fd, dst_fd) => {
                    let src_fd = *src_fd;
                    let dst_fd = *dst_fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(src_fd),
                        fd_var: None,
                        kind: RedirectKind::DupInput,
                        target: Word::literal(dst_fd.to_string()),
                    });
                }
                tokens::Token::DupFdClose(fd) => {
                    let fd = *fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(fd),
                        fd_var: None,
                        kind: RedirectKind::DupInput,
                        target: Word::literal("-"),
                    });
                }
                tokens::Token::RedirectFdIn(fd) => {
                    let fd = *fd;
                    self.advance();
                    if let Ok(target) = self.expect_word() {
                        redirects.push(Redirect {
                            fd: Some(fd),
                            fd_var: None,
                            kind: RedirectKind::Input,
                            target,
                        });
                    }
                }
                _ => break,
            }
        }
    }

    /// Extract fd-variable name from `{varname}` pattern in the last word.
    /// If the last word is a single literal `{identifier}`, pop it and return the name.
    /// Used for `exec {var}>file` / `exec {var}>&-` syntax.
    fn pop_fd_var(words: &mut Vec<Word>) -> Option<String> {
        if let Some(last) = words.last()
            && last.parts.len() == 1
            && let WordPart::Literal(ref s) = last.parts[0]
            && s.starts_with('{')
            && s.ends_with('}')
            && s.len() > 2
            && s[1..s.len() - 1]
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_')
        {
            let var_name = s[1..s.len() - 1].to_string();
            words.pop();
            return Some(var_name);
        }
        None
    }

    fn parse_simple_command(&mut self) -> Result<Option<SimpleCommand>> {
        self.tick()?;
        self.skip_newlines()?;
        self.check_error_token()?;
        let start_span = self.current_span;

        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();

        loop {
            match &self.current_token {
                Some(tokens::Token::Word(w))
                | Some(tokens::Token::LiteralWord(w))
                | Some(tokens::Token::QuotedWord(w))
                | Some(tokens::Token::QuotedGlobWord(w)) => {
                    let is_literal =
                        matches!(&self.current_token, Some(tokens::Token::LiteralWord(_)));
                    let is_quoted = matches!(
                        &self.current_token,
                        Some(tokens::Token::QuotedWord(_)) | Some(tokens::Token::QuotedGlobWord(_))
                    );
                    let is_glob_quoted =
                        matches!(&self.current_token, Some(tokens::Token::QuotedGlobWord(_)));
                    // Clone early to release borrow on self.current_token
                    let w = w.clone();

                    // Stop if this word cannot start a command (like 'then', 'fi', etc.)
                    if words.is_empty() && Self::is_non_command_word(&w) {
                        break;
                    }

                    // Check for assignment (only before the command name, not for literal words)
                    if words.is_empty()
                        && !is_literal
                        && let Some((assignment, needs_advance)) = self.try_parse_assignment(&w)
                    {
                        if needs_advance {
                            self.advance();
                        }
                        assignments.push(assignment);
                        continue;
                    }

                    // Handle compound array assignment in arg position:
                    // declare -a arr=(x y z) → arr=(x y z) as single arg
                    if w.ends_with('=') && !words.is_empty() {
                        self.advance();
                        if let Some(word) = self.try_parse_compound_array_arg(w.clone()) {
                            words.push(word);
                            continue;
                        }
                        // Not a compound assignment — treat as regular word
                        let word = if is_literal {
                            Word {
                                parts: vec![WordPart::Literal(w)],
                                quoted: true,
                                has_unquoted_glob: false,
                            }
                        } else {
                            let mut word = self.parse_word(w);
                            if is_quoted {
                                word.quoted = true;
                            }
                            if is_glob_quoted {
                                word.has_unquoted_glob = true;
                            }
                            word
                        };
                        words.push(word);
                        continue;
                    }

                    let word = if is_literal {
                        Word {
                            parts: vec![WordPart::Literal(w)],
                            quoted: true,
                            has_unquoted_glob: false,
                        }
                    } else {
                        let mut word = self.parse_word(w);
                        if is_quoted {
                            word.quoted = true;
                        }
                        if is_glob_quoted {
                            word.has_unquoted_glob = true;
                        }
                        word
                    };
                    words.push(word);
                    self.advance();
                }
                Some(tokens::Token::RedirectOut) | Some(tokens::Token::Clobber) => {
                    let kind = if matches!(&self.current_token, Some(tokens::Token::Clobber)) {
                        RedirectKind::Clobber
                    } else {
                        RedirectKind::Output
                    };
                    let fd_var = Self::pop_fd_var(&mut words);
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        fd_var,
                        kind,
                        target,
                    });
                }
                Some(tokens::Token::RedirectAppend) => {
                    let fd_var = Self::pop_fd_var(&mut words);
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        fd_var,
                        kind: RedirectKind::Append,
                        target,
                    });
                }
                Some(tokens::Token::RedirectIn) => {
                    let fd_var = Self::pop_fd_var(&mut words);
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        fd_var,
                        kind: RedirectKind::Input,
                        target,
                    });
                }
                Some(tokens::Token::HereString) => {
                    let fd_var = Self::pop_fd_var(&mut words);
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        fd_var,
                        kind: RedirectKind::HereString,
                        target,
                    });
                }
                Some(tokens::Token::HereDoc) | Some(tokens::Token::HereDocStrip) => {
                    let strip_tabs =
                        matches!(self.current_token, Some(tokens::Token::HereDocStrip));
                    self.parse_heredoc_redirect(strip_tabs, &mut redirects)?;
                    break;
                }
                Some(tokens::Token::ProcessSubIn) | Some(tokens::Token::ProcessSubOut) => {
                    let word = self.expect_word()?;
                    words.push(word);
                }
                Some(tokens::Token::RedirectBoth) => {
                    let fd_var = Self::pop_fd_var(&mut words);
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: None,
                        fd_var,
                        kind: RedirectKind::OutputBoth,
                        target,
                    });
                }
                Some(tokens::Token::DupOutput) => {
                    let fd_var = Self::pop_fd_var(&mut words);
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: if fd_var.is_some() { None } else { Some(1) },
                        fd_var,
                        kind: RedirectKind::DupOutput,
                        target,
                    });
                }
                Some(tokens::Token::RedirectFd(fd)) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: Some(fd),
                        fd_var: None,
                        kind: RedirectKind::Output,
                        target,
                    });
                }
                Some(tokens::Token::RedirectFdAppend(fd)) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: Some(fd),
                        fd_var: None,
                        kind: RedirectKind::Append,
                        target,
                    });
                }
                Some(tokens::Token::DupFd(src_fd, dst_fd)) => {
                    let src_fd = *src_fd;
                    let dst_fd = *dst_fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(src_fd),
                        fd_var: None,
                        kind: RedirectKind::DupOutput,
                        target: Word::literal(dst_fd.to_string()),
                    });
                }
                Some(tokens::Token::DupInput) => {
                    let fd_var = Self::pop_fd_var(&mut words);
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: if fd_var.is_some() { None } else { Some(0) },
                        fd_var,
                        kind: RedirectKind::DupInput,
                        target,
                    });
                }
                Some(tokens::Token::DupFdIn(src_fd, dst_fd)) => {
                    let src_fd = *src_fd;
                    let dst_fd = *dst_fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(src_fd),
                        fd_var: None,
                        kind: RedirectKind::DupInput,
                        target: Word::literal(dst_fd.to_string()),
                    });
                }
                Some(tokens::Token::DupFdClose(fd)) => {
                    let fd = *fd;
                    self.advance();
                    redirects.push(Redirect {
                        fd: Some(fd),
                        fd_var: None,
                        kind: RedirectKind::DupInput,
                        target: Word::literal("-"),
                    });
                }
                Some(tokens::Token::RedirectFdIn(fd)) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word()?;
                    redirects.push(Redirect {
                        fd: Some(fd),
                        fd_var: None,
                        kind: RedirectKind::Input,
                        target,
                    });
                }
                // { and } as arguments (not in command position) are literal words
                Some(tokens::Token::LeftBrace) | Some(tokens::Token::RightBrace)
                    if !words.is_empty() =>
                {
                    let sym = if matches!(self.current_token, Some(tokens::Token::LeftBrace)) {
                        "{"
                    } else {
                        "}"
                    };
                    words.push(Word::literal(sym));
                    self.advance();
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
                    has_unquoted_glob: false,
                };
                self.advance();
                Ok(word)
            }
            Some(tokens::Token::QuotedWord(w)) | Some(tokens::Token::QuotedGlobWord(w)) => {
                // Double-quoted: parse for variable expansion
                let word = self.parse_word(w.clone());
                self.advance();
                Ok(word)
            }
            Some(tokens::Token::ProcessSubIn) | Some(tokens::Token::ProcessSubOut) => {
                // Process substitution <(cmd) or >(cmd).
                //
                // Issue #1333: extract the body from the original source via span
                // offsets rather than reconstructing a string from the token stream.
                // Token-string reconstruction wraps `QuotedGlobWord` in `"..."`,
                // erasing the unquoted-glob boundary and breaking glob expansion
                // for patterns like `./"$var"*.ext` inside `<(...)`.
                let is_input = matches!(self.current_token, Some(tokens::Token::ProcessSubIn));
                // Span end of the `<(` / `>(` token is exactly the start of the body.
                let body_start_offset = self.current_span.end.offset;
                self.advance();

                let mut depth = 1;
                let body_end_offset;
                loop {
                    match &self.current_token {
                        Some(tokens::Token::LeftParen) => {
                            depth += 1;
                            self.advance();
                        }
                        Some(tokens::Token::RightParen) => {
                            depth -= 1;
                            if depth == 0 {
                                // Body ends at the start of the matching `)`.
                                body_end_offset = self.current_span.start.offset;
                                self.advance();
                                break;
                            }
                            self.advance();
                        }
                        Some(tokens::Token::ProcessSubIn) | Some(tokens::Token::ProcessSubOut) => {
                            // Nested <( / >( opens another paren level.
                            depth += 1;
                            self.advance();
                        }
                        Some(tokens::Token::Error(e)) => {
                            let msg = e.clone();
                            self.advance();
                            return Err(Error::parse(format!(
                                "lexer error in process substitution: {}",
                                msg
                            )));
                        }
                        None => {
                            return Err(Error::parse(
                                "unexpected end of input in process substitution".to_string(),
                            ));
                        }
                        _ => {
                            self.advance();
                        }
                    }
                }

                let cmd_str = self
                    .source_slice(body_start_offset, body_end_offset)
                    .unwrap_or_default();

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
                    has_unquoted_glob: false,
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
            Some(tokens::Token::Word(w))
            | Some(tokens::Token::QuotedWord(w))
            | Some(tokens::Token::QuotedGlobWord(w)) => Some(self.parse_word(w.clone())),
            Some(tokens::Token::LiteralWord(w)) => Some(Word {
                parts: vec![WordPart::Literal(w.clone())],
                quoted: true,
                has_unquoted_glob: false,
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
                | Some(tokens::Token::QuotedGlobWord(_))
        )
    }

    #[allow(dead_code)]
    /// Get the string content if current token is a word
    fn current_word_str(&self) -> Option<String> {
        match &self.current_token {
            Some(tokens::Token::Word(w))
            | Some(tokens::Token::LiteralWord(w))
            | Some(tokens::Token::QuotedWord(w))
            | Some(tokens::Token::QuotedGlobWord(w)) => Some(w.clone()),
            _ => None,
        }
    }

    /// Parse a word string into a Word with proper parts (variables, literals)
    fn parse_word(&self, s: String) -> Word {
        let mut parts = Vec::new();
        let mut chars = s.chars().peekable();
        let mut current = String::new();

        while let Some(ch) = chars.next() {
            if ch == '\x00' {
                // NUL sentinel from lexer: next char is a literal (escaped in source).
                if let Some(literal_ch) = chars.next() {
                    current.push(literal_ch);
                }
            } else if ch == '$' {
                // Flush current literal
                if !current.is_empty() {
                    parts.push(WordPart::Literal(std::mem::take(&mut current)));
                }

                // Check for $'...' - ANSI-C quoting
                if chars.peek() == Some(&'\'') {
                    chars.next(); // consume opening '
                    let mut ansi = String::new();
                    while let Some(c) = chars.next() {
                        if c == '\'' {
                            break;
                        }
                        if c == '\\' {
                            if let Some(esc) = chars.next() {
                                match esc {
                                    'n' => ansi.push('\n'),
                                    't' => ansi.push('\t'),
                                    'r' => ansi.push('\r'),
                                    'a' => ansi.push('\x07'),
                                    'b' => ansi.push('\x08'),
                                    'e' | 'E' => ansi.push('\x1B'),
                                    '\\' => ansi.push('\\'),
                                    '\'' => ansi.push('\''),
                                    _ => {
                                        ansi.push('\\');
                                        ansi.push(esc);
                                    }
                                }
                            }
                        } else {
                            ansi.push(c);
                        }
                    }
                    parts.push(WordPart::Literal(ansi));
                } else if chars.peek() == Some(&'(') {
                    // Check for $( - command substitution or arithmetic
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
                            if c == '}'
                                || c == '['
                                || c == '*'
                                || c == '@'
                                || c == ':'
                                || c == '-'
                                || c == '='
                                || c == '+'
                                || c == '?'
                            {
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
                            // ${!var} - indirect expansion (no operator)
                            chars.next(); // consume '}'
                            parts.push(WordPart::IndirectExpansion {
                                name: var_name,
                                operator: None,
                                operand: String::new(),
                                colon_variant: false,
                            });
                        } else if chars.peek() == Some(&':') {
                            // ${!var:op} - indirect expansion with colon operator
                            let mut lookahead = chars.clone();
                            lookahead.next(); // skip ':'
                            if matches!(
                                lookahead.peek(),
                                Some(&'-') | Some(&'=') | Some(&'+') | Some(&'?')
                            ) {
                                chars.next(); // consume ':'
                                let op_char = chars.next().unwrap();
                                let operand = self.read_brace_operand(&mut chars);
                                let operator = match op_char {
                                    '-' => ParameterOp::UseDefault,
                                    '=' => ParameterOp::AssignDefault,
                                    '+' => ParameterOp::UseReplacement,
                                    '?' => ParameterOp::Error,
                                    _ => unreachable!(),
                                };
                                parts.push(WordPart::IndirectExpansion {
                                    name: var_name,
                                    operator: Some(operator),
                                    operand,
                                    colon_variant: true,
                                });
                            } else {
                                // Not a param op after ':', treat as prefix match fallback
                                let mut suffix = String::new();
                                while let Some(&c) = chars.peek() {
                                    if c == '}' {
                                        chars.next();
                                        break;
                                    }
                                    suffix.push(chars.next().unwrap());
                                }
                                parts.push(WordPart::Variable(format!("!{}{}", var_name, suffix)));
                            }
                        } else if matches!(
                            chars.peek(),
                            Some(&'-') | Some(&'=') | Some(&'+') | Some(&'?')
                        ) {
                            // ${!var-op} - indirect expansion with non-colon operator
                            let op_char = chars.next().unwrap();
                            let operand = self.read_brace_operand(&mut chars);
                            let operator = match op_char {
                                '-' => ParameterOp::UseDefault,
                                '=' => ParameterOp::AssignDefault,
                                '+' => ParameterOp::UseReplacement,
                                '?' => ParameterOp::Error,
                                _ => unreachable!(),
                            };
                            parts.push(WordPart::IndirectExpansion {
                                name: var_name,
                                operator: Some(operator),
                                operand,
                                colon_variant: false,
                            });
                        } else {
                            // ${!prefix*} or ${!prefix@} - prefix matching
                            let mut suffix = String::new();
                            while let Some(&c) = chars.peek() {
                                if c == '}' {
                                    chars.next();
                                    break;
                                }
                                suffix.push(chars.next().unwrap());
                            }
                            // Strip trailing * or @
                            if suffix.ends_with('*') || suffix.ends_with('@') {
                                let full_prefix =
                                    format!("{}{}", var_name, &suffix[..suffix.len() - 1]);
                                parts.push(WordPart::PrefixMatch(full_prefix));
                            } else {
                                parts.push(WordPart::Variable(format!("!{}{}", var_name, suffix)));
                            }
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

                        // Handle special parameters: ${@...}, ${*...}
                        if var_name.is_empty()
                            && let Some(&c) = chars.peek()
                            && matches!(c, '@' | '*')
                        {
                            var_name.push(chars.next().unwrap());
                        }

                        // Check for array access ${arr[index]} or ${arr[@]:offset:length}
                        if chars.peek() == Some(&'[') {
                            chars.next(); // consume '['
                            let mut index = String::new();
                            // Track nesting so nested ${...} containing
                            // brackets (e.g. ${#arr[@]}) don't prematurely
                            // close the subscript.
                            let mut bracket_depth: i32 = 0;
                            let mut brace_depth: i32 = 0;
                            while let Some(&c) = chars.peek() {
                                if c == ']' && bracket_depth == 0 && brace_depth == 0 {
                                    chars.next();
                                    break;
                                }
                                match c {
                                    '[' => bracket_depth += 1,
                                    ']' => bracket_depth -= 1,
                                    '$' => {
                                        index.push(chars.next().unwrap());
                                        if chars.peek() == Some(&'{') {
                                            brace_depth += 1;
                                            index.push(chars.next().unwrap());
                                            continue;
                                        }
                                        continue;
                                    }
                                    '{' => brace_depth += 1,
                                    '}' if brace_depth > 0 => brace_depth -= 1,
                                    '}' => {}
                                    _ => {}
                                }
                                index.push(chars.next().unwrap());
                            }
                            // Strip surrounding quotes from index (e.g. "foo" -> foo)
                            if index.len() >= 2
                                && ((index.starts_with('"') && index.ends_with('"'))
                                    || (index.starts_with('\'') && index.ends_with('\'')))
                            {
                                index = index[1..index.len() - 1].to_string();
                            }
                            // After ], check for operators on array subscripts
                            if let Some(&next_c) = chars.peek() {
                                if next_c == ':' {
                                    // Peek ahead to distinguish param ops (:- := :+ :?) from slice (:N)
                                    let mut lookahead = chars.clone();
                                    lookahead.next(); // skip ':'
                                    let is_param_op = matches!(
                                        lookahead.peek(),
                                        Some(&'-') | Some(&'=') | Some(&'+') | Some(&'?')
                                    );
                                    if is_param_op {
                                        chars.next(); // consume ':'
                                        let arr_name = format!("{}[{}]", var_name, index);
                                        let op_char = chars.next().unwrap();
                                        let operand = self.read_brace_operand(&mut chars);
                                        let operator = match op_char {
                                            '-' => ParameterOp::UseDefault,
                                            '=' => ParameterOp::AssignDefault,
                                            '+' => ParameterOp::UseReplacement,
                                            '?' => ParameterOp::Error,
                                            _ => unreachable!(),
                                        };
                                        parts.push(WordPart::ParameterExpansion {
                                            name: arr_name,
                                            operator,
                                            operand,
                                            colon_variant: true,
                                        });
                                    } else {
                                        // Array slice ${arr[@]:offset:length}
                                        chars.next(); // consume ':'
                                        let mut offset = String::new();
                                        while let Some(&c) = chars.peek() {
                                            if c == ':' || c == '}' {
                                                break;
                                            }
                                            offset.push(chars.next().unwrap());
                                        }
                                        let length = if chars.peek() == Some(&':') {
                                            chars.next();
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
                                        if chars.peek() == Some(&'}') {
                                            chars.next();
                                        }
                                        parts.push(WordPart::ArraySlice {
                                            name: var_name,
                                            offset,
                                            length,
                                        });
                                    }
                                } else if matches!(next_c, '-' | '+' | '=' | '?') {
                                    // Non-colon operators on array: ${arr[@]-default}
                                    let arr_name = format!("{}[{}]", var_name, index);
                                    let op_char = chars.next().unwrap();
                                    let operand = self.read_brace_operand(&mut chars);
                                    let operator = match op_char {
                                        '-' => ParameterOp::UseDefault,
                                        '=' => ParameterOp::AssignDefault,
                                        '+' => ParameterOp::UseReplacement,
                                        '?' => ParameterOp::Error,
                                        _ => unreachable!(),
                                    };
                                    parts.push(WordPart::ParameterExpansion {
                                        name: arr_name,
                                        operator,
                                        operand,
                                        colon_variant: false,
                                    });
                                } else {
                                    // Plain array access ${arr[index]}
                                    if chars.peek() == Some(&'}') {
                                        chars.next();
                                    }
                                    parts.push(WordPart::ArrayAccess {
                                        name: var_name,
                                        index,
                                    });
                                }
                            } else {
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
                                        Some(&'-') | Some(&'=') | Some(&'+') | Some(&'?') => {
                                            let op_char = chars.next().unwrap();
                                            let operand = self.read_brace_operand(&mut chars);
                                            let operator = match op_char {
                                                '-' => ParameterOp::UseDefault,
                                                '=' => ParameterOp::AssignDefault,
                                                '+' => ParameterOp::UseReplacement,
                                                '?' => ParameterOp::Error,
                                                _ => unreachable!(),
                                            };
                                            parts.push(WordPart::ParameterExpansion {
                                                name: var_name,
                                                operator,
                                                operand,
                                                colon_variant: true,
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
                                // Non-colon test operators: ${var-default}, ${var+alt}, ${var=assign}, ${var?err}
                                '-' | '=' | '+' | '?' => {
                                    let op_char = chars.next().unwrap();
                                    let operand = self.read_brace_operand(&mut chars);
                                    let operator = match op_char {
                                        '-' => ParameterOp::UseDefault,
                                        '=' => ParameterOp::AssignDefault,
                                        '+' => ParameterOp::UseReplacement,
                                        '?' => ParameterOp::Error,
                                        _ => unreachable!(),
                                    };
                                    parts.push(WordPart::ParameterExpansion {
                                        name: var_name,
                                        operator,
                                        operand,
                                        colon_variant: false,
                                    });
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
                                            colon_variant: false,
                                        });
                                    } else {
                                        let op = self.read_brace_operand(&mut chars);
                                        parts.push(WordPart::ParameterExpansion {
                                            name: var_name,
                                            operator: ParameterOp::RemovePrefixShort,
                                            operand: op,
                                            colon_variant: false,
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
                                            colon_variant: false,
                                        });
                                    } else {
                                        let op = self.read_brace_operand(&mut chars);
                                        parts.push(WordPart::ParameterExpansion {
                                            name: var_name,
                                            operator: ParameterOp::RemoveSuffixShort,
                                            operand: op,
                                            colon_variant: false,
                                        });
                                    }
                                }
                                '/' => {
                                    chars.next();
                                    let replace_all = if chars.peek() == Some(&'/') {
                                        chars.next();
                                        true
                                    } else {
                                        false
                                    };
                                    let mut pattern = String::new();
                                    while let Some(&ch) = chars.peek() {
                                        if ch == '/' || ch == '}' {
                                            break;
                                        }
                                        if ch == '\\' {
                                            chars.next();
                                            if let Some(&next) = chars.peek()
                                                && next == '/'
                                            {
                                                pattern.push(chars.next().unwrap());
                                                continue;
                                            }
                                            pattern.push('\\');
                                            continue;
                                        }
                                        pattern.push(chars.next().unwrap());
                                    }
                                    let replacement = if chars.peek() == Some(&'/') {
                                        chars.next();
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
                                        colon_variant: false,
                                    });
                                }
                                '^' => {
                                    chars.next();
                                    let op = if chars.peek() == Some(&'^') {
                                        chars.next();
                                        ParameterOp::UpperAll
                                    } else {
                                        ParameterOp::UpperFirst
                                    };
                                    if chars.peek() == Some(&'}') {
                                        chars.next();
                                    }
                                    parts.push(WordPart::ParameterExpansion {
                                        name: var_name,
                                        operator: op,
                                        operand: String::new(),
                                        colon_variant: false,
                                    });
                                }
                                ',' => {
                                    chars.next();
                                    let op = if chars.peek() == Some(&',') {
                                        chars.next();
                                        ParameterOp::LowerAll
                                    } else {
                                        ParameterOp::LowerFirst
                                    };
                                    if chars.peek() == Some(&'}') {
                                        chars.next();
                                    }
                                    parts.push(WordPart::ParameterExpansion {
                                        name: var_name,
                                        operator: op,
                                        operand: String::new(),
                                        colon_variant: false,
                                    });
                                }
                                '@' => {
                                    chars.next();
                                    if let Some(&op) = chars.peek() {
                                        chars.next();
                                        if chars.peek() == Some(&'}') {
                                            chars.next();
                                        }
                                        parts.push(WordPart::Transformation {
                                            name: var_name,
                                            operator: op,
                                        });
                                    } else {
                                        if chars.peek() == Some(&'}') {
                                            chars.next();
                                        }
                                        parts.push(WordPart::Variable(var_name));
                                    }
                                }
                                '}' => {
                                    chars.next();
                                    if !var_name.is_empty() {
                                        parts.push(WordPart::Variable(var_name));
                                    }
                                }
                                _ => {
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
            has_unquoted_glob: false,
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

    #[test]
    fn test_heredoc_pipe() {
        let parser = Parser::new("cat <<EOF | sort\nc\na\nb\nEOF\n");
        let script = parser.parse().unwrap();
        assert!(
            matches!(&script.commands[0], Command::Pipeline(_)),
            "heredoc with pipe should parse as Pipeline"
        );
    }

    #[test]
    fn test_heredoc_multiple_on_line() {
        let input = "while cat <<E1 && cat <<E2; do cat <<E3; break; done\n1\nE1\n2\nE2\n3\nE3\n";
        let parser = Parser::new(input);
        let script = parser.parse().unwrap();
        assert_eq!(script.commands.len(), 1);
        if let Command::Compound(comp, _) = &script.commands[0] {
            if let CompoundCommand::While(w) = comp {
                assert!(
                    !w.condition.is_empty(),
                    "while condition should be non-empty"
                );
                assert!(!w.body.is_empty(), "while body should be non-empty");
            } else {
                panic!("expected While compound command");
            }
        } else {
            panic!("expected Compound command");
        }
    }

    #[test]
    fn test_empty_function_body_rejected() {
        let parser = Parser::new("f() { }");
        assert!(
            parser.parse().is_err(),
            "empty function body should be rejected"
        );
    }

    #[test]
    fn test_empty_while_body_rejected() {
        let parser = Parser::new("while true; do\ndone");
        assert!(
            parser.parse().is_err(),
            "empty while body should be rejected"
        );
    }

    #[test]
    fn test_empty_for_body_rejected() {
        let parser = Parser::new("for i in 1 2 3; do\ndone");
        assert!(parser.parse().is_err(), "empty for body should be rejected");
    }

    #[test]
    fn test_empty_if_then_rejected() {
        let parser = Parser::new("if true; then\nfi");
        assert!(
            parser.parse().is_err(),
            "empty then clause should be rejected"
        );
    }

    #[test]
    fn test_empty_else_rejected() {
        let parser = Parser::new("if false; then echo yes; else\nfi");
        assert!(
            parser.parse().is_err(),
            "empty else clause should be rejected"
        );
    }

    #[test]
    fn test_unterminated_single_quote_rejected() {
        let parser = Parser::new("echo 'unterminated");
        assert!(
            parser.parse().is_err(),
            "unterminated single quote should be rejected"
        );
    }

    #[test]
    fn test_unterminated_double_quote_rejected() {
        let parser = Parser::new("echo \"unterminated");
        assert!(
            parser.parse().is_err(),
            "unterminated double quote should be rejected"
        );
    }

    #[test]
    fn test_nonempty_function_body_accepted() {
        let parser = Parser::new("f() { echo hi; }");
        assert!(
            parser.parse().is_ok(),
            "non-empty function body should be accepted"
        );
    }

    #[test]
    fn test_nonempty_while_body_accepted() {
        let parser = Parser::new("while true; do echo hi; done");
        assert!(
            parser.parse().is_ok(),
            "non-empty while body should be accepted"
        );
    }

    /// Issue #600: Subscript reader must handle nested ${...} containing brackets.
    #[test]
    fn test_nested_expansion_in_array_subscript() {
        // ${arr[$RANDOM % ${#arr[@]}]} must parse without error.
        // The subscript contains ${#arr[@]} which has its own [ and ].
        let parser = Parser::new("echo ${arr[$RANDOM % ${#arr[@]}]}");
        let script = parser.parse().unwrap();
        assert_eq!(script.commands.len(), 1);
        if let Command::Simple(cmd) = &script.commands[0] {
            assert_eq!(cmd.name.to_string(), "echo");
            assert_eq!(cmd.args.len(), 1);
            // The arg should contain an ArrayAccess with the full nested index
            let arg = &cmd.args[0];
            let has_array_access = arg.parts.iter().any(|p| {
                matches!(
                    p,
                    WordPart::ArrayAccess { name, index }
                    if name == "arr" && index.contains("${#arr[@]}")
                )
            });
            assert!(
                has_array_access,
                "expected ArrayAccess with nested index, got: {:?}",
                arg.parts
            );
        } else {
            panic!("expected simple command");
        }
    }

    /// Assignment with nested subscript must parse (previously caused fuel exhaustion).
    #[test]
    fn test_assignment_nested_subscript_parses() {
        let parser = Parser::new("x=${arr[$RANDOM % ${#arr[@]}]}");
        assert!(
            parser.parse().is_ok(),
            "assignment with nested subscript should parse"
        );
    }
}
