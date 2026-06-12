//! AWK parser - turns a program string into an `AwkProgram` AST.

use std::collections::HashMap;

use super::{AwkAction, AwkExpr, AwkFunctionDef, AwkOutputTarget, AwkPattern, AwkProgram, AwkRule};
use crate::builtins::limits::{
    AWK_MAX_MULTI_SUBSCRIPTS as MAX_AWK_MULTI_SUBSCRIPTS,
    AWK_MAX_PARSER_DEPTH as MAX_AWK_PARSER_DEPTH,
};
use crate::builtins::search_common::build_regex;
use crate::error::{Error, Result};

pub(super) struct AwkParser<'a> {
    input: &'a str,
    pos: usize,
    /// Current recursion depth for expression parsing
    depth: usize,
    /// When true, `>` and `>>` are output redirection, not comparison ops.
    /// Set during print/printf argument parsing per POSIX awk semantics.
    in_print_context: bool,
}

impl<'a> AwkParser<'a> {
    fn is_identifier_start(c: char) -> bool {
        c.is_alphabetic() || c == '_'
    }

    fn is_identifier_continue(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    pub(super) fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            depth: 0,
            in_print_context: false,
        }
    }

    /// Get the character at the current byte position (char-boundary safe).
    /// Returns None if pos is at or past end of input.
    fn current_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    /// Advance pos past the current character (handles multi-byte UTF-8).
    fn advance(&mut self) {
        if let Some(c) = self.current_char() {
            self.pos += c.len_utf8();
        }
    }

    fn consume_while(&mut self, predicate: fn(char) -> bool) {
        while self.pos < self.input.len() {
            let c = self.current_char().unwrap();
            if predicate(c) {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// THREAT[TM-DOS-027]: Increment depth, error if limit exceeded
    fn push_depth(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_AWK_PARSER_DEPTH {
            return Err(Error::Execution(format!(
                "awk: expression nesting too deep ({} levels, max {})",
                self.depth, MAX_AWK_PARSER_DEPTH
            )));
        }
        Ok(())
    }

    /// Decrement depth after leaving a recursive parse
    fn pop_depth(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    pub(super) fn parse(&mut self) -> Result<AwkProgram> {
        let mut program = AwkProgram {
            begin_actions: Vec::new(),
            main_rules: Vec::new(),
            end_actions: Vec::new(),
            functions: HashMap::new(),
        };

        self.skip_whitespace();

        while self.pos < self.input.len() {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            // Check for function/BEGIN/END
            if self.matches_keyword("function") {
                self.skip_whitespace();
                let (name, func_def) = self.parse_function_def()?;
                program.functions.insert(name, func_def);
            } else if self.matches_keyword("BEGIN") {
                self.skip_whitespace();
                let actions = self.parse_action_block()?;
                program.begin_actions.extend(actions);
            } else if self.matches_keyword("END") {
                self.skip_whitespace();
                let actions = self.parse_action_block()?;
                program.end_actions.extend(actions);
            } else {
                // Pattern-action rule
                let rule = self.parse_rule()?;
                program.main_rules.push(rule);
            }

            self.skip_whitespace();
        }

        // If no rules, add default print rule
        if program.main_rules.is_empty()
            && program.begin_actions.is_empty()
            && program.end_actions.is_empty()
        {
            program.main_rules.push(AwkRule {
                pattern: None,
                actions: vec![AwkAction::Print(
                    vec![AwkExpr::Field(Box::new(AwkExpr::Number(0.0)))],
                    None,
                )],
            });
        }

        Ok(program)
    }

    /// Parse a user-defined function: function name(params) { body }
    fn parse_function_def(&mut self) -> Result<(String, AwkFunctionDef)> {
        // Parse function name
        let name = self.read_identifier()?;
        self.skip_whitespace();

        // Expect '('
        if self.pos >= self.input.len() || self.current_char().unwrap() != '(' {
            return Err(Error::Execution(
                "awk: expected '(' after function name".to_string(),
            ));
        }
        self.pos += 1;

        // Parse parameter list
        let mut params = Vec::new();
        self.skip_whitespace();
        while self.pos < self.input.len() && self.current_char().unwrap() != ')' {
            if !params.is_empty() {
                if self.current_char().unwrap() == ',' {
                    self.pos += 1;
                }
                self.skip_whitespace();
            }
            if self.pos < self.input.len() && self.current_char().unwrap() != ')' {
                params.push(self.read_identifier()?);
                self.skip_whitespace();
            }
        }

        // Expect ')'
        if self.pos >= self.input.len() || self.current_char().unwrap() != ')' {
            return Err(Error::Execution(
                "awk: expected ')' after function parameters".to_string(),
            ));
        }
        self.pos += 1;
        self.skip_whitespace();

        // Parse function body as action block
        let body = self.parse_action_block()?;

        Ok((name, AwkFunctionDef { params, body }))
    }

    /// Read an identifier (alphanumeric + underscore)
    fn read_identifier(&mut self) -> Result<String> {
        let start = self.pos;
        self.consume_while(Self::is_identifier_continue);
        if self.pos == start {
            return Err(Error::Execution("awk: expected identifier".to_string()));
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn matches_keyword(&mut self, keyword: &str) -> bool {
        if self.input[self.pos..].starts_with(keyword) {
            let after = self.pos + keyword.len();
            if after >= self.input.len() || {
                let c = self.input[after..].chars().next().unwrap();
                !c.is_alphanumeric() && c != '_'
            } {
                self.pos = after;
                return true;
            }
        }
        false
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            let c = self.current_char().unwrap();
            if c.is_whitespace() {
                self.advance();
            } else if c == '#' {
                // Comment - skip to end of line (may contain multi-byte chars)
                while self.pos < self.input.len() && self.current_char().unwrap() != '\n' {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn parse_rule(&mut self) -> Result<AwkRule> {
        let pattern = self.parse_pattern()?;
        self.skip_whitespace();

        let actions = if self.pos < self.input.len() && self.current_char().unwrap() == '{' {
            self.parse_action_block()?
        } else if pattern.is_some() {
            // Default action is print
            vec![AwkAction::Print(
                vec![AwkExpr::Field(Box::new(AwkExpr::Number(0.0)))],
                None,
            )]
        } else {
            Vec::new()
        };

        Ok(AwkRule { pattern, actions })
    }

    /// Parse a `/regex/` literal into an `AwkPattern::Regex`.
    /// Assumes the leading `/` is at `self.pos`.
    fn parse_regex_pattern(&mut self) -> Result<AwkPattern> {
        self.pos += 1; // skip opening '/'
        let start = self.pos;
        while self.pos < self.input.len() {
            let c = self.current_char().unwrap();
            if c == '/' {
                let pattern = &self.input[start..self.pos];
                self.pos += 1;
                let regex = build_regex(pattern)
                    .map_err(|e| Error::Execution(format!("awk: invalid regex: {}", e)))?;
                return Ok(AwkPattern::Regex(regex));
            } else if c == '\\' {
                self.pos += 1; // skip '\\' (ASCII)
                self.advance(); // skip next char (may be multi-byte)
            } else {
                self.advance(); // regex content may be multi-byte
            }
        }
        Err(Error::Execution("awk: unterminated regex".to_string()))
    }

    fn parse_pattern(&mut self) -> Result<Option<AwkPattern>> {
        let Some(first_pat) = self.parse_single_pattern()? else {
            return Ok(None);
        };

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != ',' {
            return Ok(Some(first_pat));
        }

        // THREAT[TM-DOS-027]: awk ranges contain exactly two operands. Do not
        // recursively parse comma chains (`1,1,1,...`) because attacker-sized
        // chains can exhaust the host stack before runtime limits apply.
        self.pos += 1; // consume ','
        let second_pat = self
            .parse_single_pattern()?
            .ok_or_else(|| Error::Execution("awk: expected second pattern in range".to_string()))?;

        self.skip_whitespace();
        if self.pos < self.input.len() && self.current_char().unwrap() == ',' {
            return Err(Error::Execution(
                "awk: unexpected ',' after range pattern".to_string(),
            ));
        }

        Ok(Some(AwkPattern::Range(
            Box::new(first_pat),
            Box::new(second_pat),
        )))
    }

    fn parse_single_pattern(&mut self) -> Result<Option<AwkPattern>> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Ok(None);
        }

        let c = self.current_char().unwrap();

        if c == '/' {
            Ok(Some(self.parse_regex_pattern()?))
        } else if c == '{' {
            Ok(None)
        } else {
            let expr = self.parse_expression()?;
            Ok(Some(AwkPattern::Expression(expr)))
        }
    }

    fn parse_action_block(&mut self) -> Result<Vec<AwkAction>> {
        self.skip_whitespace();

        if self.pos >= self.input.len() || self.current_char().unwrap() != '{' {
            return Err(Error::Execution("awk: expected '{'".to_string()));
        }
        self.pos += 1;

        let mut actions = Vec::new();

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                return Err(Error::Execution(
                    "awk: unterminated action block".to_string(),
                ));
            }

            // Skip empty statements (consecutive semicolons from newline normalization)
            while self.pos < self.input.len() && self.current_char().unwrap() == ';' {
                self.pos += 1;
                self.skip_whitespace();
            }
            if self.pos >= self.input.len() {
                return Err(Error::Execution(
                    "awk: unterminated action block".to_string(),
                ));
            }

            let c = self.current_char().unwrap();
            if c == '}' {
                self.pos += 1;
                break;
            }

            let action = self.parse_action()?;
            actions.push(action);

            self.skip_whitespace();
            // Allow semicolon separator
            if self.pos < self.input.len() && self.current_char().unwrap() == ';' {
                self.pos += 1;
            }
        }

        Ok(actions)
    }

    fn parse_action(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        // Check for keywords
        if self.matches_keyword("print") {
            return self.parse_print();
        }
        if self.matches_keyword("printf") {
            return self.parse_printf();
        }
        if self.matches_keyword("next") {
            return Ok(AwkAction::Next);
        }
        if self.matches_keyword("break") {
            return Ok(AwkAction::Break);
        }
        if self.matches_keyword("continue") {
            return Ok(AwkAction::Continue);
        }
        if self.matches_keyword("delete") {
            return self.parse_delete();
        }
        if self.matches_keyword("getline") {
            return self.parse_getline();
        }
        if self.matches_keyword("exit") {
            self.skip_whitespace();
            if self.pos < self.input.len() {
                let c = self.current_char().unwrap();
                if c != '}' && c != ';' {
                    let expr = self.parse_expression()?;
                    return Ok(AwkAction::Exit(Some(expr)));
                }
            }
            return Ok(AwkAction::Exit(None));
        }
        if self.matches_keyword("return") {
            self.skip_whitespace();
            if self.pos < self.input.len() {
                let c = self.current_char().unwrap();
                if c != '}' && c != ';' {
                    let expr = self.parse_expression()?;
                    return Ok(AwkAction::Return(Some(expr)));
                }
            }
            return Ok(AwkAction::Return(None));
        }
        if self.matches_keyword("if") {
            return self.parse_if();
        }
        if self.matches_keyword("for") {
            return self.parse_for();
        }
        if self.matches_keyword("while") {
            return self.parse_while();
        }
        if self.matches_keyword("do") {
            return self.parse_do_while();
        }

        // Otherwise it's an expression (including assignment)
        let expr = self.parse_expression()?;

        // Check if it's an assignment
        match expr {
            AwkExpr::Assign(name, val) => Ok(AwkAction::Assign(name, *val)),
            AwkExpr::ArrayAssign(name, key, val) => Ok(AwkAction::ArrayAssign(name, *key, *val)),
            _ => Ok(AwkAction::Expression(expr)),
        }
    }

    fn parse_print(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();
        let mut args = Vec::new();

        // Enable print context so `>` and `>>` are not parsed as comparison
        self.in_print_context = true;

        loop {
            if self.pos >= self.input.len() {
                break;
            }
            let c = self.current_char().unwrap();
            if c == '}' || c == ';' {
                break;
            }
            // Stop at output redirection operators
            if c == '>' || c == '|' {
                break;
            }

            let expr = self.parse_expression()?;
            args.push(expr);

            self.skip_whitespace();
            if self.pos < self.input.len() && self.current_char().unwrap() == ',' {
                self.pos += 1;
                self.skip_whitespace();
            } else {
                break;
            }
        }

        if args.is_empty() {
            args.push(AwkExpr::Field(Box::new(AwkExpr::Number(0.0))));
        }

        let target = self.parse_output_target()?;
        self.in_print_context = false;

        Ok(AwkAction::Print(args, target))
    }

    fn parse_printf(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        // Enable print context so `>` and `>>` are not parsed as comparison
        self.in_print_context = true;

        // Handle optional parenthesized form: printf("format", args)
        let has_parens = self.pos < self.input.len() && self.current_char().unwrap() == '(';
        if has_parens {
            self.pos += 1;
            self.skip_whitespace();
        }

        // Parse format string — accepts string literals or expressions
        if self.pos >= self.input.len() {
            self.in_print_context = false;
            return Err(Error::Execution(
                "awk: printf requires format string".to_string(),
            ));
        }

        let format_expr = if self.current_char().unwrap() == '"' {
            // String literal format — parse directly
            let s = self.parse_string()?;
            AwkExpr::String(s)
        } else {
            // Expression as format string (e.g., printf substr($1,1,1))
            self.parse_expression()?
        };
        let mut args = Vec::new();

        self.skip_whitespace();
        while self.pos < self.input.len() && self.current_char().unwrap() == ',' {
            self.pos += 1;
            self.skip_whitespace();
            let expr = self.parse_expression()?;
            args.push(expr);
            self.skip_whitespace();
        }

        if has_parens && self.pos < self.input.len() && self.current_char().unwrap() == ')' {
            self.pos += 1;
        }

        let target = self.parse_output_target()?;
        self.in_print_context = false;

        Ok(AwkAction::Printf(format_expr, args, target))
    }

    /// Parse optional output target after print/printf arguments: `> file`, `>> file`, `| cmd`.
    /// Pipe is unsupported and returns a clear error.
    fn parse_output_target(&mut self) -> Result<Option<AwkOutputTarget>> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Ok(None);
        }

        let c = self.current_char().unwrap();
        if c == '>' {
            self.pos += 1;
            // Check for >>
            let append = self.pos < self.input.len() && self.current_char().unwrap() == '>';
            if append {
                self.pos += 1;
            }
            self.skip_whitespace();
            // Temporarily disable print context to parse the target as a normal expression
            self.in_print_context = false;
            let target = self.parse_expression()?;
            self.in_print_context = true;
            if append {
                Ok(Some(AwkOutputTarget::Append(target)))
            } else {
                Ok(Some(AwkOutputTarget::Truncate(target)))
            }
        } else if c == '|' {
            // Pipe output (e.g., `print ... | "cmd"`) not supported in virtual mode
            Err(Error::Execution(
                "awk: pipe output redirection (|) is not supported".to_string(),
            ))
        } else {
            Ok(None)
        }
    }

    /// THREAT[TM-DOS-027]: Track depth for nested if/action blocks
    fn parse_if(&mut self) -> Result<AwkAction> {
        self.push_depth()?;

        self.skip_whitespace();

        if self.pos >= self.input.len() || self.current_char().unwrap() != '(' {
            self.pop_depth();
            return Err(Error::Execution("awk: expected '(' after if".to_string()));
        }
        self.pos += 1;

        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != ')' {
            self.pop_depth();
            return Err(Error::Execution(
                "awk: expected ')' after condition".to_string(),
            ));
        }
        self.pos += 1;

        self.skip_whitespace();
        let then_actions = if self.current_char().unwrap() == '{' {
            self.parse_action_block()?
        } else {
            vec![self.parse_action()?]
        };

        self.skip_whitespace();
        // Consume optional ';' before else
        if self.pos < self.input.len() && self.current_char().unwrap() == ';' {
            self.pos += 1;
            self.skip_whitespace();
        }
        let else_actions = if self.matches_keyword("else") {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.current_char().unwrap() == '{' {
                self.parse_action_block()?
            } else {
                vec![self.parse_action()?]
            }
        } else {
            Vec::new()
        };

        self.pop_depth();
        Ok(AwkAction::If(condition, then_actions, else_actions))
    }

    fn parse_for(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        if self.pos >= self.input.len() || self.current_char().unwrap() != '(' {
            return Err(Error::Execution("awk: expected '(' after for".to_string()));
        }
        self.pos += 1;
        self.skip_whitespace();

        // Check for `for (key in arr)` syntax
        let saved_pos = self.pos;
        if let Ok(AwkExpr::Variable(var_name)) = self.parse_primary() {
            self.skip_whitespace();
            if self.matches_keyword("in") {
                self.skip_whitespace();
                // Parse array name
                let start = self.pos;
                self.consume_while(Self::is_identifier_continue);
                let arr_name = self.input[start..self.pos].to_string();

                self.skip_whitespace();
                if self.pos >= self.input.len() || self.current_char().unwrap() != ')' {
                    return Err(Error::Execution("awk: expected ')' in for-in".to_string()));
                }
                self.pos += 1;

                self.skip_whitespace();
                let body = if self.pos < self.input.len() && self.current_char().unwrap() == '{' {
                    self.parse_action_block()?
                } else {
                    vec![self.parse_action()?]
                };

                return Ok(AwkAction::ForIn(var_name, arr_name, body));
            }
        }

        // Not for-in, backtrack and parse C-style for
        self.pos = saved_pos;

        // Parse init
        let init_expr = self.parse_expression()?;
        let init = match init_expr {
            AwkExpr::Assign(name, val) => AwkAction::Assign(name, *val),
            AwkExpr::ArrayAssign(name, key, val) => AwkAction::ArrayAssign(name, *key, *val),
            _ => AwkAction::Expression(init_expr),
        };

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != ';' {
            return Err(Error::Execution(
                "awk: expected ';' in for statement".to_string(),
            ));
        }
        self.pos += 1;

        // Parse condition
        self.skip_whitespace();
        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != ';' {
            return Err(Error::Execution(
                "awk: expected ';' in for statement".to_string(),
            ));
        }
        self.pos += 1;

        // Parse update
        self.skip_whitespace();
        let update_expr = self.parse_expression()?;
        let update = match update_expr {
            AwkExpr::Assign(name, val) => AwkAction::Assign(name, *val),
            AwkExpr::ArrayAssign(name, key, val) => AwkAction::ArrayAssign(name, *key, *val),
            _ => AwkAction::Expression(update_expr),
        };

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != ')' {
            return Err(Error::Execution(
                "awk: expected ')' in for statement".to_string(),
            ));
        }
        self.pos += 1;

        self.skip_whitespace();
        let body = if self.pos < self.input.len() && self.current_char().unwrap() == '{' {
            self.parse_action_block()?
        } else {
            vec![self.parse_action()?]
        };

        Ok(AwkAction::For(
            Box::new(init),
            condition,
            Box::new(update),
            body,
        ))
    }

    fn parse_while(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        if self.pos >= self.input.len() || self.current_char().unwrap() != '(' {
            return Err(Error::Execution(
                "awk: expected '(' after while".to_string(),
            ));
        }
        self.pos += 1;

        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != ')' {
            return Err(Error::Execution(
                "awk: expected ')' after while condition".to_string(),
            ));
        }
        self.pos += 1;

        self.skip_whitespace();
        let body = if self.pos < self.input.len() && self.current_char().unwrap() == '{' {
            self.parse_action_block()?
        } else {
            vec![self.parse_action()?]
        };

        Ok(AwkAction::While(condition, body))
    }

    fn parse_do_while(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        let body = if self.pos < self.input.len() && self.current_char().unwrap() == '{' {
            self.parse_action_block()?
        } else {
            vec![self.parse_action()?]
        };

        self.skip_whitespace();
        if !self.matches_keyword("while") {
            return Err(Error::Execution(
                "awk: expected 'while' after do body".to_string(),
            ));
        }

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != '(' {
            return Err(Error::Execution(
                "awk: expected '(' after do-while".to_string(),
            ));
        }
        self.pos += 1;

        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.current_char().unwrap() != ')' {
            return Err(Error::Execution(
                "awk: expected ')' in do-while".to_string(),
            ));
        }
        self.pos += 1;

        Ok(AwkAction::DoWhile(condition, body))
    }

    fn parse_delete(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        // Parse array name
        let start = self.pos;
        self.consume_while(Self::is_identifier_continue);
        let arr_name = self.input[start..self.pos].to_string();

        self.skip_whitespace();
        if self.pos < self.input.len() && self.current_char().unwrap() == '[' {
            self.pos += 1;
            let index = self.parse_expression()?;
            self.skip_whitespace();
            if self.pos >= self.input.len() || self.current_char().unwrap() != ']' {
                return Err(Error::Execution("awk: expected ']'".to_string()));
            }
            self.pos += 1;
            Ok(AwkAction::Delete(arr_name, index))
        } else {
            // delete entire array
            Ok(AwkAction::Delete(
                arr_name,
                AwkExpr::String("*".to_string()),
            ))
        }
    }

    /// Parse `getline [var] [< file]`.
    ///
    /// Forms:
    /// - `getline`           — read next input record into $0
    /// - `getline var`       — read next input record into var
    /// - `getline var < file` — read next line from file into var
    /// - `getline < file`    — read next line from file into $0
    fn parse_getline(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        // Check what follows: variable name, '<', or end of statement
        let mut var: Option<String> = None;

        if self.pos < self.input.len() {
            let c = self.current_char().unwrap();
            // If next char is an identifier start (not '<', ';', '}', etc.), parse variable
            if Self::is_identifier_start(c) {
                let start = self.pos;
                self.advance();
                self.consume_while(Self::is_identifier_continue);
                var = Some(self.input[start..self.pos].to_string());
                self.skip_whitespace();
            }
        }

        // Check for '< file' redirection
        if self.pos < self.input.len() && self.current_char().unwrap() == '<' {
            self.pos += 1; // consume '<'
            self.skip_whitespace();
            let file = self.parse_primary()?;
            return Ok(AwkAction::GetlineFile { var, file });
        }

        // Plain getline (with optional var — but plain `getline var` without
        // file redirection just reads next input line into var; for now we
        // treat that the same as plain getline which updates $0).
        // TODO: `getline var` should store into var without updating $0
        Ok(AwkAction::Getline)
    }

    /// Parse `getline [var] [< file]` as an expression returning 1/0/-1.
    fn parse_getline_expr(&mut self) -> Result<AwkExpr> {
        self.skip_whitespace();

        let mut var: Option<String> = None;

        if self.pos < self.input.len() {
            let c = self.current_char().unwrap();
            if Self::is_identifier_start(c) {
                let start = self.pos;
                self.advance();
                self.consume_while(Self::is_identifier_continue);
                var = Some(self.input[start..self.pos].to_string());
                self.skip_whitespace();
            }
        }

        // Check for '< file' redirection
        if self.pos < self.input.len() && self.current_char().unwrap() == '<' {
            self.pos += 1; // consume '<'
            self.skip_whitespace();
            let file = self.parse_primary()?;
            return Ok(AwkExpr::GetlineFile {
                var,
                file: Box::new(file),
            });
        }

        // Plain getline expression without file — use FuncCall to represent
        // TODO: support plain getline as expression (advance input line)
        Ok(AwkExpr::FuncCall("__getline".to_string(), vec![]))
    }

    /// THREAT[TM-DOS-027]: Track depth on every expression entry
    fn parse_expression(&mut self) -> Result<AwkExpr> {
        self.push_depth()?;
        let result = self.parse_assignment();
        self.pop_depth();
        result
    }

    fn parse_assignment(&mut self) -> Result<AwkExpr> {
        let expr = self.parse_ternary()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Ok(expr);
        }

        // Check for compound assignment operators (+=, -=, *=, /=, %=)
        let compound_ops = ["+=", "-=", "*=", "/=", "%="];
        for op in compound_ops {
            if self.input[self.pos..].starts_with(op) {
                self.pos += op.len();
                self.skip_whitespace();
                let value = self.parse_assignment()?;

                match &expr {
                    AwkExpr::Variable(name) => {
                        let bin_op = &op[..1];
                        let current = AwkExpr::Variable(name.clone());
                        let combined =
                            AwkExpr::BinOp(Box::new(current), bin_op.to_string(), Box::new(value));
                        return Ok(AwkExpr::Assign(name.clone(), Box::new(combined)));
                    }
                    AwkExpr::FuncCall(fname, args)
                        if fname == "__array_access" && args.len() == 2 =>
                    {
                        if let AwkExpr::Variable(arr_name) = &args[0] {
                            let bin_op = &op[..1];
                            return Ok(AwkExpr::CompoundArrayAssign(
                                arr_name.clone(),
                                Box::new(args[1].clone()),
                                bin_op.to_string(),
                                Box::new(value),
                            ));
                        }
                        return Err(Error::Execution(
                            "awk: invalid assignment target".to_string(),
                        ));
                    }
                    AwkExpr::Field(index) => {
                        let bin_op = &op[..1];
                        let current = AwkExpr::Field(index.clone());
                        let combined =
                            AwkExpr::BinOp(Box::new(current), bin_op.to_string(), Box::new(value));
                        return Ok(AwkExpr::FieldAssign(index.clone(), Box::new(combined)));
                    }
                    _ => {
                        return Err(Error::Execution(
                            "awk: invalid assignment target".to_string(),
                        ));
                    }
                }
            }
        }

        // Simple assignment
        if self.current_char().unwrap() == '=' {
            let next = self.input[self.pos..].chars().nth(1);
            if next != Some('=') && next != Some('~') {
                self.pos += 1;
                self.skip_whitespace();
                let value = self.parse_assignment()?;

                match expr {
                    AwkExpr::Variable(name) => {
                        return Ok(AwkExpr::Assign(name, Box::new(value)));
                    }
                    AwkExpr::FuncCall(ref fname, ref args)
                        if fname == "__array_access" && args.len() == 2 =>
                    {
                        if let AwkExpr::Variable(arr_name) = &args[0] {
                            return Ok(AwkExpr::ArrayAssign(
                                arr_name.clone(),
                                Box::new(args[1].clone()),
                                Box::new(value),
                            ));
                        }
                        return Err(Error::Execution(
                            "awk: invalid assignment target".to_string(),
                        ));
                    }
                    AwkExpr::Field(index) => {
                        return Ok(AwkExpr::FieldAssign(index, Box::new(value)));
                    }
                    _ => {
                        return Err(Error::Execution(
                            "awk: invalid assignment target".to_string(),
                        ));
                    }
                }
            }
        }

        Ok(expr)
    }

    fn parse_ternary(&mut self) -> Result<AwkExpr> {
        let expr = self.parse_or()?;

        self.skip_whitespace();
        if self.pos < self.input.len() && self.current_char().unwrap() == '?' {
            self.pos += 1;
            self.skip_whitespace();
            let then_expr = self.parse_expression()?;
            self.skip_whitespace();
            if self.pos < self.input.len() && self.current_char().unwrap() == ':' {
                self.pos += 1;
                self.skip_whitespace();
                let else_expr = self.parse_expression()?;
                // Encode ternary as a function call for evaluation
                return Ok(AwkExpr::FuncCall(
                    "__ternary".to_string(),
                    vec![expr, then_expr, else_expr],
                ));
            }
        }

        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<AwkExpr> {
        let mut left = self.parse_and()?;

        loop {
            self.skip_whitespace();
            if self.input[self.pos..].starts_with("||") {
                self.pos += 2;
                self.skip_whitespace();
                let right = self.parse_and()?;
                left = AwkExpr::BinOp(Box::new(left), "||".to_string(), Box::new(right));
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<AwkExpr> {
        let mut left = self.parse_comparison()?;

        loop {
            self.skip_whitespace();
            if self.input[self.pos..].starts_with("&&") {
                self.pos += 2;
                self.skip_whitespace();
                let right = self.parse_comparison()?;
                left = AwkExpr::BinOp(Box::new(left), "&&".to_string(), Box::new(right));
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<AwkExpr> {
        let left = self.parse_concat()?;

        self.skip_whitespace();

        // Check for `in` operator: (key in arr)
        if self.matches_keyword("in") {
            self.skip_whitespace();
            let start = self.pos;
            self.consume_while(Self::is_identifier_continue);
            let arr_name = self.input[start..self.pos].to_string();
            return Ok(AwkExpr::InArray(Box::new(left), arr_name));
        }

        // In print context, `>` and `>>` are output redirection, not comparison.
        // `>=` remains a comparison operator even in print context.
        let ops: &[&str] = if self.in_print_context {
            &["==", "!=", "<=", ">=", "<", "~", "!~"]
        } else {
            &["==", "!=", "<=", ">=", "<", ">", "~", "!~"]
        };

        for op in ops {
            if self.input[self.pos..].starts_with(op) {
                self.pos += op.len();
                self.skip_whitespace();
                let right = self.parse_concat()?;
                return Ok(AwkExpr::BinOp(
                    Box::new(left),
                    op.to_string(),
                    Box::new(right),
                ));
            }
        }

        Ok(left)
    }

    fn is_keyword_at_pos(&self) -> bool {
        let remaining = &self.input[self.pos..];
        let keywords = [
            "in", "if", "else", "while", "for", "do", "break", "continue", "next", "exit",
            "return", "delete", "getline", "print", "printf", "function",
        ];
        for kw in keywords {
            if remaining.starts_with(kw) {
                let after = self.pos + kw.len();
                if after >= self.input.len() || {
                    let c = self.input[after..].chars().next().unwrap();
                    !c.is_alphanumeric() && c != '_'
                } {
                    return true;
                }
            }
        }
        false
    }

    fn parse_concat(&mut self) -> Result<AwkExpr> {
        let mut parts = vec![self.parse_additive()?];

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            let c = self.current_char().unwrap();
            // Check if this could be the start of another value for concatenation
            if c == '"' || c == '$' || Self::is_identifier_start(c) || c == '(' {
                // But not if it's a keyword or operator
                let remaining = &self.input[self.pos..];
                if !remaining.starts_with("||")
                    && !remaining.starts_with("&&")
                    && !remaining.starts_with("==")
                    && !remaining.starts_with("!=")
                    && !self.is_keyword_at_pos()
                    && let Ok(next) = self.parse_additive()
                {
                    parts.push(next);
                    continue;
                }
            }
            break;
        }

        if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(AwkExpr::Concat(parts))
        }
    }

    fn parse_additive(&mut self) -> Result<AwkExpr> {
        let mut left = self.parse_multiplicative()?;

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            let c = self.current_char().unwrap();
            if c == '+' || c == '-' {
                // Don't consume if it's a compound assignment operator (+=, -=)
                let next = self.input[self.pos..].chars().nth(1);
                if next == Some('=') {
                    break;
                }
                self.pos += 1;
                self.skip_whitespace();
                let right = self.parse_multiplicative()?;
                left = AwkExpr::BinOp(Box::new(left), c.to_string(), Box::new(right));
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<AwkExpr> {
        let mut left = self.parse_power()?;

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            let c = self.current_char().unwrap();
            if c == '*' || c == '/' || c == '%' {
                // Don't consume ** (power operator)
                if c == '*' && self.input[self.pos..].chars().nth(1) == Some('*') {
                    break;
                }
                // Don't consume if it's a compound assignment operator (*=, /=, %=)
                let next = self.input[self.pos..].chars().nth(1);
                if next == Some('=') {
                    break;
                }
                self.pos += 1;
                self.skip_whitespace();
                let right = self.parse_power()?;
                left = AwkExpr::BinOp(Box::new(left), c.to_string(), Box::new(right));
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_power(&mut self) -> Result<AwkExpr> {
        let base = self.parse_unary()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Ok(base);
        }

        // Check for ^ or **
        if self.current_char().unwrap() == '^' {
            self.pos += 1;
            self.skip_whitespace();
            let exp = self.parse_unary()?;
            return Ok(AwkExpr::BinOp(
                Box::new(base),
                "^".to_string(),
                Box::new(exp),
            ));
        }
        if self.input[self.pos..].starts_with("**") {
            self.pos += 2;
            self.skip_whitespace();
            let exp = self.parse_unary()?;
            return Ok(AwkExpr::BinOp(
                Box::new(base),
                "^".to_string(),
                Box::new(exp),
            ));
        }

        Ok(base)
    }

    /// THREAT[TM-DOS-027]: Track depth on unary self-recursion
    fn parse_unary(&mut self) -> Result<AwkExpr> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Err(Error::Execution(
                "awk: unexpected end of expression".to_string(),
            ));
        }

        // Pre-increment: ++var or ++arr[key]
        if self.input[self.pos..].starts_with("++") {
            self.pos += 2;
            self.skip_whitespace();
            match self.parse_primary()? {
                AwkExpr::Variable(name) => return Ok(AwkExpr::PreIncrement(name)),
                AwkExpr::FuncCall(ref fname, ref args)
                    if fname == "__array_access" && args.len() == 2 =>
                {
                    if let AwkExpr::Variable(arr_name) = &args[0] {
                        return Ok(AwkExpr::CompoundArrayAssign(
                            arr_name.clone(),
                            Box::new(args[1].clone()),
                            "+".to_string(),
                            Box::new(AwkExpr::Number(1.0)),
                        ));
                    }
                    return Err(Error::Execution(
                        "awk: expected variable after ++".to_string(),
                    ));
                }
                _ => {
                    return Err(Error::Execution(
                        "awk: expected variable after ++".to_string(),
                    ));
                }
            }
        }

        // Pre-decrement: --var or --arr[key]
        if self.input[self.pos..].starts_with("--") {
            self.pos += 2;
            self.skip_whitespace();
            match self.parse_primary()? {
                AwkExpr::Variable(name) => return Ok(AwkExpr::PreDecrement(name)),
                AwkExpr::FuncCall(ref fname, ref args)
                    if fname == "__array_access" && args.len() == 2 =>
                {
                    if let AwkExpr::Variable(arr_name) = &args[0] {
                        return Ok(AwkExpr::CompoundArrayAssign(
                            arr_name.clone(),
                            Box::new(args[1].clone()),
                            "-".to_string(),
                            Box::new(AwkExpr::Number(1.0)),
                        ));
                    }
                    return Err(Error::Execution(
                        "awk: expected variable after --".to_string(),
                    ));
                }
                _ => {
                    return Err(Error::Execution(
                        "awk: expected variable after --".to_string(),
                    ));
                }
            }
        }

        let c = self.current_char().unwrap();

        if c == '-' {
            self.pos += 1;
            self.push_depth()?;
            let expr = self.parse_unary();
            self.pop_depth();
            return Ok(AwkExpr::UnaryOp("-".to_string(), Box::new(expr?)));
        }

        if c == '!' {
            self.pos += 1;
            self.push_depth()?;
            let expr = self.parse_unary();
            self.pop_depth();
            return Ok(AwkExpr::UnaryOp("!".to_string(), Box::new(expr?)));
        }

        if c == '+' {
            self.pos += 1;
            self.push_depth()?;
            let result = self.parse_unary();
            self.pop_depth();
            return result;
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<AwkExpr> {
        let expr = self.parse_primary()?;

        // Check for postfix ++ / --
        if self.pos + 1 < self.input.len() {
            if self.input[self.pos..].starts_with("++") {
                match &expr {
                    AwkExpr::Variable(name) => {
                        self.pos += 2;
                        return Ok(AwkExpr::PostIncrement(name.clone()));
                    }
                    AwkExpr::FuncCall(fname, args)
                        if fname == "__array_access" && args.len() == 2 =>
                    {
                        // arr[key]++ → compound array assign with +1
                        if let AwkExpr::Variable(arr_name) = &args[0] {
                            self.pos += 2;
                            return Ok(AwkExpr::CompoundArrayAssign(
                                arr_name.clone(),
                                Box::new(args[1].clone()),
                                "+".to_string(),
                                Box::new(AwkExpr::Number(1.0)),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            if self.input[self.pos..].starts_with("--") {
                match &expr {
                    AwkExpr::Variable(name) => {
                        self.pos += 2;
                        return Ok(AwkExpr::PostDecrement(name.clone()));
                    }
                    AwkExpr::FuncCall(fname, args)
                        if fname == "__array_access" && args.len() == 2 =>
                    {
                        if let AwkExpr::Variable(arr_name) = &args[0] {
                            self.pos += 2;
                            return Ok(AwkExpr::CompoundArrayAssign(
                                arr_name.clone(),
                                Box::new(args[1].clone()),
                                "-".to_string(),
                                Box::new(AwkExpr::Number(1.0)),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<AwkExpr> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Err(Error::Execution(
                "awk: unexpected end of expression".to_string(),
            ));
        }

        let c = self.current_char().unwrap();

        // Field reference $
        if c == '$' {
            self.pos += 1;
            self.push_depth()?;
            let index = self.parse_primary()?;
            self.pop_depth();
            return Ok(AwkExpr::Field(Box::new(index)));
        }

        // Number
        if c.is_ascii_digit() || c == '.' {
            return self.parse_number();
        }

        // String
        if c == '"' {
            let s = self.parse_string()?;
            return Ok(AwkExpr::String(s));
        }

        // Regex literal /pattern/
        if c == '/' {
            self.pos += 1;
            let start = self.pos;
            while self.pos < self.input.len() {
                let c = self.current_char().unwrap();
                if c == '/' {
                    let pattern = &self.input[start..self.pos];
                    self.pos += 1;
                    return Ok(AwkExpr::Regex(pattern.to_string()));
                } else if c == '\\' {
                    self.pos += 1; // skip '\\' (ASCII)
                    self.advance(); // skip next char (may be multi-byte)
                } else {
                    self.advance(); // regex content may be multi-byte
                }
            }
            return Err(Error::Execution("awk: unterminated regex".to_string()));
        }

        // Parenthesized expression: `>` inside parens is comparison, not redirection
        if c == '(' {
            self.pos += 1;
            self.push_depth()?;
            let saved_print_ctx = self.in_print_context;
            self.in_print_context = false;
            let expr = self.parse_expression()?;
            self.in_print_context = saved_print_ctx;
            self.pop_depth();
            self.skip_whitespace();
            if self.pos >= self.input.len() || self.current_char().unwrap() != ')' {
                return Err(Error::Execution("awk: expected ')'".to_string()));
            }
            self.pos += 1;
            return Ok(expr);
        }

        // Variable or function call
        if Self::is_identifier_start(c) {
            let start = self.pos;
            self.advance();
            self.consume_while(Self::is_identifier_continue);
            let name = self.input[start..self.pos].to_string();

            // getline [var] [< file] as expression (returns 1/0/-1)
            if name == "getline" {
                return self.parse_getline_expr();
            }

            self.skip_whitespace();
            if self.pos < self.input.len() && self.current_char().unwrap() == '(' {
                // Function call
                self.pos += 1;
                let mut args = Vec::new();
                loop {
                    self.skip_whitespace();
                    if self.pos < self.input.len() && self.current_char().unwrap() == ')' {
                        self.pos += 1;
                        break;
                    }
                    let arg = self.parse_expression()?;
                    args.push(arg);
                    self.skip_whitespace();
                    if self.pos < self.input.len() && self.current_char().unwrap() == ',' {
                        self.pos += 1;
                    }
                }
                return Ok(AwkExpr::FuncCall(name, args));
            }

            // Array indexing: arr[index] or arr[e1,e2,...] (multi-subscript with SUBSEP)
            if self.pos < self.input.len() && self.current_char().unwrap() == '[' {
                self.pos += 1; // consume '['
                let mut subscripts = vec![self.parse_expression()?];
                self.skip_whitespace();
                // THREAT[TM-DOS-027]: SUBSEP_CONCAT still evaluates recursively, so cap
                // attacker-controlled comma lists before folding them into a left-deep AST.
                while self.pos < self.input.len() && self.current_char().unwrap() == ',' {
                    if subscripts.len() >= MAX_AWK_MULTI_SUBSCRIPTS {
                        return Err(Error::Execution(format!(
                            "awk: too many array subscripts (max {})",
                            MAX_AWK_MULTI_SUBSCRIPTS
                        )));
                    }
                    self.pos += 1; // consume ','
                    self.skip_whitespace();
                    subscripts.push(self.parse_expression()?);
                    self.skip_whitespace();
                }
                if self.pos >= self.input.len() || self.current_char().unwrap() != ']' {
                    return Err(Error::Execution("awk: expected ']'".to_string()));
                }
                self.pos += 1; // consume ']'
                let index_expr = if subscripts.len() == 1 {
                    subscripts.remove(0)
                } else {
                    // Join multiple subscripts with SUBSEP
                    let mut result = subscripts.remove(0);
                    for sub in subscripts {
                        result = AwkExpr::BinOp(
                            Box::new(result),
                            "SUBSEP_CONCAT".to_string(),
                            Box::new(sub),
                        );
                    }
                    result
                };
                return Ok(AwkExpr::FuncCall(
                    "__array_access".to_string(),
                    vec![AwkExpr::Variable(name), index_expr],
                ));
            }

            return Ok(AwkExpr::Variable(name));
        }

        Err(Error::Execution(format!(
            "awk: unexpected character: {}",
            c
        )))
    }

    fn parse_number(&mut self) -> Result<AwkExpr> {
        let start = self.pos;
        while self.pos < self.input.len() {
            let c = self.current_char().unwrap();
            if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '-' || c == '+' {
                self.pos += 1;
            } else {
                break;
            }
        }

        let num_str = &self.input[start..self.pos];
        let num: f64 = num_str
            .parse()
            .map_err(|_| Error::Execution(format!("awk: invalid number: {}", num_str)))?;

        Ok(AwkExpr::Number(num))
    }

    fn parse_string(&mut self) -> Result<String> {
        if self.pos >= self.input.len() || self.current_char().unwrap() != '"' {
            return Err(Error::Execution("awk: expected string".to_string()));
        }
        self.pos += 1; // skip opening '"' (ASCII)

        let mut result = String::new();
        while self.pos < self.input.len() {
            let c = self.current_char().unwrap();
            if c == '"' {
                self.pos += 1; // skip closing '"' (ASCII)
                return Ok(result);
            } else if c == '\\' {
                self.pos += 1; // skip '\\' (ASCII)
                if self.pos < self.input.len() {
                    let escaped = self.current_char().unwrap();
                    match escaped {
                        'n' => {
                            result.push('\n');
                            self.advance();
                        }
                        't' => {
                            result.push('\t');
                            self.advance();
                        }
                        'r' => {
                            result.push('\r');
                            self.advance();
                        }
                        '\\' => {
                            result.push('\\');
                            self.advance();
                        }
                        '"' => {
                            result.push('"');
                            self.advance();
                        }
                        'u' => {
                            // gawk 5.3+ Unicode escape: \u followed by 1-8 hex digits
                            self.advance(); // skip 'u'
                            let mut hex = String::new();
                            while hex.len() < 8
                                && self.pos < self.input.len()
                                && self.current_char().is_some_and(|c| c.is_ascii_hexdigit())
                            {
                                hex.push(self.current_char().unwrap());
                                self.pos += 1; // hex digits are ASCII
                            }
                            if hex.is_empty() {
                                result.push('\\');
                                result.push('u');
                            } else if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                                if let Some(ch) = char::from_u32(cp) {
                                    result.push(ch);
                                } else {
                                    // Invalid Unicode code point — keep literal
                                    result.push('\\');
                                    result.push('u');
                                    result.push_str(&hex);
                                }
                            } else {
                                result.push('\\');
                                result.push('u');
                                result.push_str(&hex);
                            }
                        }
                        _ => {
                            result.push('\\');
                            result.push(escaped);
                            self.advance();
                        }
                    }
                }
            } else {
                result.push(c);
                self.advance(); // character may be multi-byte
            }
        }

        Err(Error::Execution("awk: unterminated string".to_string()))
    }
}
