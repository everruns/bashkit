//! Fuzz target for the csv builtin
//!
//! Tests the custom CSV parser to find:
//! - Panics on mismatched quotes or malformed fields
//! - Edge cases with embedded newlines, empty fields, various delimiters
//! - Memory exhaustion from pathological input
//! - Incorrect parsing of escaped quotes
//!
//! Run with: cargo +nightly fuzz run csv_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Skip empty input
        if input.trim().is_empty() {
            return;
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let mut bash = bashkit::Bash::builder()
                .limits(
                    bashkit::ExecutionLimits::new()
                        .max_commands(50)
                        .max_subst_depth(3)
                        .max_stdout_bytes(4096)
                        .max_stderr_bytes(4096)
                        .timeout(std::time::Duration::from_millis(200)),
                )
                .build();

            // Test 1: parse CSV and list headers
            let script = format!(
                "echo '{}' | csv headers 2>/dev/null; true",
                input.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script).await;

            // Test 2: parse CSV and count rows
            let script2 = format!(
                "echo '{}' | csv count 2>/dev/null; true",
                input.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script2).await;

            // Test 3: parse CSV and select first column
            let script3 = format!(
                "echo '{}' | csv select 1 2>/dev/null; true",
                input.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script3).await;

            // Test 4: parse CSV with custom delimiter
            let script4 = format!(
                "echo '{}' | csv -d '\\t' headers 2>/dev/null; true",
                input.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script4).await;
        });
    }
});
