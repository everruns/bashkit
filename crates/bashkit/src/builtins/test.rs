//! test builtin command ([ and test)

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The test builtin command.
pub struct Test;

#[async_trait]
impl Builtin for Test {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Handle empty args - returns false
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(String::new(), 1));
        }

        // Parse and evaluate the expression
        let result = evaluate_expression(ctx.args);

        if result {
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::err(String::new(), 1))
        }
    }
}

/// The [ builtin (alias for test, but expects ] as last arg)
pub struct Bracket;

#[async_trait]
impl Builtin for Bracket {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Check for closing ]
        if ctx.args.is_empty() || ctx.args.last() != Some(&"]".to_string()) {
            return Ok(ExecResult::err("missing ]\n".to_string(), 2));
        }

        // Remove the trailing ]
        let args: Vec<String> = ctx.args[..ctx.args.len() - 1].to_vec();

        // Handle empty args - returns false
        if args.is_empty() {
            return Ok(ExecResult::err(String::new(), 1));
        }

        // Parse and evaluate the expression
        let result = evaluate_expression(&args);

        if result {
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::err(String::new(), 1))
        }
    }
}

/// Evaluate a test expression
fn evaluate_expression(args: &[String]) -> bool {
    if args.is_empty() {
        return false;
    }

    // Handle negation
    if args[0] == "!" {
        return !evaluate_expression(&args[1..]);
    }

    // Handle parentheses (basic support)
    if args[0] == "(" && args.last().map(|s| s.as_str()) == Some(")") {
        return evaluate_expression(&args[1..args.len() - 1]);
    }

    // Look for binary operators
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            // Logical operators (lowest precedence)
            "-a" if i > 0 => {
                return evaluate_expression(&args[..i]) && evaluate_expression(&args[i + 1..]);
            }
            "-o" if i > 0 => {
                return evaluate_expression(&args[..i]) || evaluate_expression(&args[i + 1..]);
            }
            _ => {}
        }
    }

    // Now handle binary comparisons and unary tests
    match args.len() {
        1 => {
            // Single arg: true if non-empty string
            !args[0].is_empty()
        }
        2 => {
            // Unary operators
            evaluate_unary(&args[0], &args[1])
        }
        3 => {
            // Binary operators
            evaluate_binary(&args[0], &args[1], &args[2])
        }
        _ => false,
    }
}

/// Evaluate a unary test expression
fn evaluate_unary(op: &str, arg: &str) -> bool {
    match op {
        // String tests
        "-z" => arg.is_empty(),
        "-n" => !arg.is_empty(),

        // File tests (basic support - always false for now as we don't have real FS access)
        "-e" | "-a" => false, // file exists
        "-f" => false,        // regular file
        "-d" => false,        // directory
        "-r" => false,        // readable
        "-w" => false,        // writable
        "-x" => false,        // executable
        "-s" => false,        // file exists and has size > 0
        "-L" | "-h" => false, // symbolic link
        "-p" => false,        // named pipe
        "-S" => false,        // socket
        "-b" => false,        // block device
        "-c" => false,        // character device
        "-t" => false,        // file descriptor is open and refers to a terminal

        _ => false,
    }
}

/// Evaluate a binary test expression
fn evaluate_binary(left: &str, op: &str, right: &str) -> bool {
    match op {
        // String comparisons
        "=" | "==" => left == right,
        "!=" => left != right,
        "<" => left < right,
        ">" => left > right,

        // Numeric comparisons
        "-eq" => parse_int(left) == parse_int(right),
        "-ne" => parse_int(left) != parse_int(right),
        "-lt" => parse_int(left) < parse_int(right),
        "-le" => parse_int(left) <= parse_int(right),
        "-gt" => parse_int(left) > parse_int(right),
        "-ge" => parse_int(left) >= parse_int(right),

        // File comparisons (not implemented)
        "-nt" | "-ot" | "-ef" => false,

        _ => false,
    }
}

fn parse_int(s: &str) -> i64 {
    s.parse().unwrap_or(0)
}
