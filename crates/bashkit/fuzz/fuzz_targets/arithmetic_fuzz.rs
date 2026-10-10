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

        // Reject inputs that themselves contain banned substrings. An
        // arithmetic error names the expression and the unparsed rest
        // verbatim, so the script's own text comes back in stderr -- real bash
        // does the same. An echoed user-controlled string that happens to
        // contain e.g. `Tok::` is not a TM-INF-022 leak: the tokenizer's enum
        // is never formatted. Run 271 found `:A>>=::TTA:::Tok::::`, whose
        // diagnostic reads `... (error token is ":A>>=::TTA:::Tok::::")`.
        // The arithmetic diagnostic is not one of the real-shell templates
        // `strip_real_shell_error_lines` recognizes, so filter at the input
        // layer, as `glob_fuzz` and `cpython_fuzz` do, and keep the leak
        // detector strict for genuine internals.
        if bashkit::testing::input_echo_would_trip(input) {
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
