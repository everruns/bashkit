//! seq builtin - print a sequence of numbers

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The seq builtin - print a sequence of numbers.
///
/// Usage: seq [OPTION]... LAST
///        seq [OPTION]... FIRST LAST
///        seq [OPTION]... FIRST INCREMENT LAST
///
/// Options:
///   -s STRING  Use STRING as separator (default: newline)
///   -w         Equalize width by padding with leading zeroes
pub struct Seq;

#[async_trait]
impl Builtin for Seq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut separator = "\n".to_string();
        let mut equal_width = false;
        let mut nums: Vec<String> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-s" => {
                    i += 1;
                    if i < ctx.args.len() {
                        separator = ctx.args[i].clone();
                    }
                }
                "-w" => equal_width = true,
                arg if arg.starts_with("-s") => {
                    // -sSEP (no space)
                    separator = arg[2..].to_string();
                }
                _ => {
                    nums.push(ctx.args[i].clone());
                }
            }
            i += 1;
        }

        if nums.is_empty() {
            return Ok(ExecResult::err("seq: missing operand\n".to_string(), 1));
        }

        let (first, increment, last) = match nums.len() {
            1 => {
                let last: f64 = match nums[0].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("seq: invalid floating point argument: '{}'\n", nums[0]),
                            1,
                        ));
                    }
                };
                (1.0_f64, 1.0_f64, last)
            }
            2 => {
                let first: f64 = match nums[0].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("seq: invalid floating point argument: '{}'\n", nums[0]),
                            1,
                        ));
                    }
                };
                let last: f64 = match nums[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("seq: invalid floating point argument: '{}'\n", nums[1]),
                            1,
                        ));
                    }
                };
                (first, 1.0, last)
            }
            _ => {
                let first: f64 = match nums[0].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("seq: invalid floating point argument: '{}'\n", nums[0]),
                            1,
                        ));
                    }
                };
                let increment: f64 = match nums[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("seq: invalid floating point argument: '{}'\n", nums[1]),
                            1,
                        ));
                    }
                };
                let last: f64 = match nums[2].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("seq: invalid floating point argument: '{}'\n", nums[2]),
                            1,
                        ));
                    }
                };
                (first, increment, last)
            }
        };

        if increment == 0.0 {
            return Ok(ExecResult::err("seq: zero increment\n".to_string(), 1));
        }

        // Determine if all values are integers
        let all_integer = first.fract() == 0.0 && increment.fract() == 0.0 && last.fract() == 0.0;

        // Calculate width for -w flag
        let width = if equal_width && all_integer {
            let first_w = format!("{}", first as i64).len();
            let last_w = format!("{}", last as i64).len();
            first_w.max(last_w)
        } else {
            0
        };

        let mut output = String::new();
        let mut current = first;
        let mut first_item = true;

        // Safety: limit iterations to prevent infinite loops
        let max_iterations = 1_000_000;
        let mut count = 0;

        loop {
            if increment > 0.0 && current > last + f64::EPSILON {
                break;
            }
            if increment < 0.0 && current < last - f64::EPSILON {
                break;
            }
            count += 1;
            if count > max_iterations {
                break;
            }

            if !first_item {
                output.push_str(&separator);
            }
            first_item = false;

            if all_integer {
                let val = current as i64;
                if equal_width {
                    output.push_str(&format!("{:0>width$}", val, width = width));
                } else {
                    output.push_str(&format!("{}", val));
                }
            } else {
                // Format float, removing trailing zeros
                let formatted = format!("{:.10}", current);
                let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
                output.push_str(trimmed);
            }

            current += increment;
        }

        if !output.is_empty() {
            output.push('\n');
        }

        Ok(ExecResult::ok(output))
    }
}
