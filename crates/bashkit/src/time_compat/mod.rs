//! Cross-platform `Instant`/`SystemTime`.
//!
//! `std::time::Instant`/`SystemTime` panic unconditionally on
//! wasm32-unknown-unknown ("time not implemented on this platform" — no OS
//! clock, no extension point to satisfy; see
//! `library/std/src/sys/time/unsupported.rs` in the Rust source). `web-time`
//! is a drop-in replacement backed by `Performance.now()`/`Date.now()` via
//! web-sys on that target. Every other target reads `std::time` directly,
//! including the WASI targets (wasip1, wasip2, wasip1-threads), which have real
//! clocks. Use this module's `Instant`/`SystemTime`/`UNIX_EPOCH` instead of
//! `std::time`'s directly anywhere wall-clock time is read.
//!
//! `web-time` is only correct when the wasm host *is* a JS host, so it sits
//! behind the `wasm_js` feature. Without it, `wasm32-unknown-unknown` takes
//! [`host_clock`], where the embedder supplies the clock (and the timer, by
//! spinning on it). See that module for the contract.

use std::future::Future;
use std::time::Duration;

#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    not(feature = "wasm_js")
))]
pub mod host_clock;

#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    not(feature = "wasm_js")
))]
pub(crate) use host_clock::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "wasm_js"))]
pub(crate) use web_time::{Instant, SystemTime, UNIX_EPOCH};

// WASI targets (wasip1, wasip2, wasip1-threads) have real clocks in std, and
// no tokio timer driver worth routing `Instant` through.
#[cfg(all(target_arch = "wasm32", not(target_os = "unknown")))]
pub(crate) use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::{SystemTime, UNIX_EPOCH};

// Tokio's instant follows the runtime's paused/advanced monotonic clock. This
// makes shell deadlines and `time` deterministic under virtual-time runtimes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use tokio::time::Instant;

/// Portable timer future. JS-host wasm uses `setTimeout`; native targets keep
/// tokio's runtime timer. The wasm future is wrapped as `Send` because the
/// target is deliberately single-threaded (the same invariant as JS builtins).
pub(crate) async fn sleep(duration: Duration) {
    #[cfg(all(
        target_arch = "wasm32",
        target_os = "unknown",
        not(feature = "wasm_js")
    ))]
    {
        // No timers and no other thread to make progress: spin the host clock.
        // Blocking is the point — the single-poll driver these embedders use
        // (`now_or_never`) can never come back to a pending timer future.
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            std::hint::spin_loop();
        }
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "wasm_js"))]
    send_wrapper::SendWrapper::new(gloo_timers::future::TimeoutFuture::new(duration_millis(
        duration,
    )))
    .await;

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    tokio::time::sleep(duration).await;
}

pub(crate) struct TimeoutElapsed;

/// Race a future against a host wall-clock deadline on every supported target.
pub(crate) async fn timeout<F: Future>(
    duration: Duration,
    future: F,
) -> Result<F::Output, TimeoutElapsed> {
    #[cfg(all(
        target_arch = "wasm32",
        target_os = "unknown",
        not(feature = "wasm_js")
    ))]
    {
        // Poll the future, then check the host clock. Without timers there is
        // nothing to race against, so a still-pending future past the deadline
        // is the timeout.
        use std::pin::pin;
        use std::task::Poll;

        let deadline = Instant::now() + duration;
        let mut future = pin!(future);
        return std::future::poll_fn(move |cx| match future.as_mut().poll(cx) {
            Poll::Ready(output) => Poll::Ready(Ok(output)),
            Poll::Pending if Instant::now() >= deadline => Poll::Ready(Err(TimeoutElapsed)),
            Poll::Pending => Poll::Pending,
        })
        .await;
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "wasm_js"))]
    {
        use futures_util::future::{Either, select};
        let timer = send_wrapper::SendWrapper::new(gloo_timers::future::TimeoutFuture::new(
            duration_millis(duration),
        ));
        futures_util::pin_mut!(future, timer);
        match select(future, timer).await {
            Either::Left((output, _)) => Ok(output),
            Either::Right(_) => Err(TimeoutElapsed),
        }
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_| TimeoutElapsed)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "wasm_js"))]
fn duration_millis(duration: Duration) -> u32 {
    (duration.as_secs_f64() * 1000.0)
        .ceil()
        .clamp(0.0, u32::MAX as f64) as u32
}

/// Convert to a `chrono::DateTime<Utc>`.
///
/// `chrono` implements `From<std::time::SystemTime>` but not
/// `From<web_time::SystemTime>` — same API shape, different type, so the
/// blanket impl doesn't apply on wasm32. Goes through `duration_since`
/// instead, which both `SystemTime`s support identically.
pub(crate) fn to_chrono_utc(t: SystemTime) -> chrono::DateTime<chrono::Utc> {
    let epoch = || chrono::DateTime::from_timestamp(0, 0).expect("epoch is representable");
    match t.duration_since(UNIX_EPOCH) {
        Ok(dur) => chrono::DateTime::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos())
            .unwrap_or_else(epoch),
        Err(e) => {
            // `dur` is how far *before* the epoch `t` is. chrono represents
            // negative timestamps as (floor_secs, nanos) where nanos is the
            // non-negative remainder added back — e.g. epoch - 500ms is
            // (-1, 500_000_000), not (0, 0). Preserve that decomposition
            // instead of truncating the subsecond part.
            let dur = e.duration();
            let (secs, nanos) = if dur.subsec_nanos() == 0 {
                (-(dur.as_secs() as i64), 0)
            } else {
                (
                    -(dur.as_secs() as i64) - 1,
                    1_000_000_000 - dur.subsec_nanos(),
                )
            };
            chrono::DateTime::from_timestamp(secs, nanos).unwrap_or_else(epoch)
        }
    }
}

/// Current wall-clock time as a `chrono::DateTime<Utc>`.
///
/// Use instead of `chrono::Utc::now()`: chrono reads the clock through
/// `std::time::SystemTime` (which panics on wasm32-unknown-unknown) or, with
/// its `wasmbind` feature, through the JS `Date` — neither works for a non-JS
/// wasm embedder. This routes through [`SystemTime`], so every target gets the
/// clock it actually has.
pub(crate) fn now_utc() -> chrono::DateTime<chrono::Utc> {
    to_chrono_utc(SystemTime::now())
}

/// Convert from a `chrono::DateTime`. Mirror of [`to_chrono_utc`] — see there
/// for why this can't just be a `From`/`Into` conversion.
pub(crate) fn from_chrono<Tz: chrono::TimeZone>(dt: chrono::DateTime<Tz>) -> SystemTime {
    let utc = dt.with_timezone(&chrono::Utc);
    let secs = utc.timestamp();
    let nanos = utc.timestamp_subsec_nanos();
    if secs >= 0 {
        UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos)
    } else if nanos == 0 {
        UNIX_EPOCH - std::time::Duration::new((-secs) as u64, 0)
    } else {
        // Mirror of the decomposition in `to_chrono_utc`: `secs` is the
        // floor, so the actual distance before the epoch is one second
        // less than `-secs`, plus the complementary nanos.
        UNIX_EPOCH - std::time::Duration::new((-secs - 1) as u64, 1_000_000_000 - nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_utc_tracks_the_system_clock() {
        let before = chrono::Utc::now();
        let sampled = now_utc();
        let after = chrono::Utc::now();
        assert!(
            sampled >= before && sampled <= after,
            "now_utc() {sampled} outside [{before}, {after}]"
        );
    }

    #[test]
    fn now_utc_is_monotonic_across_calls() {
        assert!(now_utc() <= now_utc());
    }

    #[test]
    fn to_chrono_utc_epoch() {
        let dt = to_chrono_utc(UNIX_EPOCH);
        assert_eq!((dt.timestamp(), dt.timestamp_subsec_nanos()), (0, 0));
    }

    #[test]
    fn to_chrono_utc_pre_epoch_whole_second() {
        let t = UNIX_EPOCH - std::time::Duration::from_secs(2);
        let dt = to_chrono_utc(t);
        assert_eq!((dt.timestamp(), dt.timestamp_subsec_nanos()), (-2, 0));
    }

    #[test]
    fn to_chrono_utc_pre_epoch_subsecond() {
        // Reviewer's case: 500ms before the epoch must round-trip as
        // 1969-12-31T23:59:59.500Z, i.e. (-1, 500_000_000) — not (0, 0).
        let t = UNIX_EPOCH - std::time::Duration::from_millis(500);
        let dt = to_chrono_utc(t);
        assert_eq!(
            (dt.timestamp(), dt.timestamp_subsec_nanos()),
            (-1, 500_000_000)
        );
        assert_eq!(dt.to_rfc3339(), "1969-12-31T23:59:59.500+00:00");
    }

    #[test]
    fn from_chrono_pre_epoch_subsecond() {
        let dt = chrono::DateTime::from_timestamp(-1, 500_000_000).unwrap();
        let t = from_chrono(dt);
        assert_eq!(
            t.duration_since(UNIX_EPOCH).unwrap_err().duration(),
            std::time::Duration::from_millis(500)
        );
    }

    #[test]
    fn round_trip_pre_epoch_subsecond() {
        let original = UNIX_EPOCH - std::time::Duration::from_millis(500);
        let round_tripped = from_chrono(to_chrono_utc(original));
        assert_eq!(round_tripped, original);
    }

    #[test]
    fn round_trip_post_epoch_subsecond() {
        let original = UNIX_EPOCH + std::time::Duration::from_millis(500);
        let round_tripped = from_chrono(to_chrono_utc(original));
        assert_eq!(round_tripped, original);
    }
}
