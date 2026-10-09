// Integration and security tests for real uutils programs running as wasm
// guests (`wasm-coreutils` feature).
//
// Threat IDs refer to knowledge/security/threat-model.md (TM-WCU-*).
// Covers: gap-filling registration, multicall, VFS read/write and cwd,
// stdin/pipelines, exit codes, host isolation, limits (time, memory,
// output), per-call isolation, and error-message hygiene (TM-INF-022).

#![cfg(feature = "wasm-coreutils")]

use bashkit::testing::assert_no_leak;
use bashkit::{Bash, ExecutionLimits, WasmCoreutil, WasmCoreutilsLimits};
use std::time::{Duration, Instant};

fn bash() -> Bash {
    Bash::builder().wasm_coreutils().build()
}

async fn run(script: &str) -> bashkit::ExecResult {
    let mut bash = bash();
    let r = bash.exec(script).await.expect("exec");
    assert_no_leak(&r, script, &[]);
    r
}

// --- registration ---------------------------------------------------------

#[tokio::test]
async fn fills_missing_utilities() {
    let r = run("pathchk -p 'ok_name' && echo valid").await;
    assert_eq!(r.stdout, "valid\n");
    let r =
        run("printf 'x\\n--\\ny\\n' > /tmp/in; cd /tmp && csplit -s in '/--/' && cat xx01").await;
    assert_eq!(r.stdout, "--\ny\n");
    let r = run("coreutils factor 12 97").await;
    assert_eq!(r.stdout, "12: 2 2 3\n97: 97\n");
}

#[tokio::test]
async fn without_feature_call_missing_stays_missing() {
    let mut bash = Bash::builder().build();
    let r = bash.exec("pathchk x").await.unwrap();
    assert_eq!(r.exit_code, 127);
}

#[tokio::test]
async fn native_builtins_are_kept_by_default() {
    // `cat` stays native; the multicall reaches the wasm one.
    let r = run("printf 'a\\nb\\n' > /tmp/f; cat /tmp/f; coreutils cat -n /tmp/f").await;
    assert_eq!(r.stdout, "a\nb\n     1\ta\n     2\tb\n");
}

#[tokio::test]
async fn replace_native_routes_to_wasm() {
    let mut bash = Bash::builder()
        .wasm_coreutils_replace_native(WasmCoreutilsLimits::default())
        .build();
    // GNU sort -h understands human sizes.
    let r = bash
        .exec("printf '1K\\n3M\\n2\\n' | sort -h")
        .await
        .unwrap();
    assert_eq!(r.stdout, "2\n1K\n3M\n");
}

#[tokio::test]
async fn multicall_list_and_errors() {
    let r = run("coreutils --list").await;
    assert!(r.stdout.lines().any(|l| l == "sha512sum"));
    assert_eq!(r.stdout.lines().count(), WasmCoreutil::utilities().len());
    let r = run("coreutils bash -c true").await;
    assert_eq!(r.exit_code, 127);
    assert_eq!(r.stderr, "coreutils: bash: unknown utility\n");
    let r = run("coreutils").await;
    assert_eq!(r.exit_code, 1);
}

// --- files, cwd, stdin ------------------------------------------------------

#[tokio::test]
async fn reads_vfs_relative_to_cwd() {
    let r = run("mkdir -p /w && cd /w && printf 'abc' > data && coreutils sha512sum data").await;
    assert!(r.stdout.starts_with("ddaf35a193617aba"), "{}", r.stdout);
    assert!(r.stdout.ends_with("  data\n"), "{}", r.stdout);
}

#[tokio::test]
async fn writes_land_in_vfs() {
    let r = run(
        "cd /tmp && printf '1\\n2\\n3\\n4\\n' > in && coreutils split -l 2 in part_ && cat part_aa part_ab",
    )
    .await;
    assert_eq!(r.stdout, "1\n2\n3\n4\n");
    let r = run("cd /tmp && echo hi > x && coreutils unlink x && test ! -e x && echo gone").await;
    assert_eq!(r.stdout, "gone\n");
}

#[tokio::test]
async fn stdin_and_pipelines() {
    let r =
        run("printf 'b\\na\\nc\\n' | coreutils sort -r | coreutils head -n 2 | tr '\\n' ,").await;
    assert_eq!(r.stdout, "c,b,");
}

#[tokio::test]
async fn exit_codes_and_errors_propagate() {
    let r = run("coreutils cat /nope").await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stderr, "cat: /nope: No such file or directory\n");
    let r = run("coreutils false; echo $?").await;
    assert_eq!(r.stdout, "1\n");
}

#[tokio::test]
async fn only_exported_variables_reach_the_guest() {
    let r = run("LOCAL=1; export SHOWN=2; coreutils printenv SHOWN LOCAL").await;
    assert_eq!(r.stdout, "2\n");
}

// --- TM-WCU-001: host isolation -------------------------------------------

#[tokio::test]
async fn host_files_are_not_reachable() {
    // A file that certainly exists on the host, by absolute path.
    let host = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let r = run(&format!("coreutils cat '{host}'")).await;
    assert_ne!(r.exit_code, 0);
    assert!(r.stdout.is_empty());
}

#[tokio::test]
async fn parent_traversal_stays_in_vfs() {
    let r = run("cd /tmp && coreutils ls ../../../../").await;
    let root = run("coreutils ls /").await;
    assert_eq!(r.stdout, root.stdout);
}

// --- TM-WCU-002: limits -----------------------------------------------------

#[tokio::test]
async fn busy_guest_hits_timeout() {
    let mut bash = Bash::builder()
        .wasm_coreutils_with_limits(
            WasmCoreutilsLimits::default().max_duration(Duration::from_millis(300)),
        )
        .build();
    let start = Instant::now();
    // factor of a large semiprime keeps the guest busy for a long time.
    let r = bash
        .exec("coreutils factor 340282366920938463463374607431768211297")
        .await
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(5));
    if r.exit_code == 124 {
        assert_eq!(r.stderr, "factor: execution timed out after 0.3s\n");
    }
}

#[tokio::test]
async fn output_is_capped() {
    let mut bash = Bash::builder()
        .wasm_coreutils_with_limits(WasmCoreutilsLimits::default().max_output(1000))
        .build();
    let r = bash.exec("coreutils seq 1 100000").await.unwrap();
    assert!(r.stdout.len() <= 1000);
    assert!(r.stderr.contains("output truncated at 1000 bytes"));
}

#[tokio::test]
async fn memory_is_capped() {
    let mut bash = Bash::builder()
        .wasm_coreutils_with_limits(WasmCoreutilsLimits::default().max_memory(4 << 20))
        .build();
    // sort holds its whole input; 8 MiB of lines cannot fit in 4 MiB.
    let r = bash
        .exec("coreutils seq 1 1000000 | coreutils sort -r | coreutils head -1")
        .await
        .unwrap();
    assert_ne!(r.stdout, "999999\n");
}

#[tokio::test]
async fn request_deadline_wins() {
    let mut bash = Bash::builder()
        .wasm_coreutils()
        .limits(ExecutionLimits::new().timeout(Duration::from_millis(300)))
        .build();
    let start = Instant::now();
    let _ = bash
        .exec("coreutils factor 340282366920938463463374607431768211297")
        .await;
    assert!(start.elapsed() < Duration::from_secs(5));
}

// --- TM-WCU-003: per-call isolation -----------------------------------------

#[tokio::test]
async fn tenants_do_not_share_files() {
    let mut a = bash();
    let mut b = bash();
    a.exec("echo secret > /tmp/s").await.unwrap();
    let r = b.exec("coreutils cat /tmp/s").await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(!r.stdout.contains("secret"));
}

#[tokio::test]
async fn concurrent_calls_are_independent() {
    let mut handles = Vec::new();
    for i in 0..16 {
        handles.push(tokio::spawn(async move {
            let mut bash = bash();
            let r = bash
                .exec(&format!("coreutils seq {i} | coreutils tail -1"))
                .await
                .unwrap();
            (i, r.stdout)
        }));
    }
    for h in handles {
        let (i, out) = h.await.unwrap();
        let want = if i == 0 {
            String::new()
        } else {
            format!("{i}\n")
        };
        assert_eq!(out, want);
    }
}

// --- adversarial: limits are hard, not best-effort --------------------------
//
// Each test drives a guest that would otherwise run forever or grow without
// bound, and asserts the limit actually stops it.

fn with(limits: WasmCoreutilsLimits) -> Bash {
    Bash::builder().wasm_coreutils_with_limits(limits).build()
}

// TM-WCU-002: an endless guest always ends at max_duration with 124.
#[tokio::test]
async fn endless_guest_is_killed_at_max_duration() {
    let mut bash = with(WasmCoreutilsLimits::default().max_duration(Duration::from_millis(300)));
    let start = Instant::now();
    let r = bash.exec("coreutils seq inf > /dev/null").await.unwrap();
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(r.exit_code, 124);
    assert_eq!(r.stderr, "seq: execution timed out after 0.3s\n");
}

// TM-WCU-002: cancelling the request stops a running guest promptly.
#[tokio::test]
async fn cancellation_stops_a_running_guest() {
    let mut bash = with(WasmCoreutilsLimits::default().max_duration(Duration::from_secs(30)));
    let cancel = bash.cancellation_token();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let start = Instant::now();
    let r = bash.exec("coreutils seq inf > /dev/null").await;
    assert!(start.elapsed() < Duration::from_secs(5), "{r:?}"); // debug-ok: test diagnostics
    assert!(r.is_err() || r.unwrap().exit_code != 0);
}

// TM-WCU-002: guest instructions count against the request's work budget
// while the guest runs, not only after it exits.
#[tokio::test]
async fn work_budget_stops_a_running_guest() {
    let mut bash = Bash::builder()
        .wasm_coreutils_with_limits(
            WasmCoreutilsLimits::default().max_duration(Duration::from_secs(30)),
        )
        .limits(ExecutionLimits::new().max_work_units(20_000))
        .build();
    let start = Instant::now();
    let r = bash.exec("coreutils seq inf > /dev/null").await;
    assert!(start.elapsed() < Duration::from_secs(5));
    let err = r
        .expect_err("work budget must fail the request")
        .to_string();
    assert!(err.contains("work"), "{err}");
}

// TM-WCU-002: stdout and stderr share one cap; a flood on either is cut.
#[tokio::test]
async fn stderr_flood_is_capped() {
    let mut bash = with(WasmCoreutilsLimits::default().max_output(2000));
    let args: String = (0..2000).map(|i| format!(" /missing/{i}")).collect();
    let r = bash.exec(&format!("coreutils cat{args}")).await.unwrap();
    assert!(r.stderr.len() < 2100, "{}", r.stderr.len());
    assert!(r.stderr.ends_with("cat: output truncated at 2000 bytes\n"));
}

#[tokio::test]
async fn endless_output_is_capped_and_stopped() {
    let mut bash = with(
        WasmCoreutilsLimits::default()
            .max_output(4096)
            .max_duration(Duration::from_millis(500)),
    );
    let r = bash.exec("coreutils seq inf").await.unwrap();
    assert!(r.stdout.len() <= 4096);
    assert_eq!(r.exit_code, 124);
}

// TM-WCU-002: guest memory never exceeds max_memory, and a request above
// the 256 MiB slot size is clamped rather than honored.
#[tokio::test]
async fn memory_cap_fails_inside_the_guest() {
    let mut bash = with(WasmCoreutilsLimits::default().max_memory(8 << 20));
    // shuf materializes the whole range: 100M entries cannot fit in 8 MiB.
    let r = bash.exec("coreutils shuf -i 1-100000000").await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(r.stdout.is_empty());
    assert!(r.stderr.contains("memory"), "{}", r.stderr);
}

#[tokio::test]
async fn oversized_memory_request_is_clamped() {
    let mut bash = with(WasmCoreutilsLimits::default().max_memory(usize::MAX));
    // 100M u64 entries is ~800 MiB, more than one 256 MiB slot.
    let r = bash
        .exec("coreutils shuf -i 1-100000000 | wc -l")
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "0");
}

// TM-WCU-002 / TM-DOS: guest writes obey the VFS limits, with the errno a
// real kernel would report.
#[tokio::test]
async fn guest_writes_obey_filesystem_limits() {
    let mut bash = Bash::builder()
        .wasm_coreutils()
        .filesystem_limits(
            bashkit::FsLimits::new()
                .max_file_size(1 << 20)
                .max_total_bytes(2 << 20)
                .max_file_count(64),
        )
        .build();
    let r = bash
        .exec("cd /tmp && coreutils truncate -s 10G big; echo $?")
        .await
        .unwrap();
    assert_eq!(r.stdout, "1\n");
    assert!(r.stderr.contains("File too large"), "{}", r.stderr);

    let r = bash
        .exec("coreutils seq 1 400000 | coreutils tee /tmp/a > /dev/null; echo $?")
        .await
        .unwrap();
    assert_eq!(r.stdout, "1\n", "{}", r.stderr);
    assert!(r.stderr.contains("File too large"), "{}", r.stderr);

    let r = bash
        .exec("cd /tmp && coreutils seq 1 200 > n && coreutils split -l 1 n p_; echo $?")
        .await
        .unwrap();
    assert_eq!(r.stdout, "1\n");
    assert!(r.stderr.contains("No space left on device"), "{}", r.stderr);
    let usage = bash.fs().usage();
    assert!(usage.file_count <= 64);
    assert!(usage.total_bytes <= 2 << 20);
}

// --- adversarial: sandbox boundary -----------------------------------------

// TM-WCU-001: symlinks the guest creates resolve inside the VFS, loops fail
// cleanly, and absolute targets never reach the host.
#[tokio::test]
async fn guest_symlinks_stay_in_the_vfs() {
    let host = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let r = run(&format!(
        "cd /tmp && coreutils ln -s '{host}' h && coreutils cat h; echo $?; \
         coreutils ln -s ../../../../../../../host-only p && coreutils cat p; echo $?; \
         coreutils readlink p"
    ))
    .await;
    // `..` past the VFS root clamps at `/`: the link names /host-only in
    // the VFS, which does not exist.
    assert_eq!(
        r.stdout, "1\n1\n../../../../../../../host-only\n",
        "{}",
        r.stderr
    );

    let r = run("cd /tmp && coreutils ln -s a b && coreutils ln -s b a && coreutils cat a").await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stderr, "cat: a: Symbolic link loop\n");
}

#[tokio::test]
async fn guest_cannot_escape_root_by_cwd_or_dotdot() {
    let r = run("cd / && coreutils realpath ../../.. && coreutils pwd -P").await;
    assert_eq!(r.stdout, "/\n/\n");
    let r =
        run("mkdir -p /a/b && cd /a/b && coreutils ls ../../../../../../ && echo -- && ls /").await;
    let (up, root) = r.stdout.split_once("--\n").unwrap();
    assert_eq!(up, root);
}

// TM-WCU-003 / TM-INF: the guest sees exactly the exported shell variables;
// host process environment and internal variables never reach it.
#[tokio::test]
async fn guest_environment_is_only_exported_shell_variables() {
    let r = run("export A=1; coreutils printenv | coreutils sort").await;
    for line in r.stdout.lines() {
        let name = line.split('=').next().unwrap();
        assert!(!name.starts_with("__BASHKIT"), "{line}");
        assert!(!name.starts_with("CARGO"), "host env leaked: {line}");
        assert!(!name.starts_with("RUST"), "host env leaked: {line}");
    }
    assert!(r.stdout.lines().any(|l| l == "A=1"));
    assert!(
        r.stdout.lines().any(|l| l == "PWD=/home/user"),
        "{}",
        r.stdout
    );
}

// TM-WCU-004 / TM-INF-022: no utility, on help or bad input, prints Rust
// panic reports, toolchain paths or Debug-formatted errors.
#[tokio::test]
async fn no_utility_leaks_internal_shapes() {
    let mut bash = bash();
    bash.exec("printf 'b\\na\\n' > /tmp/f; mkdir -p /tmp/d")
        .await
        .unwrap();
    for util in WasmCoreutil::utilities() {
        for args in ["--help", "--bogus-flag", "/tmp/f /tmp/d", "/nope"] {
            let script = format!("coreutils {util} {args} < /tmp/f");
            let r = bash.exec(&script).await.unwrap();
            for out in [&r.stdout, &r.stderr] {
                for bad in ["panicked at", "/rustc/", "Os {", "kind: ", "RUST_BACKTRACE"] {
                    assert!(!out.contains(bad), "{script}: {bad} in {out}");
                }
            }
        }
    }
}

// --- audit follow-ups -------------------------------------------------------

// TM-WCU-005: `builtin_filter` also governs guest utilities, by name and
// through the multicall.
#[tokio::test]
async fn builtin_filter_blocks_guest_utilities() {
    let mut bash = Bash::builder()
        .builtin_filter(|name| name != "rm" && name != "pathchk")
        .wasm_coreutils()
        .build();
    let r = bash
        .exec("echo x > /tmp/f; coreutils rm /tmp/f; echo $?; cat /tmp/f; pathchk a; echo $?")
        .await
        .unwrap();
    assert_eq!(r.stdout, "127\nx\n127\n");
    assert!(r.stderr.contains("coreutils: rm: unknown utility"));
    let r = bash.exec("coreutils --list").await.unwrap();
    assert!(!r.stdout.lines().any(|l| l == "rm" || l == "pathchk"));
    assert!(r.stdout.lines().any(|l| l == "sort"));
}

// A builtin the embedder registers keeps its name.
#[tokio::test]
async fn embedder_builtin_wins_over_guest() {
    struct Mine;
    #[async_trait::async_trait]
    impl bashkit::Builtin for Mine {
        async fn execute(
            &self,
            _: bashkit::BuiltinContext<'_>,
        ) -> bashkit::Result<bashkit::ExecResult> {
            Ok(bashkit::ExecResult::ok("mine\n".to_string()))
        }
    }
    let mut bash = Bash::builder()
        .wasm_coreutils()
        .builtin("pathchk", Box::new(Mine))
        .build();
    let r = bash.exec("pathchk x").await.unwrap();
    assert_eq!(r.stdout, "mine\n");
}

// TM-INF-018: guest clocks follow the virtual clock, like `date`.
#[tokio::test]
async fn guest_clock_follows_fixed_epoch() {
    let mut bash = Bash::builder()
        .wasm_coreutils()
        .fixed_epoch(1_000_000_000)
        .build();
    let r = bash
        .exec("date +%s; coreutils date +%s; coreutils touch /tmp/t; coreutils date -r /tmp/t +%s")
        .await
        .unwrap();
    assert_eq!(
        r.stdout, "1000000000\n1000000000\n1000000000\n",
        "{}",
        r.stderr
    );
}

// TM-WCU-002: /dev/null is a sink, not a file buffered in host memory, so
// writes to it are not held to the file size limit.
#[tokio::test]
async fn dev_null_is_not_buffered() {
    let mut bash = Bash::builder()
        .wasm_coreutils()
        .filesystem_limits(bashkit::FsLimits::new().max_file_size(64 << 10))
        .build();
    let r = bash
        .exec("coreutils seq 1 200000 | coreutils tee /dev/null | coreutils tail -1")
        .await
        .unwrap();
    assert_eq!(r.stdout, "200000\n", "{}", r.stderr);
}

// TM-WCU-002: an absurd path length is refused before it is copied.
#[tokio::test]
async fn huge_path_is_refused() {
    let long = "a/".repeat(5000);
    // Not `run`: the guest echoes the (user-supplied) path in its message.
    let mut bash = Bash::builder().wasm_coreutils().build();
    let r = bash
        .exec(&format!("coreutils touch '/tmp/{long}x'; echo $?"))
        .await
        .unwrap();
    assert_eq!(r.stdout, "1\n");
    assert!(r.stderr.contains("too long"), "{}", r.stderr);
}

// Big directories list correctly through the cached listing.
#[tokio::test]
async fn large_directory_listing_is_complete() {
    let r = run(
        "mkdir /big && cd /big && coreutils seq 1 3000 | while read i; do : > f$i; done; \
                 coreutils ls /big | coreutils wc -l; coreutils ls -a / | coreutils head -2",
    )
    .await;
    assert_eq!(r.stdout, "3000\n.\n..\n");
}

// Exit status keeps only the low 8 bits, like a real process.
#[tokio::test]
async fn exit_status_is_eight_bits() {
    let r = run("coreutils expr 1 + ; echo $?").await;
    assert_eq!(r.stdout, "2\n");
}
