//! WASM-specific smoke tests for bashkit platform-compatible time types.
//!
//! Run with: wasm-pack test --node
//!     or: wasm-pack test --headless --chrome

use wasm_bindgen_test::*;

#[wasm_bindgen_test]
fn system_time_now_does_not_panic() {
    let _now = bashkit::time::SystemTime::now();
}

#[wasm_bindgen_test]
fn unix_epoch_is_before_now() {
    let epoch = bashkit::time::UNIX_EPOCH;
    let now = bashkit::time::SystemTime::now();
    let duration = now.duration_since(epoch).expect("now should be after epoch");
    assert!(duration.as_secs() > 1_000_000_000, "expected year 2001+");
}

#[wasm_bindgen_test]
fn chrono_roundtrip_utc() {
    let now = bashkit::time::SystemTime::now();
    let dt = bashkit::time::to_chrono_utc(now);
    let back = bashkit::time::from_chrono(dt);

    let diff = now
        .duration_since(back)
        .unwrap_or_else(|e| e.duration())
        .as_millis();
    assert!(diff < 2, "roundtrip drift should be < 2ms, got {}ms", diff);
}

#[wasm_bindgen_test]
fn duration_arithmetic() {
    let a = bashkit::time::Duration::from_secs(10);
    let b = bashkit::time::Duration::from_secs(5);
    assert_eq!((a + b).as_secs(), 15);
}
