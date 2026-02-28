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

    // ==================== basic ranges ====================

    #[tokio::test]
    async fn seq_single_arg() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n3\n4\n5\n");
    }

    #[tokio::test]
    async fn seq_two_args() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["3".to_string(), "6".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "3\n4\n5\n6\n");
    }

    #[tokio::test]
    async fn seq_three_args_increment() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["1".to_string(), "2".to_string(), "9".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "1\n3\n5\n7\n9\n");
    }

    #[tokio::test]
    async fn seq_descending() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-1".to_string(), "1".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "5\n4\n3\n2\n1\n");
    }

    #[tokio::test]
    async fn seq_single_element() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["1".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn seq_empty_range() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        // first > last with positive increment => empty output
        let args = vec!["5".to_string(), "1".to_string(), "3".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    // ==================== separator (-s) ====================

    #[tokio::test]
    async fn seq_custom_separator() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-s".to_string(), ",".to_string(), "3".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "1,2,3\n");
    }

    #[tokio::test]
    async fn seq_separator_no_space() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-s,".to_string(), "3".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "1,2,3\n");
    }

    // ==================== zero-padding (-w) ====================

    #[tokio::test]
    async fn seq_zero_padding() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-w".to_string(), "8".to_string(), "10".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "08\n09\n10\n");
    }

    #[tokio::test]
    async fn seq_zero_padding_large() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-w".to_string(), "1".to_string(), "100".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines[0], "001");
        assert_eq!(lines[99], "100");
    }

    // ==================== error cases ====================

    #[tokio::test]
    async fn seq_missing_operand() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn seq_invalid_number() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid floating point"));
    }

    #[tokio::test]
    async fn seq_zero_increment() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["1".to_string(), "0".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("zero increment"));
    }

    // ==================== negative numbers ====================

    #[tokio::test]
    async fn seq_negative_range() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-3".to_string(), "0".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Seq.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "-3\n-2\n-1\n0\n");
    }
}
