//! Fuzz target for snapshot decoding.
//!
//! Snapshot bytes are the one input hosts routinely load from storage they do
//! not fully control — a database row, an object store, a cache — and the
//! integrity digest is explicitly *not* a security boundary (TM-SNAP-001). So
//! every decoder below must treat arbitrary bytes as a normal error path.
//!
//! This release reads the v2 container without writing it, which makes the
//! decoder the entire attack surface it adds: bytes written by a *newer*
//! bashkit, arriving at an older one. That is the case this target covers.
//!
//! Run with: cargo +nightly fuzz run snapshot_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

use bashkit::{Bash, Snapshot};

fuzz_target!(|data: &[u8]| {
    // Cap input so the fuzzer spends its time on decoder logic rather than on
    // legitimately-large allocations (threat model V1).
    if data.len() > 1_000_000 {
        return;
    }

    // Both formats dispatch off the body prefix, so arbitrary bytes reach
    // either the v1 JSON path or the v2 container path. Each is a clean error
    // or a valid snapshot; never a panic.
    let _ = Snapshot::from_bytes(data);
    let _ = Snapshot::from_bytes_keyed(data, b"fuzz-key");

    // Restoring into a live instance must also stay on the error path, and
    // must leave the instance usable either way.
    let mut bash = Bash::new();
    let _ = bash.restore_snapshot(data);
    let _ = bash.restore_snapshot_keyed(data, b"fuzz-key");

    // If a snapshot did decode, re-encoding it must not panic — a round trip
    // is the operation a host performs on every resume.
    if let Ok(snapshot) = Snapshot::from_bytes(data) {
        let _ = snapshot.to_bytes();
    }
});
