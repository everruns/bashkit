//! Fuzz target for the template builtin
//!
//! Tests the custom Mustache/Handlebars template engine to find:
//! - Panics on mismatched delimiters or deeply nested sections
//! - Stack overflow from recursive block expansion
//! - Edge cases in variable substitution and control flow blocks
//! - Memory exhaustion from pathological template patterns
//!
//! Run with: cargo +nightly fuzz run template_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM
        if input.len() > 1024 {
            return;
        }

        // Split input into template (first line) and JSON data (rest)
        let (template, json_data) = match input.find('\n') {
            Some(pos) => (&input[..pos], &input[pos + 1..]),
            None => (input, "{\"name\":\"world\",\"items\":[1,2,3]}" as &str),
        };

        // Skip empty templates
        if template.trim().is_empty() {
            return;
        }

        // Reject deeply nested template blocks
        let depth = template.matches("{{#").count();
        if depth > 10 {
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

            // Test 1: render template with JSON data via stdin
            let script = format!(
                "echo '{}' | template -d /dev/stdin '{}' 2>/dev/null; true",
                json_data.replace('\'', "'\\''"),
                template.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script).await;

            // Test 2: render template with --strict mode
            let script2 = format!(
                "echo '{}' | template --strict -d /dev/stdin '{}' 2>/dev/null; true",
                json_data.replace('\'', "'\\''"),
                template.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script2).await;

            // Test 3: render template with -e (HTML escape) flag
            let script3 = format!(
                "echo '{}' | template -e -d /dev/stdin '{}' 2>/dev/null; true",
                json_data.replace('\'', "'\\''"),
                template.replace('\'', "'\\''"),
            );
            let _ = bash.exec(&script3).await;
        });
    }
});
