//! jq-only JSON input compatibility normalization.
//!
//! Real jq deployments accept literal U+0000..U+001F controls inside JSON
//! strings. Keep that exception at this builtin boundary: the shared JSON
//! parser and every other JSON contract remain strict.

use std::borrow::Cow;

use super::{Context, Val};
use crate::limits::{ExecutionBudget, ExecutionBudgetLease};

#[derive(Debug)]
pub(super) struct NormalizedInput<'a> {
    text: Cow<'a, str>,
    _lease: Option<ExecutionBudgetLease>,
}

impl<'a> NormalizedInput<'a> {
    pub(super) fn as_str(&self) -> &str {
        &self.text
    }
}

#[derive(Debug)]
pub(super) enum NormalizeError {
    InvalidJson(String),
    Resource(crate::error::Error),
}

impl From<crate::limits::LimitExceeded> for NormalizeError {
    fn from(error: crate::limits::LimitExceeded) -> Self {
        Self::Resource(error.into())
    }
}

impl From<crate::Error> for NormalizeError {
    fn from(error: crate::Error) -> Self {
        Self::Resource(error)
    }
}

/// Cache the end of the last line inspected, so many values on one long line
/// never rescan its suffix quadratically.
#[derive(Default)]
pub(super) struct Lines {
    counted: usize,
    lines: usize,
}

impl Lines {
    pub(super) fn at(&mut self, text: &str, end: usize, ctx: &Context<'_>) -> crate::Result<usize> {
        if end > self.counted {
            let last = end.saturating_sub(1);
            let mut eol = text.len();
            for (i, chunk) in text.as_bytes()[last..].chunks(1024).enumerate() {
                if let Some(p) = chunk.iter().position(|&c| c == b'\n') {
                    ctx.consume_budget_work((p + 1) as u64)?;
                    eol = last + i * 1024 + p + 1;
                    break;
                }
                ctx.consume_budget_work(chunk.len() as u64)?;
            }
            for chunk in text.as_bytes()[self.counted..eol].chunks(1024) {
                ctx.consume_budget_work(chunk.len() as u64)?;
                self.lines += chunk.iter().filter(|&&c| c == b'\n').count();
            }
            self.counted = eol;
        }
        Ok(self.lines)
    }
}

/// Only one input tree exists at a time. Conversion occurs under the jq meter,
/// then the temporary tree is dropped before the evaluator requests more input.
pub(super) fn values<'a, 'shell>(
    text: &'a str,
    ctx: &'a Context<'shell>,
) -> impl Iterator<Item = std::result::Result<(Val, usize), NormalizeError>> + 'a {
    let mut lines = Lines::default();
    checked_value_spans(text, |n| ctx.consume_budget_work(n)).map(move |span| {
        let (start, end) = span?;
        ctx.consume_budget_work(1)?;
        let value = super::parse_jq_json_value(ctx, &text[start..end])?
            .map_err(NormalizeError::InvalidJson)?;
        let value = super::convert::jq_into_val(value);
        Ok((value, lines.at(text, end, ctx)?))
    })
}

/// THREAT[TM-DOS-100]: Normalize in one bounded pass. Work is charged before
/// scanning, and live bytes are reserved before allocating or growing output.
pub(super) fn normalize<'a>(
    budget: Option<&ExecutionBudget>,
    input: &'a str,
) -> std::result::Result<NormalizedInput<'a>, NormalizeError> {
    if let Some(budget) = budget {
        budget.consume_work(u64::try_from(input.len()).unwrap_or(u64::MAX))?;
    }

    let mut in_string = false;
    let mut escaped = false;
    let mut output: Option<String> = None;
    let mut lease = None;
    let mut copied_through = 0usize;

    for (offset, byte) in input.bytes().enumerate() {
        if !in_string {
            if byte == b'"' {
                in_string = true;
            }
            continue;
        }

        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'"' => in_string = false,
            0x00..=0x1f => {
                if output.is_none() {
                    let Some(capacity) = input.len().checked_add(5) else {
                        return Err(NormalizeError::InvalidJson(
                            "jq: normalized input too large".into(),
                        ));
                    };
                    lease = budget
                        .map(|budget| budget.lease_bytes(capacity))
                        .transpose()?;
                    output = Some(String::with_capacity(capacity));
                } else if let Some(reservation) = &mut lease {
                    reservation.grow(5)?;
                }

                let normalized = output.as_mut().expect("output initialized above");
                normalized.reserve_exact(5);
                normalized.push_str(&input[copied_through..offset]);
                use std::fmt::Write;
                write!(normalized, "\\u{byte:04x}").expect("writing to String cannot fail");
                copied_through = offset + 1;
            }
            _ => {}
        }
    }

    if in_string {
        return Err(NormalizeError::InvalidJson(
            "jq: invalid JSON: unterminated string at end of input".into(),
        ));
    }

    let text = match output {
        Some(mut normalized) => {
            normalized.push_str(&input[copied_through..]);
            Cow::Owned(normalized)
        }
        None => Cow::Borrowed(input),
    };
    Ok(NormalizedInput {
        text,
        _lease: lease,
    })
}

/// Byte spans of the top-level values in a JSON text stream, split the way
/// jq tokenizes: strings, bracketed values, and literal tokens that run to
/// the next whitespace or structural character. The parser validates each
/// span; this only finds boundaries.
pub(super) fn value_spans(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    checked_value_spans(text, |_| Ok(())).map(|span| span.expect("infallible span scan"))
}

fn checked_value_spans<'a>(
    text: &'a str,
    mut checkpoint: impl FnMut(u64) -> crate::Result<()> + 'a,
) -> impl Iterator<Item = crate::Result<(usize, usize)>> + 'a {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let result = (|| -> crate::Result<Option<(usize, usize)>> {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
                if i.is_multiple_of(1024) {
                    checkpoint(1024)?;
                }
            }
            if i >= b.len() {
                return Ok(None);
            }
            let start = i;
            let scalar = !matches!(b[i], b'[' | b'{' | b'"');
            let (mut depth, mut in_str, mut esc) = (0usize, false, false);
            while i < b.len() {
                let c = b[i];
                i += 1;
                if i.is_multiple_of(1024) {
                    checkpoint(1024)?;
                }
                if in_str {
                    if esc {
                        esc = false;
                    } else if c == b'\\' {
                        esc = true;
                    } else if c == b'"' {
                        in_str = false;
                        if depth == 0 {
                            break;
                        }
                    }
                    continue;
                }
                if scalar {
                    if c.is_ascii_whitespace()
                        || matches!(c, b'[' | b'{' | b'"' | b']' | b'}' | b',' | b':')
                    {
                        i -= 1;
                        if i == start {
                            // A stray structural character: its own (invalid) span.
                            i += 1;
                            if i.is_multiple_of(1024) {
                                checkpoint(1024)?;
                            }
                        }
                        break;
                    }
                    continue;
                }
                match c {
                    b'"' => in_str = true,
                    b'[' | b'{' => depth += 1,
                    b']' | b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Some((start, i)))
        })();
        match result {
            Ok(Some(span)) => Some(Ok(span)),
            Ok(None) => {
                done = true;
                None
            }
            Err(error) => {
                done = true;
                Some(Err(error))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::limits::{ExecutionBudgetExceeded, ExecutionLimits, LimitExceeded};

    fn budget(work: u64, live: u64) -> ExecutionBudget {
        let limits = ExecutionLimits::new()
            .max_work_units(work)
            .max_live_intermediate_bytes(live);
        ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)))
    }

    #[test]
    fn scan_charges_work_before_processing() {
        let budget = budget(3, 100);
        let NormalizeError::Resource(error) = normalize(Some(&budget), "\"a\n\"").unwrap_err()
        else {
            panic!("expected resource limit");
        };
        assert!(matches!(
            error,
            crate::error::Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::WorkUnits { .. }
            ))
        ));
    }

    #[test]
    fn allocation_is_leased_at_exact_boundary() {
        let input = "\"a\n\"";
        let exact = u64::try_from(input.len() + 5).unwrap();
        assert!(normalize(Some(&budget(100, exact)), input).is_ok());

        let NormalizeError::Resource(error) =
            normalize(Some(&budget(100, exact - 1)), input).unwrap_err()
        else {
            panic!("expected resource limit");
        };
        assert!(matches!(
            error,
            crate::error::Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::LiveBytes { .. }
            ))
        ));
    }

    #[test]
    fn each_control_growth_is_leased_before_append() {
        let input = "\"\n\t\"";
        let one_control = u64::try_from(input.len() + 5).unwrap();
        let NormalizeError::Resource(error) =
            normalize(Some(&budget(100, one_control)), input).unwrap_err()
        else {
            panic!("expected resource limit");
        };
        assert!(matches!(
            error,
            crate::error::Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::LiveBytes { .. }
            ))
        ));
    }

    #[test]
    fn value_spans_split_like_jq() {
        let text = "1 \"a b\" [1, [2]] {\"k\": \"}\"}2[3]";
        let parts: Vec<&str> = super::value_spans(text).map(|(a, b)| &text[a..b]).collect();
        assert_eq!(
            parts,
            ["1", "\"a b\"", "[1, [2]]", "{\"k\": \"}\"}", "2", "[3]"]
        );
        let glued = "1\u{1}2";
        assert_eq!(
            super::value_spans(glued).collect::<Vec<_>>(),
            [(0, glued.len())]
        );
    }

    #[tokio::test]
    async fn value_lines_count_lines_read() {
        for (text, expected) in [
            ("1\n2\n3", "1\n2\n2\n"),
            ("[\n1\n]\n2", "3\n3\n"),
            ("1 2", "0\n0\n"),
        ] {
            let mut bash = crate::Bash::builder().build();
            let result = bash
                .exec_with_options(
                    "jq input_line_number",
                    crate::ExecOptions::new().stdin(text),
                )
                .await
                .unwrap();
            assert_eq!(result.exit_code, 0, "{}", result.stderr);
            assert_eq!(result.stdout, expected);
        }
    }
}
