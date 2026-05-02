//! numfmt builtin - convert numbers to/from human-readable format
//!
//! Supports --to=si/iec/iec-i, --from=si/iec/auto, --suffix, --padding,
//! --round, --format, --field, --delimiter.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Maximum output size to prevent memory exhaustion.
/// THREAT[TM-DOS-059]: Bound numfmt output
const MAX_OUTPUT_BYTES: usize = 1_048_576;
const MAX_PADDING_WIDTH: usize = MAX_OUTPUT_BYTES;
const MAX_FORMAT_PRECISION: usize = MAX_OUTPUT_BYTES;

pub struct Numfmt;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Scale {
    None,
    Si,
    Iec,
    IecI,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RoundMode {
    FromZero,
    TowardsZero,
    Up,
    Down,
    Nearest,
}

struct Options {
    from: Scale,
    to: Scale,
    suffix: String,
    padding: i32,
    round: RoundMode,
    format: Option<String>,
    field: usize,
    delimiter: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            from: Scale::None,
            to: Scale::None,
            suffix: String::new(),
            padding: 0,
            round: RoundMode::FromZero,
            format: None,
            field: 1,
            delimiter: None,
        }
    }
}

fn parse_scale(s: &str) -> std::result::Result<Scale, String> {
    match s {
        "none" => Ok(Scale::None),
        "si" => Ok(Scale::Si),
        "iec" => Ok(Scale::Iec),
        "iec-i" => Ok(Scale::IecI),
        "auto" => Ok(Scale::Auto),
        _ => Err(format!("numfmt: invalid unit size: '{}'\n", s)),
    }
}

fn parse_round(s: &str) -> std::result::Result<RoundMode, String> {
    match s {
        "up" => Ok(RoundMode::Up),
        "down" => Ok(RoundMode::Down),
        "from-zero" => Ok(RoundMode::FromZero),
        "towards-zero" => Ok(RoundMode::TowardsZero),
        "nearest" => Ok(RoundMode::Nearest),
        _ => Err(format!("numfmt: invalid rounding mode: '{}'\n", s)),
    }
}

/// SI suffixes: K=1000, M=1e6, G=1e9, T=1e12, P=1e15, E=1e18
const SI_SUFFIXES: &[(char, f64)] = &[
    ('K', 1e3),
    ('M', 1e6),
    ('G', 1e9),
    ('T', 1e12),
    ('P', 1e15),
    ('E', 1e18),
];

/// IEC suffixes: K=1024, M=1024^2, G=1024^3, ...
const IEC_SUFFIXES: &[(char, f64)] = &[
    ('K', 1024.0),
    ('M', 1_048_576.0),
    ('G', 1_073_741_824.0),
    ('T', 1_099_511_627_776.0),
    ('P', 1_125_899_906_842_624.0),
    ('E', 1_152_921_504_606_846_976.0),
];

fn round_value(val: f64, mode: RoundMode) -> f64 {
    match mode {
        RoundMode::Up => val.ceil(),
        RoundMode::Down => val.floor(),
        RoundMode::FromZero => {
            if val >= 0.0 {
                val.ceil()
            } else {
                val.floor()
            }
        }
        RoundMode::TowardsZero => {
            if val >= 0.0 {
                val.floor()
            } else {
                val.ceil()
            }
        }
        RoundMode::Nearest => val.round(),
    }
}

/// Parse an input number, possibly with a suffix (from --from mode).
fn parse_number(input: &str, from: Scale) -> std::result::Result<f64, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err(format!("numfmt: invalid number: '{}'\n", input.trim_end()));
    }

    match from {
        Scale::None => s
            .parse::<f64>()
            .map_err(|_| format!("numfmt: invalid number: '{}'\n", s)),
        Scale::Si | Scale::Iec | Scale::IecI | Scale::Auto => {
            // Try to split trailing suffix
            let (num_part, suffix) = split_suffix(s);
            let base: f64 = num_part
                .parse()
                .map_err(|_| format!("numfmt: invalid number: '{}'\n", s))?;

            if suffix.is_empty() {
                return Ok(base);
            }

            let suffix_upper = suffix.to_ascii_uppercase();
            let Some(first_char) = suffix_upper.chars().next() else {
                return Err(format!("numfmt: invalid suffix in input: '{}'\n", s));
            };

            // Auto: if suffix ends with 'i' (like Ki, Mi), use IEC; otherwise SI
            let use_iec = match from {
                Scale::Iec | Scale::IecI => true,
                Scale::Auto => suffix_upper.ends_with('I') && suffix_upper.len() >= 2,
                _ => false,
            };

            let table = if use_iec { IEC_SUFFIXES } else { SI_SUFFIXES };

            for &(c, factor) in table {
                if first_char == c {
                    return Ok(base * factor);
                }
            }

            Err(format!("numfmt: invalid suffix in input: '{}'\n", s))
        }
    }
}

fn split_suffix(s: &str) -> (&str, &str) {
    // Find where the numeric part ends
    let end = s
        .rfind(|c: char| c.is_ascii_digit() || c == '.')
        .map(|i| i + 1)
        .unwrap_or(0);
    (&s[..end], &s[end..])
}

/// Format a number for output with --to mode.
fn format_number(val: f64, to: Scale, round: RoundMode, suffix: &str, padding: i32) -> String {
    let formatted = match to {
        Scale::None => {
            let rounded = round_value(val, round);
            if rounded.fract() == 0.0 && rounded.abs() < i64::MAX as f64 {
                format!("{}{}", rounded as i64, suffix)
            } else {
                format!("{}{}", rounded, suffix)
            }
        }
        Scale::Si => format_with_scale(val, SI_SUFFIXES, false, round, suffix),
        Scale::Iec => format_with_scale(val, IEC_SUFFIXES, false, round, suffix),
        Scale::IecI => format_with_scale(val, IEC_SUFFIXES, true, round, suffix),
        Scale::Auto => {
            // --to=auto not valid, treat as none
            let rounded = round_value(val, round);
            format!("{}{}", rounded, suffix)
        }
    };

    apply_padding(&formatted, padding)
}

fn format_with_scale(
    val: f64,
    table: &[(char, f64)],
    iec_i_suffix: bool,
    round: RoundMode,
    suffix: &str,
) -> String {
    let abs_val = val.abs();

    // Find the largest unit that gives a value >= 1
    let mut chosen: Option<(char, f64)> = None;
    for &(c, factor) in table {
        if abs_val >= factor {
            chosen = Some((c, factor));
        }
    }

    match chosen {
        Some((c, factor)) => {
            let scaled = val / factor;
            let display = format_scaled_value(scaled, round);
            if iec_i_suffix {
                format!("{}{}i{}", display, c, suffix)
            } else {
                format!("{}{}{}", display, c, suffix)
            }
        }
        None => {
            // Value too small for any suffix
            let rounded = round_value(val, round);
            if rounded.fract() == 0.0 && rounded.abs() < i64::MAX as f64 {
                format!("{}{}", rounded as i64, suffix)
            } else {
                format!("{}{}", rounded, suffix)
            }
        }
    }
}

/// Format a scaled value like "1.0", "1.5", etc.
/// GNU numfmt shows one decimal place when the value is < 10.
fn format_scaled_value(val: f64, round: RoundMode) -> String {
    let abs = val.abs();
    if abs < 10.0 {
        // One decimal place, with rounding applied to the tenths
        let shifted = val * 10.0;
        let rounded = round_value(shifted, round) / 10.0;
        format!("{:.1}", rounded)
    } else {
        let rounded = round_value(val, round);
        format!("{}", rounded as i64)
    }
}

fn apply_padding(s: &str, padding: i32) -> String {
    let width = padding.unsigned_abs() as usize;
    if width > MAX_PADDING_WIDTH {
        return s.to_string();
    }
    if width <= s.len() {
        return s.to_string();
    }
    if padding > 0 {
        // Right-align (pad with spaces on left)
        format!("{:>width$}", s, width = width)
    } else {
        // Left-align (pad with spaces on right)
        format!("{:<width$}", s, width = width)
    }
}

fn parse_padding(val: &str) -> std::result::Result<i32, String> {
    let padding: i32 = val
        .parse()
        .map_err(|_| format!("numfmt: invalid padding value: '{}'\n", val))?;
    if padding.unsigned_abs() as usize > MAX_PADDING_WIDTH {
        return Err(format!("numfmt: padding too large: '{}'\n", val));
    }
    Ok(padding)
}

fn parse_options(args: &[String]) -> std::result::Result<(Options, Vec<String>), String> {
    let mut opts = Options::default();
    let mut operands = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            operands.extend(args[i + 1..].iter().cloned());
            break;
        } else if let Some(val) = arg.strip_prefix("--to=") {
            opts.to = parse_scale(val)?;
        } else if arg == "--to" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --to\n".to_string());
            }
            opts.to = parse_scale(&args[i])?;
        } else if let Some(val) = arg.strip_prefix("--from=") {
            opts.from = parse_scale(val)?;
        } else if arg == "--from" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --from\n".to_string());
            }
            opts.from = parse_scale(&args[i])?;
        } else if let Some(val) = arg.strip_prefix("--suffix=") {
            opts.suffix = val.to_string();
        } else if arg == "--suffix" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --suffix\n".to_string());
            }
            opts.suffix = args[i].clone();
        } else if let Some(val) = arg.strip_prefix("--padding=") {
            opts.padding = parse_padding(val)?;
        } else if arg == "--padding" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --padding\n".to_string());
            }
            opts.padding = parse_padding(&args[i])?;
        } else if let Some(val) = arg.strip_prefix("--round=") {
            opts.round = parse_round(val)?;
        } else if arg == "--round" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --round\n".to_string());
            }
            opts.round = parse_round(&args[i])?;
        } else if let Some(val) = arg.strip_prefix("--format=") {
            opts.format = Some(val.to_string());
        } else if arg == "--format" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --format\n".to_string());
            }
            opts.format = Some(args[i].clone());
        } else if let Some(val) = arg.strip_prefix("--field=") {
            opts.field = val
                .parse()
                .map_err(|_| format!("numfmt: invalid field value: '{}'\n", val))?;
            if opts.field == 0 {
                return Err("numfmt: invalid field value: '0'\n".to_string());
            }
        } else if arg == "--field" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --field\n".to_string());
            }
            opts.field = args[i]
                .parse()
                .map_err(|_| format!("numfmt: invalid field value: '{}'\n", &args[i]))?;
            if opts.field == 0 {
                return Err("numfmt: invalid field value: '0'\n".to_string());
            }
        } else if let Some(val) = arg.strip_prefix("--delimiter=") {
            opts.delimiter = Some(val.to_string());
        } else if arg == "--delimiter" || arg == "-d" {
            i += 1;
            if i >= args.len() {
                return Err("numfmt: missing argument for --delimiter\n".to_string());
            }
            opts.delimiter = Some(args[i].clone());
        } else if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            // Unknown short option
            return Err(format!("numfmt: invalid option -- '{}'\n", &arg[1..]));
        } else if arg.starts_with("--") {
            return Err(format!("numfmt: unrecognized option '{}'\n", arg));
        } else {
            operands.push(arg.clone());
        }
        i += 1;
    }

    Ok((opts, operands))
}

fn convert_line(line: &str, opts: &Options) -> std::result::Result<String, String> {
    if let Some(ref delim) = opts.delimiter {
        // Split by delimiter, convert the specified field
        let parts: Vec<&str> = line.split(delim.as_str()).collect();
        let field_idx = opts.field - 1;
        if field_idx >= parts.len() {
            return Ok(line.to_string());
        }
        let val = parse_number(parts[field_idx], opts.from)?;
        let converted = format_number(val, opts.to, opts.round, &opts.suffix, opts.padding);
        let mut result_parts: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        result_parts[field_idx] = converted;
        Ok(result_parts.join(delim))
    } else if opts.field > 1 {
        // Split by whitespace, convert the specified field
        let parts: Vec<&str> = line.split_whitespace().collect();
        let field_idx = opts.field - 1;
        if field_idx >= parts.len() {
            return Ok(line.to_string());
        }
        let val = parse_number(parts[field_idx], opts.from)?;
        let converted = format_number(val, opts.to, opts.round, &opts.suffix, opts.padding);
        let mut result_parts: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        result_parts[field_idx] = converted;
        Ok(result_parts.join(" "))
    } else {
        // Convert the whole line (trimmed)
        let trimmed = line.trim();
        // Strip user suffix before parsing if present
        let to_parse = if !opts.suffix.is_empty() {
            trimmed.strip_suffix(&opts.suffix).unwrap_or(trimmed)
        } else {
            trimmed
        };
        let val = parse_number(to_parse, opts.from)?;
        format_with_printf(val, opts)
    }
}

fn format_with_printf(val: f64, opts: &Options) -> std::result::Result<String, String> {
    if let Some(ref fmt) = opts.format {
        // Basic printf-style: support %f, %g, %e with optional width/precision
        apply_printf_format(val, fmt, &opts.suffix, opts.padding)
    } else {
        Ok(format_number(
            val,
            opts.to,
            opts.round,
            &opts.suffix,
            opts.padding,
        ))
    }
}

fn apply_printf_format(
    val: f64,
    fmt: &str,
    suffix: &str,
    padding: i32,
) -> std::result::Result<String, String> {
    // Find the % format specifier
    let Some(pct_pos) = fmt.find('%') else {
        return Ok(format!("{}{}", fmt, suffix));
    };

    let before = &fmt[..pct_pos];
    let rest = &fmt[pct_pos + 1..];

    // Find the conversion character (f, g, e, d)
    let conv_pos = rest
        .find(['f', 'g', 'e', 'd', 'i'])
        .ok_or_else(|| format!("numfmt: invalid format '{}'\n", fmt))?;

    let spec = &rest[..conv_pos];
    let conv = rest.as_bytes()[conv_pos] as char;
    let after = &rest[conv_pos + 1..];

    let formatted = match conv {
        'f' => {
            if let Some(dot_pos) = spec.find('.') {
                let precision: usize = spec[dot_pos + 1..]
                    .parse()
                    .map_err(|_| format!("numfmt: invalid format '{}'\n", fmt))?;
                if precision > MAX_FORMAT_PRECISION {
                    return Err(format!("numfmt: format precision too large: '{}'\n", fmt));
                }
                format!("{:.prec$}", val, prec = precision)
            } else {
                format!("{:.6}", val)
            }
        }
        'g' => format!("{}", val),
        'e' => format!("{:e}", val),
        'd' | 'i' => format!("{}", val as i64),
        _ => unreachable!(),
    };

    let result = format!("{}{}{}{}", before, formatted, suffix, after);
    Ok(apply_padding(&result, padding))
}

#[async_trait]
impl Builtin for Numfmt {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: numfmt [OPTION]... [NUMBER]...\nReformat NUMBER(s), or the numbers from standard input.\n\n  --from=UNIT\tauto-scale input numbers to UNITs (none, si, iec, iec-i, auto)\n  --to=UNIT\tauto-scale output numbers to UNITs (none, si, iec, iec-i)\n  --suffix=SUFFIX\tadd SUFFIX to output numbers\n  --padding=N\tpad the output to N characters\n  --round=METHOD\tuse METHOD for rounding (up, down, from-zero, towards-zero, nearest)\n  --format=FORMAT\tuse printf-style FORMAT\n  --field=N\treplace the number in input field N (default 1)\n  -d, --delimiter=X\tuse X instead of whitespace for field delimiter\n  --help\t\t\tdisplay this help and exit\n  --version\t\toutput version information and exit\n",
            Some("numfmt (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, operands) = match parse_options(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(e, 1)),
        };

        let mut output = String::new();

        if operands.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                for line in stdin.lines() {
                    if output.len() > MAX_OUTPUT_BYTES {
                        break;
                    }
                    match convert_line(line, &opts) {
                        Ok(converted) => {
                            output.push_str(&converted);
                            output.push('\n');
                        }
                        Err(e) => return Ok(ExecResult::err(e, 2)),
                    }
                }
            }
        } else {
            // Process each operand
            for operand in &operands {
                if output.len() > MAX_OUTPUT_BYTES {
                    break;
                }
                match convert_line(operand, &opts) {
                    Ok(converted) => {
                        output.push_str(&converted);
                        output.push('\n');
                    }
                    Err(e) => return Ok(ExecResult::err(e, 2)),
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number_plain() {
        assert_eq!(parse_number("1024", Scale::None).unwrap(), 1024.0);
        assert_eq!(parse_number("1048576", Scale::None).unwrap(), 1048576.0);
    }

    #[test]
    fn test_parse_number_iec() {
        assert_eq!(parse_number("1K", Scale::Iec).unwrap(), 1024.0);
        assert_eq!(parse_number("1M", Scale::Iec).unwrap(), 1_048_576.0);
    }

    #[test]
    fn test_parse_number_si() {
        assert_eq!(parse_number("1K", Scale::Si).unwrap(), 1000.0);
        assert_eq!(parse_number("1M", Scale::Si).unwrap(), 1_000_000.0);
    }

    #[test]
    fn test_format_to_iec() {
        let s = format_number(1_048_576.0, Scale::Iec, RoundMode::FromZero, "", 0);
        assert_eq!(s, "1.0M");
    }

    #[test]
    fn test_format_to_si() {
        let s = format_number(1_048_576.0, Scale::Si, RoundMode::FromZero, "", 0);
        assert_eq!(s, "1.1M");
    }

    #[test]
    fn test_format_to_iec_i() {
        let s = format_number(1_048_576.0, Scale::IecI, RoundMode::FromZero, "", 0);
        assert_eq!(s, "1.0Mi");
    }

    #[test]
    fn test_format_with_suffix() {
        let s = format_number(1_048_576.0, Scale::Iec, RoundMode::FromZero, "B", 0);
        assert_eq!(s, "1.0MB");
    }

    #[test]
    fn test_round_modes() {
        assert_eq!(round_value(1.1, RoundMode::Up), 2.0);
        assert_eq!(round_value(1.9, RoundMode::Down), 1.0);
        assert_eq!(round_value(1.5, RoundMode::Nearest), 2.0);
        assert_eq!(round_value(-1.5, RoundMode::FromZero), -2.0);
        assert_eq!(round_value(-1.5, RoundMode::TowardsZero), -1.0);
    }

    #[test]
    fn test_padding() {
        let s = format_number(1024.0, Scale::Iec, RoundMode::FromZero, "", 10);
        assert_eq!(s, "      1.0K");
    }

    #[test]
    fn test_invalid_number() {
        assert!(parse_number("abc", Scale::None).is_err());
        assert!(parse_number("", Scale::None).is_err());
    }

    #[test]
    fn test_parse_options_rejects_oversized_padding() {
        let args = vec!["--padding=2000000".to_string(), "1".to_string()];
        match parse_options(&args) {
            Ok(_) => panic!("expected parse_options to fail for oversized padding"),
            Err(err) => assert!(err.contains("padding too large")),
        }
    }

    #[test]
    fn test_apply_printf_format_rejects_oversized_precision() {
        let err = apply_printf_format(1.0, "%.2000000f", "", 0).unwrap_err();
        assert!(err.contains("format precision too large"));
    }

    // TM-INF-022: garbage-input stderr must not leak parser Debug shapes.
    #[tokio::test]
    async fn no_leak_garbage_input() {
        let r =
            crate::builtins::debug_leak_check::run(r#"echo 'not a number' | numfmt --from=iec"#)
                .await;
        crate::builtins::debug_leak_check::assert_no_leak(
            &r,
            "numfmt_garbage",
            &["ParseFloatError", "ParseIntError"],
        );
    }
}
