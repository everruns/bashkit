//! Fuzz target for the archive builtins (tar, gzip, gunzip)
//!
//! Tests tar/gzip decompression with binary format parsing to find:
//! - Panics on malformed headers or truncated archives
//! - Zip bomb detection bypass via decompression ratio checks
//! - Memory exhaustion from pathological compressed data
//! - Edge cases in gzip header parsing and tar entry extraction
//!
//! Run with: cargo +nightly fuzz run archive_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Limit input size to prevent OOM (binary data, no UTF-8 requirement)
    if data.len() > 2048 {
        return;
    }

    // Need at least some data to be meaningful
    if data.is_empty() {
        return;
    }

    // Convert to a hex-escaped string for safe shell embedding
    let hex: String = data.iter().map(|b| format!("\\x{b:02x}")).collect();
    if hex.len() > 16384 {
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
                    .max_stdout_bytes(8192)
                    .max_stderr_bytes(4096)
                    .timeout(std::time::Duration::from_millis(200)),
            )
            .build();

        // Test 1: attempt gunzip on arbitrary binary data
        let script = format!(
            "printf '{}' | gunzip 2>/dev/null; true",
            hex,
        );
        let _ = bash.exec(&script).await;

        // Test 2: attempt tar listing on arbitrary binary data
        let script2 = format!(
            "printf '{}' | tar -tf - 2>/dev/null; true",
            hex,
        );
        let _ = bash.exec(&script2).await;

        // Test 3: attempt gzip then gunzip roundtrip on valid UTF-8
        if let Ok(text) = std::str::from_utf8(data) {
            if text.len() <= 512 {
                let script3 = format!(
                    "echo '{}' | gzip | gunzip 2>/dev/null; true",
                    text.replace('\'', "'\\''"),
                );
                let _ = bash.exec(&script3).await;
            }
        }
    });
});
