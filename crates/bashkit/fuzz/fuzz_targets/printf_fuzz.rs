//! Fuzz target for the printf builtin
//!
//! Tests printf format string parsing and argument formatting to find:
//! - Panics on malformed format specifiers (width, precision, type)
//! - Stack overflow from pathological format strings
//! - Numeric conversion edge cases (%d, %o, %x on non-numeric input)
//! - Escape sequence handling (\n, \x, \0, etc.)
//!
//! Run with: cargo +nightly fuzz run printf_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into format string (first line) and arguments (remaining lines)
        let (format, args_str) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "" as &str),
        };

        // Skip empty format strings
        if format.is_empty() {
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

            // Build argument list from remaining lines
            let args: Vec<&str> = if args_str.is_empty() {
                vec![]
            } else {
                args_str.lines().collect()
            };
            let args_escaped: Vec<String> = args
                .iter()
                .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
                .collect();
            let args_joined = args_escaped.join(" ");

            // Test 1: printf with fuzzed format string and arguments
            let script = format!(
                "printf '{}' {}",
                format.replace('\'', "'\\''"),
                args_joined,
            );
            bashkit::testing::fuzz_exec(&mut bash, &script, "printf_fuzz", &[]).await;

            // Test 2: printf with -v flag (assign to variable)
            let script2 = format!(
                "printf -v result '{}' {}",
                format.replace('\'', "'\\''"),
                args_joined,
            );
            bashkit::testing::fuzz_exec(&mut bash, &script2, "printf_fuzz", &[]).await;

            // Test 3: printf with numeric format specifiers and fuzzed args
            let script3 = format!(
                "printf '%d %o %x' {}",
                args_joined,
            );
            bashkit::testing::fuzz_exec(&mut bash, &script3, "printf_fuzz", &[]).await;
        });
    }
});
