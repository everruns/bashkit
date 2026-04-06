//! Fuzz target for the jq builtin
//!
//! Tests jq filter expression parsing and JSON processing to find:
//! - Panics in the jaq filter compiler
//! - Stack overflow from deeply nested filters
//! - ReDoS or CPU exhaustion from pathological patterns
//! - Memory exhaustion from recursive JSON generation
//!
//! Run with: cargo +nightly fuzz run jq_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into filter (first line) and JSON data (rest)
        let (filter, json_data) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "{}" as &str),
        };

        // Skip empty filters
        if filter.trim().is_empty() {
            return;
        }

        // Reject deeply nested expressions
        let depth: i32 = filter
            .bytes()
            .map(|b| match b {
                b'(' | b'[' | b'{' => 1,
                b')' | b']' | b'}' => -1,
                _ => 0,
            })
            .scan(0i32, |acc, d| {
                *acc += d;
                Some(*acc)
            })
            .max()
            .unwrap_or(0);
        if depth > 20 {
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

            // Test 1: pipe JSON through jq filter
            let script = format!(
                "echo '{}' | jq '{}' 2>/dev/null; true",
                json_data.replace('\'', "'\\''"),
                filter.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script).await;

            // Test 2: jq with -r (raw output) flag
            let script2 = format!(
                "echo '{}' | jq -r '{}' 2>/dev/null; true",
                json_data.replace('\'', "'\\''"),
                filter.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script2).await;
        });
    }
});
