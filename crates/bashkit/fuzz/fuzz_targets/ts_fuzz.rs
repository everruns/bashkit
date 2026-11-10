//! Fuzz target for the embedded ZapCode `ts` builtin (vendored zapcode_vendor)
//!
//! Feeds arbitrary programs and command lines to `ts` to find:
//! - Host panics or aborts in the vendored ZapCode parser, compiler or VM
//! - Memory-limit bypasses (TM-TS-026): the metered allocator charges each
//!   run; a program that outgrows `max_memory` (32 MiB here) trips libFuzzer's
//!   RSS limit or aborts on a failed allocation
//! - Time-limit bypasses: each exec must return near its wall-clock budget
//! - Sandbox escapes: host environment (FUZZ_HOST_CANARY)
//! - Internal Debug shapes leaking into stderr (TM-INF-022)
//!
//! Decision: install `bashkit::MeteredAllocator` as the global allocator, the
//! same way every shipped binding does. Without it `ts` fails closed and the
//! target would only exercise the refusal path.
//!
//! Run with: cargo +nightly fuzz run ts_fuzz --features typescript -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: bashkit::MeteredAllocator = bashkit::MeteredAllocator;

/// Wall-clock budget per exec. ts gets 500 ms; the slack covers setup on a
/// loaded fuzz runner. A hang past this is a time-limit bypass.
const EXEC_BUDGET: Duration = Duration::from_secs(5);

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    // Keep error messages (which may quote the source) under the stderr cap.
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
            .typescript_with_limits(
                bashkit::TypeScriptLimits::default()
                    .max_duration(Duration::from_millis(500))
                    .max_memory(32 * 1024 * 1024)
                    .max_allocations(200_000),
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
            .write_file(std::path::Path::new("/fuzz.ts"), input.as_bytes())
            .await;

        let quoted = input.replace('\'', "'\\''");
        let scripts = [
            // Program from a VFS file.
            "ts /fuzz.ts".to_string(),
            // Program from stdin.
            "ts < /fuzz.ts".to_string(),
            // Program inline via -c.
            format!("ts -c '{quoted}'"),
            // Input as a command line (CLI option parsing).
            format!("ts '{quoted}' -c 'console.log(1)'"),
        ];
        // Once per process: prove ts really runs (allocator installed), so the
        // target cannot silently fuzz only the fail-closed path.
        static CANARY: std::sync::Once = std::sync::Once::new();
        let mut canary_ok = true;
        if !CANARY.is_completed() {
            let r = bash.exec("ts -c 'console.log(6*7)'").await.unwrap();
            canary_ok = r.stdout.to_string() == "42\n";
            CANARY.call_once(|| {});
        }
        assert!(canary_ok, "[ts_fuzz] ts does not run; check the allocator");
        for script in &scripts {
            let start = Instant::now();
            bashkit::testing::fuzz_exec(&mut bash, script, "ts_fuzz", &[]).await;
            let elapsed = start.elapsed();
            assert!(
                elapsed < EXEC_BUDGET,
                "[ts_fuzz] exec took {elapsed:?} (budget {EXEC_BUDGET:?}): {script}"
            );
        }
    });
});
