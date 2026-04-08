//! Fuzz target for the yaml builtin
//!
//! Tests the hand-written YAML parser to find:
//! - Panics on malformed YAML documents
//! - Stack overflow from deeply nested structures
//! - Edge cases with anchors, special characters, multiline strings
//! - Memory exhaustion from pathological input
//!
//! Run with: cargo +nightly fuzz run yaml_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into YAML content and query path
        let (yaml_doc, query) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "." as &str),
        };

        // Skip empty documents
        if yaml_doc.trim().is_empty() {
            return;
        }

        // Reject deeply nested structures
        let depth = yaml_doc
            .bytes()
            .filter(|&b| b == b'{' || b == b'[')
            .count();
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

            // Test 1: parse YAML and query with get
            let script = format!(
                "echo '{}' | yaml get '{}' 2>/dev/null; true",
                yaml_doc.replace('\'', "'\\''"),
                query.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script).await;

            // Test 2: parse YAML and list keys
            let script2 = format!(
                "echo '{}' | yaml keys 2>/dev/null; true",
                yaml_doc.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script2).await;

            // Test 3: parse YAML and get type
            let script3 = format!(
                "echo '{}' | yaml type 2>/dev/null; true",
                yaml_doc.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script3).await;
        });
    }
});
