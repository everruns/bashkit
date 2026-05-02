//! Fuzz target for the grep builtin
//!
//! Tests regex pattern compilation and matching to find:
//! - ReDoS from catastrophic backtracking on pathological patterns
//! - Panics in bracket expression parsing or extended regex features
//! - Edge cases in case-insensitive matching, invert, and context lines
//! - Graceful rejection of invalid regex patterns
//!
//! Run with: cargo +nightly fuzz run grep_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into regex pattern (first line) and search text (rest)
        let (pattern, text) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "hello world\nfoo bar\nbaz qux\n" as &str),
        };

        // Skip empty patterns
        if pattern.trim().is_empty() {
            return;
        }

        // Reject deeply nested regex groups (ReDoS mitigation)
        let depth: i32 = pattern
            .bytes()
            .map(|b| match b {
                b'(' | b'[' => 1,
                b')' | b']' => -1,
                _ => 0,
            })
            .scan(0i32, |acc, d| {
                *acc += d;
                Some(*acc)
            })
            .max()
            .unwrap_or(0);
        if depth > 15 {
            return;
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            bashkit::testing::fuzz_init();
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

            // Test 1: basic grep pattern matching
            let script = format!(
                "echo '{}' | grep '{}'",
                text.replace('\'', "'\\''"),
                pattern.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script, "grep_fuzz", &[]).await;

            // Test 2: extended regex (-E)
            let script2 = format!(
                "echo '{}' | grep -E '{}'",
                text.replace('\'', "'\\''"),
                pattern.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script2, "grep_fuzz", &[]).await;

            // Test 3: case-insensitive with line numbers (-in)
            let script3 = format!(
                "echo '{}' | grep -in '{}'",
                text.replace('\'', "'\\''"),
                pattern.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script3, "grep_fuzz", &[]).await;

            // Test 4: inverted match with count (-vc)
            let script4 = format!(
                "echo '{}' | grep -vc '{}'",
                text.replace('\'', "'\\''"),
                pattern.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script4, "grep_fuzz", &[]).await;
        });
    }
});
