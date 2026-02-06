//! Fuzz target for the Bashkit parser
//!
//! This target attempts to parse arbitrary input to find:
//! - Parser crashes/panics
//! - Stack overflows (deep nesting)
//! - Infinite loops or hangs
//!
//! Run with: cargo +nightly fuzz run parser_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8 (bash scripts are text)
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM (threat model V1)
        if input.len() > 1_000_000 {
            return;
        }

        // Attempt to parse - should never panic
        let parser = bashkit::parser::Parser::new(input);
        let _ = parser.parse();
    }
});
