//! Fuzz target for snapshot decoding.
//!
//! Snapshot bytes are the one input hosts routinely load from storage they do
//! not fully control — a database row, an object store, a cache — and the
//! integrity digest is explicitly *not* a security boundary (TM-SNAP-001). So
//! every decoder below must treat arbitrary bytes as a normal error path.
//!
//! This target checks:
//! - Decoding arbitrary bytes never panics (no slice-index, no unwrap, no
//!   arithmetic overflow, no allocation from an attacker-declared length)
//! - Object-id parsing never panics on arbitrary strings, including non-ASCII
//!   and multi-byte characters at any offset
//! - A decode that succeeds produces state that re-encodes without panicking
//!
//! Run with: cargo +nightly fuzz run snapshot_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

use bashkit::{Bash, CheckoutPolicy, ObjectId, Snapshot, SnapshotGraph};
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    // Cap input so the fuzzer spends its time on decoder logic rather than on
    // legitimately-large allocations (threat model V1).
    if data.len() > 1_000_000 {
        return;
    }

    // 1. The packed container path, both formats. Arbitrary bytes are either a
    //    clean error or a valid snapshot; never a panic.
    let _ = Snapshot::from_bytes(data);
    let _ = Snapshot::from_bytes_keyed(data, b"fuzz-key");

    // Restoring into a live instance must also stay on the error path, and
    // must leave the instance usable either way.
    let mut bash = Bash::new();
    let _ = bash.restore_snapshot_with_policy(data, CheckoutPolicy::Force);

    // If a snapshot did decode, re-encoding it must not panic — a round trip
    // is the operation a host performs on every resume.
    if let Ok(snapshot) = Snapshot::from_bytes(data) {
        let _ = snapshot.to_bytes();
    }

    // 2. Object-id parsing. Ids arrive as caller-supplied strings from three
    //    languages, so any byte sequence can reach `from_hex`.
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = ObjectId::from_hex(text);
        // Sub-slices at character boundaries exercise lengths around the
        // 64-byte check without constructing invalid UTF-8.
        for (offset, _) in text.char_indices().take(64) {
            let _ = ObjectId::from_hex(&text[offset..]);
        }
    }

    // 3. The graph walkers, against a store built from the fuzz input. Keys are
    //    derived from the data so some inputs land on a real object id and
    //    exercise the decode path rather than the not-found path.
    if data.len() >= 32 {
        let mut root_bytes = [0u8; 32];
        root_bytes.copy_from_slice(&data[..32]);
        let root = ObjectId::from_bytes(root_bytes);

        let mut store: HashMap<ObjectId, Vec<u8>> = HashMap::new();
        store.insert(root, data[32..].to_vec());

        let _ = SnapshotGraph::read_commit(root, &store);
        let _ = SnapshotGraph::parents(root, &store);
        let _ = SnapshotGraph::meta(root, &store);
        let _ = SnapshotGraph::capabilities(root, &store);
        let _ = SnapshotGraph::ancestry(root, &store, 1000);
        let _ = SnapshotGraph::plan_checkout(root, &store);
        let _ = SnapshotGraph::reachable(root, &store);
        let _ = SnapshotGraph::diff(root, root, &store);

        let mut target = Bash::new();
        let _ = target.checkout(root, &store, CheckoutPolicy::Force);
    }
});
