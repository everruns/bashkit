//! Platform-compatible time types.
//!
//! On native targets this re-exports `std::time` directly.
//! On `wasm32-unknown-unknown` it uses `web_time` so that
//! `SystemTime::now()` works in the browser instead of panicking.

#[cfg(target_family = "wasm")]
pub use web_time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(not(target_family = "wasm"))]
pub use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Convert a [`chrono::DateTime`] to our platform-compatible [`SystemTime`].
///
/// `chrono`'s `From` impls only cover `std::time::SystemTime`, so this helper
/// bridges the gap on WASM where we use `web_time::SystemTime`.
pub fn from_chrono<Tz: chrono::TimeZone>(dt: chrono::DateTime<Tz>) -> SystemTime {
    let secs = dt.timestamp();
    let nanos = dt.timestamp_subsec_nanos();
    UNIX_EPOCH + Duration::from_secs(secs as u64) + Duration::from_nanos(nanos as u64)
}

/// Convert our platform-compatible [`SystemTime`] to a [`chrono::DateTime<Utc>`].
///
/// `chrono`'s `From` impls only cover `std::time::SystemTime`, so this helper
/// bridges the gap on WASM where we use `web_time::SystemTime`.
pub fn to_chrono_utc(st: SystemTime) -> chrono::DateTime<chrono::Utc> {
    let duration = st.duration_since(UNIX_EPOCH).unwrap_or_default();
    chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH)
}
