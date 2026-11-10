//! Memory guards called from `bashkit fork:` edits in the vendored ZapCode VM.
//!
//! Decision (security, TM-TS-026): ZapCode 1.5.1 never enforces
//! `memory_limit_bytes`, and Rust aborts the whole process when an allocation
//! fails. Two layers keep one tenant's script from reaching that abort:
//!
//! 1. Every instruction checks the per-VM live heap ([`crate::vm_meter`],
//!    charged by `MeteredAllocator`), so gradual growth stops at the limit.
//! 2. Operations whose output can dwarf their inputs (strings are shared
//!    `Arc<str>`, so `[s, s, s].join()` costs 3 pointers but yields 3 copies)
//!    size the output first and refuse before allocating it. Sizing walks the
//!    value once and stops as soon as it passes the remaining budget, so the
//!    walk costs at most one step per byte the VM may still allocate.
//!
//! Plain copies (array literals, `concat`, spread, template literals) need no
//! pre-check: each is at most a small multiple of memory already charged, and
//! layer 1 stops the run at the next instruction.
//!
//! See knowledge/runtimes/zapcode-runtime.md.

use std::cell::Cell;

use crate::vm_meter;
use crate::zapcode_vendor::{Value, ZapcodeError};

/// Upper bound for the text of one number (`-1.7976931348623157e308`).
const NUMBER_TEXT_BYTES: usize = 24;
/// Rough heap cost of one `Value` in a fresh array (the enum plus a small
/// `Arc<str>` allocation for a one-character string).
const VALUE_BYTES: usize = 64;

thread_local! {
    // Nested `to_js_string` calls (arrays of arrays) are covered by the
    // outermost sizing pass; skip re-sizing inside it.
    static SIZING: Cell<bool> = const { Cell::new(false) };
}

/// The message for a refused reservation or a VM over its limit.
pub(crate) fn limit_error() -> ZapcodeError {
    ZapcodeError::MemoryLimitExceeded("script heap reached max_memory".to_string())
}

/// True when the run must stop for memory (checked at every instruction).
pub(crate) fn over_limit() -> bool {
    vm_meter::over_limit()
}

/// Reserve `bytes` for an output about to be built.
pub(crate) fn reserve(bytes: usize) -> Result<(), ZapcodeError> {
    if vm_meter::reserve(bytes) {
        Ok(())
    } else {
        Err(limit_error())
    }
}

/// Reserve room for `n` new values (e.g. `split("")`).
pub(crate) fn reserve_values(n: usize) -> Result<(), ZapcodeError> {
    reserve(n.saturating_mul(VALUE_BYTES))
}

/// Size check for `Value::to_js_string` on an array. Returns false when the
/// text would not fit; the caller returns an empty string and the VM stops at
/// its next instruction (the refusal is sticky).
pub(crate) fn fits_js_string(arr: &[Value]) -> bool {
    if SIZING.with(Cell::get) {
        return true;
    }
    let len = js_text_len(arr, vm_meter::remaining()).unwrap_or(usize::MAX);
    vm_meter::reserve(len)
}

/// Mark the start of a guarded `to_js_string`; nested calls skip sizing.
pub(crate) struct SizingScope(bool);

impl SizingScope {
    pub(crate) fn enter() -> Self {
        Self(SIZING.with(|s| s.replace(true)))
    }
}

impl Drop for SizingScope {
    fn drop(&mut self) {
        SIZING.with(|s| s.set(self.0));
    }
}

/// Size check for `Array.prototype.join(sep)`.
pub(crate) fn reserve_join(arr: &[Value], sep: &str) -> Result<(), ZapcodeError> {
    let seps = arr.len().saturating_sub(1).saturating_mul(sep.len());
    let items = js_items_len(arr, vm_meter::remaining()).unwrap_or(usize::MAX);
    reserve(items.saturating_add(seps))
}

/// Size check for `JSON.stringify(value)`.
pub(crate) fn reserve_json(value: &Value) -> Result<(), ZapcodeError> {
    let len = json_len(value, vm_meter::remaining()).unwrap_or(usize::MAX);
    reserve(len)
}

/// Size check for `String.prototype.replace`/`replaceAll`.
pub(crate) fn reserve_replace(
    s: &str,
    search: &str,
    replacement: &str,
    all: bool,
) -> Result<(), ZapcodeError> {
    let matches = if search.is_empty() {
        if all { s.chars().count() + 1 } else { 1 }
    } else if all {
        s.matches(search).count()
    } else {
        usize::from(s.contains(search))
    };
    let grow = replacement.len().saturating_sub(search.len());
    reserve(s.len().saturating_add(matches.saturating_mul(grow)))
}

/// Text length of `arr.join(",")` with nested arrays flattened the way
/// `to_js_string` does. `None` once it passes `budget`.
fn js_text_len(arr: &[Value], budget: usize) -> Option<usize> {
    let commas = arr.len().saturating_sub(1);
    let items = js_items_len(arr, budget.checked_sub(commas)?)?;
    items.checked_add(commas).filter(|n| *n <= budget)
}

fn js_items_len(arr: &[Value], budget: usize) -> Option<usize> {
    let mut total = 0usize;
    for v in arr {
        let left = budget.checked_sub(total)?;
        total = total.checked_add(js_value_len(v, left)?)?;
        if total > budget {
            return None;
        }
    }
    Some(total)
}

fn js_value_len(v: &Value, budget: usize) -> Option<usize> {
    match v {
        Value::String(s) => Some(s.len()),
        Value::Array(inner) => js_text_len(inner, budget),
        Value::Object(_) => Some("[object Object]".len()),
        Value::Generator(_) => Some("[object Generator]".len()),
        _ => Some(NUMBER_TEXT_BYTES),
    }
    .filter(|n| *n <= budget)
}

/// Worst-case `value_to_json` length (every quote/backslash escaped).
fn json_len(v: &Value, budget: usize) -> Option<usize> {
    let n = match v {
        Value::String(s) => json_str_len(s),
        Value::Array(arr) => {
            let mut total = 2 + arr.len().saturating_sub(1);
            for item in arr {
                total = total.checked_add(json_len(item, budget.checked_sub(total)?)?)?;
            }
            total
        }
        Value::Object(map) => {
            let mut total = 2 + map.len().saturating_sub(1);
            for (k, item) in map {
                total = total.checked_add(json_str_len(k) + 1)?;
                total = total.checked_add(json_len(item, budget.checked_sub(total)?)?)?;
            }
            total
        }
        _ => NUMBER_TEXT_BYTES,
    };
    (n <= budget).then_some(n)
}

fn json_str_len(s: &str) -> usize {
    s.len() + 2 + s.bytes().filter(|b| *b == b'"' || *b == b'\\').count()
}
