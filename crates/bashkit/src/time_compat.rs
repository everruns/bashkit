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
