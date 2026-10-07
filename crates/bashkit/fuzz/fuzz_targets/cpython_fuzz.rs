//! Fuzz target for the CPython (wasm) python3 builtin
//!
//! Feeds arbitrary programs and command lines to `python3` to find:
//! - Host panics or aborts from guest traps, WASI calls or exit handling
//! - Sandbox escapes: host files, host environment (FUZZ_HOST_CANARY)
//! - Limit bypasses: wall clock, memory, output
//! - Internal Debug shapes leaking into stderr (TM-INF-022)
//!
//! Run with: cargo +nightly fuzz run cpython_fuzz --features cpython -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    // Keep tracebacks (which quote the source) under the stderr cap.
    if input.len() > 256 || bashkit::testing::input_echo_would_trip(input) {
        return;
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        bashkit::testing::fuzz_init();
        let mut bash = bashkit::Bash::builder()
            .cpython_with_limits(
                bashkit::CPythonLimits::default()
                    .max_duration(std::time::Duration::from_millis(500))
                    .max_memory(128 * 1024 * 1024)
                    .max_output(4096),
            )
            .limits(
                bashkit::ExecutionLimits::new()
                    .max_commands(50)
                    .max_stdout_bytes(4096)
                    .max_stderr_bytes(1024)
                    .timeout(std::time::Duration::from_secs(2)),
            )
            .build();

        // Program from a VFS file.
        let _ = bash
            .fs()
            .write_file(std::path::Path::new("/fuzz.py"), input.as_bytes())
            .await;
        bashkit::testing::fuzz_exec(&mut bash, "python3 /fuzz.py", "cpython_fuzz", &[]).await;

        // Program from stdin.
        bashkit::testing::fuzz_exec(&mut bash, "python3 < /fuzz.py", "cpython_fuzz", &[]).await;

        // Input as a command line (CLI option parsing).
        let quoted = input.replace('\'', "'\\''");
        let script = format!("python3 '{quoted}' -c 'print(1)'");
        bashkit::testing::fuzz_exec(&mut bash, &script, "cpython_fuzz", &[]).await;
    });
});
