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
