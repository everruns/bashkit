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
use super::limits::PRINTF_MAX_DIAG_CHARS as MAX_PRINTF_DIAG_CHARS;
use super::{Builtin, Context, Date, MAX_FORMAT_WIDTH};
use crate::error::Result;
use crate::interpreter::{ExecResult, is_internal_variable};

/// printf builtin - formatted string output
///
/// Holds the sandbox clock so `%(fmt)T` reads the same virtual time as `date`.
#[derive(Default)]
pub struct Printf {
    clock: Date,
}

impl Printf {
    /// printf whose `%(fmt)T` uses `clock` (fixed epoch / offset aware).
    pub fn with_clock(clock: Date) -> Self {
        Self { clock }
    }
}

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
                // `--` ends options; the next word is the format.
                Some(arg) if arg == "--" => match args_iter.next() {
                    Some(f) => break f.clone(),
                    None => return Ok(ExecResult::ok(String::new())),
                },
                Some(arg) => break arg.clone(),
                None => return Ok(ExecResult::ok(String::new())),
            }
        };

        let args: Vec<String> = args_iter.cloned().collect();
        let format = escape_format_backslash_c(&format).into_owned();
        let (format, args) = match expand_quote_directives(&format, args) {
            Ok(v) => v,
            Err(err) => return Ok(ExecResult::err(err, 1)),
        };
        let (format, args) = match expand_time_directives(&format, args, |seconds, fmt| {
            self.clock.strftime(ctx.env.get("TZ"), seconds, fmt)
        }) {
            Ok(v) => v,
            Err(err) => return Ok(ExecResult::err(format!("{err}\n"), 1)),
        };
        let output = match render_printf_bytes(&format, &args) {
            Ok(output) => output,
            Err(err) => return Ok(ExecResult::err(err, 1)),
        };

        if let Some(name) = var_name {
            // THREAT[TM-INJ-009]: Block internal variable prefix injection via printf -v
            if is_internal_variable(&name) {
                return Ok(ExecResult::ok(String::new()));
            }
            // Variables are text; a non-UTF-8 byte is decoded lossily here.
            ctx.variables
                .insert(name, String::from_utf8_lossy(&output).into_owned());
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::ok_bytes(output))
        }
    }
}

#[cfg(test)]
fn render_printf(format: &str, args: &[String]) -> std::result::Result<String, String> {
    render_printf_bytes(format, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Bytes are canonical: `\xff` must reach stdout as one 0xff byte.
pub(super) fn render_printf_bytes(
    format: &str,
    args: &[String],
) -> std::result::Result<Vec<u8>, String> {
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
        return Ok(out);
    }

    while !fmt_args.is_exhausted() {
        if write_format_pass(format.as_bytes(), &mut fmt_args, &mut out, &mut format_seen)? {
            break;
        }
        fmt_args.start_next_batch();
    }

    Ok(out)
}

/// In a printf FORMAT (unlike a `%b` argument) bash prints `\c` literally;
/// the uutils formatter would stop output there, so escape the backslash.
fn escape_format_backslash_c(format: &str) -> Cow<'_, str> {
    if !format.contains("\\c") {
        return Cow::Borrowed(format);
    }
    let mut out = String::with_capacity(format.len() + 4);
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('c') => out.push_str("\\\\c"),
            Some(n) => {
                out.push('\\');
                out.push(n);
            }
            None => out.push('\\'),
        }
    }
    Cow::Owned(out)
}

/// One `%b` / `%q` / `%Q` directive the uutils formatter cannot handle.
struct QuoteSlot {
    conv: u8,
    precision: Option<usize>,
}

/// Rewrite `%[flags][width][.prec]{b,q,Q}` that the uutils formatter rejects
/// (any flag/width/precision, and every `%Q`) into `%[flags][width]s`,
/// pre-rendering the matching arguments.
///
/// Decision: bash pads `%b`/`%q` itself and never with zeros, applies the
/// precision to the expanded/quoted text, while `%Q` applies it to the
/// argument before quoting. Directives using `*` are left to the formatter.
fn expand_quote_directives(
    format: &str,
    mut args: Vec<String>,
) -> std::result::Result<(String, Vec<String>), String> {
    let bytes = format.as_bytes();
    let mut out = String::with_capacity(format.len());
    let mut slots: Vec<Option<QuoteSlot>> = Vec::new();
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'%' if bytes.get(i + 1) == Some(&b'%') => i += 2,
            b'%' => {
                let start = i;
                i += 1;
                let flags_start = i;
                while i < bytes.len() && b"-+ #0'".contains(&bytes[i]) {
                    i += 1;
                }
                let flags = &format[flags_start..i];
                let width_start = i;
                let mut star = false;
                if bytes.get(i) == Some(&b'*') {
                    slots.push(None);
                    star = true;
                    i += 1;
                } else {
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let width = &format[width_start..i];
                let mut precision = None;
                if bytes.get(i) == Some(&b'.') {
                    i += 1;
                    if bytes.get(i) == Some(&b'*') {
                        slots.push(None);
                        star = true;
                        i += 1;
                    } else {
                        let p = i;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                        precision = Some(format[p..i].parse().unwrap_or(0));
                    }
                }
                let Some(&conv) = bytes.get(i) else { break };
                i += 1;
                let decorated = i - start > 2;
                if !star && (conv == b'Q' || (matches!(conv, b'b' | b'q') && decorated)) {
                    out.push_str(&format[copied..start]);
                    out.push('%');
                    out.push_str(&flags.replace('0', ""));
                    out.push_str(width);
                    out.push('s');
                    copied = i;
                    slots.push(Some(QuoteSlot { conv, precision }));
                } else {
                    slots.push(None);
                }
            }
            _ => i += 1,
        }
    }
    if !slots.iter().any(Option::is_some) {
        return Ok((format.to_string(), args));
    }
    out.push_str(&format[copied..]);

    let n = slots.len();
    let truncate = |s: String, p: Option<usize>| match p {
        Some(p) => s.chars().take(p).collect(),
        None => s,
    };
    let render = |spec: &str, arg: &str| -> std::result::Result<String, String> {
        render_printf_bytes(spec, &[arg.to_string()])
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    for (idx, arg) in args.iter_mut().enumerate() {
        if let Some(slot) = &slots[idx % n] {
            *arg = match slot.conv {
                b'b' => truncate(render("%b", arg)?, slot.precision),
                b'q' => truncate(render("%q", arg)?, slot.precision),
                _ => render("%q", &truncate(arg.clone(), slot.precision))?,
            };
        }
    }
    Ok((out, args))
}

/// One argument-consuming slot of a format pass.
enum Slot {
    Plain,
    /// `%(fmt)T` with its strftime format.
    Time(String),
}

/// Rewrite bash's `%(fmt)T` into `%s` and pre-format the matching arguments.
///
/// Decision: the generated uutils formatter has no `%(...)T`, and it is
/// generated code we don't edit. So each `%[flags][width][.prec](fmt)T`
/// becomes `%[flags][width][.prec]s`, and every argument that lands on such a
/// directive (arg i -> slot i % slots, as the format repeats) is replaced by
/// the formatted time. Argument `""` or `-1` means now. `-2` (bash: shell
/// start time) also means now; the shell start instant isn't tracked.
/// Missing arguments for a time slot also mean now.
fn expand_time_directives(
    format: &str,
    mut args: Vec<String>,
    mut strftime: impl FnMut(Option<i64>, &str) -> std::result::Result<String, String>,
) -> std::result::Result<(String, Vec<String>), String> {
    if !format.contains(")T") {
        return Ok((format.to_string(), args));
    }
    let bytes = format.as_bytes();
    let mut out = String::with_capacity(format.len());
    let mut slots = Vec::new();
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'%' if bytes.get(i + 1) == Some(&b'%') => i += 2,
            b'%' => {
                i += 1;
                while i < bytes.len() && b"-+ #0'".contains(&bytes[i]) {
                    i += 1;
                }
                for part in 0..2 {
                    if part == 1 {
                        if bytes.get(i) != Some(&b'.') {
                            break;
                        }
                        i += 1;
                    }
                    if bytes.get(i) == Some(&b'*') {
                        slots.push(Slot::Plain);
                        i += 1;
                    } else {
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                if bytes.get(i) == Some(&b'(')
                    && let Some(close) = format[i..].find(")T")
                {
                    let fmt = &format[i + 1..i + close];
                    out.push_str(&format[copied..i]);
                    out.push('s');
                    slots.push(Slot::Time(if fmt.is_empty() {
                        "%X".to_string()
                    } else {
                        fmt.to_string()
                    }));
                    i += close + 2;
                    copied = i;
                } else if i < bytes.len() {
                    slots.push(Slot::Plain);
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&format[copied..]);
    if !slots.iter().any(|s| matches!(s, Slot::Time(_))) {
        return Ok((format.to_string(), args));
    }

    // Pad the last pass up to its final time slot so a missing time
    // argument still prints "now" (bash treats it as -1).
    let n = slots.len();
    let last_time = slots
        .iter()
        .rposition(|s| matches!(s, Slot::Time(_)))
        .unwrap_or(0);
    let pass_start = if args.is_empty() {
        0
    } else {
        (args.len() - 1) / n * n
    };
    let filled = args.len() - pass_start;
    if args.is_empty() || filled < n {
        let want = pass_start + last_time + 1;
        while args.len() < want {
            args.push(String::new());
        }
    }

    for (idx, arg) in args.iter_mut().enumerate() {
        if let Slot::Time(fmt) = &slots[idx % n] {
            let seconds = match arg.trim() {
                "" | "-1" | "-2" => None,
                s => Some(parse_leading_i64(s)),
            };
            *arg = strftime(seconds, fmt)?;
        }
    }
    Ok((out, args))
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

    fn fake_time(seconds: Option<i64>, fmt: &str) -> std::result::Result<String, String> {
        Ok(format!(
            "<{fmt}@{}>",
            seconds.map_or("now".into(), |s| s.to_string())
        ))
    }

    #[test]
    fn time_directive_rewritten_to_string_slot() {
        let (f, a) =
            expand_time_directives("%-5(%H)T|%s\\n", vec!["7".into(), "x".into()], fake_time)
                .unwrap();
        assert_eq!(f, "%-5s|%s\\n");
        assert_eq!(a, vec!["<%H@7>".to_string(), "x".to_string()]);
    }

    #[test]
    fn time_directive_maps_args_across_passes_and_pads_missing() {
        let (_, a) = expand_time_directives(
            "%s %(%F)T",
            vec!["a".into(), "1".into(), "b".into()],
            fake_time,
        )
        .unwrap();
        assert_eq!(a, vec!["a", "<%F@1>", "b", "<%F@now>"]);
        let (_, a) = expand_time_directives("%()T", vec![], fake_time).unwrap();
        assert_eq!(a, vec!["<%X@now>"]);
    }

    #[test]
    fn time_directive_skips_literal_percent_and_plain_formats() {
        let (f, a) = expand_time_directives("%% %s", vec!["x".into()], fake_time).unwrap();
        assert_eq!((f.as_str(), a), ("%% %s", vec!["x".to_string()]));
        let err = expand_time_directives("%(%F)T", vec![], |_, _| Err("bad".into()));
        assert_eq!(err.unwrap_err(), "bad");
    }

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
    fn zero_integer_with_explicit_zero_precision_emits_no_digits() {
        assert_eq!(render_printf("<%.0d>", &["0".into()]).unwrap(), "<>");
        assert_eq!(render_printf("<%.0u>", &["0".into()]).unwrap(), "<>");
        assert_eq!(render_printf("<%#.0x>", &["0".into()]).unwrap(), "<>");
        assert_eq!(render_printf("<%#.0o>", &["0".into()]).unwrap(), "<0>");
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
            // The `Range`/`Option<Range>` payloads are upstream's source
            // spans; TM-INF-022 requires the rendered diagnostic never
            // expose them, so the leak check covers both spanned and
            // unspanned constructions.
            FormatError::SpecError(vec![b'?'], 0..2),
            FormatError::IoError(std::io::Error::other("io failed")),
            FormatError::NoMoreArguments,
            FormatError::InvalidArgument(FormatArgument::String("x".into())),
            FormatError::TooManySpecs(b"%s %s".to_vec()),
            FormatError::NeedAtLeastOneSpec(b"plain".to_vec()),
            FormatError::WrongSpecType,
            FormatError::InvalidPrecision("bad".into()),
            FormatError::EndsWithPercent(b"%".to_vec()),
            FormatError::MissingHex(None),
            FormatError::MissingHex(Some(0..2)),
            FormatError::InvalidCharacter('u', b"d800".to_vec(), None),
            FormatError::InvalidCharacter('u', b"d800".to_vec(), Some(0..6)),
            FormatError::InvalidEncoding(
                super::super::generated::format_support::NonUtf8OsStrError::new_for_test("x"),
            ),
        ];

        for err in variants {
            let result = ExecResult::err(render_printf_error(&err), 1);
            crate::testing::assert_no_leak(&result, "printf_format_error_variant", &[]);
        }
    }

    #[test]
    fn format_backslash_c_is_literal() {
        let f = escape_format_backslash_c("a\\cb\\\\c");
        assert_eq!(render_printf(&f, &[]).unwrap(), "a\\cb\\c");
    }

    #[test]
    fn padded_b_q_and_upper_q_directives() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (f, a) =
            expand_quote_directives("[%05b|%05q|%05Q]", args(&["a\\tb", "c d", "e"])).unwrap();
        assert_eq!(render_printf(&f, &a).unwrap(), "[  a\tb| c\\ d|    e]");
        let (f, a) =
            expand_quote_directives("[%.2q|%.2Q|%-6q]", args(&["abc", "a bc", "x"])).unwrap();
        assert_eq!(render_printf(&f, &a).unwrap(), "[ab|a\\ |x     ]");
    }
}
