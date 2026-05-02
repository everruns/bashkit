//! Fuzz target for the tomlq builtin
//!
//! Tests the hand-written TOML parser to find:
//! - Panics on malformed TOML documents
//! - Edge cases with nested tables, inline tables, multiline strings
//! - Incorrect datetime parsing
//! - Memory exhaustion from pathological input
//!
//! Run with: cargo +nightly fuzz run tomlq_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into TOML content and query path
        let (toml_doc, query) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "." as &str),
        };

        // Skip empty documents
        if toml_doc.trim().is_empty() {
            return;
        }

        // Reject deeply nested structures
        let depth = toml_doc
            .bytes()
            .filter(|&b| b == b'[' || b == b'{')
            .count();
        if depth > 20 {
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

            // Test 1: parse TOML and query by path
            let script = format!(
                "echo '{}' | tomlq '{}'",
                toml_doc.replace('\'', "'\\''"),
                query.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script, "tomlq_fuzz", &[]).await;

            // Test 2: parse TOML with dot-path query
            let script2 = format!(
                "echo '{}' | tomlq",
                toml_doc.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script2, "tomlq_fuzz", &[]).await;
        });
    }
});
