//! Truthful formatting for the reserved-word `time` execution wrapper.
//!
//! Important decision: only interpreter-owned measurements are reportable.
//! Host-process CPU/RSS values describe the embedder, not the wrapped virtual
//! command, so GNU fields for those values deterministically say unavailable.

#[derive(Debug, Clone, Copy)]
pub(super) struct TimeUsage {
    pub(super) elapsed: std::time::Duration,
    pub(super) exit_status: i32,
    pub(super) commands: usize,
    pub(super) loops: usize,
    pub(super) work_units: u64,
}

pub(super) fn validate_time_format(format: &str) -> Result<(), String> {
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        let Some(field) = chars.next() else {
            return Err("%".to_string());
        };
        if field == '{' {
            let mut name = String::new();
            let mut closed = false;
            for ch in chars.by_ref() {
                if ch == '}' {
                    closed = true;
                    break;
                }
                name.push(ch);
            }
            if !closed || !matches!(name.as_str(), "commands" | "loops" | "work_units") {
                return Err(format!("%{{{name}}}"));
            }
        } else if !matches!(
            field,
            '%' | 'e'
                | 'E'
                | 'x'
                | 'U'
                | 'S'
                | 'M'
                | 'P'
                | 'C'
                | 'K'
                | 'D'
                | 'p'
                | 'X'
                | 'Z'
                | 'F'
                | 'R'
                | 'W'
                | 'c'
                | 'w'
                | 'I'
                | 'O'
                | 'r'
                | 's'
                | 'k'
        ) {
            return Err(format!("%{field}"));
        }
    }
    Ok(())
}

pub(super) fn render_time_format(
    format: &str,
    usage: &TimeUsage,
    max_bytes: usize,
) -> Result<String, ()> {
    let mut output = String::with_capacity(format.len().saturating_add(32));
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => push_time_report(&mut output, "\n", max_bytes)?,
                Some('t') => push_time_report(&mut output, "\t", max_bytes)?,
                Some('\\') => push_time_report(&mut output, "\\", max_bytes)?,
                Some(other) => {
                    push_time_report(&mut output, "\\", max_bytes)?;
                    push_time_char(&mut output, other, max_bytes)?;
                }
                None => push_time_report(&mut output, "\\", max_bytes)?,
            }
            continue;
        }
        if ch != '%' {
            push_time_char(&mut output, ch, max_bytes)?;
            continue;
        }

        match chars.next().expect("validated time format") {
            '%' => push_time_report(&mut output, "%", max_bytes)?,
            'e' => push_time_report(
                &mut output,
                &format!("{:.2}", usage.elapsed.as_secs_f64()),
                max_bytes,
            )?,
            'E' => {
                let total = usage.elapsed.as_secs_f64();
                let hours = (total / 3600.0).floor() as u64;
                let minutes = ((total % 3600.0) / 60.0).floor() as u64;
                push_time_report(
                    &mut output,
                    &format!("{hours}:{minutes:02}:{:.2}", total % 60.0),
                    max_bytes,
                )?;
            }
            'x' => push_time_report(&mut output, &usage.exit_status.to_string(), max_bytes)?,
            '{' => {
                let name: String = chars.by_ref().take_while(|ch| *ch != '}').collect();
                match name.as_str() {
                    "commands" => {
                        push_time_report(&mut output, &usage.commands.to_string(), max_bytes)?
                    }
                    "loops" => push_time_report(&mut output, &usage.loops.to_string(), max_bytes)?,
                    "work_units" => {
                        push_time_report(&mut output, &usage.work_units.to_string(), max_bytes)?
                    }
                    _ => unreachable!("validated named time field"),
                }
            }
            _ => push_time_report(&mut output, "unavailable", max_bytes)?,
        }
    }
    if !output.ends_with('\n') {
        push_time_report(&mut output, "\n", max_bytes)?;
    }
    Ok(output)
}

fn push_time_report(output: &mut String, value: &str, max_bytes: usize) -> Result<(), ()> {
    if output.len().saturating_add(value.len()) > max_bytes {
        return Err(());
    }
    output.push_str(value);
    Ok(())
}

fn push_time_char(output: &mut String, value: char, max_bytes: usize) -> Result<(), ()> {
    if output.len().saturating_add(value.len_utf8()) > max_bytes {
        return Err(());
    }
    output.push(value);
    Ok(())
}

/// The reserved word's report under `TIMEFORMAT` (bash
/// `print_formatted_time`): `%[p][l]R` is the elapsed time with `p` (0-3,
/// default 3) truncated decimals, `l` as `XmY.YYYs`; `%%` is `%`. Host CPU
/// fields (`%U`, `%S`, `%P`) say `unavailable` like every other report. A
/// newline follows; an empty format prints nothing. `Err` carries an
/// invalid format character (bash warns and prints no report).
pub(super) fn render_timeformat(
    format: &str,
    elapsed: std::time::Duration,
) -> Result<String, char> {
    if format.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::with_capacity(format.len() + 16);
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' || chars.peek().is_none() {
            out.push(ch);
            continue;
        }
        let mut prec = 3u32;
        let mut long = false;
        if let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
            prec = d.min(3);
            chars.next();
        }
        if chars.peek() == Some(&'l') {
            long = true;
            chars.next();
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('R') => {
                let mut secs = elapsed.as_secs();
                if long {
                    out.push_str(&format!("{}m", secs / 60));
                    secs %= 60;
                }
                out.push_str(&secs.to_string());
                if prec > 0 {
                    let millis = elapsed.subsec_millis();
                    let digits = format!("{millis:03}");
                    out.push('.');
                    out.push_str(&digits[..prec as usize]);
                }
                if long {
                    out.push('s');
                }
            }
            Some('U' | 'S' | 'P') => out.push_str("unavailable"),
            Some(other) => return Err(other),
            None => return Err('%'),
        }
    }
    out.push('\n');
    Ok(out)
}

pub(super) fn verbose_time_report(usage: &TimeUsage) -> String {
    format!(
        "Elapsed (wall clock) time: {:.2}\n\
User CPU time: unavailable\n\
System CPU time: unavailable\n\
Maximum resident set size: unavailable\n\
Bashkit commands: {}\n\
Bashkit loop iterations: {}\n\
Bashkit work units: {}\n\
Exit status: {}\n",
        usage.elapsed.as_secs_f64(),
        usage.commands,
        usage.loops,
        usage.work_units,
        usage.exit_status
    )
}

pub(super) fn sanitize_time_path(path: &str) -> String {
    path.chars()
        .take(256)
        .map(|ch| if ch.is_control() { '?' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::render_timeformat;
    use std::time::Duration;

    #[test]
    fn timeformat_precision_and_long_form() {
        let d = Duration::from_millis(61_234);
        assert_eq!(render_timeformat("%R", d).unwrap(), "61.234\n");
        assert_eq!(render_timeformat("%0R", d).unwrap(), "61\n");
        assert_eq!(render_timeformat("%2lR", d).unwrap(), "1m1.23s\n");
        assert_eq!(
            render_timeformat("%9R|%%|%U", d).unwrap(),
            "61.234|%|unavailable\n"
        );
        assert_eq!(render_timeformat("", d).unwrap(), "");
        assert_eq!(render_timeformat("%Q", d), Err('Q'));
    }
}
