//! jq's `stderr`, `debug` and `halt_error` messages.
//!
//! Important decisions:
//!  - jaq-std's `stderr_empty` / `debug_empty` natives write to the host
//!    process (`log`). bashkit replaces them so the text lands in the jq
//!    command's own stderr, in order with its error messages, and never
//!    reaches the host.
//!  - Natives are plain fn pointers, so the text goes through a
//!    thread-local buffer. The filter runs synchronously on one thread;
//!    `take` drains it after each result.
//!  - The buffer is capped (`MAX_MESSAGE_BYTES`) so a looping `debug` cannot
//!    grow memory without bound; past the cap text is dropped.

use super::convert::val_to_jq_capped;
use super::format::{Indent, render};
use super::jaq_json::Val;
use std::cell::RefCell;

/// Cap on buffered stderr text per jq run.
pub(super) const MAX_MESSAGE_BYTES: usize = 1 << 20;

thread_local! {
    static BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

fn push(s: &str) {
    BUF.with(|b| {
        let mut b = b.borrow_mut();
        let room = MAX_MESSAGE_BYTES.saturating_sub(b.len());
        if s.len() <= room {
            b.push_str(s);
        } else {
            let mut end = room;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            b.push_str(&s[..end]);
        }
    });
}

fn compact(v: &Val) -> String {
    match val_to_jq_capped(v, MAX_MESSAGE_BYTES) {
        Some(j) => render(&j, Indent::Compact),
        None => "<value too large>".to_string(),
    }
}

/// `stderr`: a string as is, anything else as compact JSON, no newline.
pub(super) fn stderr(v: &Val) {
    match v {
        Val::TStr(s) | Val::BStr(s) => {
            let bytes: &[u8] = (**s).as_ref();
            push(&String::from_utf8_lossy(bytes));
        }
        _ => push(&compact(v)),
    }
}

/// `debug`: `["DEBUG:",<value>]` and a newline.
pub(super) fn debug(v: &Val) {
    push(&format!("[\"DEBUG:\",{}]\n", compact(v)));
}

/// Drain the buffered text.
pub(super) fn take() -> String {
    BUF.with(|b| std::mem::take(&mut *b.borrow_mut()))
}
