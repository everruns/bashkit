//! bc builtin - arbitrary-precision calculator
//!
//! Supports scale, basic arithmetic (+, -, *, /, %, ^), comparisons,
//! and -l math library functions (s, c, a, l, e).

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The bc builtin - arbitrary-precision calculator.
///
/// Usage: echo "expression" | bc [-l]
///
/// Supports:
///   - scale=N for decimal precision
///   - +, -, *, /, %, ^ operators
///   - Comparison: ==, !=, <, >, <=, >=
///   - -l flag: math library (s, c, a, l, e, sqrt)
///   - Multiple expressions separated by newlines or semicolons
pub struct Bc;

#[async_trait]
impl Builtin for Bc {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut math_lib = false;
        let mut expr_args: Vec<&str> = Vec::new();

        for arg in ctx.args {
            match arg.as_str() {
                "-l" => math_lib = true,
                _ => expr_args.push(arg),
            }
        }

        // bc reads from stdin primarily
        let input = if let Some(stdin) = ctx.stdin {
            stdin.to_string()
        } else if !expr_args.is_empty() {
            // Some agents pass expression as argument
            expr_args.join(" ")
        } else {
            return Ok(ExecResult::ok(String::new()));
        };

        let default_scale = if math_lib { 20 } else { 0 };
        let mut state = BcState::new(default_scale);
        let mut output = String::new();

        // Split input into statements by newlines and semicolons
        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            for stmt in line.split(';') {
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }

                match state.execute_statement(stmt) {
                    Ok(Some(val)) => {
                        output.push_str(&val);
                        output.push('\n');
                    }
                    Ok(None) => {} // assignment, no output
                    Err(e) => {
                        return Ok(ExecResult::err(format!("(standard_in) 1: {}\n", e), 1));
                    }
                }
            }
        }

        Ok(ExecResult::ok(output))
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some("bc: Arbitrary-precision calculator. Use 'echo \"scale=2; 1/3\" | bc' for decimals.")
    }
}

struct BcState {
    scale: u32,
    variables: std::collections::HashMap<String, f64>,
}

impl BcState {
    fn new(default_scale: u32) -> Self {
        Self {
            scale: default_scale,
            variables: std::collections::HashMap::new(),
        }
    }

    fn execute_statement(&mut self, stmt: &str) -> std::result::Result<Option<String>, String> {
        // Check for scale assignment
        if let Some(val_str) = stmt.strip_prefix("scale=") {
            let val_str = val_str.trim();
            let val: u32 = val_str
                .parse()
                .map_err(|_| format!("parse error: {}", val_str))?;
            self.scale = val;
            return Ok(None);
        }

        // Check for variable assignment (simple: var=expr)
        if let Some(eq_pos) = stmt.find('=') {
            let lhs = stmt[..eq_pos].trim();
            // Make sure it's not == or != or <= or >=
            let after = stmt.get(eq_pos + 1..eq_pos + 2).unwrap_or("");
            let before = if eq_pos > 0 {
                stmt.get(eq_pos - 1..eq_pos).unwrap_or("")
            } else {
                ""
            };
            if after != "="
                && before != "!"
                && before != "<"
                && before != ">"
                && is_valid_identifier(lhs)
            {
                let rhs = stmt[eq_pos + 1..].trim();
                let val = self.evaluate_expr(rhs)?;
                self.variables.insert(lhs.to_string(), val);
                return Ok(None);
            }
        }

        // Expression - evaluate and return result
        let val = self.evaluate_expr(stmt)?;
        Ok(Some(self.format_number(val)))
    }

    fn format_number(&self, val: f64) -> String {
        if self.scale == 0 {
            // Integer mode - truncate toward zero like bc does
            let truncated = val.trunc() as i64;
            format!("{}", truncated)
        } else {
            // Fixed decimal places
            let formatted = format!("{:.prec$}", val, prec = self.scale as usize);
            // Remove trailing zeros but keep at least scale digits? No, bc keeps them.
            formatted
        }
    }

    fn evaluate_expr(&self, expr: &str) -> std::result::Result<f64, String> {
        let tokens = tokenize(expr)?;
        let mut parser = ExprParser::new(&tokens, self);
        let val = parser.parse_comparison()?;
        Ok(val)
    }
}

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

fn tokenize(expr: &str) -> std::result::Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\r' => i += 1,
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let num_str: String = chars[start..i].iter().collect();
                let val: f64 = num_str
                    .parse()
                    .map_err(|_| format!("parse error: {}", num_str))?;
                tokens.push(Token::Number(val));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                tokens.push(Token::Ident(ident));
            }
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Ne);
                i += 2;
            }
            '<' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Le);
                i += 2;
            }
            '>' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Ge);
                i += 2;
            }
            '=' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Eq);
                i += 2;
            }
            '<' => {
                tokens.push(Token::Lt);
                i += 1;
            }
            '>' => {
                tokens.push(Token::Gt);
                i += 1;
            }
            c => return Err(format!("illegal character: {}", c)),
        }
    }

    Ok(tokens)
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    pos: usize,
    state: &'a BcState,
}

impl<'a> ExprParser<'a> {
    fn new(tokens: &'a [Token], state: &'a BcState) -> Self {
        Self {
            tokens,
            pos: 0,
            state,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn parse_comparison(&mut self) -> std::result::Result<f64, String> {
        let mut left = self.parse_additive()?;

        while let Some(tok) = self.peek() {
            match tok {
                Token::Eq => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = if (left - right).abs() < f64::EPSILON {
                        1.0
                    } else {
                        0.0
                    };
                }
                Token::Ne => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = if (left - right).abs() >= f64::EPSILON {
                        1.0
                    } else {
                        0.0
                    };
                }
                Token::Lt => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = if left < right { 1.0 } else { 0.0 };
                }
                Token::Gt => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = if left > right { 1.0 } else { 0.0 };
                }
                Token::Le => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = if left <= right { 1.0 } else { 0.0 };
                }
                Token::Ge => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = if left >= right { 1.0 } else { 0.0 };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> std::result::Result<f64, String> {
        let mut left = self.parse_multiplicative()?;

        while let Some(tok) = self.peek() {
            match tok {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left += right;
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left -= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> std::result::Result<f64, String> {
        let mut left = self.parse_power()?;

        while let Some(tok) = self.peek() {
            match tok {
                Token::Star => {
                    self.advance();
                    let right = self.parse_power()?;
                    left *= right;
                }
                Token::Slash => {
                    self.advance();
                    let right = self.parse_power()?;
                    if right == 0.0 {
                        return Err("divide by zero".to_string());
                    }
                    left /= right;
                }
                Token::Percent => {
                    self.advance();
                    let right = self.parse_power()?;
                    if right == 0.0 {
                        return Err("divide by zero".to_string());
                    }
                    left %= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn parse_power(&mut self) -> std::result::Result<f64, String> {
        let base = self.parse_unary()?;

        if let Some(Token::Caret) = self.peek() {
            self.advance();
            let exp = self.parse_power()?; // right-associative
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> std::result::Result<f64, String> {
        if let Some(Token::Minus) = self.peek() {
            self.advance();
            let val = self.parse_unary()?;
            return Ok(-val);
        }
        if let Some(Token::Plus) = self.peek() {
            self.advance();
            return self.parse_unary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> std::result::Result<f64, String> {
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.advance();
                Ok(n)
            }
            Some(Token::Ident(name)) => {
                self.advance();
                // Check for function call
                if let Some(Token::LParen) = self.peek() {
                    self.advance(); // consume (
                    let arg = self.parse_comparison()?;
                    match self.peek() {
                        Some(Token::RParen) => {
                            self.advance();
                        }
                        _ => return Err("missing )".to_string()),
                    }
                    return self.call_function(&name, arg);
                }
                // Variable lookup
                if name == "scale" {
                    return Ok(self.state.scale as f64);
                }
                Ok(*self.state.variables.get(&name).unwrap_or(&0.0))
            }
            Some(Token::LParen) => {
                self.advance();
                let val = self.parse_comparison()?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.advance();
                    }
                    _ => return Err("missing )".to_string()),
                }
                Ok(val)
            }
            _ => Err("parse error".to_string()),
        }
    }

    fn call_function(&self, name: &str, arg: f64) -> std::result::Result<f64, String> {
        match name {
            "s" => Ok(arg.sin()),          // sine
            "c" => Ok(arg.cos()),          // cosine
            "a" => Ok(arg.atan()),         // arctangent
            "l" => {                       // natural log
                if arg <= 0.0 {
                    return Err("log of non-positive number".to_string());
                }
                Ok(arg.ln())
            }
            "e" => Ok(arg.exp()),          // e^x
            "sqrt" => {
                if arg < 0.0 {
                    return Err("square root of negative number".to_string());
                }
                Ok(arg.sqrt())
            }
            _ => Err(format!("undefined function: {}", name)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn setup() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();
        fs.mkdir(&cwd, true).await.unwrap();
        (fs, cwd, variables)
    }

    async fn run_bc(input: &str, args: &[&str]) -> ExecResult {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), Some(input));
        Bc.execute(ctx).await.unwrap()
    }

    // ==================== basic arithmetic ====================

    #[tokio::test]
    async fn bc_addition() {
        let result = run_bc("1+2\n", &[]).await;
        assert_eq!(result.stdout, "3\n");
    }

    #[tokio::test]
    async fn bc_subtraction() {
        let result = run_bc("10-3\n", &[]).await;
        assert_eq!(result.stdout, "7\n");
    }

    #[tokio::test]
    async fn bc_multiplication() {
        let result = run_bc("6*7\n", &[]).await;
        assert_eq!(result.stdout, "42\n");
    }

    #[tokio::test]
    async fn bc_division_integer() {
        let result = run_bc("10/3\n", &[]).await;
        assert_eq!(result.stdout, "3\n"); // scale=0 truncates
    }

    #[tokio::test]
    async fn bc_modulo() {
        let result = run_bc("10%3\n", &[]).await;
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn bc_power() {
        let result = run_bc("2^10\n", &[]).await;
        assert_eq!(result.stdout, "1024\n");
    }

    // ==================== scale ====================

    #[tokio::test]
    async fn bc_scale_division() {
        let result = run_bc("scale=2; 10/3\n", &[]).await;
        assert_eq!(result.stdout, "3.33\n");
    }

    #[tokio::test]
    async fn bc_scale_4() {
        let result = run_bc("scale=4; 1/3\n", &[]).await;
        assert_eq!(result.stdout, "0.3333\n");
    }

    #[tokio::test]
    async fn bc_financial_calc() {
        let result = run_bc("scale=2; 100.50 * 1.0825\n", &[]).await;
        assert_eq!(result.stdout, "108.79\n");
    }

    // ==================== comparisons ====================

    #[tokio::test]
    async fn bc_compare_equal() {
        let result = run_bc("5==5\n", &[]).await;
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn bc_compare_not_equal() {
        let result = run_bc("5!=3\n", &[]).await;
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn bc_compare_less() {
        let result = run_bc("3<5\n", &[]).await;
        assert_eq!(result.stdout, "1\n");
    }

    // ==================== math library (-l) ====================

    #[tokio::test]
    async fn bc_math_lib_scale() {
        let result = run_bc("1/3\n", &["-l"]).await;
        // -l sets scale=20
        assert!(result.stdout.starts_with("0."));
        assert!(result.stdout.len() > 10);
    }

    #[tokio::test]
    async fn bc_sqrt() {
        let result = run_bc("scale=4; sqrt(2)\n", &[]).await;
        assert_eq!(result.stdout, "1.4142\n");
    }

    // ==================== variables ====================

    #[tokio::test]
    async fn bc_variable_assignment() {
        let result = run_bc("x=5; x*2\n", &[]).await;
        assert_eq!(result.stdout, "10\n");
    }

    // ==================== parentheses ====================

    #[tokio::test]
    async fn bc_parentheses() {
        let result = run_bc("(2+3)*4\n", &[]).await;
        assert_eq!(result.stdout, "20\n");
    }

    // ==================== negative numbers ====================

    #[tokio::test]
    async fn bc_negative() {
        let result = run_bc("-5+3\n", &[]).await;
        assert_eq!(result.stdout, "-2\n");
    }

    // ==================== errors ====================

    #[tokio::test]
    async fn bc_divide_by_zero() {
        let result = run_bc("1/0\n", &[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("divide by zero"));
    }

    #[tokio::test]
    async fn bc_empty_input() {
        let result = run_bc("", &[]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    // ==================== multiple expressions ====================

    #[tokio::test]
    async fn bc_multiple_lines() {
        let result = run_bc("1+1\n2+2\n3+3\n", &[]).await;
        assert_eq!(result.stdout, "2\n4\n6\n");
    }
}
