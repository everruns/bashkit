//! Fuzz target for the embedded Monty python3 builtin (bashkit-monty fork)
//!
//! Feeds arbitrary programs and command lines to `python3` to find:
//! - Host panics or aborts in the forked Monty interpreter, parser or bridges
//! - Memory-limit bypasses: the metered allocator charges each VM; a program
//!   that outgrows `max_memory` (32 MiB here) trips libFuzzer's RSS limit
//! - Time-limit bypasses: each exec must return near its wall-clock budget
//! - Sandbox escapes: host environment (FUZZ_HOST_CANARY)
//! - Internal Debug shapes leaking into stderr (TM-INF-022)
//!
//! Decision: install `bashkit::MeteredAllocator` as the global allocator, the
//! same way every shipped binding does. Without it `python3` fails closed and
//! the target would only exercise the refusal path.
//!
//! Run with: cargo +nightly fuzz run python_fuzz --features python -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: bashkit::MeteredAllocator = bashkit::MeteredAllocator;

/// Wall-clock budget per exec. Python gets 500 ms; the slack covers runtime
/// setup on a loaded fuzz runner. A hang past this is a time-limit bypass.
const EXEC_BUDGET: Duration = Duration::from_secs(5);

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    // Keep tracebacks (which quote the source) under the stderr cap.
    if input.len() > 512 || bashkit::testing::input_echo_would_trip(input) {
        return;
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        bashkit::testing::fuzz_init();
        let mut bash = bashkit::Bash::builder()
            .python_with_limits(
                bashkit::PythonLimits::default()
                    .max_duration(Duration::from_millis(500))
                    .max_memory(32 * 1024 * 1024)
                    .max_recursion(200),
            )
            .limits(
                bashkit::ExecutionLimits::new()
                    .max_commands(50)
                    .max_stdout_bytes(4096)
                    .max_stderr_bytes(1024)
                    .timeout(Duration::from_secs(2)),
            )
            .build();

        let _ = bash
            .fs()
            .write_file(std::path::Path::new("/fuzz.py"), input.as_bytes())
            .await;

        let quoted = input.replace('\'', "'\\''");
        let scripts = [
            // Program from a VFS file.
            "python3 /fuzz.py".to_string(),
            // Program from stdin.
            "python3 < /fuzz.py".to_string(),
            // Program inline via -c.
            format!("python3 -c '{quoted}'"),
            // Input as a command line (CLI option parsing).
            format!("python3 '{quoted}' -c 'print(1)'"),
        ];
        for script in &scripts {
            let start = Instant::now();
            bashkit::testing::fuzz_exec(&mut bash, script, "python_fuzz", &[]).await;
            let elapsed = start.elapsed();
            assert!(
                elapsed < EXEC_BUDGET,
                "[python_fuzz] exec took {elapsed:?} (budget {EXEC_BUDGET:?}): {script}"
            );
        }
    });
});
