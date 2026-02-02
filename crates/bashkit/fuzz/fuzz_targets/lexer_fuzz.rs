//! Fuzz target for the BashKit lexer
//!
//! This target tokenizes arbitrary input to find:
//! - Lexer crashes/panics
//! - Infinite loops in tokenization
//! - Memory issues with unusual input
//!
//! Run with: cargo +nightly fuzz run lexer_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8 (bash scripts are text)
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1_000_000 {
            return;
        }

        // Tokenize all input - should never panic
        let mut lexer = bashkit::parser::Lexer::new(input);
        while lexer.next_token().is_some() {
            // Consume all tokens
        }
    }
});
