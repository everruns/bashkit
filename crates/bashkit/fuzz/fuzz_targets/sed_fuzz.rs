//! Fuzz target for the sed builtin
//!
//! Tests sed command parsing and BRE-to-ERE regex conversion to find:
//! - Panics in sed command parsing or address ranges
//! - Stack overflow from deeply nested regex groups
//! - ReDoS from pathological BRE/ERE patterns
//! - Edge cases in BRE-to-ERE conversion logic
//!
//! Run with: cargo +nightly fuzz run sed_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into sed expression (first line) and input data (rest)
        let (expr, input_data) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "hello world\nfoo bar\nbaz qux\n" as &str),
        };

        // Skip empty expressions
        if expr.trim().is_empty() {
            return;
        }

        // Reject deeply nested regex groups
        let depth: i32 = expr
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

            // Test 1: basic sed expression
            let script = format!(
                "echo '{}' | sed '{}'",
                input_data.replace('\'', "'\\''"),
                expr.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script, "sed_fuzz", &[]).await;

            // Test 2: sed with -E (extended regex) flag
            let script2 = format!(
                "echo '{}' | sed -E '{}'",
                input_data.replace('\'', "'\\''"),
                expr.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script2, "sed_fuzz", &[]).await;

            // Test 3: sed with -n (suppress output) flag
            let script3 = format!(
                "echo '{}' | sed -n '{}'",
                input_data.replace('\'', "'\\''"),
                expr.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script3, "sed_fuzz", &[]).await;
        });
    }
});
