//! Clock for non-JS `wasm32-unknown-unknown` embedders.
//!
//! Important decision: on `wasm32-unknown-unknown` bashkit's default clock is
//! `web-time`, which reads `Performance.now()`/`Date.now()` through
//! wasm-bindgen. That is right for a JS host and impossible anywhere else — a
//! wasm component running in a runtime with no JS engine (wasmtime with no
//! WASI, or the wasmtime-based guest of a Hyperlight micro-VM) cannot satisfy
//! those imports, so the module fails to instantiate.
//!
//! With the `wasm_js` feature off, the clock becomes a single symbol the
//! embedder must define, mirroring `getrandom`'s custom-backend contract:
//!
//! ```ignore
//! #[unsafe(no_mangle)]
//! extern "Rust" fn __bashkit_host_now_micros() -> u64 {
//!     host::now_micros() // however the embedder reaches its host
//! }
//! ```
//!
//! Microseconds since the Unix epoch, assumed non-decreasing. It backs both
//! [`SystemTime`] (wall clock) and [`Instant`] (monotonic); an embedder with
//! two distinct clocks should feed the monotonic one, since bashkit only
//! measures durations with `Instant` and only formats dates with `SystemTime`.
//! Omitting the symbol is a link error, not a silent panic at first use.

use std::ops::{Add, Sub};
use std::time::Duration;

unsafe extern "Rust" {
    safe fn __bashkit_host_now_micros() -> u64;
}

fn now_duration() -> Duration {
    Duration::from_micros(__bashkit_host_now_micros())
}

/// Wall clock, measured from [`UNIX_EPOCH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SystemTime(Duration);

/// The Unix epoch, the zero point of the host clock.
pub const UNIX_EPOCH: SystemTime = SystemTime(Duration::ZERO);

/// Returned when a [`SystemTime`] comparison runs backwards; mirrors
/// `std::time::SystemTimeError`.
#[derive(Debug, Clone)]
pub struct SystemTimeError(Duration);

impl SystemTimeError {
    /// How far the other side lies in the future.
    pub fn duration(&self) -> Duration {
        self.0
    }
}

impl SystemTime {
    /// Current wall-clock time from the host.
    pub fn now() -> Self {
        Self(now_duration())
    }

    /// Time elapsed since `earlier`, or how far it lies ahead.
    pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
        self.0
            .checked_sub(earlier.0)
            .ok_or_else(|| SystemTimeError(earlier.0 - self.0))
    }
}

impl Add<Duration> for SystemTime {
    type Output = SystemTime;
    fn add(self, rhs: Duration) -> SystemTime {
        SystemTime(self.0 + rhs)
    }
}

impl Sub<Duration> for SystemTime {
    type Output = SystemTime;
    fn sub(self, rhs: Duration) -> SystemTime {
        SystemTime(self.0.saturating_sub(rhs))
    }
}

/// Monotonic clock, for measuring elapsed time and deadlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(Duration);

impl Instant {
    /// Current reading of the host's monotonic clock.
    pub fn now() -> Self {
        Self(now_duration())
    }

    /// Time since this instant was taken; zero if the host clock went backwards.
    pub fn elapsed(&self) -> Duration {
        now_duration().saturating_sub(self.0)
    }

    /// Time from `earlier` to here, or `None` if `earlier` is later.
    // Only reached through the `bash_tool` deadline path, so it is dead code in
    // a minimal embedder build.
    #[allow(dead_code)]
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0)
    }

    /// This instant advanced by `duration`, or `None` on overflow.
    pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_add(duration).map(Instant)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, rhs: Duration) -> Instant {
        Instant(self.0 + rhs)
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, rhs: Instant) -> Duration {
        self.0.saturating_sub(rhs.0)
    }
}
