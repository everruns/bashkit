//! In-process pipe between concurrent pipeline stages.
//!
//! Important decision: a pipe is a shared byte buffer, not a channel of
//! chunks. The writing stage appends whole command outputs at command
//! boundaries (overshooting the capacity is allowed, so a single builtin's
//! output is never split) and then waits at its next command until the buffer
//! drops below [`PIPE_CAPACITY`]. The reading stage pulls bytes on demand.
//! Closing the read end makes the next append fail, which the writer turns
//! into SIGPIPE (exit 141), like a write to a pipe with no readers.
//!
//! Both ends are polled on the same task (see `execute_pipeline`), so there
//! are no threads: each side keeps one waker slot.

use std::future::poll_fn;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

/// Bytes buffered before a writer waits.
///
/// Linux buffers 64 KiB, but every byte a producer runs ahead costs
/// commands from the execution budget: an endless `while :; do echo x; done`
/// would spend ~64K commands before `head` could stop it. 4 KiB (`PIPE_BUF`)
/// keeps that waste small; the only visible effect is that a finite
/// producer writing between 4 and 64 KiB into a reader that quits early
/// exits 141 where bash would usually exit 0.
pub(crate) const PIPE_CAPACITY: usize = 4 * 1024;

#[derive(Default)]
struct State {
    buf: Vec<u8>,
    /// Bytes at the start of `buf` already consumed.
    pos: usize,
    write_closed: bool,
    read_closed: bool,
    /// A write found the read end closed.
    broken: bool,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
}

impl State {
    fn available(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn wake_reader(&mut self) {
        if let Some(w) = self.read_waker.take() {
            w.wake();
        }
    }

    fn wake_writer(&mut self) {
        if let Some(w) = self.write_waker.take() {
            w.wake();
        }
    }

    fn take(&mut self, n: usize) -> Vec<u8> {
        let end = self.pos + n.min(self.available());
        let out = self.buf[self.pos..end].to_vec();
        self.pos = end;
        if self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        }
        self.wake_writer();
        out
    }
}

/// Shared pipe state; ends are tracked with [`WriteEnd`] / [`ReadEnd`].
#[derive(Default)]
pub(crate) struct Pipe {
    state: Mutex<State>,
}

impl Pipe {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Append `data`. Returns false (and marks the pipe broken) when nobody
    /// reads any more.
    pub(crate) fn write(&self, data: &[u8]) -> bool {
        if data.is_empty() {
            return true;
        }
        let mut s = self.lock();
        if s.read_closed {
            s.broken = true;
            return false;
        }
        s.buf.extend_from_slice(data);
        s.wake_reader();
        true
    }

    /// A write hit a closed read end.
    pub(crate) fn is_broken(&self) -> bool {
        self.lock().broken
    }

    /// Wait until the buffer has room or the reader is gone.
    pub(crate) async fn writable(&self) {
        poll_fn(|cx: &mut Context<'_>| {
            let mut s = self.lock();
            if s.read_closed || s.available() < PIPE_CAPACITY {
                Poll::Ready(())
            } else {
                s.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }

    pub(crate) fn close_write(&self) {
        let mut s = self.lock();
        s.write_closed = true;
        s.wake_reader();
    }

    pub(crate) fn close_read(&self) {
        let mut s = self.lock();
        s.read_closed = true;
        s.buf.clear();
        s.pos = 0;
        s.wake_writer();
    }

    /// Wait for data or end of input; returns the available bytes, empty at
    /// end of input.
    pub(crate) async fn read_some(&self) -> Vec<u8> {
        self.read_chunk(false).await
    }

    /// Like [`read_some`](Self::read_some), but stops after the first
    /// newline, leaving the rest in the pipe (and the writer paused).
    pub(crate) async fn read_line_chunk(&self) -> Vec<u8> {
        self.read_chunk(true).await
    }

    async fn read_chunk(&self, line: bool) -> Vec<u8> {
        poll_fn(|cx: &mut Context<'_>| {
            let mut s = self.lock();
            let mut n = s.available();
            if line && let Some(i) = s.buf[s.pos..].iter().position(|&b| b == b'\n') {
                n = i + 1;
            }
            if n > 0 {
                Poll::Ready(s.take(n))
            } else if s.write_closed {
                Poll::Ready(Vec::new())
            } else {
                s.read_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }

    /// Read everything until the writer closes.
    #[cfg(test)]
    pub(crate) async fn read_to_end(&self) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let chunk = self.read_some().await;
            if chunk.is_empty() {
                return out;
            }
            out.extend_from_slice(&chunk);
        }
    }
}

/// Write end owned by a producing stage; closes on drop so the reader sees
/// end of input even if the stage is cancelled.
pub(crate) struct WriteEnd(pub(crate) Arc<Pipe>);

impl Drop for WriteEnd {
    fn drop(&mut self) {
        self.0.close_write();
    }
}

/// Read end owned by a consuming stage; closes on drop so the writer gets
/// SIGPIPE on its next write.
pub(crate) struct ReadEnd(pub(crate) Arc<Pipe>);

impl Drop for ReadEnd {
    fn drop(&mut self) {
        self.0.close_read();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    #[tokio::test]
    async fn read_sees_writes_then_eof() {
        let p = Pipe::new();
        assert!(p.write(b"ab"));
        assert!(p.write(b"c"));
        p.close_write();
        assert_eq!(p.read_to_end().await, b"abc");
        assert!(p.read_some().await.is_empty());
    }

    #[tokio::test]
    async fn line_reads_leave_the_rest() {
        let p = Pipe::new();
        p.write(b"a\nb\nc");
        p.close_write();
        assert_eq!(p.read_line_chunk().await, b"a\n");
        assert_eq!(p.read_line_chunk().await, b"b\n");
        assert_eq!(p.read_line_chunk().await, b"c");
        assert!(p.read_line_chunk().await.is_empty());
    }

    #[tokio::test]
    async fn write_after_reader_closed_breaks() {
        let p = Pipe::new();
        assert!(p.write(b"x"));
        drop(ReadEnd(Arc::clone(&p)));
        assert!(!p.is_broken());
        assert!(!p.write(b"y"));
        assert!(p.is_broken());
        // An empty write is not a write.
        assert!(p.write(b""));
    }

    #[tokio::test]
    async fn writer_waits_until_drained() {
        let p = Pipe::new();
        p.write(&vec![b'a'; PIPE_CAPACITY]);
        let mut wait = Box::pin(p.writable());
        let waker = futures_util::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(p.read_some().await.len(), PIPE_CAPACITY);
        assert!(wait.as_mut().poll(&mut cx).is_ready());
    }
}
