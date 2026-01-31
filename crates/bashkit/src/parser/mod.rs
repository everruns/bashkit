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
    /// Lookahead token for function parsing
    peeked_token: Option<tokens::Token>,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given input.
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self {
            lexer,
            current_token,
            peeked_token: None,
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
        let first = match self.parse_command()? {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        let mut commands = vec![first];

        while matches!(self.current_token, Some(tokens::Token::Pipe)) {
            self.advance();
            self.skip_newlines();

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
        self.skip_newlines();

        // Check for compound commands and function keyword
        if let Some(tokens::Token::Word(w)) = &self.current_token {
            let word = w.clone();
            match word.as_str() {
                "if" => return self.parse_if().map(|c| Some(Command::Compound(c))),
                "for" => return self.parse_for().map(|c| Some(Command::Compound(c))),
                "while" => return self.parse_while().map(|c| Some(Command::Compound(c))),
                "until" => return self.parse_until().map(|c| Some(Command::Compound(c))),
                "case" => return self.parse_case().map(|c| Some(Command::Compound(c))),
                "function" => return self.parse_function_keyword().map(Some),
                _ => {
                    // Check for POSIX-style function: name() { body }
                    if matches!(self.peek_next(), Some(tokens::Token::LeftParen)) {
                        return self.parse_function_posix().map(Some);
                    }
                }
            }
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
        self.advance(); // consume 'if'
        self.skip_newlines();

        // Parse condition
        let condition = self.parse_compound_list("then")?;

        // Expect 'then'
        self.expect_keyword("then")?;
        self.skip_newlines();

        // Parse then branch
        let then_branch = self.parse_compound_list_until(&["elif", "else", "fi"])?;

        // Parse elif branches
        let mut elif_branches = Vec::new();
        while self.is_keyword("elif") {
            self.advance(); // consume 'elif'
            self.skip_newlines();

            let elif_condition = self.parse_compound_list("then")?;
            self.expect_keyword("then")?;
            self.skip_newlines();

            let elif_body = self.parse_compound_list_until(&["elif", "else", "fi"])?;
            elif_branches.push((elif_condition, elif_body));
        }

        // Parse else branch
        let else_branch = if self.is_keyword("else") {
            self.advance(); // consume 'else'
            self.skip_newlines();
            Some(self.parse_compound_list("fi")?)
        } else {
            None
        };

        // Expect 'fi'
        self.expect_keyword("fi")?;

        Ok(CompoundCommand::If(IfCommand {
            condition,
            then_branch,
            elif_branches,
            else_branch,
        }))
    }

    /// Parse a for loop
    fn parse_for(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume 'for'
        self.skip_newlines();

        // Expect variable name
        let variable = match &self.current_token {
            Some(tokens::Token::Word(w)) => w.clone(),
            _ => {
                return Err(Error::Parse(
                    "expected variable name in for loop".to_string(),
                ))
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
                    Some(tokens::Token::Word(w)) => {
                        words.push(self.parse_word(w.clone()));
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

        self.skip_newlines();

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines();

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Expect 'done'
        self.expect_keyword("done")?;

        Ok(CompoundCommand::For(ForCommand {
            variable,
            words,
            body,
        }))
    }

    /// Parse a while loop
    fn parse_while(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume 'while'
        self.skip_newlines();

        // Parse condition
        let condition = self.parse_compound_list("do")?;

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines();

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Expect 'done'
        self.expect_keyword("done")?;

        Ok(CompoundCommand::While(WhileCommand { condition, body }))
    }

    /// Parse an until loop
    fn parse_until(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume 'until'
        self.skip_newlines();

        // Parse condition
        let condition = self.parse_compound_list("do")?;

        // Expect 'do'
        self.expect_keyword("do")?;
        self.skip_newlines();

        // Parse body
        let body = self.parse_compound_list("done")?;

        // Expect 'done'
        self.expect_keyword("done")?;

        Ok(CompoundCommand::Until(UntilCommand { condition, body }))
    }

    /// Parse a case statement: case WORD in pattern) commands ;; ... esac
    fn parse_case(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume 'case'
        self.skip_newlines();

        // Get the word to match against
        let word = self.expect_word()?;
        self.skip_newlines();

        // Expect 'in'
        self.expect_keyword("in")?;
        self.skip_newlines();

        // Parse case items
        let mut cases = Vec::new();
        while !self.is_keyword("esac") && self.current_token.is_some() {
            self.skip_newlines();
            if self.is_keyword("esac") {
                break;
            }

            // Parse patterns (pattern1 | pattern2 | ...)
            // Optional leading (
            if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
                self.advance();
            }

            let mut patterns = Vec::new();
            while let Some(tokens::Token::Word(w)) = &self.current_token {
                patterns.push(self.parse_word(w.clone()));
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
                return Err(Error::Parse("expected ')' after case pattern".to_string()));
            }
            self.advance();
            self.skip_newlines();

            // Parse commands until ;; or esac
            let mut commands = Vec::new();
            while !self.is_case_terminator()
                && !self.is_keyword("esac")
                && self.current_token.is_some()
            {
                if let Some(cmd) = self.parse_command_list()? {
                    commands.push(cmd);
                }
                self.skip_newlines();
            }

            cases.push(CaseItem { patterns, commands });

            // Consume ;; if present
            if self.is_case_terminator() {
                self.advance_double_semicolon();
            }
            self.skip_newlines();
        }

        // Expect 'esac'
        self.expect_keyword("esac")?;

        Ok(CompoundCommand::Case(CaseCommand { word, cases }))
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
        self.advance(); // consume '('
        self.skip_newlines();

        let mut commands = Vec::new();
        while !matches!(self.current_token, Some(tokens::Token::RightParen) | None) {
            self.skip_newlines();
            if matches!(self.current_token, Some(tokens::Token::RightParen)) {
                break;
            }
            if let Some(cmd) = self.parse_command_list()? {
                commands.push(cmd);
            }
        }

        if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
            return Err(Error::Parse("expected ')' to close subshell".to_string()));
        }
        self.advance(); // consume ')'

        Ok(CompoundCommand::Subshell(commands))
    }

    /// Parse a brace group
    fn parse_brace_group(&mut self) -> Result<CompoundCommand> {
        self.advance(); // consume '{'
        self.skip_newlines();

        let mut commands = Vec::new();
        while !matches!(self.current_token, Some(tokens::Token::RightBrace) | None) {
            self.skip_newlines();
            if matches!(self.current_token, Some(tokens::Token::RightBrace)) {
                break;
            }
            if let Some(cmd) = self.parse_command_list()? {
                commands.push(cmd);
            }
        }

        if !matches!(self.current_token, Some(tokens::Token::RightBrace)) {
            return Err(Error::Parse(
                "expected '}' to close brace group".to_string(),
            ));
        }
        self.advance(); // consume '}'

        Ok(CompoundCommand::BraceGroup(commands))
    }

    /// Parse function definition with 'function' keyword: function name { body }
    fn parse_function_keyword(&mut self) -> Result<Command> {
        self.advance(); // consume 'function'
        self.skip_newlines();

        // Get function name
        let name = match &self.current_token {
            Some(tokens::Token::Word(w)) => w.clone(),
            _ => return Err(Error::Parse("expected function name".to_string())),
        };
        self.advance();
        self.skip_newlines();

        // Optional () after name
        if matches!(self.current_token, Some(tokens::Token::LeftParen)) {
            self.advance(); // consume '('
            if !matches!(self.current_token, Some(tokens::Token::RightParen)) {
                return Err(Error::Parse(
                    "expected ')' in function definition".to_string(),
                ));
            }
            self.advance(); // consume ')'
            self.skip_newlines();
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
        self.skip_newlines();

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
            self.skip_newlines();

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

    /// Reserved words that terminate simple commands (cannot appear as arguments)
    const TERMINATING_WORDS: &'static [&'static str] = &[
        "then", "else", "elif", "fi", "do", "done", "esac", "in", "}", ")",
    ];

    /// Reserved words that cannot start a simple command
    const NON_COMMAND_WORDS: &'static [&'static str] =
        &["then", "else", "elif", "fi", "do", "done", "esac", "in"];

    /// Check if a word is a terminating reserved word
    fn is_terminating_word(word: &str) -> bool {
        Self::TERMINATING_WORDS.contains(&word)
    }

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

    /// Check if a word is an assignment (NAME=value)
    fn is_assignment(word: &str) -> Option<(&str, &str)> {
        // Find the first =
        if let Some(eq_pos) = word.find('=') {
            let name = &word[..eq_pos];
            let value = &word[eq_pos + 1..];

            // Name must be valid identifier: starts with letter or _, followed by alnum or _
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
            return Some((name, value));
        }
        None
    }

    /// Parse a simple command with redirections
    fn parse_simple_command(&mut self) -> Result<Option<SimpleCommand>> {
        self.skip_newlines();

        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();

        loop {
            match &self.current_token {
                Some(tokens::Token::Word(w)) => {
                    // Stop if this word cannot start a command (like 'then', 'fi', etc.)
                    if words.is_empty() && Self::is_non_command_word(w) {
                        break;
                    }
                    // Stop if we see a terminating word as an argument
                    if !words.is_empty() && Self::is_terminating_word(w) {
                        break;
                    }

                    // Check for assignment (only before the command name)
                    if words.is_empty() {
                        if let Some((name, value)) = Self::is_assignment(w) {
                            assignments.push(Assignment {
                                name: name.to_string(),
                                value: self.parse_word(value.to_string()),
                            });
                            self.advance();
                            continue;
                        }
                    }

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
