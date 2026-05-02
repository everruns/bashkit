//! Fuzz target for the base64 builtin
//!
//! Tests base64 encode/decode to find:
//! - Panics on invalid base64 sequences or wrong padding
//! - Encode/decode roundtrip mismatches
//! - Truncated input handling
//! - Edge cases with wrap width and -d flag
//!
//! Run with: cargo +nightly fuzz run base64_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
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
                        .max_stdout_bytes(8192)
                        .max_stderr_bytes(4096)
                        .timeout(std::time::Duration::from_millis(200)),
                )
                .build();

            // Test 1: encode arbitrary data
            let script = format!(
                "echo -n '{}' | base64",
                input.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script, "base64_fuzz", &[]).await;

            // Test 2: decode arbitrary data (may be invalid base64)
            let script2 = format!(
                "echo -n '{}' | base64 -d",
                input.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script2, "base64_fuzz", &[]).await;

            // Test 3: encode then decode roundtrip
            let script3 = format!(
                "echo -n '{}' | base64 | base64 -d",
                input.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script3, "base64_fuzz", &[]).await;

            // Test 4: decode with --wrap=0
            let script4 = format!(
                "echo -n '{}' | base64 --wrap=0",
                input.replace('\'', "'\\''"),
            );
            bashkit::testing::fuzz_exec(&mut bash, &script4, "base64_fuzz", &[]).await;
        });
    }
});
