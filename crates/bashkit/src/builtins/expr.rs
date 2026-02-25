//! expr builtin - evaluate expressions
//!
//! Supports arithmetic, string, and comparison operations.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The expr builtin - evaluate expressions.
///
/// Usage: expr EXPRESSION
///
/// Arithmetic: expr ARG1 + ARG2, - , \* , / , %
/// Comparison: expr ARG1 = ARG2, != , < , > , <= , >=
/// String: expr length STRING, expr substr STRING POS LEN, expr match STRING REGEX
/// Pattern: expr STRING : REGEX
pub struct Expr;

#[async_trait]
impl Builtin for Expr {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("expr: missing operand\n".to_string(), 2));
        }

        let args: Vec<&str> = ctx.args.iter().map(|s| s.as_str()).collect();
        match evaluate(&args) {
            Ok(val) => {
                let exit_code = if val == "0" || val.is_empty() { 1 } else { 0 };
                Ok(ExecResult::with_code(format!("{}\n", val), exit_code))
            }
            Err(msg) => Ok(ExecResult::err(format!("expr: {}\n", msg), 2)),
        }
    }
}

fn evaluate(args: &[&str]) -> std::result::Result<String, String> {
    if args.is_empty() {
        return Err("missing operand".to_string());
    }

    // Handle keyword operations first
    if args.len() >= 2 && args[0] == "length" {
        return Ok(args[1].len().to_string());
    }

    if args.len() >= 4 && args[0] == "substr" {
        let s = args[1];
        let pos: usize = args[2]
            .parse()
            .map_err(|_| "non-integer argument".to_string())?;
        let len: usize = args[3]
            .parse()
            .map_err(|_| "non-integer argument".to_string())?;
        if pos == 0 || pos > s.len() {
            return Ok(String::new());
        }
        let start = pos - 1; // 1-based to 0-based
        let end = (start + len).min(s.len());
        return Ok(s[start..end].to_string());
    }

    if args.len() >= 3 && args[0] == "index" {
        let s = args[1];
        let chars = args[2];
        for (i, c) in s.chars().enumerate() {
            if chars.contains(c) {
                return Ok((i + 1).to_string());
            }
        }
        return Ok("0".to_string());
    }

    if args.len() >= 3 && args[0] == "match" {
        return match_pattern(args[1], args[2]);
    }

    // Single value
    if args.len() == 1 {
        return Ok(args[0].to_string());
    }

    // Binary operations: ARG1 OP ARG2
    if args.len() == 3 {
        let left = args[0];
        let op = args[1];
        let right = args[2];

        // Pattern match: STRING : REGEX
        if op == ":" {
            return match_pattern(left, right);
        }

        // Try arithmetic
        let left_num = left.parse::<i64>();
        let right_num = right.parse::<i64>();

        match op {
            "+" | "-" | "*" | "/" | "%" => {
                let a = left_num.map_err(|_| "non-integer argument".to_string())?;
                let b = right_num.map_err(|_| "non-integer argument".to_string())?;
                let result = match op {
                    "+" => a.checked_add(b).ok_or("integer overflow")?,
                    "-" => a.checked_sub(b).ok_or("integer overflow")?,
                    "*" => a.checked_mul(b).ok_or("integer overflow")?,
                    "/" => {
                        if b == 0 {
                            return Err("division by zero".to_string());
                        }
                        a / b
                    }
                    "%" => {
                        if b == 0 {
                            return Err("division by zero".to_string());
                        }
                        a % b
                    }
                    _ => unreachable!(),
                };
                return Ok(result.to_string());
            }
            "=" => {
                return Ok(if left == right { "1" } else { "0" }.to_string());
            }
            "!=" => {
                return Ok(if left != right { "1" } else { "0" }.to_string());
            }
            "<" | ">" | "<=" | ">=" => {
                // Compare as integers if both are numbers, otherwise as strings
                let result = if let (Ok(a), Ok(b)) = (left_num, right_num) {
                    match op {
                        "<" => a < b,
                        ">" => a > b,
                        "<=" => a <= b,
                        ">=" => a >= b,
                        _ => unreachable!(),
                    }
                } else {
                    match op {
                        "<" => left < right,
                        ">" => left > right,
                        "<=" => left <= right,
                        ">=" => left >= right,
                        _ => unreachable!(),
                    }
                };
                return Ok(if result { "1" } else { "0" }.to_string());
            }
            "|" => {
                // OR: return left if non-zero/non-empty, else right
                if !left.is_empty() && left != "0" {
                    return Ok(left.to_string());
                }
                return Ok(right.to_string());
            }
            "&" => {
                // AND: return left if both are non-zero/non-empty, else 0
                let l_true = !left.is_empty() && left != "0";
                let r_true = !right.is_empty() && right != "0";
                if l_true && r_true {
                    return Ok(left.to_string());
                }
                return Ok("0".to_string());
            }
            _ => {}
        }
    }

    // Fallback: return first arg
    Ok(args[0].to_string())
}

/// Match a string against a pattern (anchored at start, like expr : behavior)
fn match_pattern(s: &str, pattern: &str) -> std::result::Result<String, String> {
    // Simple pattern matching - expr patterns are anchored at start
    // For now, support basic patterns: . (any char), .* (any), literal
    // Check if pattern has capturing group \(...\)
    let has_group = pattern.contains("\\(") && pattern.contains("\\)");

    if has_group {
        // Extract the group pattern
        // For simplicity, handle common case: prefix\(.*\)suffix
        if let Some(start) = pattern.find("\\(") {
            if let Some(end) = pattern.find("\\)") {
                let before = &pattern[..start];
                let inner = &pattern[start + 2..end];
                let _after = &pattern[end + 2..];

                // Simple: if before matches start, capture inner
                if let Some(rest) = s.strip_prefix(before) {
                    let matched = simple_match(rest, inner);
                    return Ok(matched);
                }
            }
        }
        Ok(String::new())
    } else {
        // No group: return number of matched characters
        let count = count_match(s, pattern);
        Ok(count.to_string())
    }
}

/// Count how many characters from start of s match the pattern
fn count_match(s: &str, pattern: &str) -> usize {
    // Build a simple matcher
    let mut si = 0;
    let mut pi = 0;
    let s_chars: Vec<char> = s.chars().collect();
    let p_chars: Vec<char> = pattern.chars().collect();

    while pi < p_chars.len() && si < s_chars.len() {
        if pi + 1 < p_chars.len() && p_chars[pi + 1] == '*' {
            // X* - match zero or more of X
            let match_char = p_chars[pi];
            pi += 2;
            // Greedy: match as many as possible
            while si < s_chars.len() && char_matches(s_chars[si], match_char) {
                si += 1;
            }
        } else if p_chars[pi] == '.' {
            // . matches any character
            si += 1;
            pi += 1;
        } else if s_chars[si] == p_chars[pi] {
            si += 1;
            pi += 1;
        } else {
            break;
        }
    }

    // Check if we consumed the entire pattern
    // Handle trailing X* patterns (they can match zero)
    while pi + 1 < p_chars.len() && p_chars[pi + 1] == '*' {
        pi += 2;
    }

    if pi >= p_chars.len() {
        si
    } else {
        0
    }
}

/// Simple match returning the matched portion
fn simple_match(s: &str, pattern: &str) -> String {
    let count = count_match(s, pattern);
    s[..count].to_string()
}

fn char_matches(c: char, pattern: char) -> bool {
    pattern == '.' || c == pattern
}
