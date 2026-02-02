//! Fuzz target for arithmetic expansion
//!
//! This target tests arithmetic parsing and evaluation to find:
//! - Integer overflow/underflow issues
//! - Division by zero handling
//! - Parsing errors with unusual expressions
//!
//! Run with: cargo +nightly fuzz run arithmetic_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size
        if input.len() > 10_000 {
            return;
        }

        // Wrap input in arithmetic expansion context
        let script = format!("echo $(({}))", input);

        // Parse and execute - should handle errors gracefully
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let mut bash = bashkit::Bash::builder()
                .limits(
                    bashkit::ExecutionLimits::new()
                        .max_commands(100)
                        .timeout(std::time::Duration::from_millis(100)),
                )
                .build();

            // Should not panic, errors are acceptable
            let _ = bash.exec(&script).await;
        });
    }
});
