//! jq-only JSON input compatibility normalization.
//!
//! Real jq deployments accept literal U+0000..U+001F controls inside JSON
//! strings. Keep that exception at this builtin boundary: the shared JSON
//! parser and every other JSON contract remain strict.

use std::borrow::Cow;

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
}
