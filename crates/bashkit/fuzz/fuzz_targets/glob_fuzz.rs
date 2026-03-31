//! Fuzz target for glob/pathname expansion
//!
//! Tests glob pattern matching and expansion to find:
//! - Exponential blowup with pathological patterns (TM-DOS-031)
//! - Stack overflow with deeply nested extglob operators
//! - Panics on malformed bracket expressions
//! - Resource exhaustion from recursive globstar patterns
//!
//! Run with: cargo +nightly fuzz run glob_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM on huge patterns
        if input.len() > 512 {
            return;
        }

        // Reject deeply nested extglob operators that could blow up
        let nesting: i32 = input
            .bytes()
            .map(|b| match b {
                b'(' => 1,
                b')' => -1,
                _ => 0,
            })
            .scan(0i32, |acc, d| {
                *acc += d;
                Some(*acc)
            })
            .max()
            .unwrap_or(0);
        if nesting > 15 {
            return;
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            // Build a small VFS with some files to match against
            let mut bash = bashkit::Bash::builder()
                .limits(
                    bashkit::ExecutionLimits::new()
                        .max_commands(50)
                        .timeout(std::time::Duration::from_millis(200)),
                )
                .mount_text("/tmp/a.txt", "")
                .mount_text("/tmp/b.sh", "")
                .mount_text("/tmp/c.md", "")
                .mount_text("/tmp/sub/d.txt", "")
                .mount_text("/tmp/sub/e.rs", "")
                .mount_text("/tmp/.hidden", "")
                .build();

            // Test 1: glob expansion via ls (triggers expand_glob)
            let script = format!("ls /tmp/{} 2>/dev/null; true", input);
            let _ = bash.exec(&script).await;

            // Test 2: pattern matching via case statement
            let script2 = format!(
                "case \"test.txt\" in {}) echo match;; *) echo no;; esac",
                input
            );
            let _ = bash.exec(&script2).await;

            // Test 3: [[ conditional pattern matching
            let script3 = format!("if [[ \"hello.world\" == {} ]]; then echo y; fi", input);
            let _ = bash.exec(&script3).await;
        });
    }
});
