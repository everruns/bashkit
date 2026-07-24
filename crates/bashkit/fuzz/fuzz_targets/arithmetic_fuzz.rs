//! Fuzz target for arithmetic expansion
//!
//! This target tests arithmetic parsing and evaluation to find:
//! - Integer overflow/underflow issues
//! - Division by zero handling
//! - Parsing errors with unusual expressions
//!
//! Run with: cargo +nightly fuzz run arithmetic_fuzz -- -max_total_time=300

#![no_main]

use bashkit_fuzz::is_arithmetic_expression;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size — 512 bytes is enough to exercise all arithmetic
        // paths without hitting OOM on deeply nested expressions
        if input.len() > 512 {
            return;
        }

        // Keep this target inside arithmetic expansion. Shell syntax or
        // unbalanced grouping could close `$((...))` and execute the remainder
        // as a command, which belongs in the parser and interpreter fuzzers.
        if !is_arithmetic_expression(input) {
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
            bashkit::testing::fuzz_init();
            let mut bash = bashkit::Bash::builder()
                .limits(
                    bashkit::ExecutionLimits::new()
                        .max_commands(100)
                        .max_function_depth(10)
                        .max_subst_depth(5)
                        .max_stdout_bytes(4096)
                        .max_stderr_bytes(4096)
                        .timeout(std::time::Duration::from_millis(100)),
                )
                .build();

            // Should not panic, errors are acceptable
            bashkit::testing::fuzz_exec(&mut bash, &script, "arithmetic_fuzz", &[]).await;
        });
    }
});
