//! Fuzz target for the awk builtin
//!
//! Tests AWK program parsing and execution to find:
//! - Panics in the AWK expression parser
//! - Stack overflow from deeply nested expressions or function calls
//! - ReDoS from pathological regex patterns
//! - Memory exhaustion from unbounded field/record processing
//!
//! Run with: cargo +nightly fuzz run awk_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into AWK program (first line) and input data (rest)
        let (program, input_data) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "a b c\n1 2 3\n" as &str),
        };

        // Skip empty programs
        if program.trim().is_empty() {
            return;
        }

        // Reject deeply nested expressions
        let depth: i32 = program
            .bytes()
            .map(|b| match b {
                b'(' | b'{' => 1,
                b')' | b'}' => -1,
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

            // Test 1: pipe data through awk program
            let script = format!(
                "echo '{}' | awk '{}'",
                input_data.replace('\'', "'\\''"),
                program.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script, "awk_fuzz", &[]).await;

            // Test 2: awk with -F (field separator) flag
            let script2 = format!(
                "echo '{}' | awk -F: '{}'",
                input_data.replace('\'', "'\\''"),
                program.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script2, "awk_fuzz", &[]).await;
        });
    }
});
