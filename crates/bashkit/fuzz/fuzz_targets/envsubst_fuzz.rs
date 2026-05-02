//! Fuzz target for the envsubst builtin
//!
//! Tests environment variable substitution to find:
//! - Panics on malformed variable references ($, ${, ${VAR, etc.)
//! - Edge cases with special characters in variable names
//! - Nested or recursive variable references
//! - Unclosed braces and partial substitution syntax
//!
//! Run with: cargo +nightly fuzz run envsubst_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Reject deeply nested braces
        let depth: i32 = input
            .bytes()
            .map(|b| match b {
                b'{' => 1,
                b'}' => -1,
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
                .env("HOME", "/home/user")
                .env("PATH", "/usr/bin")
                .env("LANG", "en_US.UTF-8")
                .env("TESTVAR", "hello world")
                .build();

            // Test 1: envsubst on fuzzed text with variable references
            let script = format!(
                "echo '{}' | envsubst",
                input.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script, "envsubst_fuzz", &[]).await;

            // Test 2: envsubst with -v flag to list variables
            let script2 = format!(
                "echo '{}' | envsubst -v",
                input.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script2, "envsubst_fuzz", &[]).await;

            // Test 3: envsubst with SHELL-FORMAT restriction
            let script3 = format!(
                "echo '{}' | envsubst '$HOME $PATH'",
                input.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script3, "envsubst_fuzz", &[]).await;
        });
    }
});
