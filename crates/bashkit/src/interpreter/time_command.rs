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
