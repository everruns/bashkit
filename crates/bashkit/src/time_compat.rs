//! Cross-platform `Instant`/`SystemTime`.
//!
//! `std::time::Instant`/`SystemTime` panic unconditionally on
//! wasm32-unknown-unknown ("time not implemented on this platform" — no OS
//! clock, no extension point to satisfy; see
//! `library/std/src/sys/time/unsupported.rs` in the Rust source). `web-time`
//! is a drop-in replacement backed by `Performance.now()`/`Date.now()` via
//! web-sys on that target, and a transparent re-export of `std::time`
//! everywhere else (including wasm32-wasip1-threads, which has real WASI
//! clocks). Use this module's `Instant`/`SystemTime`/`UNIX_EPOCH` instead of
//! `std::time`'s directly anywhere wall-clock time is read.

#[cfg(target_arch = "wasm32")]
pub(crate) use web_time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
            let dur = e.duration();
            chrono::DateTime::from_timestamp(-(dur.as_secs() as i64), 0).unwrap_or_else(epoch)
        }
    }
}

/// Convert from a `chrono::DateTime`. Mirror of [`to_chrono_utc`] — see there
/// for why this can't just be a `From`/`Into` conversion.
pub(crate) fn from_chrono<Tz: chrono::TimeZone>(dt: chrono::DateTime<Tz>) -> SystemTime {
    let utc = dt.with_timezone(&chrono::Utc);
    let secs = utc.timestamp();
    let nanos = utc.timestamp_subsec_nanos();
    if secs >= 0 {
        UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos)
    } else {
        UNIX_EPOCH - std::time::Duration::new((-secs) as u64, 0)
    }
}
