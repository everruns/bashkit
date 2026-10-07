//! Fuzz target for the CPython HTTP bridge request decoder
//!
//! Feeds arbitrary bytes as the request record the guest hands to the
//! `bashkit.http_request` host import, to find:
//! - Host panics on malformed or truncated records
//! - Requests the host would dispatch with CR/LF/NUL in header values,
//!   invalid header names, host-owned headers (`Host`, framing), unsupported
//!   methods or oversized fields (TM-PY-CPY-003)
//!
//! Run with: cargo +nightly fuzz run cpython_http_fuzz --features cpython -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    bashkit::testing::check_cpython_http_request(data);
});
