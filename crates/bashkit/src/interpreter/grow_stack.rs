//! Grow the native stack on demand while polling interpreter futures.
//!
//! Decision (TM-DOS-142): every interpreter level (`execute_command`) is a
//! boxed future, but polling a nest of them still recurses on the caller's
//! native stack. Per-kind depth caps (functions, `source`, child shells,
//! `eval`) bound how many levels exist, yet their *product* (nested `if`s
//! inside a recursing function, `eval` inside functions, ...) could still
//! exhaust a 2 MiB tokio worker stack, and a stack overflow aborts the whole
//! process: every tenant in it. Rather than tuning ever more caps against
//! debug and release frame sizes, each command poll checks the remaining
//! stack and, when it is below [`RED_ZONE`], continues on a fresh heap
//! segment of [`SEGMENT`] bytes (`stacker`). The depth limits still bound
//! how many segments one execution can use, so memory stays bounded.
//!
//! On wasm32 `stacker` cannot switch stacks; the wrapper is a plain poll.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Remaining stack below which a poll moves to a new segment. Must exceed
/// the stack one interpreter level can use between two command polls in a
/// debug build (measured at ~150 KiB for an `eval` level).
#[cfg(not(target_arch = "wasm32"))]
const RED_ZONE: usize = 512 * 1024;

/// Size of each additional stack segment.
#[cfg(not(target_arch = "wasm32"))]
const SEGMENT: usize = 4 * 1024 * 1024;

/// Future wrapper whose `poll` runs with at least [`RED_ZONE`] bytes of stack.
pub(super) struct GrowStack<F>(Pin<Box<F>>);

impl<F: Future> GrowStack<F> {
    pub(super) fn new(inner: F) -> Self {
        Self(Box::pin(inner))
    }
}

impl<F: Future> Future for GrowStack<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.0.as_mut();
        #[cfg(not(target_arch = "wasm32"))]
        {
            stacker::maybe_grow(RED_ZONE, SEGMENT, || inner.poll(cx))
        }
        #[cfg(target_arch = "wasm32")]
        {
            inner.poll(cx)
        }
    }
}
