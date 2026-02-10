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
    ArrayAssign(String, Box<AwkExpr>, Box<AwkExpr>), // arr[key] = val
    CompoundArrayAssign(String, Box<AwkExpr>, String, Box<AwkExpr>), // arr[key] += val
    Concat(Vec<AwkExpr>),
    FuncCall(String, Vec<AwkExpr>),
    Regex(String),
    Match(Box<AwkExpr>, String),             // expr ~ /pattern/
    PostIncrement(String),                   // var++
    PostDecrement(String),                   // var--
    PreIncrement(String),                    // ++var
    PreDecrement(String),                    // --var
    InArray(Box<AwkExpr>, String),           // key in arr
    FieldAssign(Box<AwkExpr>, Box<AwkExpr>), // $n = val
}

#[derive(Debug)]
enum AwkAction {
    Print(Vec<AwkExpr>),
    Printf(String, Vec<AwkExpr>),
    Assign(String, AwkExpr),
    ArrayAssign(String, AwkExpr, AwkExpr), // arr[key] = val
    If(AwkExpr, Vec<AwkAction>, Vec<AwkAction>),
    While(AwkExpr, Vec<AwkAction>),
    DoWhile(AwkExpr, Vec<AwkAction>),
    For(Box<AwkAction>, AwkExpr, Box<AwkAction>, Vec<AwkAction>),
    ForIn(String, String, Vec<AwkAction>), // for (key in arr) { body }
    Next,
    Break,
    Continue,
    Delete(String, AwkExpr), // delete arr[key]
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
            AwkValue::String(s) => {
                if s.is_empty() {
                    return false;
                }
                // In awk, numeric strings evaluate as numbers in boolean context
                if let Ok(n) = s.parse::<f64>() {
                    n != 0.0
                } else {
                    true
                }
            }
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
            "$0" => {
                let s = value.as_string();
                // Re-split fields when $0 is modified
                if self.fs == " " {
                    self.fields = s.split_whitespace().map(String::from).collect();
                } else {
                    self.fields = s.split(&self.fs).map(String::from).collect();
                }
                self.nf = self.fields.len();
                self.variables
                    .insert("NF".to_string(), AwkValue::Number(self.nf as f64));
                self.variables.insert(name.to_string(), value);
            }
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
        if self.matches_keyword("break") {
            return Ok(AwkAction::Break);
        }
        if self.matches_keyword("continue") {
            return Ok(AwkAction::Continue);
        }
        if self.matches_keyword("delete") {
            return self.parse_delete();
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
        // Consume optional ';' before else
        if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == ';' {
            self.pos += 1;
            self.skip_whitespace();
        }
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

    fn parse_for(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != '(' {
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
                while self.pos < self.input.len() {
                    let c = self.input.chars().nth(self.pos).unwrap();
                    if c.is_alphanumeric() || c == '_' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                let arr_name = self.input[start..self.pos].to_string();

                self.skip_whitespace();
                if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ')'
                {
                    return Err(Error::Execution("awk: expected ')' in for-in".to_string()));
                }
                self.pos += 1;

                self.skip_whitespace();
                let body = if self.pos < self.input.len()
                    && self.input.chars().nth(self.pos).unwrap() == '{'
                {
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
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ';' {
            return Err(Error::Execution(
                "awk: expected ';' in for statement".to_string(),
            ));
        }
        self.pos += 1;

        // Parse condition
        self.skip_whitespace();
        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ';' {
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
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ')' {
            return Err(Error::Execution(
                "awk: expected ')' in for statement".to_string(),
            ));
        }
        self.pos += 1;

        self.skip_whitespace();
        let body =
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '{' {
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

        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != '(' {
            return Err(Error::Execution(
                "awk: expected '(' after while".to_string(),
            ));
        }
        self.pos += 1;

        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ')' {
            return Err(Error::Execution(
                "awk: expected ')' after while condition".to_string(),
            ));
        }
        self.pos += 1;

        self.skip_whitespace();
        let body =
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '{' {
                self.parse_action_block()?
            } else {
                vec![self.parse_action()?]
            };

        Ok(AwkAction::While(condition, body))
    }

    fn parse_do_while(&mut self) -> Result<AwkAction> {
        self.skip_whitespace();

        let body =
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '{' {
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
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != '(' {
            return Err(Error::Execution(
                "awk: expected '(' after do-while".to_string(),
            ));
        }
        self.pos += 1;

        let condition = self.parse_expression()?;

        self.skip_whitespace();
        if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ')' {
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
        while self.pos < self.input.len() {
            let c = self.input.chars().nth(self.pos).unwrap();
            if c.is_alphanumeric() || c == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let arr_name = self.input[start..self.pos].to_string();

        self.skip_whitespace();
        if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '[' {
            self.pos += 1;
            let index = self.parse_expression()?;
            self.skip_whitespace();
            if self.pos >= self.input.len() || self.input.chars().nth(self.pos).unwrap() != ']' {
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
        if self.input.chars().nth(self.pos).unwrap() == '=' {
            let next = self.input.chars().nth(self.pos + 1);
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
        if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == '?' {
            self.pos += 1;
            self.skip_whitespace();
            let then_expr = self.parse_expression()?;
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input.chars().nth(self.pos).unwrap() == ':' {
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
            while self.pos < self.input.len() {
                let c = self.input.chars().nth(self.pos).unwrap();
                if c.is_alphanumeric() || c == '_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let arr_name = self.input[start..self.pos].to_string();
            return Ok(AwkExpr::InArray(Box::new(left), arr_name));
        }

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

    fn is_keyword_at_pos(&self) -> bool {
        let remaining = &self.input[self.pos..];
        let keywords = [
            "in", "if", "else", "while", "for", "do", "break", "continue", "next", "exit",
            "delete", "print", "printf",
        ];
        for kw in keywords {
            if remaining.starts_with(kw) {
                let after = self.pos + kw.len();
                if after >= self.input.len()
                    || !self.input.chars().nth(after).unwrap().is_alphanumeric()
                {
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

            let c = self.input.chars().nth(self.pos).unwrap();
            // Check if this could be the start of another value for concatenation
            if c == '"' || c == '$' || c.is_alphabetic() || c == '(' {
                // But not if it's a keyword or operator
                let remaining = &self.input[self.pos..];
                if !remaining.starts_with("||")
                    && !remaining.starts_with("&&")
                    && !remaining.starts_with("==")
                    && !remaining.starts_with("!=")
                    && !self.is_keyword_at_pos()
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
        let mut left = self.parse_power()?;

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }

            let c = self.input.chars().nth(self.pos).unwrap();
            if c == '*' || c == '/' || c == '%' {
                // Don't consume ** (power operator)
                if c == '*' && self.input.chars().nth(self.pos + 1) == Some('*') {
                    break;
                }
                // Don't consume if it's a compound assignment operator (*=, /=, %=)
                let next = self.input.chars().nth(self.pos + 1);
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
        if self.input.chars().nth(self.pos).unwrap() == '^' {
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

        // Pre-increment: ++var
        if self.input[self.pos..].starts_with("++") {
            self.pos += 2;
            self.skip_whitespace();
            if let Ok(AwkExpr::Variable(name)) = self.parse_primary() {
                return Ok(AwkExpr::PreIncrement(name));
            }
            return Err(Error::Execution(
                "awk: expected variable after ++".to_string(),
            ));
        }

        // Pre-decrement: --var
        if self.input[self.pos..].starts_with("--") {
            self.pos += 2;
            self.skip_whitespace();
            if let Ok(AwkExpr::Variable(name)) = self.parse_primary() {
                return Ok(AwkExpr::PreDecrement(name));
            }
            return Err(Error::Execution(
                "awk: expected variable after --".to_string(),
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

        let c = self.input.chars().nth(self.pos).unwrap();

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
            self.push_depth()?;
            let expr = self.parse_expression()?;
            self.pop_depth();
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

/// Flow control signal from action execution
#[derive(Debug, PartialEq)]
enum AwkFlow {
    Continue,          // Normal execution
    Next,              // Skip to next record
    Break,             // Break out of loop
    LoopContinue,      // Continue to next loop iteration
    Exit(Option<i32>), // Exit program with optional code
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
                    "^" => AwkValue::Number(l.as_number().powf(r.as_number())),
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
            AwkExpr::ArrayAssign(name, key, val) => {
                let k = self.eval_expr(key).as_string();
                let v = self.eval_expr(val);
                let full_key = format!("{}[{}]", name, k);
                self.state.set_variable(&full_key, v.clone());
                v
            }
            AwkExpr::CompoundArrayAssign(name, key, op, val) => {
                let k = self.eval_expr(key).as_string();
                let full_key = format!("{}[{}]", name, k);
                let current = self.state.get_variable(&full_key).as_number();
                let rhs = self.eval_expr(val).as_number();
                let result = match op.as_str() {
                    "+" => current + rhs,
                    "-" => current - rhs,
                    "*" => current * rhs,
                    "/" => current / rhs,
                    "%" => current % rhs,
                    _ => rhs,
                };
                let v = AwkValue::Number(result);
                self.state.set_variable(&full_key, v.clone());
                v
            }
            AwkExpr::FieldAssign(index, val) => {
                let n = self.eval_expr(index).as_number() as usize;
                let v = self.eval_expr(val);
                if n == 0 {
                    self.state.set_variable("$0", v.clone());
                } else {
                    // Extend fields if needed
                    while self.state.fields.len() < n {
                        self.state.fields.push(String::new());
                    }
                    self.state.fields[n - 1] = v.as_string();
                    self.state.nf = self.state.fields.len();
                    // Rebuild $0
                    let new_line = self.state.fields.join(&self.state.ofs);
                    self.state.set_variable("$0", AwkValue::String(new_line));
                }
                v
            }
            AwkExpr::PostIncrement(name) => {
                let current = self.state.get_variable(name).as_number();
                self.state
                    .set_variable(name, AwkValue::Number(current + 1.0));
                AwkValue::Number(current) // Return old value
            }
            AwkExpr::PostDecrement(name) => {
                let current = self.state.get_variable(name).as_number();
                self.state
                    .set_variable(name, AwkValue::Number(current - 1.0));
                AwkValue::Number(current) // Return old value
            }
            AwkExpr::PreIncrement(name) => {
                let current = self.state.get_variable(name).as_number();
                let new_val = current + 1.0;
                self.state.set_variable(name, AwkValue::Number(new_val));
                AwkValue::Number(new_val) // Return new value
            }
            AwkExpr::PreDecrement(name) => {
                let current = self.state.get_variable(name).as_number();
                let new_val = current - 1.0;
                self.state.set_variable(name, AwkValue::Number(new_val));
                AwkValue::Number(new_val) // Return new value
            }
            AwkExpr::InArray(key, arr_name) => {
                let k = self.eval_expr(key).as_string();
                let full_key = format!("{}[{}]", arr_name, k);
                let exists = !matches!(self.state.get_variable(&full_key), AwkValue::Uninitialized);
                AwkValue::Number(if exists { 1.0 } else { 0.0 })
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
                    // Check if the argument is an array name - if so, return element count
                    if let AwkExpr::Variable(ref arr_name) = args[0] {
                        let prefix = format!("{}[", arr_name);
                        let count = self
                            .state
                            .variables
                            .keys()
                            .filter(|k| k.starts_with(&prefix))
                            .count();
                        if count > 0 {
                            return AwkValue::Number(count as f64);
                        }
                    }
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
            "match" => {
                if args.len() < 2 {
                    return AwkValue::Number(0.0);
                }
                let s = self.eval_expr(&args[0]).as_string();
                let pattern = self.eval_expr(&args[1]).as_string();
                if let Ok(re) = Regex::new(&pattern) {
                    if let Some(m) = re.find(&s) {
                        let rstart = m.start() + 1; // awk is 1-indexed
                        let rlength = m.end() - m.start();
                        self.state
                            .set_variable("RSTART", AwkValue::Number(rstart as f64));
                        self.state
                            .set_variable("RLENGTH", AwkValue::Number(rlength as f64));
                        AwkValue::Number(rstart as f64)
                    } else {
                        self.state.set_variable("RSTART", AwkValue::Number(0.0));
                        self.state.set_variable("RLENGTH", AwkValue::Number(-1.0));
                        AwkValue::Number(0.0)
                    }
                } else {
                    AwkValue::Number(0.0)
                }
            }
            "gensub" => {
                // gensub(regexp, replacement, how [, target])
                if args.len() < 3 {
                    return AwkValue::Uninitialized;
                }
                let pattern = self.eval_expr(&args[0]).as_string();
                let replacement = self.eval_expr(&args[1]).as_string();
                let how = self.eval_expr(&args[2]).as_string();
                let target = if args.len() > 3 {
                    self.eval_expr(&args[3]).as_string()
                } else {
                    self.state.get_field(0).as_string()
                };
                if let Ok(re) = Regex::new(&pattern) {
                    if how == "g" || how == "G" {
                        AwkValue::String(re.replace_all(&target, replacement.as_str()).to_string())
                    } else {
                        // Replace nth occurrence (default 1st)
                        let n = how.parse::<usize>().unwrap_or(1);
                        let mut count = 0;
                        let result = re.replace_all(&target, |caps: &regex::Captures| -> String {
                            count += 1;
                            if count == n {
                                replacement.clone()
                            } else {
                                caps[0].to_string()
                            }
                        });
                        AwkValue::String(result.to_string())
                    }
                } else {
                    AwkValue::String(target)
                }
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
            "__ternary" => {
                // Ternary operator: cond ? then : else
                if args.len() < 3 {
                    return AwkValue::Uninitialized;
                }
                let cond = self.eval_expr(&args[0]);
                if cond.as_bool() {
                    self.eval_expr(&args[1])
                } else {
                    self.eval_expr(&args[2])
                }
            }
            _ => AwkValue::Uninitialized,
        }
    }

    fn format_string(&self, format: &str, values: &[AwkValue]) -> String {
        let mut result = String::new();
        let mut chars = format.chars().peekable();
        let mut value_idx = 0;

        while let Some(c) = chars.next() {
            if c == '\\' {
                // Handle escape sequences in format strings
                match chars.peek() {
                    Some('n') => {
                        chars.next();
                        result.push('\n');
                    }
                    Some('t') => {
                        chars.next();
                        result.push('\t');
                    }
                    Some('r') => {
                        chars.next();
                        result.push('\r');
                    }
                    Some('\\') => {
                        chars.next();
                        result.push('\\');
                    }
                    _ => result.push('\\'),
                }
            } else if c == '%' {
                if chars.peek() == Some(&'%') {
                    chars.next();
                    result.push('%');
                    continue;
                }

                // Parse format specifier: %[flags][width][.precision]type
                let mut left_align = false;
                let mut zero_pad = false;
                let mut plus_sign = false;
                let mut width: Option<usize> = None;
                let mut precision: Option<usize> = None;
                let mut conversion = ' ';

                // Parse flags
                loop {
                    match chars.peek() {
                        Some(&'-') => {
                            left_align = true;
                            chars.next();
                        }
                        Some(&'0') if width.is_none() => {
                            zero_pad = true;
                            chars.next();
                        }
                        Some(&'+') => {
                            plus_sign = true;
                            chars.next();
                        }
                        _ => break,
                    }
                }

                // Parse width
                let mut w = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        w.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !w.is_empty() {
                    width = w.parse().ok();
                }

                // Parse precision
                if chars.peek() == Some(&'.') {
                    chars.next();
                    let mut p = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() {
                            p.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    precision = if p.is_empty() {
                        Some(0)
                    } else {
                        p.parse().ok()
                    };
                }

                // Parse conversion character
                if let Some(&c) = chars.peek() {
                    if c.is_ascii_alphabetic() {
                        conversion = c;
                        chars.next();
                    }
                }

                if value_idx < values.len() {
                    let val = &values[value_idx];
                    value_idx += 1;

                    let formatted = match conversion {
                        'd' | 'i' => {
                            let n = val.as_number() as i64;
                            if plus_sign && n >= 0 {
                                format!("+{}", n)
                            } else {
                                format!("{}", n)
                            }
                        }
                        'f' => {
                            let n = val.as_number();
                            let prec = precision.unwrap_or(6);
                            format!("{:.prec$}", n)
                        }
                        'g' => {
                            let n = val.as_number();
                            let prec = precision.unwrap_or(6);
                            // %g: use shorter of %e or %f, strip trailing zeros
                            let s = format!("{:.prec$e}", n);
                            let f = format!("{:.prec$}", n);
                            if s.len() < f.len() {
                                s
                            } else {
                                f
                            }
                        }
                        'e' | 'E' => {
                            let n = val.as_number();
                            let prec = precision.unwrap_or(6);
                            format!("{:.prec$e}", n)
                        }
                        's' => {
                            let mut s = val.as_string();
                            if let Some(p) = precision {
                                s = s.chars().take(p).collect();
                            }
                            s
                        }
                        'c' => {
                            // %c: print character from ASCII code or first char of string
                            let n = val.as_number();
                            if n > 0.0 && n < 128.0 {
                                String::from(n as u8 as char)
                            } else {
                                let s = val.as_string();
                                s.chars().next().map(String::from).unwrap_or_default()
                            }
                        }
                        'x' | 'X' => {
                            let n = val.as_number() as i64;
                            if conversion == 'X' {
                                format!("{:X}", n)
                            } else {
                                format!("{:x}", n)
                            }
                        }
                        'o' => {
                            let n = val.as_number() as i64;
                            format!("{:o}", n)
                        }
                        _ => val.as_string(),
                    };

                    // Apply width and alignment
                    if let Some(w) = width {
                        if formatted.len() < w {
                            let padding = w - formatted.len();
                            if left_align {
                                result.push_str(&formatted);
                                for _ in 0..padding {
                                    result.push(' ');
                                }
                            } else if zero_pad
                                && matches!(conversion, 'd' | 'i' | 'f' | 'x' | 'X' | 'o')
                            {
                                for _ in 0..padding {
                                    result.push('0');
                                }
                                result.push_str(&formatted);
                            } else {
                                for _ in 0..padding {
                                    result.push(' ');
                                }
                                result.push_str(&formatted);
                            }
                        } else {
                            result.push_str(&formatted);
                        }
                    } else {
                        result.push_str(&formatted);
                    }
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    /// Execute action. Returns flow control signal.
    fn exec_action(&mut self, action: &AwkAction) -> AwkFlow {
        // Limit iterations to prevent infinite loops
        const MAX_LOOP_ITERS: usize = 100_000;

        match action {
            AwkAction::Print(exprs) => {
                let parts: Vec<String> = exprs
                    .iter()
                    .map(|e| self.eval_expr(e).as_string())
                    .collect();
                self.output.push_str(&parts.join(&self.state.ofs));
                self.output.push_str(&self.state.ors);
                AwkFlow::Continue
            }
            AwkAction::Printf(format, args) => {
                let values: Vec<AwkValue> = args.iter().map(|a| self.eval_expr(a)).collect();
                self.output.push_str(&self.format_string(format, &values));
                AwkFlow::Continue
            }
            AwkAction::Assign(name, expr) => {
                let value = self.eval_expr(expr);
                self.state.set_variable(name, value);
                AwkFlow::Continue
            }
            AwkAction::ArrayAssign(name, key, val) => {
                let k = self.eval_expr(key).as_string();
                let v = self.eval_expr(val);
                let full_key = format!("{}[{}]", name, k);
                self.state.set_variable(&full_key, v);
                AwkFlow::Continue
            }
            AwkAction::If(cond, then_actions, else_actions) => {
                let actions = if self.eval_expr(cond).as_bool() {
                    then_actions
                } else {
                    else_actions
                };
                for action in actions {
                    match self.exec_action(action) {
                        AwkFlow::Continue => {}
                        flow => return flow,
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::While(cond, actions) => {
                let mut iters = 0;
                while self.eval_expr(cond).as_bool() {
                    iters += 1;
                    if iters > MAX_LOOP_ITERS {
                        break;
                    }
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break {
                        break;
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::DoWhile(cond, actions) => {
                let mut iters = 0;
                loop {
                    iters += 1;
                    if iters > MAX_LOOP_ITERS {
                        break;
                    }
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break || !self.eval_expr(cond).as_bool() {
                        break;
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::For(init, cond, update, actions) => {
                self.exec_action(init);
                let mut iters = 0;
                while self.eval_expr(cond).as_bool() {
                    iters += 1;
                    if iters > MAX_LOOP_ITERS {
                        break;
                    }
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break {
                        break;
                    }
                    self.exec_action(update);
                }
                AwkFlow::Continue
            }
            AwkAction::ForIn(var, arr_name, actions) => {
                // Collect array keys matching the pattern arr_name[*]
                let prefix = format!("{}[", arr_name);
                let keys: Vec<String> = self
                    .state
                    .variables
                    .keys()
                    .filter(|k| k.starts_with(&prefix) && k.ends_with(']'))
                    .map(|k| k[prefix.len()..k.len() - 1].to_string())
                    .collect();

                for key in keys {
                    self.state.set_variable(var, AwkValue::String(key));
                    let mut do_break = false;
                    for action in actions {
                        match self.exec_action(action) {
                            AwkFlow::Continue => {}
                            AwkFlow::Break => {
                                do_break = true;
                                break;
                            }
                            AwkFlow::LoopContinue => break,
                            flow => return flow,
                        }
                    }
                    if do_break {
                        break;
                    }
                }
                AwkFlow::Continue
            }
            AwkAction::Delete(arr_name, key) => {
                let k = self.eval_expr(key).as_string();
                if k == "*" {
                    // Delete all entries in the array
                    let prefix = format!("{}[", arr_name);
                    let keys: Vec<String> = self
                        .state
                        .variables
                        .keys()
                        .filter(|k| k.starts_with(&prefix))
                        .cloned()
                        .collect();
                    for key in keys {
                        self.state.variables.remove(&key);
                    }
                } else {
                    let full_key = format!("{}[{}]", arr_name, k);
                    self.state.variables.remove(&full_key);
                }
                AwkFlow::Continue
            }
            AwkAction::Next => AwkFlow::Next,
            AwkAction::Break => AwkFlow::Break,
            AwkAction::Continue => AwkFlow::LoopContinue,
            AwkAction::Exit(expr) => {
                let code = expr.as_ref().map(|e| self.eval_expr(e).as_number() as i32);
                AwkFlow::Exit(code)
            }
            AwkAction::Expression(expr) => {
                self.eval_expr(expr);
                AwkFlow::Continue
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
        let mut pre_vars: Vec<(String, String)> = Vec::new();
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
            } else if arg == "-v" {
                // Variable assignment: -v var=value
                i += 1;
                if i < ctx.args.len() {
                    if let Some(eq_pos) = ctx.args[i].find('=') {
                        let name = ctx.args[i][..eq_pos].to_string();
                        let mut value = ctx.args[i][eq_pos + 1..].to_string();
                        // Strip surrounding quotes if present (shell may pass them)
                        if (value.starts_with('"') && value.ends_with('"'))
                            || (value.starts_with('\'') && value.ends_with('\''))
                        {
                            value = value[1..value.len() - 1].to_string();
                        }
                        pre_vars.push((name, value));
                    }
                }
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

        // Set pre-assigned variables (-v)
        for (name, value) in &pre_vars {
            let awk_val = if let Ok(n) = value.parse::<f64>() {
                AwkValue::Number(n)
            } else {
                AwkValue::String(value.clone())
            };
            interp.state.set_variable(name, awk_val);
        }

        // Run BEGIN actions
        let mut exit_code: Option<i32> = None;
        for action in &program.begin_actions {
            if let AwkFlow::Exit(code) = interp.exec_action(action) {
                exit_code = code;
                // Run END actions even after exit
                for end_action in &program.end_actions {
                    if let AwkFlow::Exit(_) = interp.exec_action(end_action) {
                        break;
                    }
                }
                return Ok(ExecResult::with_code(interp.output, exit_code.unwrap_or(0)));
            }
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

                for rule in &program.main_rules {
                    // Check pattern
                    let matches = match &rule.pattern {
                        Some(pattern) => interp.matches_pattern(pattern),
                        None => true,
                    };

                    if matches {
                        let mut next_record = false;
                        for action in &rule.actions {
                            match interp.exec_action(action) {
                                AwkFlow::Continue => {}
                                AwkFlow::Next => {
                                    next_record = true;
                                    break;
                                }
                                AwkFlow::Exit(code) => {
                                    exit_code = code;
                                    break 'files;
                                }
                                _ => {}
                            }
                        }
                        if next_record {
                            break;
                        }
                    }
                }
            }
        }

        // Run END actions (awk runs END even after exit in main body)
        for action in &program.end_actions {
            if let AwkFlow::Exit(code) = interp.exec_action(action) {
                if exit_code.is_none() {
                    exit_code = code;
                }
                break;
            }
        }

        Ok(ExecResult::with_code(interp.output, exit_code.unwrap_or(0)))
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

    // === New tests for added features ===

    #[tokio::test]
    async fn test_awk_for_c_style() {
        let result = run_awk(&["BEGIN{for(i=1;i<=5;i++) print i}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\n2\n3\n4\n5\n");
    }

    #[tokio::test]
    async fn test_awk_for_with_body_block() {
        let result = run_awk(&["BEGIN{for(i=0;i<3;i++){print i}}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "0\n1\n2\n");
    }

    #[tokio::test]
    async fn test_awk_while_loop() {
        let result = run_awk(&["BEGIN{i=1; while(i<=3){print i; i++}}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\n2\n3\n");
    }

    #[tokio::test]
    async fn test_awk_do_while() {
        let result = run_awk(&["BEGIN{i=1; do{print i; i++}while(i<=3)}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\n2\n3\n");
    }

    #[tokio::test]
    async fn test_awk_post_increment() {
        let result = run_awk(&["{print i++}"], Some("a\nb\nc")).await.unwrap();
        assert_eq!(result.stdout, "0\n1\n2\n");
    }

    #[tokio::test]
    async fn test_awk_pre_increment() {
        let result = run_awk(&["{print ++i}"], Some("a\nb\nc")).await.unwrap();
        assert_eq!(result.stdout, "1\n2\n3\n");
    }

    #[tokio::test]
    async fn test_awk_post_decrement() {
        let result = run_awk(&["BEGIN{x=3; print x--; print x}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "3\n2\n");
    }

    #[tokio::test]
    async fn test_awk_array_assign() {
        let result = run_awk(&[r#"BEGIN{a[1]="x"; a[2]="y"; print a[1], a[2]}"#], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "x y\n");
    }

    #[tokio::test]
    async fn test_awk_array_in_operator() {
        let result = run_awk(
            &[r#"BEGIN{a["foo"]=1; if("foo" in a) print "yes"; if("bar" in a) print "no"}"#],
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_awk_for_in_loop() {
        let result = run_awk(
            &[r#"BEGIN{a[1]="x"; a[2]="y"; for(k in a) count++; print count}"#],
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "2\n");
    }

    #[tokio::test]
    async fn test_awk_delete_array_element() {
        let result = run_awk(
            &[r#"BEGIN{a[1]=1; a[2]=2; delete a[1]; for(k in a) print k, a[k]}"#],
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "2 2\n");
    }

    #[tokio::test]
    async fn test_awk_v_flag() {
        let result = run_awk(&["-v", "x=hello", "BEGIN{print x}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_awk_v_flag_numeric() {
        let result = run_awk(&["-v", "n=42", "BEGIN{print n+1}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "43\n");
    }

    #[tokio::test]
    async fn test_awk_break_in_for() {
        let result = run_awk(&["BEGIN{for(i=1;i<=10;i++){if(i>3) break; print i}}"], None)
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\n2\n3\n");
    }

    #[tokio::test]
    async fn test_awk_continue_in_for() {
        let result = run_awk(
            &["BEGIN{for(i=1;i<=5;i++){if(i==3) continue; print i}}"],
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "1\n2\n4\n5\n");
    }

    #[tokio::test]
    async fn test_awk_ternary() {
        let result = run_awk(&[r#"{print ($1>2 ? "big" : "small")}"#], Some("1\n3\n2"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "small\nbig\nsmall\n");
    }

    #[tokio::test]
    async fn test_awk_field_assignment() {
        let result = run_awk(&[r#"{$2="new"; print}"#], Some("one two three"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "one new three\n");
    }

    #[tokio::test]
    async fn test_awk_csv_to_json_pattern() {
        // This is the pattern LLMs use for CSV→JSON conversion
        let result = run_awk(
            &[
                "-F,",
                r#"NR==1{for(i=1;i<=NF;i++) h[i]=$i; next} {for(i=1;i<=NF;i++) printf "%s=%s ", h[i], $i; print ""}"#,
            ],
            Some("name,age\nalice,30\nbob,25"),
        )
        .await
        .unwrap();
        assert!(result.stdout.contains("name=alice"));
        assert!(result.stdout.contains("age=30"));
        assert!(result.stdout.contains("name=bob"));
    }

    #[tokio::test]
    async fn test_awk_compound_array_assign() {
        let result = run_awk(
            &[r#"{count[$1]++} END{for(k in count) print k, count[k]}"#],
            Some("a\nb\na\nc\nb\na"),
        )
        .await
        .unwrap();
        // Order may vary, so check contents
        assert!(result.stdout.contains("a 3"));
        assert!(result.stdout.contains("b 2"));
        assert!(result.stdout.contains("c 1"));
    }

    #[tokio::test]
    async fn test_awk_next_statement() {
        let result = run_awk(&["NR==2{next} {print}"], Some("line1\nline2\nline3"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "line1\nline3\n");
    }

    #[tokio::test]
    async fn test_awk_exit_statement() {
        let result = run_awk(&["NR==2{exit} {print}"], Some("line1\nline2\nline3"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "line1\n");
    }

    #[tokio::test]
    async fn test_awk_revenue_calculation() {
        // This is the exact eval task pattern
        let result = run_awk(
            &["-F,", "NR>1{total+=$2*$3} END{print total}"],
            Some("product,price,quantity\nwidget,10,5\ngadget,25,3\ndoohickey,7,12\nsprocket,15,8"),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "329\n");
    }
}
