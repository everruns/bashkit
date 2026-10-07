//! seq builtin - print a sequence of numbers
//!
//! Decision: output formatting follows GNU `seq.c`: the default format is
//! `%.Pf` with P the larger fraction-digit count of FIRST and INCREMENT
//! (LAST does not count), `-w` zero-pads to the wider of FIRST/LAST in that
//! format, `-f` goes through the printf formatter, and each value is
//! computed as FIRST + i*INCREMENT so steps do not accumulate rounding.

use async_trait::async_trait;

use super::limits::{SEQ_MAX_LINES, SEQ_MAX_OUTPUT_BYTES, cap_exceeded};
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
///   -f FORMAT  Use printf-style floating-point FORMAT
///   -s STRING  Use STRING as separator (default: newline)
///   -w         Equalize width by padding with leading zeroes
pub struct Seq;

/// A parsed operand with GNU's precision/width bookkeeping.
struct Operand {
    value: f64,
    /// Digits after the decimal point; `None` when not representable
    /// as fixed-point (hex, inf, nan).
    precision: Option<usize>,
    width: usize,
}

fn parse_operand(arg: &str) -> Option<Operand> {
    let t = arg.trim_start();
    let value: f64 = {
        let lower = t.to_ascii_lowercase();
        let body = lower.trim_start_matches(['+', '-']);
        if let Some(hex) = body.strip_prefix("0x") {
            let neg = lower.starts_with('-');
            let v = u64::from_str_radix(hex, 16).ok()? as f64;
            if neg { -v } else { v }
        } else {
            t.parse().ok()?
        }
    };
    if value.is_nan() {
        return None;
    }
    let plain = !t.to_ascii_lowercase().contains(['x', 'i', 'n']);
    let precision = plain.then(|| {
        let mantissa_end = t.find(['e', 'E']).unwrap_or(t.len());
        let frac = t[..mantissa_end]
            .split_once('.')
            .map_or(0, |(_, f)| f.len());
        let exp: i64 = t
            .get(mantissa_end + 1..)
            .and_then(|e| e.parse().ok())
            .unwrap_or(0);
        if exp < 0 {
            frac + exp.unsigned_abs() as usize
        } else {
            frac.saturating_sub(exp as usize)
        }
    });
    Some(Operand {
        value,
        precision,
        width: t.len(),
    })
}

/// GNU `get_default_format`, as (zero-pad width, precision).
fn default_format(
    first: &Operand,
    step: &Operand,
    last: &Operand,
    equal_width: bool,
) -> Option<(usize, usize)> {
    let prec = first.precision?.max(step.precision?);
    let last_prec = last.precision?;
    if !equal_width {
        return Some((0, prec));
    }
    let first_prec = first.precision?;
    let mut first_width = first.width + (prec - first_prec);
    let mut last_width = (last.width + prec).saturating_sub(last_prec);
    if last_prec > 0 && prec == 0 {
        last_width = last_width.saturating_sub(1);
    }
    if last_prec == 0 && prec > 0 {
        last_width += 1;
    }
    if first_prec == 0 && prec > 0 {
        first_width += 1;
    }
    Some((first_width.max(last_width), prec))
}

fn invalid_arg(arg: &str) -> ExecResult {
    ExecResult::err(
        format!(
            "seq: invalid floating point argument: '{arg}'\nTry 'seq --help' for more information.\n"
        ),
        1,
    )
}

#[async_trait]
impl Builtin for Seq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: seq [OPTION]... LAST\n  or:  seq [OPTION]... FIRST LAST\n  or:  seq [OPTION]... FIRST INCREMENT LAST\nPrint numbers from FIRST to LAST, in steps of INCREMENT.\n\n  -f, --format=FORMAT\tuse printf style floating-point FORMAT\n  -s, --separator=STRING\tuse STRING to separate numbers (default: \\n)\n  -w, --equal-width\tequalize width by padding with leading zeroes\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("seq (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        // GNU checks each option position: an argument that looks like a
        // negative number ends option parsing, so `seq -3 -1` counts while
        // `seq -s -1 1 2` still takes `-1` as the separator.
        let mut separator = "\n".to_string();
        let mut equal_width = false;
        let mut format: Option<String> = None;
        let mut nums: Vec<String> = Vec::new();
        let args = ctx.args;
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            i += 1;
            let b = a.as_bytes();
            if b.len() > 1 && b[0] == b'-' && (b[1].is_ascii_digit() || b[1] == b'.') {
                nums.extend(args[i - 1..].iter().cloned());
                break;
            }
            if a == "--" {
                nums.extend(args[i..].iter().cloned());
                break;
            }
            if !a.starts_with('-') || a == "-" {
                nums.push(a.to_string());
                continue;
            }
            if let Some(long) = a.strip_prefix("--") {
                let (name, inline) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (long, None),
                };
                let key = ["equal-width", "format", "separator"]
                    .into_iter()
                    .find(|n| !name.is_empty() && n.starts_with(name));
                let Some(key) = key else {
                    return Ok(super::invalid_option("seq", a, 1));
                };
                if key == "equal-width" {
                    equal_width = true;
                    continue;
                }
                let value = match inline {
                    Some(v) => v,
                    None if i < args.len() => {
                        i += 1;
                        args[i - 1].clone()
                    }
                    None => {
                        return Ok(ExecResult::err(
                            format!("seq: option '--{key}' requires an argument\n"),
                            1,
                        ));
                    }
                };
                if key == "format" {
                    format = Some(value);
                } else {
                    separator = value;
                }
                continue;
            }
            for (pos, c) in a[1..].char_indices() {
                match c {
                    'w' => equal_width = true,
                    'f' | 's' => {
                        let attached = &a[1 + pos + 1..];
                        let value = if !attached.is_empty() {
                            attached.to_string()
                        } else if i < args.len() {
                            i += 1;
                            args[i - 1].clone()
                        } else {
                            return Ok(ExecResult::err(
                                format!("seq: option requires an argument -- '{c}'\n"),
                                1,
                            ));
                        };
                        if c == 'f' {
                            format = Some(value);
                        } else {
                            separator = value;
                        }
                        break;
                    }
                    _ => return Ok(super::invalid_option("seq", &format!("-{c}"), 1)),
                }
            }
        }
        if format.is_some() && equal_width {
            return Ok(ExecResult::err(
                "seq: format string may not be specified when printing equal width strings\n"
                    .to_string(),
                1,
            ));
        }

        if nums.is_empty() {
            return Ok(ExecResult::err("seq: missing operand\n".to_string(), 1));
        }
        if nums.len() > 3 {
            return Ok(ExecResult::err(
                format!("seq: extra operand '{}'\n", nums[3]),
                1,
            ));
        }
        let mut parsed = Vec::with_capacity(3);
        for n in &nums {
            match parse_operand(n) {
                Some(op) => parsed.push(op),
                None => return Ok(invalid_arg(n)),
            }
        }
        let one = || Operand {
            value: 1.0,
            precision: Some(0),
            width: 1,
        };
        let (first, step, last) = match parsed.len() {
            1 => (one(), one(), parsed.remove(0)),
            2 => {
                let last = parsed.remove(1);
                (parsed.remove(0), one(), last)
            }
            _ => {
                let last = parsed.remove(2);
                let step = parsed.remove(1);
                (parsed.remove(0), step, last)
            }
        };

        if step.value == 0.0 {
            return Ok(ExecResult::err("seq: zero increment\n".to_string(), 1));
        }

        let fixed = default_format(&first, &step, &last, equal_width);
        let render = |x: f64| -> std::result::Result<String, String> {
            if let Some(fmt) = &format {
                let bytes = super::printf::render_printf_bytes(fmt, &[format!("{x}")])?;
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
            Ok(match fixed {
                Some((width, prec)) => format!("{x:0width$.prec$}"),
                None => {
                    let bytes = super::printf::render_printf_bytes("%g", &[format!("{x}")])?;
                    String::from_utf8_lossy(&bytes).into_owned()
                }
            })
        };

        let mut output = String::new();
        // THREAT[TM-DOS-058]: Limit iterations and output size to prevent memory
        // exhaustion; THREAT[TM-DOS-109]: hitting either cap is reported.
        let mut capped = None;
        let mut i: u64 = 0;
        loop {
            let x = first.value + (i as f64) * step.value;
            if (step.value > 0.0 && x > last.value) || (step.value < 0.0 && x < last.value) {
                break;
            }
            if i as usize >= SEQ_MAX_LINES {
                capped = Some(("line", SEQ_MAX_LINES));
                break;
            }
            if output.len() > SEQ_MAX_OUTPUT_BYTES {
                capped = Some(("output byte", SEQ_MAX_OUTPUT_BYTES));
                break;
            }
            if i > 0 {
                output.push_str(&separator);
            }
            match render(x) {
                Ok(s) => output.push_str(&s),
                Err(e) => return Ok(ExecResult::err(e.replacen("printf:", "seq:", 1), 1)),
            }
            i += 1;
            if !x.is_finite() {
                break;
            }
        }

        if i > 0 {
            output.push('\n');
        }

        if let Some((what, limit)) = capped {
            return Ok(cap_exceeded("seq", output, what, limit));
        }
        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
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

    async fn run(args: &[&str]) -> ExecResult {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Seq.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn seq_precision_follows_first_and_step() {
        assert_eq!(run(&["1", "0.5", "2"]).await.stdout, "1.0\n1.5\n2.0\n");
        assert_eq!(run(&["1", "2.50"]).await.stdout, "1\n2\n");
        assert_eq!(
            run(&["-w", "1", "-0.5", "-1"]).await.stdout,
            "01.0\n00.5\n00.0\n-0.5\n-1.0\n"
        );
    }

    #[tokio::test]
    async fn seq_format_and_separator_value() {
        assert_eq!(run(&["-f", "%03g", "9", "10"]).await.stdout, "009\n010\n");
        assert_eq!(run(&["-s", "-1", "1", "2"]).await.stdout, "1-12\n");
        assert_eq!(run(&["-f", "%e", "1"]).await.stdout, "1.000000e+00\n");
    }
}
