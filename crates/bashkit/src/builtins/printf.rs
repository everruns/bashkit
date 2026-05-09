//! printf builtin - formatted output
//!
//! Important decision: uucore owns printf parsing/formatting semantics here;
//! bashkit keeps only shell integration plus a preflight width/precision cap
//! outside generated code so regenerating `format/` cannot erase the DoS guard.

use std::borrow::Cow;
use std::ffi::OsString;
use std::ops::ControlFlow;

use async_trait::async_trait;

use super::generated::format::{
    FormatArgument, FormatArguments, FormatError, FormatItem, parse_spec_and_escape,
};
use super::{Builtin, Context, MAX_FORMAT_WIDTH};
use crate::error::Result;
use crate::interpreter::{ExecResult, is_internal_variable};

const MAX_PRINTF_DIAG_CHARS: usize = 1_024;

/// printf builtin - formatted string output
pub struct Printf;

#[async_trait]
impl Builtin for Printf {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: printf FORMAT [ARGUMENT]...\n  or:  printf OPTION\nPrint ARGUMENT(s) according to FORMAT.\n\n  FORMAT controls the output, supports:\n    %s\tstring\n    %d, %i\tsigned integer\n    %u\tunsigned integer\n    %o\toctal\n    %x, %X\thexadecimal\n    %f, %e, %g\tfloating point\n    %c\tcharacter\n    %b\tstring with backslash escapes\n    %q\tshell-quoted string\n    \\n, \\t, \\\\, \\xHH, \\uHHHH, \\UHHHHHHHH\tescape sequences\n  -v VAR\tassign to shell variable VAR instead of printing\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("printf (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        if ctx.args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let mut args_iter = ctx.args.iter();
        let mut var_name: Option<String> = None;

        let format = loop {
            match args_iter.next() {
                Some(arg) if arg == "-v" => {
                    if let Some(vname) = args_iter.next() {
                        var_name = Some(vname.clone());
                    }
                }
                Some(arg) => break arg.clone(),
                None => return Ok(ExecResult::ok(String::new())),
            }
        };

        let args: Vec<String> = args_iter.cloned().collect();
        let output = match render_printf(&format, &args) {
            Ok(output) => output,
            Err(err) => return Ok(ExecResult::err(err, 1)),
        };

        if let Some(name) = var_name {
            // THREAT[TM-INJ-009]: Block internal variable prefix injection via printf -v
            if is_internal_variable(&name) {
                return Ok(ExecResult::ok(String::new()));
            }
            ctx.variables.insert(name, output);
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::ok(output))
        }
    }
}

fn render_printf(format: &str, args: &[String]) -> std::result::Result<String, String> {
    let format = strip_zero_hex_escapes(format);
    let format = format.as_ref();
    let values = format_arguments(args);
    validate_format_caps(format.as_bytes(), args)?;

    let mut out = Vec::new();
    let mut format_seen = false;
    let mut fmt_args = FormatArguments::new(&values);

    let stopped = write_format_pass(format.as_bytes(), &mut fmt_args, &mut out, &mut format_seen)?;
    fmt_args.start_next_batch();

    if stopped || !format_seen {
        return Ok(bytes_to_stdout_string(out));
    }

    while !fmt_args.is_exhausted() {
        if write_format_pass(format.as_bytes(), &mut fmt_args, &mut out, &mut format_seen)? {
            break;
        }
        fmt_args.start_next_batch();
    }

    Ok(bytes_to_stdout_string(out))
}

fn bytes_to_stdout_string(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn strip_zero_hex_escapes(input: &str) -> Cow<'_, str> {
    let bytes = input.as_bytes();
    let mut index = 0;
    let mut output: Option<Vec<u8>> = None;

    while index < bytes.len() {
        if bytes.get(index) == Some(&b'\\') && bytes.get(index + 1) == Some(&b'x') {
            let first = bytes.get(index + 2).copied().filter(u8::is_ascii_hexdigit);
            let second = bytes.get(index + 3).copied().filter(u8::is_ascii_hexdigit);
            let digits = [first, second];
            let digit_count = digits.iter().flatten().count();
            let zero_hex = digit_count > 0 && digits.iter().flatten().all(|digit| *digit == b'0');
            if zero_hex {
                output.get_or_insert_with(|| bytes[..index].to_vec());
                index += 2 + digit_count;
                continue;
            }
        }

        if let Some(out) = &mut output {
            out.push(bytes[index]);
        }
        index += 1;
    }

    match output {
        Some(bytes) => Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()),
        None => Cow::Borrowed(input),
    }
}

fn format_arguments(args: &[String]) -> Vec<FormatArgument> {
    args.iter()
        .map(|arg| FormatArgument::Unparsed(OsString::from(arg)))
        .collect()
}

fn write_format_pass(
    format: &[u8],
    args: &mut FormatArguments<'_>,
    out: &mut Vec<u8>,
    format_seen: &mut bool,
) -> std::result::Result<bool, String> {
    for item in parse_spec_and_escape(format) {
        let item = item.map_err(|err| render_printf_error(&err))?;
        if matches!(item, FormatItem::Spec(_)) {
            *format_seen = true;
        }
        match item
            .write(&mut *out, args)
            .map_err(|err| render_printf_error(&err))?
        {
            ControlFlow::Continue(()) => {}
            ControlFlow::Break(()) => return Ok(true),
        }
    }
    Ok(false)
}

fn render_printf_error(err: &FormatError) -> String {
    format!(
        "printf: {}\n",
        truncate_text(
            &err.to_string(),
            MAX_PRINTF_DIAG_CHARS.saturating_sub("printf: \n".len())
        )
    )
}

fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    format!("{}...", input.chars().take(keep).collect::<String>())
}

#[derive(Clone, Copy)]
enum CapArgLocation {
    NextArgument,
    Position(usize),
}

struct CapArgs<'a> {
    args: &'a [String],
    next_arg_position: usize,
    highest_arg_position: Option<usize>,
    current_offset: usize,
}

impl<'a> CapArgs<'a> {
    fn new(args: &'a [String]) -> Self {
        Self {
            args,
            next_arg_position: 0,
            highest_arg_position: None,
            current_offset: 0,
        }
    }

    fn is_exhausted(&self) -> bool {
        self.current_offset >= self.args.len()
    }

    fn start_next_batch(&mut self) {
        self.current_offset = self
            .next_arg_position
            .max(self.highest_arg_position.map_or(0, |x| x.saturating_add(1)));
        self.next_arg_position = self.current_offset;
    }

    fn next_i64(&mut self, location: CapArgLocation) -> i64 {
        self.next_arg(location).map(parse_leading_i64).unwrap_or(0)
    }

    fn consume(&mut self, location: CapArgLocation) {
        let _ = self.next_arg(location);
    }

    fn next_arg(&mut self, location: CapArgLocation) -> Option<&'a str> {
        match location {
            CapArgLocation::NextArgument => {
                let arg = self.args.get(self.next_arg_position).map(String::as_str);
                self.next_arg_position += 1;
                arg
            }
            CapArgLocation::Position(pos) => {
                let pos = pos.saturating_sub(1).saturating_add(self.current_offset);
                self.highest_arg_position =
                    Some(self.highest_arg_position.map_or(pos, |x| x.max(pos)));
                self.args.get(pos).map(String::as_str)
            }
        }
    }
}

fn validate_format_caps(format: &[u8], args: &[String]) -> std::result::Result<(), String> {
    let mut args = CapArgs::new(args);
    let (format_seen, stopped) = validate_format_caps_pass(format, &mut args)?;
    args.start_next_batch();

    if stopped || !format_seen {
        return Ok(());
    }

    while !args.is_exhausted() {
        let (_, stopped) = validate_format_caps_pass(format, &mut args)?;
        args.start_next_batch();
        if stopped {
            break;
        }
    }
    Ok(())
}

fn validate_format_caps_pass(
    format: &[u8],
    args: &mut CapArgs<'_>,
) -> std::result::Result<(bool, bool), String> {
    let mut i = 0;
    let mut format_seen = false;
    while i < format.len() {
        match format[i] {
            b'\\' if format.get(i + 1) == Some(&b'c') => return Ok((format_seen, true)),
            b'\\' => {
                i = i.saturating_add(2);
            }
            b'%' if format.get(i + 1) == Some(&b'%') => {
                i += 2;
            }
            b'%' => {
                let Some(spec) = parse_cap_spec(format, i + 1) else {
                    i += 1;
                    continue;
                };
                spec.validate(args)?;
                format_seen = true;
                i = spec.end;
            }
            _ => i += 1,
        }
    }
    Ok((format_seen, false))
}

struct CapSpec {
    end: usize,
    position: CapArgLocation,
    width: Option<CapValue>,
    precision: Option<CapValue>,
    specifier: u8,
}

enum CapValue {
    Fixed(usize),
    Asterisk(CapArgLocation),
}

impl CapSpec {
    fn validate(&self, args: &mut CapArgs<'_>) -> std::result::Result<(), String> {
        if let Some(width) = &self.width {
            let width = resolve_cap_value(width, args, true);
            reject_over_cap("width", width)?;
        }
        if let Some(precision) = &self.precision {
            let precision = resolve_cap_value(precision, args, false);
            reject_over_cap("precision", precision)?;
        }
        if is_float_specifier(self.specifier) {
            let value = args.next_arg(self.position).unwrap_or_default();
            reject_float_exponent_over_cap(value)?;
        } else {
            args.consume(self.position);
        }
        Ok(())
    }
}

fn is_float_specifier(specifier: u8) -> bool {
    matches!(
        specifier,
        b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A'
    )
}

fn reject_float_exponent_over_cap(value: &str) -> std::result::Result<(), String> {
    let Some(exponent) = parse_float_exponent(value) else {
        return Ok(());
    };
    let exponent = exponent.unsigned_abs();
    if exponent > MAX_FORMAT_WIDTH as u64 {
        return Err(format!(
            "printf: format exponent {exponent} exceeds limit {MAX_FORMAT_WIDTH}\n"
        ));
    }
    Ok(())
}

fn parse_float_exponent(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let marker = bytes
        .iter()
        .rposition(|b| matches!(b, b'e' | b'E' | b'p' | b'P'))?;
    let exponent = value.get(marker + 1..)?;
    Some(parse_leading_i64(exponent))
}

fn reject_over_cap(kind: &str, value: usize) -> std::result::Result<(), String> {
    if value > MAX_FORMAT_WIDTH {
        return Err(format!(
            "printf: format {kind} {value} exceeds limit {MAX_FORMAT_WIDTH}\n"
        ));
    }
    Ok(())
}

fn resolve_cap_value(value: &CapValue, args: &mut CapArgs<'_>, is_width: bool) -> usize {
    match value {
        CapValue::Fixed(value) => *value,
        CapValue::Asterisk(location) => {
            let value = args.next_i64(*location);
            if is_width {
                value
                    .checked_abs()
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(usize::MAX)
            } else if value < 0 {
                0
            } else {
                usize::try_from(value).unwrap_or(usize::MAX)
            }
        }
    }
}

fn parse_cap_spec(format: &[u8], start: usize) -> Option<CapSpec> {
    let mut index = start;
    let position = eat_argument_position(format, &mut index)?;

    while matches!(
        format.get(index),
        Some(b'-' | b'+' | b' ' | b'#' | b'0' | b'\'')
    ) {
        index += 1;
    }

    let width = eat_asterisk_or_number(format, &mut index);
    let precision = if format.get(index) == Some(&b'.') {
        index += 1;
        Some(eat_asterisk_or_number(format, &mut index).unwrap_or(CapValue::Fixed(0)))
    } else {
        None
    };

    while let Some(length) = parse_length(format, index) {
        index += length;
    }

    let specifier = *format.get(index)?;
    index += 1;
    if !matches!(
        specifier,
        b'c' | b's'
            | b'b'
            | b'q'
            | b'd'
            | b'i'
            | b'u'
            | b'o'
            | b'x'
            | b'X'
            | b'f'
            | b'F'
            | b'e'
            | b'E'
            | b'g'
            | b'G'
            | b'a'
            | b'A'
    ) {
        return None;
    }

    Some(CapSpec {
        end: index,
        position,
        width,
        precision,
        specifier,
    })
}

fn eat_asterisk_or_number(format: &[u8], index: &mut usize) -> Option<CapValue> {
    if format.get(*index) == Some(&b'*') {
        *index += 1;
        Some(CapValue::Asterisk(eat_argument_position(format, index)?))
    } else {
        eat_number(format, index).map(CapValue::Fixed)
    }
}

fn eat_argument_position(format: &[u8], index: &mut usize) -> Option<CapArgLocation> {
    let original_index = *index;
    let Some(pos) = eat_number(format, index) else {
        return Some(CapArgLocation::NextArgument);
    };
    if format.get(*index) == Some(&b'$') {
        *index += 1;
        Some(CapArgLocation::Position(pos))
    } else {
        *index = original_index;
        Some(CapArgLocation::NextArgument)
    }
}

fn eat_number(format: &[u8], index: &mut usize) -> Option<usize> {
    let start = *index;
    let mut value = 0usize;
    while let Some(byte) = format.get(*index) {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(byte - b'0'));
        *index += 1;
    }
    (*index > start).then_some(value)
}

fn parse_length(format: &[u8], index: usize) -> Option<usize> {
    match format.get(index)? {
        b'h' | b'l' if format.get(index + 1) == format.get(index) => Some(2),
        b'h' | b'l' | b'j' | b'z' | b't' | b'L' => Some(1),
        _ => None,
    }
}

fn parse_leading_i64(input: &str) -> i64 {
    let bytes = input.as_bytes();
    let mut index = 0;
    let sign = match bytes.first() {
        Some(b'-') => {
            index = 1;
            -1i128
        }
        Some(b'+') => {
            index = 1;
            1i128
        }
        _ => 1i128,
    };

    let start_digits = index;
    let mut value = 0i128;
    while let Some(byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(i128::from(byte - b'0'));
        index += 1;
    }

    if index == start_digits {
        return 0;
    }

    let value = value.saturating_mul(sign);
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::ExecResult;

    #[test]
    fn generated_formatter_repeats_format_until_args_exhausted() {
        assert_eq!(
            render_printf("%s=%d ", &["a".into(), "1".into(), "b".into(), "2".into()]).unwrap(),
            "a=1 b=2 "
        );
    }

    #[test]
    fn generated_formatter_handles_escapes_and_quotes() {
        assert_eq!(render_printf("a\\nb", &[]).unwrap(), "a\nb");
        assert_eq!(
            render_printf("%q", &["hello world".into()]).unwrap(),
            "hello\\ world"
        );
    }

    #[test]
    fn strips_zero_hex_escapes_at_stdout_boundary() {
        assert_eq!(render_printf("a\\x00b", &[]).unwrap(), "ab");
    }

    #[test]
    fn preserves_octal_nul_for_zero_delimited_pipelines() {
        assert_eq!(render_printf("a\\0b", &[]).unwrap().as_bytes(), b"a\0b");
    }

    #[test]
    fn rejects_fixed_width_over_cap() {
        let err = render_printf("%10001s", &["x".into()]).unwrap_err();
        assert!(err.contains("width 10001 exceeds limit"));
    }

    #[test]
    fn rejects_fixed_precision_over_cap() {
        let err = render_printf("%.10001f", &["1".into()]).unwrap_err();
        assert!(err.contains("precision 10001 exceeds limit"));
    }

    #[test]
    fn rejects_asterisk_width_over_cap() {
        let err = render_printf("%*s", &["999999".into(), "x".into()]).unwrap_err();
        assert!(err.contains("width 999999 exceeds limit"));
    }

    #[test]
    fn rejects_nested_repeat_asterisk_width_over_cap() {
        let err = render_printf("%s %*s", &["ok".into(), "999999".into(), "x".into()]).unwrap_err();
        assert!(err.contains("width 999999 exceeds limit"));
    }

    #[test]
    fn rejects_float_exponent_over_cap() {
        let err = render_printf("%f", &["1e1000000000".into()]).unwrap_err();
        assert!(err.contains("exponent 1000000000 exceeds limit"));
    }

    #[tokio::test]
    async fn no_leak_printf_format_errors() {
        let r = crate::builtins::debug_leak_check::run("printf '%10001s' x").await;
        crate::builtins::debug_leak_check::assert_no_leak(&r, "printf_width_cap", &[]);
    }

    #[test]
    fn no_leak_all_format_error_variants() {
        let variants = vec![
            FormatError::SpecError(vec![b'?']),
            FormatError::IoError(std::io::Error::other("io failed")),
            FormatError::NoMoreArguments,
            FormatError::InvalidArgument(FormatArgument::String("x".into())),
            FormatError::TooManySpecs(b"%s %s".to_vec()),
            FormatError::NeedAtLeastOneSpec(b"plain".to_vec()),
            FormatError::WrongSpecType,
            FormatError::InvalidPrecision("bad".into()),
            FormatError::EndsWithPercent(b"%".to_vec()),
            FormatError::MissingHex,
            FormatError::InvalidCharacter('u', b"d800".to_vec()),
            FormatError::InvalidEncoding(
                super::super::generated::format_support::NonUtf8OsStrError::new_for_test("x"),
            ),
        ];

        for err in variants {
            let result = ExecResult::err(render_printf_error(&err), 1);
            crate::testing::assert_no_leak(&result, "printf_format_error_variant", &[]);
        }
    }
}
