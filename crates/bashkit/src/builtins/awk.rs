//! awk - Pattern scanning and processing builtin
//!
//! Implements basic AWK functionality.
//!
//! Usage:
//!   awk '{print $1}' file
//!   awk -F: '{print $1}' /etc/passwd
//!   echo "a b c" | awk '{print $2}'
//!   awk 'BEGIN{print "start"} {print} END{print "end"}' file
//!   awk '/pattern/{print}' file
//!   awk 'NR==2{print}' file

// AWK parser uses chars().nth().unwrap() after validating position.
// This is safe because we check bounds before accessing.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;

use super::{Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// awk command - pattern scanning and processing
pub struct Awk;

#[derive(Debug)]
struct AwkProgram {
    begin_actions: Vec<AwkAction>,
    main_rules: Vec<AwkRule>,
    end_actions: Vec<AwkAction>,
}

#[derive(Debug)]
struct AwkRule {
    pattern: Option<AwkPattern>,
    actions: Vec<AwkAction>,
}

#[derive(Debug)]
enum AwkPattern {
    Regex(Regex),
    Expression(AwkExpr),
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Regex and Match used for pattern matching expansion
enum AwkExpr {
    Number(f64),
    String(String),
    Field(Box<AwkExpr>), // $n
    Variable(String),    // var
    BinOp(Box<AwkExpr>, String, Box<AwkExpr>),
    UnaryOp(String, Box<AwkExpr>),
    Assign(String, Box<AwkExpr>),
    Concat(Vec<AwkExpr>),
    FuncCall(String, Vec<AwkExpr>),
    Regex(String),
    Match(Box<AwkExpr>, String), // expr ~ /pattern/
}

#[allow(dead_code)] // While and For for future expansion
#[derive(Debug)]
enum AwkAction {
    Print(Vec<AwkExpr>),
    Printf(String, Vec<AwkExpr>),
    Assign(String, AwkExpr),
    If(AwkExpr, Vec<AwkAction>, Vec<AwkAction>),
    While(AwkExpr, Vec<AwkAction>),
    For(Box<AwkAction>, AwkExpr, Box<AwkAction>, Vec<AwkAction>),
    Next,
    #[allow(dead_code)] // Exit code support for future
    Exit(Option<AwkExpr>),
    Expression(AwkExpr),
}

struct AwkState {
    variables: HashMap<String, AwkValue>,
    fields: Vec<String>,
    fs: String,
    ofs: String,
    ors: String,
    nr: usize,
    nf: usize,
    fnr: usize,
}

#[derive(Debug, Clone)]
enum AwkValue {
    Number(f64),
    String(String),
    Uninitialized,
}

impl AwkValue {
    fn as_number(&self) -> f64 {
        match self {
            AwkValue::Number(n) => *n,
            AwkValue::String(s) => s.parse().unwrap_or(0.0),
            AwkValue::Uninitialized => 0.0,
        }
    }

    fn as_string(&self) -> String {
        match self {
            AwkValue::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            AwkValue::String(s) => s.clone(),
            AwkValue::Uninitialized => String::new(),
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            AwkValue::Number(n) => *n != 0.0,
            AwkValue::String(s) => !s.is_empty(),
            AwkValue::Uninitialized => false,
        }
    }
}

impl Default for AwkState {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
            fields: Vec::new(),
            fs: " ".to_string(),
            ofs: " ".to_string(),
            ors: "\n".to_string(),
            nr: 0,
            nf: 0,
            fnr: 0,
        }
    }
}

impl AwkState {
    fn set_line(&mut self, line: &str) {
        self.nr += 1;
        self.fnr += 1;

        // Split by field separator
        if self.fs == " " {
            // Special: split on whitespace, collapse multiple spaces
            self.fields = line.split_whitespace().map(String::from).collect();
        } else {
            self.fields = line.split(&self.fs).map(String::from).collect();
        }

        self.nf = self.fields.len();

        // Set built-in variables
        self.variables
            .insert("NR".to_string(), AwkValue::Number(self.nr as f64));
        self.variables
            .insert("NF".to_string(), AwkValue::Number(self.nf as f64));
        self.variables
            .insert("FNR".to_string(), AwkValue::Number(self.fnr as f64));
        self.variables
            .insert("$0".to_string(), AwkValue::String(line.to_string()));
    }

    fn get_field(&self, n: usize) -> AwkValue {
        if n == 0 {
            // $0 is the whole line
            self.variables
                .get("$0")
                .cloned()
                .unwrap_or(AwkValue::Uninitialized)
        } else if n <= self.fields.len() {
            AwkValue::String(self.fields[n - 1].clone())
        } else {
            AwkValue::Uninitialized
        }
    }

    fn get_variable(&self, name: &str) -> AwkValue {
        match name {
            "NR" => AwkValue::Number(self.nr as f64),
            "NF" => AwkValue::Number(self.nf as f64),
            "FNR" => AwkValue::Number(self.fnr as f64),
            "FS" => AwkValue::String(self.fs.clone()),
            "OFS" => AwkValue::String(self.ofs.clone()),
            "ORS" => AwkValue::String(self.ors.clone()),
            _ => self
                .variables
                .get(name)
                .cloned()
                .unwrap_or(AwkValue::Uninitialized),
        }
    }

    fn set_variable(&mut self, name: &str, value: AwkValue) {
        match name {
            "FS" => self.fs = value.as_string(),
            "OFS" => self.ofs = value.as_string(),
            "ORS" => self.ors = value.as_string(),
            _ => {
                self.variables.insert(name.to_string(), value);
            }
        }
    }
}

/// THREAT[TM-DOS-027]: Maximum recursion depth for awk expression parser.
/// Prevents stack overflow from deeply nested expressions like `(((((...)))))`
/// or deeply chained unary operators like `- - - - - x`.
/// Set conservatively: each recursion level uses ~1-2KB stack in debug mode.
/// 100 levels × ~2KB = ~200KB, well within typical stack limits.
const MAX_AWK_PARSER_DEPTH: usize = 100;

struct AwkParser<'a> {
    input: &'a str,
    pos: usize,
    /// Current recursion depth for expression parsing
    depth: usize,
}

impl<'a> AwkParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            depth: 0,
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

    fn parse(&mut self) -> Result<AwkProgram> {
        let mut program = AwkProgram {
            begin_actions: Vec::new(),
            main_rules: Vec::new(),
            end_actions: Vec::new(),
        };

        self.skip_whitespace();

        while self.pos < self.input.len() {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            // Check for BEGIN/END
            if self.matches_keyword("BEGIN") {
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
                actions: vec![AwkAction::Print(vec![AwkExpr::Field(Box::new(
                    AwkExpr::Number(0.0),
                ))])],
            });
        }

        Ok(program)
    }

    fn matches_keyword(&mut self, keyword: &str) -> bool {
        if self.input[self.pos..].starts_with(keyword) {
            let after = self.pos + keyword.len();
            if after >= self.input.len()
                || !self.input.chars().nth(after).unwrap().is_alphanumeric()
            {
                self.pos = after;
                return true;
            }
        }
        false
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            let c = self.input.chars().nth(self.pos).unwrap();
            if c.is_whitespace() {
                self.pos += 1;
            } else if c == '#' {
                // Comment - skip to end of line
                while self.pos < self.input.len()
                    && self.input.chars().nth(self.pos).unwrap() != '\n'
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn parse_rule(&mut self) -> Result<AwkRule> {
        let pattern = self.parse_pattern()?;
        self.skip_whitespace();

        let actions =
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '{' {
                self.parse_action_block()?
            } else if pattern.is_some() {
                // Default action is print
                vec![AwkAction::Print(vec![AwkExpr::Field(Box::new(
                    AwkExpr::Number(0.0),
                ))])]
            } else {
                Vec::new()
            };

        Ok(AwkRule { pattern, actions })
    }

    fn parse_pattern(&mut self) -> Result<Option<AwkPattern>> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Ok(None);
        }

        let c = self.input.chars().nth(self.pos).unwrap();

        // Check for regex pattern
        if c == '/' {
            self.pos += 1;
            let start = self.pos;
            while self.pos < self.input.len() {
                let c = self.input.chars().nth(self.pos).unwrap();
                if c == '/' {
                    let pattern = &self.input[start..self.pos];
                    self.pos += 1;
                    let regex = Regex::new(pattern)
                        .map_err(|e| Error::Execution(format!("awk: invalid regex: {}", e)))?;
                    return Ok(Some(AwkPattern::Regex(regex)));
                } else if c == '\\' {
                    self.pos += 2;
                } else {
                    self.pos += 1;
                }
            }
            return Err(Error::Execution("awk: unterminated regex".to_string()));
        }

        // Check for opening brace (no pattern)
        if c == '{' {
            return Ok(None);
        }

        // Expression pattern
        let expr = self.parse_expression()?;
        Ok(Some(AwkPattern::Expression(expr)))
    }

    fn parse_action_block(&mut self) -> Result<Vec<AwkAction>> {
        self.skip_whitespace();

        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != '{' {
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

            let c = self.input.chars().nth(self.pos).unwrap();
            if c == '}' {
                self.pos += 1;
                break;
            }

            let action = self.parse_action()?;
            actions.push(action);

            self.skip_whitespace();
            // Allow semicolon separator
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == ';' {
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
        if self.matches_keyword("exit") {
            self.skip_whitespace();
            if self.pos < self.input.len() {
                let c = self.input.chars().nth(self.pos).unwrap();
                if c != '}' && c != ';' {
                    let expr = self.parse_expression()?;
                    return Ok(AwkAction::Exit(Some(expr)));
                }
            }
            return Ok(AwkAction::Exit(None));
        }
        if self.matches_keyword("if") {
            return self.parse_if();
        }

        // Otherwise it's an expression (including assignment)
        let expr = self.parse_expression()?;

        // Check if it's an assignment
        if let AwkExpr::Assign(name, val) = expr {
            Ok(AwkAction::Assign(name, *val))
        } else {
            Ok(AwkAction::Expression(expr))
        }
    }

    fn parse_print(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();
        let mut args = Vec::new();

        loop {
            if self.pos >= self.input.len() {
                break;
            }
            let c = self.input.chars().nth(self.pos).unwrap();
            if c == '}' || c == ';' {
                break;
            }

            let expr = self.parse_expression()?;
            args.push(expr);

            self.skip_whitespace();
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == ',' {
                self.pos += 1;
                self.skip_whitespace();
            } else {
                break;
            }
        }

        if args.is_empty() {
            args.push(AwkExpr::Field(Box::new(AwkExpr::Number(0.0))));
        }

        Ok(AwkAction::Print(args))
    }

    fn parse_printf(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        // Parse format string
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != '"' {
            return Err(Error::Execution(
                "awk: printf requires format string".to_string(),
            ));
        }

        let format = self.parse_string()?;
        let mut args = Vec::new();

        self.skip_whitespace();
        while self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == ',' {
            self.pos += 1;
            self.skip_whitespace();
            let expr = self.parse_expression()?;
            args.push(expr);
            self.skip_whitespace();
        }

        Ok(AwkAction::Printf(format, args))
    }

    /// THREAT[TM-DOS-027]: Track depth for nested if/action blocks
    fn parse_if(&mut self) -> Result<AwkAction> {
        self.push_depth()?;

        self.skip_whitespace();

        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != '(' {
            self.pop_depth();
            return Err(Error::Execution("awk: expected '(' after if".to_string()));
        }
        self.pos += 1;

        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ')' {
            self.pop_depth();
            return Err(Error::Execution(
                "awk: expected ')' after condition".to_string(),
            ));
        }
        self.pos += 1;

        self.skip_whitespace();
        let then_actions = if self.input.chars().nth(self.pos).unwrap() == '{' {
            self.parse_action_block()?
        } else {
            vec![self.parse_action()?]
        };

        self.skip_whitespace();
        let else_actions = if self.matches_keyword("else") {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '{' {
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

                if let AwkExpr::Variable(name) = expr {
                    // Transform `x += y` into `x = x + y`
                    let bin_op = &op[..1]; // Get the operator without '='
                    let current = AwkExpr::Variable(name.clone());
                    let combined =
                        AwkExpr::BinOp(Box::new(current), bin_op.to_string(), Box::new(value));
                    return Ok(AwkExpr::Assign(name, Box::new(combined)));
                }
                return Err(Error::Execution(
                    "awk: invalid assignment target".to_string(),
                ));
            }
        }

        // Simple assignment
        if self.input.chars().nth(self.pos).unwrap() == '=' {
            let next = self.input.chars().nth(self.pos + 1);
            if next != Some('=') && next != Some('~') {
                self.pos += 1;
                self.skip_whitespace();
                let value = self.parse_assignment()?;

                if let AwkExpr::Variable(name) = expr {
                    return Ok(AwkExpr::Assign(name, Box::new(value)));
                }
                return Err(Error::Execution(
                    "awk: invalid assignment target".to_string(),
                ));
            }
        }

        Ok(expr)
    }

    fn parse_ternary(&mut self) -> Result<AwkExpr> {
        self.parse_or()
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
        let ops = ["==", "!=", "<=", ">=", "<", ">", "~", "!~"];

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

    fn parse_concat(&mut self) -> Result<AwkExpr> {
        let mut parts = vec![self.parse_additive()?];

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            let c = self.input.chars().nth(self.pos).unwrap();
            // Check if this could be the start of another value for concatenation
            if c == '"' || c == '$' || c.is_alphabetic() || c == '(' {
                // But not if it's a keyword or operator
                let remaining = &self.input[self.pos..];
                if !remaining.starts_with("||")
                    && !remaining.starts_with("&&")
                    && !remaining.starts_with("==")
                    && !remaining.starts_with("!=")
                {
                    if let Ok(next) = self.parse_additive() {
                        parts.push(next);
                        continue;
                    }
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

            let c = self.input.chars().nth(self.pos).unwrap();
            if c == '+' || c == '-' {
                // Don't consume if it's a compound assignment operator (+=, -=)
                let next = self.input.chars().nth(self.pos + 1);
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
        let mut left = self.parse_unary()?;

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            let c = self.input.chars().nth(self.pos).unwrap();
            if c == '*' || c == '/' || c == '%' {
                // Don't consume if it's a compound assignment operator (*=, /=, %=)
                let next = self.input.chars().nth(self.pos + 1);
                if next == Some('=') {
                    break;
                }
                self.pos += 1;
                self.skip_whitespace();
                let right = self.parse_unary()?;
                left = AwkExpr::BinOp(Box::new(left), c.to_string(), Box::new(right));
            } else {
                break;
            }
        }

        Ok(left)
    }

    /// THREAT[TM-DOS-027]: Track depth on unary self-recursion
    fn parse_unary(&mut self) -> Result<AwkExpr> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Err(Error::Execution(
                "awk: unexpected end of expression".to_string(),
            ));
        }

        let c = self.input.chars().nth(self.pos).unwrap();

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

        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<AwkExpr> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Err(Error::Execution(
                "awk: unexpected end of expression".to_string(),
            ));
        }

        let c = self.input.chars().nth(self.pos).unwrap();

        // Field reference $
        if c == '$' {
            self.pos += 1;
            let index = self.parse_primary()?;
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
                let c = self.input.chars().nth(self.pos).unwrap();
                if c == '/' {
                    let pattern = &self.input[start..self.pos];
                    self.pos += 1;
                    return Ok(AwkExpr::Regex(pattern.to_string()));
                } else if c == '\\' {
                    self.pos += 2; // Skip escape sequence
                } else {
                    self.pos += 1;
                }
            }
            return Err(Error::Execution("awk: unterminated regex".to_string()));
        }

        // Parenthesized expression
        if c == '(' {
            self.pos += 1;
            let expr = self.parse_expression()?;
            self.skip_whitespace();
            if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ')' {
                return Err(Error::Execution("awk: expected ')'".to_string()));
            }
            self.pos += 1;
            return Ok(expr);
        }

        // Variable or function call
        if c.is_alphabetic() || c == '_' {
            let start = self.pos;
            while self.pos < self.input.len() {
                let c = self.input.chars().nth(self.pos).unwrap();
                if c.is_alphanumeric() || c == '_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let name = self.input[start..self.pos].to_string();

            self.skip_whitespace();
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '(' {
                // Function call
                self.pos += 1;
                let mut args = Vec::new();
                loop {
                    self.skip_whitespace();
                    if self.pos < self.input.len()
                        && self.input.chars().nth(self.pos).unwrap() == ')'
                    {
                        self.pos += 1;
                        break;
                    }
                    let arg = self.parse_expression()?;
                    args.push(arg);
                    self.skip_whitespace();
                    if self.pos < self.input.len()
                        && self.input.chars().nth(self.pos).unwrap() == ','
                    {
                        self.pos += 1;
                    }
                }
                return Ok(AwkExpr::FuncCall(name, args));
            }

            // Array indexing: arr[index]
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '[' {
                self.pos += 1; // consume '['
                let index_expr = self.parse_expression()?;
                self.skip_whitespace();
                if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ']'
                {
                    return Err(Error::Execution("awk: expected ']'".to_string()));
                }
                self.pos += 1; // consume ']'
                               // Store as arr[index] where index is evaluated at runtime
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
            let c = self.input.chars().nth(self.pos).unwrap();
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
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != '"' {
            return Err(Error::Execution("awk: expected string".to_string()));
        }
        self.pos += 1;

        let mut result = String::new();
        while self.pos < self.input.len() {
            let c = self.input.chars().nth(self.pos).unwrap();
            if c == '"' {
                self.pos += 1;
                return Ok(result);
            } else if c == '\\' {
                self.pos += 1;
                if self.pos < self.input.len() {
                    let escaped = self.input.chars().nth(self.pos).unwrap();
                    match escaped {
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        'r' => result.push('\r'),
                        '\\' => result.push('\\'),
                        '"' => result.push('"'),
                        _ => {
                            result.push('\\');
                            result.push(escaped);
                        }
                    }
                    self.pos += 1;
                }
            } else {
                result.push(c);
                self.pos += 1;
            }
        }

        Err(Error::Execution("awk: unterminated string".to_string()))
    }
}

struct AwkInterpreter {
    state: AwkState,
    output: String,
}

impl AwkInterpreter {
    fn new() -> Self {
        Self {
            state: AwkState::default(),
            output: String::new(),
        }
    }

    fn eval_expr(&mut self, expr: &AwkExpr) -> AwkValue {
        match expr {
            AwkExpr::Number(n) => AwkValue::Number(*n),
            AwkExpr::String(s) => AwkValue::String(s.clone()),
            AwkExpr::Field(index) => {
                let n = self.eval_expr(index).as_number() as usize;
                self.state.get_field(n)
            }
            AwkExpr::Variable(name) => self.state.get_variable(name),
            AwkExpr::Assign(name, val) => {
                let value = self.eval_expr(val);
                self.state.set_variable(name, value.clone());
                value
            }
            AwkExpr::BinOp(left, op, right) => {
                let l = self.eval_expr(left);
                let r = self.eval_expr(right);

                match op.as_str() {
                    "+" => AwkValue::Number(l.as_number() + r.as_number()),
                    "-" => AwkValue::Number(l.as_number() - r.as_number()),
                    "*" => AwkValue::Number(l.as_number() * r.as_number()),
                    "/" => AwkValue::Number(l.as_number() / r.as_number()),
                    "%" => AwkValue::Number(l.as_number() % r.as_number()),
                    "==" => AwkValue::Number(if l.as_string() == r.as_string() {
                        1.0
                    } else {
                        0.0
                    }),
                    "!=" => AwkValue::Number(if l.as_string() != r.as_string() {
                        1.0
                    } else {
                        0.0
                    }),
                    "<" => AwkValue::Number(if l.as_number() < r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    ">" => AwkValue::Number(if l.as_number() > r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    "<=" => AwkValue::Number(if l.as_number() <= r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    ">=" => AwkValue::Number(if l.as_number() >= r.as_number() {
                        1.0
                    } else {
                        0.0
                    }),
                    "&&" => AwkValue::Number(if l.as_bool() && r.as_bool() { 1.0 } else { 0.0 }),
                    "||" => AwkValue::Number(if l.as_bool() || r.as_bool() { 1.0 } else { 0.0 }),
                    "~" => {
                        if let Ok(re) = Regex::new(&r.as_string()) {
                            AwkValue::Number(if re.is_match(&l.as_string()) {
                                1.0
                            } else {
                                0.0
                            })
                        } else {
                            AwkValue::Number(0.0)
                        }
                    }
                    "!~" => {
                        if let Ok(re) = Regex::new(&r.as_string()) {
                            AwkValue::Number(if !re.is_match(&l.as_string()) {
                                1.0
                            } else {
                                0.0
                            })
                        } else {
                            AwkValue::Number(1.0)
                        }
                    }
                    _ => AwkValue::Uninitialized,
                }
            }
            AwkExpr::UnaryOp(op, expr) => {
                let v = self.eval_expr(expr);
                match op.as_str() {
                    "-" => AwkValue::Number(-v.as_number()),
                    "!" => AwkValue::Number(if v.as_bool() { 0.0 } else { 1.0 }),
                    _ => v,
                }
            }
            AwkExpr::Concat(parts) => {
                let s: String = parts
                    .iter()
                    .map(|p| self.eval_expr(p).as_string())
                    .collect();
                AwkValue::String(s)
            }
            AwkExpr::FuncCall(name, args) => self.call_function(name, args),
            AwkExpr::Regex(pattern) => AwkValue::String(pattern.clone()),
            AwkExpr::Match(expr, pattern) => {
                let s = self.eval_expr(expr).as_string();
                if let Ok(re) = Regex::new(pattern) {
                    AwkValue::Number(if re.is_match(&s) { 1.0 } else { 0.0 })
                } else {
                    AwkValue::Number(0.0)
                }
            }
        }
    }

    fn call_function(&mut self, name: &str, args: &[AwkExpr]) -> AwkValue {
        match name {
            "length" => {
                if args.is_empty() {
                    AwkValue::Number(self.state.get_field(0).as_string().len() as f64)
                } else {
                    AwkValue::Number(self.eval_expr(&args[0]).as_string().len() as f64)
                }
            }
            "substr" => {
                if args.len() < 2 {
                    return AwkValue::Uninitialized;
                }
                let s = self.eval_expr(&args[0]).as_string();
                let start = (self.eval_expr(&args[1]).as_number() as usize).saturating_sub(1);
                let len = if args.len() > 2 {
                    self.eval_expr(&args[2]).as_number() as usize
                } else {
                    s.len()
                };
                let end = (start + len).min(s.len());
                AwkValue::String(s.chars().skip(start).take(end - start).collect())
            }
            "index" => {
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let s = self.eval_expr(&args[0]).as_string();
                let t = self.eval_expr(&args[1]).as_string();
                match s.find(&t) {
                    Some(i) => AwkValue::Number((i + 1) as f64),
                    None => AwkValue::Number(0.0),
                }
            }
            "split" => {
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let s = self.eval_expr(&args[0]).as_string();
                let sep = if args.len() > 2 {
                    self.eval_expr(&args[2]).as_string()
                } else {
                    self.state.fs.clone()
                };

                let parts: Vec<&str> = if sep == " " {
                    s.split_whitespace().collect()
                } else {
                    s.split(&sep).collect()
                };

                // Store in array variable
                if let AwkExpr::Variable(arr_name) = &args[1] {
                    for (i, part) in parts.iter().enumerate() {
                        let key = format!("{}[{}]", arr_name, i + 1);
                        self.state
                            .set_variable(&key, AwkValue::String(part.to_string()));
                    }
                }

                AwkValue::Number(parts.len() as f64)
            }
            "sprintf" => {
                if args.is_empty() {
                    return AwkValue::String(String::new());
                }
                let format = self.eval_expr(&args[0]).as_string();
                let values: Vec<AwkValue> = args[1..].iter().map(|a| self.eval_expr(a)).collect();
                AwkValue::String(self.format_string(&format, &values))
            }
            "toupper" => {
                if args.is_empty() {
                    return AwkValue::Uninitialized;
                }
                AwkValue::String(self.eval_expr(&args[0]).as_string().to_uppercase())
            }
            "tolower" => {
                if args.is_empty() {
                    return AwkValue::Uninitialized;
                }
                AwkValue::String(self.eval_expr(&args[0]).as_string().to_lowercase())
            }
            "gsub" | "sub" => {
                // gsub(regexp, replacement, target)
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let pattern = self.eval_expr(&args[0]).as_string();
                let replacement = self.eval_expr(&args[1]).as_string();

                let target_expr = if args.len() > 2 {
                    args[2].clone()
                } else {
                    AwkExpr::Field(Box::new(AwkExpr::Number(0.0)))
                };

                let target = self.eval_expr(&target_expr).as_string();

                if let Ok(re) = Regex::new(&pattern) {
                    let (result, count) = if name == "gsub" {
                        let count = re.find_iter(&target).count();
                        (
                            re.replace_all(&target, replacement.as_str()).to_string(),
                            count,
                        )
                    } else {
                        let count = if re.is_match(&target) { 1 } else { 0 };
                        (re.replace(&target, replacement.as_str()).to_string(), count)
                    };

                    // Update the target variable or field
                    match &target_expr {
                        AwkExpr::Variable(name) => {
                            self.state.set_variable(name, AwkValue::String(result));
                        }
                        AwkExpr::Field(index) => {
                            let n = self.eval_expr(index).as_number() as usize;
                            if n == 0 {
                                // $0 is stored as a variable
                                self.state.set_variable("$0", AwkValue::String(result));
                            }
                            // For other fields, we'd need to update the fields vec
                            // and rebuild $0, but for now we just support $0
                        }
                        _ => {}
                    }

                    AwkValue::Number(count as f64)
                } else {
                    AwkValue::Number(0.0)
                }
            }
            "int" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().trunc())
            }
            "sqrt" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().sqrt())
            }
            "sin" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().sin())
            }
            "cos" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().cos())
            }
            "log" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().ln())
            }
            "exp" => {
                if args.is_empty() {
                    return AwkValue::Number(0.0);
                }
                AwkValue::Number(self.eval_expr(&args[0]).as_number().exp())
            }
            "__array_access" => {
                // Internal function for array indexing: arr[index]
                if args.len() < 2 {
                    return AwkValue::Uninitialized;
                }
                let arr_name = if let AwkExpr::Variable(name) = &args[0] {
                    name.clone()
                } else {
                    return AwkValue::Uninitialized;
                };
                let index = self.eval_expr(&args[1]);
                let key = format!("{}[{}]", arr_name, index.as_string());
                self.state.get_variable(&key)
            }
            _ => AwkValue::Uninitialized,
        }
    }

    fn format_string(&self, format: &str, values: &[AwkValue]) -> String {
        let mut result = String::new();
        let mut chars = format.chars().peekable();
        let mut value_idx = 0;

        while let Some(c) = chars.next() {
            if c == '%' {
                if chars.peek() == Some(&'%') {
                    chars.next();
                    result.push('%');
                    continue;
                }

                // Parse format specifier
                let mut spec = String::from("%");
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphabetic() {
                        spec.push(c);
                        chars.next();
                        break;
                    } else if c.is_ascii_digit() || c == '-' || c == '.' || c == '+' {
                        spec.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }

                if value_idx < values.len() {
                    let val = &values[value_idx];
                    value_idx += 1;

                    if spec.ends_with('d') || spec.ends_with('i') {
                        result.push_str(&format!("{}", val.as_number() as i64));
                    } else if spec.ends_with('f') || spec.ends_with('g') || spec.ends_with('e') {
                        result.push_str(&format!("{}", val.as_number()));
                    } else if spec.ends_with('s') {
                        result.push_str(&val.as_string());
                    } else if spec.ends_with('c') {
                        let s = val.as_string();
                        if let Some(c) = s.chars().next() {
                            result.push(c);
                        }
                    } else {
                        result.push_str(&val.as_string());
                    }
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    fn exec_action(&mut self, action: &AwkAction) -> bool {
        match action {
            AwkAction::Print(exprs) => {
                let parts: Vec<String> = exprs
                    .iter()
                    .map(|e| self.eval_expr(e).as_string())
                    .collect();
                self.output.push_str(&parts.join(&self.state.ofs));
                self.output.push_str(&self.state.ors);
                true
            }
            AwkAction::Printf(format, args) => {
                let values: Vec<AwkValue> = args.iter().map(|a| self.eval_expr(a)).collect();
                self.output.push_str(&self.format_string(format, &values));
                true
            }
            AwkAction::Assign(name, expr) => {
                let value = self.eval_expr(expr);
                self.state.set_variable(name, value);
                true
            }
            AwkAction::If(cond, then_actions, else_actions) => {
                if self.eval_expr(cond).as_bool() {
                    for action in then_actions {
                        if !self.exec_action(action) {
                            return false;
                        }
                    }
                } else {
                    for action in else_actions {
                        if !self.exec_action(action) {
                            return false;
                        }
                    }
                }
                true
            }
            AwkAction::While(cond, actions) => {
                while self.eval_expr(cond).as_bool() {
                    for action in actions {
                        if !self.exec_action(action) {
                            return false;
                        }
                    }
                }
                true
            }
            AwkAction::For(init, cond, update, actions) => {
                self.exec_action(init);
                while self.eval_expr(cond).as_bool() {
                    for action in actions {
                        if !self.exec_action(action) {
                            return false;
                        }
                    }
                    self.exec_action(update);
                }
                true
            }
            AwkAction::Next => false,
            AwkAction::Exit(_) => false,
            AwkAction::Expression(expr) => {
                self.eval_expr(expr);
                true
            }
        }
    }

    fn matches_pattern(&mut self, pattern: &AwkPattern) -> bool {
        match pattern {
            AwkPattern::Regex(re) => {
                let line = self.state.get_field(0).as_string();
                re.is_match(&line)
            }
            AwkPattern::Expression(expr) => self.eval_expr(expr).as_bool(),
        }
    }
}

#[async_trait]
impl Builtin for Awk {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut program_str = String::new();
        let mut files: Vec<String> = Vec::new();
        let mut field_sep = " ".to_string();
        let mut i = 0;

        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-F" {
                i += 1;
                if i < ctx.args.len() {
                    field_sep = ctx.args[i].clone();
                }
            } else if let Some(sep) = arg.strip_prefix("-F") {
                field_sep = sep.to_string();
            } else if arg == "-f" {
                // Read program from file
                i += 1;
                if i < ctx.args.len() {
                    let path = if ctx.args[i].starts_with('/') {
                        std::path::PathBuf::from(&ctx.args[i])
                    } else {
                        ctx.cwd.join(&ctx.args[i])
                    };
                    match ctx.fs.read_file(&path).await {
                        Ok(content) => {
                            program_str = String::from_utf8_lossy(&content).into_owned();
                        }
                        Err(e) => {
                            return Ok(ExecResult::err(format!("awk: {}: {}", ctx.args[i], e), 1));
                        }
                    }
                }
            } else if arg.starts_with('-') {
                // Unknown option - ignore
            } else if program_str.is_empty() {
                program_str = arg.clone();
            } else {
                files.push(arg.clone());
            }
            i += 1;
        }

        if program_str.is_empty() {
            return Err(Error::Execution("awk: no program given".to_string()));
        }

        let mut parser = AwkParser::new(&program_str);
        let program = parser.parse()?;

        let mut interp = AwkInterpreter::new();
        interp.state.fs = field_sep;

        // Run BEGIN actions
        for action in &program.begin_actions {
            interp.exec_action(action);
        }

        // Process input
        let inputs: Vec<String> = if files.is_empty() {
            vec![ctx.stdin.unwrap_or("").to_string()]
        } else {
            let mut inputs = Vec::new();
            for file in &files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        inputs.push(String::from_utf8_lossy(&content).into_owned());
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("awk: {}: {}", file, e), 1));
                    }
                }
            }
            inputs
        };

        'files: for input in inputs {
            interp.state.fnr = 0;
            for line in input.lines() {
                interp.state.set_line(line);

                'rules: for rule in &program.main_rules {
                    // Check pattern
                    let matches = match &rule.pattern {
                        Some(pattern) => interp.matches_pattern(pattern),
                        None => true,
                    };

                    if matches {
                        for action in &rule.actions {
                            match action {
                                AwkAction::Next => continue 'rules,
                                AwkAction::Exit(_) => break 'files,
                                _ => {
                                    // exec_action returns false for Next, which we've already handled
                                    interp.exec_action(action);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Run END actions
        for action in &program.end_actions {
            interp.exec_action(action);
        }

        Ok(ExecResult::ok(interp.output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_awk(args: &[&str], stdin: Option<&str>) -> Result<ExecResult> {
        let awk = Awk;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        awk.execute(ctx).await
    }

    #[tokio::test]
    async fn test_awk_print_all() {
        let result = run_awk(&["{print}"], Some("hello\nworld")).await.unwrap();
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_awk_print_field() {
        let result = run_awk(&["{print $1}"], Some("hello world\nfoo bar"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\nfoo\n");
    }

    #[tokio::test]
    async fn test_awk_print_multiple_fields() {
        let result = run_awk(&["{print $2, $1}"], Some("hello world"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "world hello\n");
    }

    #[tokio::test]
    async fn test_awk_field_separator() {
        let result = run_awk(&["-F:", "{print $1}"], Some("root:x:0:0"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "root\n");
    }

    #[tokio::test]
    async fn test_awk_nr() {
        let result = run_awk(&["{print NR, $0}"], Some("a\nb\nc")).await.unwrap();
        assert_eq!(result.stdout, "1 a\n2 b\n3 c\n");
    }

    #[tokio::test]
    async fn test_awk_nf() {
        let result = run_awk(&["{print NF}"], Some("a b c\nd e")).await.unwrap();
        assert_eq!(result.stdout, "3\n2\n");
    }

    #[tokio::test]
    async fn test_awk_begin_end() {
        let result = run_awk(
            &["BEGIN{print \"start\"} {print} END{print \"end\"}"],
            Some("middle"),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "start\nmiddle\nend\n");
    }

    #[tokio::test]
    async fn test_awk_pattern() {
        let result = run_awk(&["/hello/{print}"], Some("hello\nworld\nhello again"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\nhello again\n");
    }

    #[tokio::test]
    async fn test_awk_condition() {
        let result = run_awk(&["NR==2{print}"], Some("line1\nline2\nline3"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "line2\n");
    }

    #[tokio::test]
    async fn test_awk_arithmetic() {
        let result = run_awk(&["{print $1 + $2}"], Some("1 2\n3 4"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "3\n7\n");
    }

    #[tokio::test]
    async fn test_awk_variables() {
        let result = run_awk(&["{sum += $1} END{print sum}"], Some("1\n2\n3\n4"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "10\n");
    }

    #[tokio::test]
    async fn test_awk_length() {
        let result = run_awk(&["{print length($0)}"], Some("hello\nhi"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "5\n2\n");
    }

    #[tokio::test]
    async fn test_awk_substr() {
        let result = run_awk(&["{print substr($0, 2, 3)}"], Some("hello"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "ell\n");
    }

    #[tokio::test]
    async fn test_awk_toupper() {
        let result = run_awk(&["{print toupper($0)}"], Some("hello"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "HELLO\n");
    }

    #[tokio::test]
    async fn test_awk_multi_statement() {
        // Test multiple statements separated by semicolon
        let result = run_awk(&["{x=1; print x}"], Some("test")).await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_awk_gsub_with_print() {
        // gsub with regex literal followed by print
        let result = run_awk(
            &[r#"{gsub(/hello/, "hi"); print}"#],
            Some("hello hello hello"),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "hi hi hi\n");
    }

    #[tokio::test]
    async fn test_awk_split_with_array_access() {
        // split with array indexing
        let result = run_awk(
            &[r#"{n = split($0, arr, ":"); print arr[2]}"#],
            Some("a:b:c"),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "b\n");
    }

    /// TM-DOS-027: Deeply nested parenthesized expressions must be rejected
    #[test]
    fn test_awk_parser_depth_limit_parens() {
        // Build expression with 150 nested parens: (((((...(1)...))))
        let depth = 150;
        let open = "(".repeat(depth);
        let close = ")".repeat(depth);
        let program = format!("{{print {open}1{close}}}");

        let mut parser = AwkParser::new(&program);
        let result = parser.parse();
        assert!(result.is_err(), "deeply nested parens must be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nesting too deep"),
            "error should mention nesting: {err}"
        );
    }

    /// TM-DOS-027: Deeply chained unary operators must be rejected
    #[test]
    fn test_awk_parser_depth_limit_unary() {
        // Build expression with 200 chained negations: - - - ... - 1
        let depth = 200;
        let prefix = "- ".repeat(depth);
        let program = format!("{{print {prefix}1}}");

        let mut parser = AwkParser::new(&program);
        let result = parser.parse();
        assert!(result.is_err(), "deeply chained unary ops must be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nesting too deep"),
            "error should mention nesting: {err}"
        );
    }

    /// TM-DOS-027: Moderate nesting within limit still works
    #[test]
    fn test_awk_parser_moderate_nesting_ok() {
        // 10 levels of parens should be fine
        let depth = 10;
        let open = "(".repeat(depth);
        let close = ")".repeat(depth);
        let program = format!("{{print {open}1{close}}}");

        let mut parser = AwkParser::new(&program);
        let result = parser.parse();
        assert!(
            result.is_ok(),
            "moderate nesting should succeed: {:?}",
            result.err()
        );
    }
}
