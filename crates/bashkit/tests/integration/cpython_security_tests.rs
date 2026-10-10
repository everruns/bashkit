// Security tests for the CPython (WASI) python3 builtin (`cpython` feature).
//
// Threat IDs refer to knowledge/security/threat-model.md (TM-PY-CPY-*).
// Covers: host filesystem/network/process isolation, stdlib integrity,
// wall-clock, memory, output, fd and file-size limits, cancellation, crash
// containment, per-call and per-tenant isolation, error-message hygiene
// (TM-INF-022) and bounded property-based fuzzing.

#![cfg(feature = "cpython")]

use bashkit::testing::{assert_no_leak, input_echo_would_trip};
use bashkit::{Bash, CPythonLimits, ExecutionLimits, FsLimits, InMemoryFs};
use proptest::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

fn bash() -> Bash {
    Bash::builder().cpython().build()
}

fn bash_limits(limits: CPythonLimits) -> Bash {
    Bash::builder().cpython_with_limits(limits).build()
}

async fn run(script: &str) -> bashkit::ExecResult {
    let r = bash().exec(script).await.expect("exec");
    assert_no_leak(&r, script, &[]);
    r
}

/// Budget for a crash-containment case, far above what the work costs.
///
/// These tests assert that a guest trap is *contained*, not that it arrives
/// quickly, and reaching a trap can be genuinely slow: exhausting the guest's
/// 4 MiB wasm stack (`MAX_WASM_STACK`) through CPython's C recursion costs
/// ~16 s of interpreted guest execution natively, and several times that under
/// AddressSanitizer. Against the default 30 s limit the shell's own wall clock
/// won the race and the call ended 124 instead of reaching the trap, which is
/// what reddened nightly run 250 (ASAN) and any sufficiently contended
/// parallel run. A trap that stops working still fails these tests, through a
/// host crash or the stdout/stderr assertions, so the wider budget costs no
/// coverage.
const CONTAINMENT_BUDGET: Duration = Duration::from_secs(300);

/// [`run`], with budgets an instrumented or contended build cannot exhaust
/// before the guest traps.
async fn run_contained(script: &str) -> bashkit::ExecResult {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().timeout(CONTAINMENT_BUDGET))
        .cpython_with_limits(CPythonLimits::default().max_duration(CONTAINMENT_BUDGET))
        .build();
    let r = bash.exec(script).await.expect("exec");
    assert_no_leak(&r, script, &[]);
    r
}

fn stderr(r: &bashkit::ExecResult) -> String {
    r.stderr.to_string()
}

// --- TM-PY-CPY-001: host filesystem is unreachable -----------------------------

#[tokio::test]
async fn host_files_are_not_reachable() {
    // A file that certainly exists on the host, by absolute path.
    let host = std::env::current_exe().unwrap();
    let script = format!(
        "python3 -c 'import os, sys; p = sys.argv[1]; print(os.path.exists(p))' '{}'",
        host.display()
    );
    let r = run(&script).await;
    assert_eq!(r.stdout, "False\n");
}

#[tokio::test]
async fn parent_traversal_is_clamped_at_vfs_root() {
    let r = run("echo vfs > /inside.txt; mkdir -p /a/b; cd /a/b; \
         python3 -c 'print(open(\"../../../../../../inside.txt\").read().strip()); import os; print(os.path.realpath(\"../../../..\"))'")
    .await;
    assert_eq!(r.stdout, "vfs\n/\n");
}

#[tokio::test]
async fn host_system_paths_absent() {
    let r = run("python3 -c '
import os
for p in [\"/proc/self/environ\", \"/proc/self/mem\", \"/sys\"]:
    print(p, os.path.exists(p))
'")
    .await;
    assert_eq!(
        r.stdout,
        "/proc/self/environ False\n/proc/self/mem False\n/sys False\n"
    );
}

#[tokio::test]
async fn rootfs_files_come_from_the_vfs_not_the_host() {
    // The default rootfs ships a virtual /etc/passwd and /root; the guest must
    // see those VFS copies, never the host's files.
    let r = run("python3 -c '
import os
print(os.path.isdir(\"/root\"))
print(open(\"/etc/passwd\").read(), end=\"\")
'")
    .await;
    let vfs = bash().exec("cat /etc/passwd").await.expect("exec").stdout;
    assert_eq!(r.stdout, format!("True\n{vfs}"));
    if let Ok(host) = std::fs::read_to_string("/etc/passwd") {
        assert_ne!(vfs, host, "VFS /etc/passwd must not mirror the host");
    }
}

#[tokio::test]
async fn symlinks_resolve_inside_the_vfs_only() {
    // TM-ESC-002: links are followed, but only within the VFS. lstat still
    // reports the link itself, and a link aimed at a host path finds nothing.
    let host = std::env::current_exe().unwrap();
    let script = format!(
        "echo secret > /target; ln -s /target /link; ln -s '{}' /hostlink; \
         python3 -c 'import os; print(os.path.islink(\"/link\"), os.readlink(\"/link\"))
print(open(\"/link\").read().strip())
print(os.path.islink(\"/hostlink\"), os.path.exists(\"/hostlink\"))
os.unlink(\"/link\")
print(os.path.exists(\"/target\"), os.path.lexists(\"/link\"))'",
        host.display()
    );
    let r = run(&script).await;
    assert_eq!(r.stdout, "True /target\nsecret\nTrue False\nTrue False\n");
}

#[tokio::test]
async fn host_environment_not_visible() {
    // Only exported shell variables (plus PWD) reach the guest; the host
    // process environment (HOME, PATH, CARGO_*, ...) never does.
    let r = run("python3 -c 'import os; print(sorted(os.environ))'").await;
    assert_eq!(r.stdout, "['PWD']\n");
}

// --- TM-PY-CPY-002: stdlib integrity -----------------------------------------------

#[tokio::test]
async fn stdlib_zip_is_read_only() {
    let r = run("python3 -c '
import os
z = \"/usr/local/lib/python314.zip\"
for op in (lambda: open(z, \"w\"), lambda: open(z, \"ab\"), lambda: os.remove(z),
           lambda: os.rename(z, \"/z\"), lambda: os.truncate(z, 0),
           lambda: open(\"/usr/local/lib/x.py\", \"w\"), lambda: os.mkdir(\"/usr/local/lib/m\"),
           lambda: os.rmdir(\"/usr/local/lib\")):
    try:
        op(); print(\"ALLOWED\")
    except OSError:
        print(\"denied\")
print(len(open(z, \"rb\").read()) > 1000000)
'")
    .await;
    assert_eq!(r.stdout, "denied\n".repeat(8) + "True\n");
}

#[tokio::test]
async fn vfs_files_cannot_shadow_stdlib() {
    // A tenant can put files under /usr/local/lib in its own VFS, but the zip
    // path always serves the embedded stdlib, files next to it are not on
    // sys.path, and preloaded modules come from the snapshot.
    let r = run("mkdir -p /usr/local/lib; echo 'print(\"pwned\")' > /usr/local/lib/json.py; \
         echo 'print(\"pwned\")' > /usr/local/lib/evilmod.py; \
         python3 -c 'import json, os; print(json.dumps(1), open(\"/usr/local/lib/python314.zip\", \"rb\").read(2))
try:
    import evilmod
except ImportError:
    print(\"not importable\")'")
    .await;
    assert_eq!(r.stdout, "1 b'PK'\nnot importable\n");
}

// --- TM-PY-CPY-003: no network, processes, threads or native code ---------------

#[tokio::test]
async fn network_unavailable() {
    let r = run("python3 -c '
import socket
for f in (lambda: socket.create_connection((\"127.0.0.1\", 80), timeout=1),
          lambda: socket.socket(socket.AF_INET, socket.SOCK_STREAM),
          lambda: socket.getaddrinfo(\"example.com\", 80)):
    try:
        f(); print(\"ALLOWED\")
    except (OSError, AttributeError):
        print(\"denied\")
'")
    .await;
    assert_eq!(r.stdout, "denied\ndenied\ndenied\n");
}

#[tokio::test]
async fn urllib_cannot_fetch() {
    // Real CPython prints a long traceback here; only the outcome matters.
    let r = bash()
        .exec("python3 -c 'import urllib.request; urllib.request.urlopen(\"http://example.com\")'")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(r.stdout.is_empty());
}

#[tokio::test]
async fn processes_unavailable() {
    let r = run("python3 -c '
import os, subprocess
for f in (lambda: subprocess.run([\"ls\"]), lambda: os.system(\"ls\"), lambda: os.fork(),
          lambda: os.execv(\"/bin/sh\", [\"sh\"]), lambda: os.popen(\"ls\")):
    try:
        f(); print(\"ALLOWED\")
    except (OSError, AttributeError):
        print(\"denied\")
'")
    .await;
    assert_eq!(r.stdout, "denied\n".repeat(5));
}

#[tokio::test]
async fn threads_and_native_code_unavailable() {
    let r = run("python3 -c '
import threading
try:
    threading.Thread(target=print).start(); print(\"ALLOWED\")
except RuntimeError:
    print(\"denied\")
for m in (\"ctypes\", \"_ctypes\", \"mmap\", \"multiprocessing\", \"_posixsubprocess\"):
    try:
        __import__(m); print(m, \"ALLOWED\")
    except ImportError:
        print(\"denied\")
'")
    .await;
    assert_eq!(r.stdout, "denied\n".repeat(6));
}

// --- TM-PY-CPY-004: CPU time ----------------------------------------------------------

#[tokio::test]
async fn infinite_loop_times_out() {
    let mut bash = bash_limits(CPythonLimits::default().max_duration(Duration::from_millis(300)));
    let start = Instant::now();
    let r = bash
        .exec("python3 -c 'while True: pass'; echo \"after $?\"")
        .await
        .unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(r.stdout, "after 124\n");
    assert!(stderr(&r).contains("python3: execution timed out"));
    assert_no_leak(&r, "timeout", &[]);
}

#[tokio::test]
async fn sleep_is_bounded_by_deadline() {
    let mut bash = bash_limits(CPythonLimits::default().max_duration(Duration::from_millis(300)));
    let start = Instant::now();
    let r = bash
        .exec("python3 -c 'import time; time.sleep(3600)'")
        .await
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(10));
    assert_eq!(r.exit_code, 124);
}

#[tokio::test]
async fn swallowing_exceptions_cannot_escape_timeout() {
    let mut bash = bash_limits(CPythonLimits::default().max_duration(Duration::from_millis(300)));
    let r = bash
        .exec(
            "python3 -c '
while True:
    try:
        while True: pass
    except BaseException:
        pass'",
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 124);
}

#[tokio::test]
async fn shell_timeout_tighter_than_python_limit_wins() {
    let mut bash = Bash::builder()
        .cpython()
        .limits(ExecutionLimits::new().timeout(Duration::from_millis(400)))
        .build();
    let start = Instant::now();
    let _ = bash.exec("python3 -c 'while True: pass'").await;
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn cancellation_stops_busy_guest() {
    let mut bash = bash();
    let token = bash.cancellation_token();
    let cancel = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.store(true, Ordering::Relaxed);
    });
    let start = Instant::now();
    let r = bash.exec("python3 -c 'while True: pass'").await;
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(r.unwrap_err().to_string(), "execution cancelled");
}

// --- TM-PY-CPY-005: memory --------------------------------------------------------------

#[tokio::test]
async fn huge_allocation_raises_memory_error() {
    let r = run("python3 -c 'b = bytearray(1 << 30)'").await;
    assert_eq!(r.exit_code, 1);
    assert!(stderr(&r).ends_with("MemoryError\n"), "{}", stderr(&r));
}

#[tokio::test]
async fn incremental_growth_raises_memory_error() {
    let r = run("python3 -c '
l = []
while True:
    l.append(\"x\" * 100000)'")
    .await;
    assert_eq!(r.exit_code, 1);
    assert!(stderr(&r).contains("MemoryError"), "{}", stderr(&r));
}

#[tokio::test]
async fn memory_error_is_catchable_and_call_continues() {
    let r = run("python3 -c '
try:
    b = bytearray(1 << 30)
except MemoryError:
    print(\"caught\")
print(sum(range(10)))'")
    .await;
    assert_eq!(r.stdout, "caught\n45\n");
}

#[tokio::test]
async fn memory_cap_below_snapshot_fails_cleanly() {
    let mut bash = bash_limits(CPythonLimits::default().max_memory(1 << 20));
    let r = bash
        .exec("python3 -c 'print(1)'; echo after")
        .await
        .unwrap();
    assert_eq!(r.stdout, "after\n");
    assert!(
        stderr(&r).starts_with("python3: fatal error:"),
        "{}",
        stderr(&r)
    );
    assert_no_leak(&r, "tiny memory", &[]);
}

#[tokio::test]
async fn file_buffers_share_the_memory_budget() {
    // Open files are host-side buffers; they are capped so a guest cannot
    // grow host memory past its own budget.
    let mut bash = Bash::builder()
        .fs(Arc::new(InMemoryFs::with_limits(
            FsLimits::new()
                .max_file_size(1 << 30)
                .max_total_bytes(1 << 31),
        )))
        .cpython_with_limits(CPythonLimits::default().max_memory(96 << 20))
        .build();
    let r = bash
        .exec(
            "python3 -c '
f = open(\"/big\", \"wb\")
chunk = b\"x\" * (1 << 20)
try:
    for _ in range(4096):
        f.write(chunk); f.flush()
    print(\"ALLOWED\")
except OSError:
    print(\"capped\")'",
        )
        .await
        .unwrap();
    assert_eq!(r.stdout, "capped\n", "{}", r.stderr);
}

// --- TM-PY-CPY-006: output, files, descriptors ----------------------------------------------

#[tokio::test]
async fn output_is_capped_and_marked_truncated() {
    let mut bash = bash_limits(CPythonLimits::default().max_output(1000));
    let r = bash
        .exec("python3 -c 'import sys; sys.stdout.write(\"x\" * 100000); sys.stderr.write(\"e\" * 100000)'")
        .await
        .unwrap();
    let note = "python3: output truncated at 1000 bytes\n";
    assert!(r.stdout.len() + r.stderr.len() <= 1000 + note.len());
    assert!(stderr(&r).ends_with(note), "{}", stderr(&r));
}

#[tokio::test]
async fn output_flood_loop_is_bounded() {
    let mut bash = bash_limits(
        CPythonLimits::default()
            .max_output(4096)
            .max_duration(Duration::from_millis(500)),
    );
    let r = bash
        .exec("python3 -c 'while True: print(\"y\" * 1000)'")
        .await
        .unwrap();
    assert!(r.stdout.len() <= 4096);
    assert_eq!(r.exit_code, 124);
}

#[tokio::test]
async fn vfs_file_size_limit_applies() {
    let mut bash = Bash::builder()
        .fs(Arc::new(InMemoryFs::with_limits(
            FsLimits::new().max_file_size(1 << 20),
        )))
        .cpython()
        .build();
    let r = bash
        .exec(
            "python3 -c '
try:
    with open(\"/f\", \"wb\") as f: f.write(b\"x\" * (2 << 20))
    print(\"ALLOWED\")
except OSError:
    print(\"too large\")'",
        )
        .await
        .unwrap();
    assert_eq!(r.stdout, "too large\n", "{}", r.stderr);
}

#[tokio::test]
async fn descriptor_table_is_bounded() {
    let r = run("python3 -c '
fs = []
try:
    for i in range(5000): fs.append(open(\"/fd%d\" % i, \"w\"))
except OSError as e:
    print(\"EMFILE\", len(fs) < 1100)'")
    .await;
    assert_eq!(r.stdout, "EMFILE True\n");
}

// --- TM-PY-CPY-007: crash containment ---------------------------------------------------------

#[tokio::test]
async fn deep_c_recursion_is_contained() {
    // The nesting depth is load-bearing, not arbitrary: at 20,000 `repr`
    // simply succeeds and nothing traps, so the property would go untested.
    // Every depth from 22,000 up costs the same 12.5-16 s (the time is spent
    // exhausting the stack, not building the list), so a smaller depth buys
    // no speed and only moves the case toward that cliff.
    let r = run_contained(
        "python3 -c '
l = []
for _ in range(200000): l = [l]
print(repr(l))'; echo \"after $?\"",
    )
    .await;
    assert!(r.stdout.to_string().ends_with("after 1\n"), "{}", r.stdout);
    assert!(
        stderr(&r).contains("python3: fatal error:"),
        "{}",
        stderr(&r)
    );
}

#[tokio::test]
async fn parser_bomb_is_contained() {
    let r = run("python3 -c 'eval(\"(\" * 100000 + \")\" * 100000)'; echo \"after $?\"").await;
    assert_eq!(r.stdout, "after 1\n");
}

#[tokio::test]
async fn python_recursion_limit_enforced() {
    let mut bash = bash_limits(CPythonLimits::default().max_recursion(100));
    let r = bash
        .exec(
            "python3 -c '
import sys
def f(n): return f(n + 1)
try:
    f(0)
except RecursionError:
    print(\"limited\", sys.getrecursionlimit())'",
        )
        .await
        .unwrap();
    assert_eq!(r.stdout, "limited 100\n");
}

#[tokio::test]
async fn abort_is_contained() {
    let r = run("python3 -c 'import os; os.abort()'; echo \"after $?\"").await;
    assert_eq!(r.stdout, "after 1\n");
    assert_eq!(stderr(&r), "python3: fatal error: interpreter aborted\n");
}

#[tokio::test]
async fn shell_continues_after_guest_failures() {
    let r = run("python3 -c 'import os; os.abort()'; \
         python3 -c 'b = bytearray(1 << 30)' 2>/dev/null; \
         python3 -c 'print(\"healthy\")'")
    .await;
    assert_eq!(r.stdout, "healthy\n");
}

// --- TM-PY-CPY-008: isolation across calls and tenants -----------------------------------------

#[tokio::test]
async fn no_state_crosses_calls() {
    let r = run("python3 -c '
import builtins, sys, os, json
builtins.open = None
sys.path.insert(0, \"/evil\")
json.dumps = lambda *a, **k: \"tampered\"
os.environ[\"X\"] = \"1\"'; \
         python3 -c 'import builtins, sys, json, os; print(builtins.open is not None, \"/evil\" in sys.path, json.dumps(1), \"X\" in os.environ)'")
    .await;
    assert_eq!(r.stdout, "True False 1 False\n");
}

#[tokio::test]
async fn concurrent_tenants_are_isolated() {
    let mut handles = Vec::new();
    for i in 0..8 {
        handles.push(tokio::spawn(async move {
            let mut bash = bash();
            let script = format!(
                "python3 -c 'open(\"/id\", \"w\").write(\"{i}\")'; \
                 python3 -c 'import builtins; builtins.T = {i}; print(open(\"/id\").read())'; \
                 python3 -c 'import builtins; print(hasattr(builtins, \"T\"))'"
            );
            let r = bash.exec(&script).await.unwrap();
            assert_eq!(r.stdout, format!("{i}\nFalse\n"));
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

// --- TM-INF-022: error-message hygiene ---------------------------------------------------------

#[tokio::test]
async fn error_paths_do_not_leak_internals() {
    for script in [
        "python3 -Z",
        "python3 /missing.py",
        "python3 -c 'import os; os.abort()'",
        "python3 -c 'raise ValueError(1)'",
        "python3 -c 'def ('",
        "python3 -c 'import nope'",
        "python3 -c 'open(\"/usr/local/lib/python314.zip\", \"w\")'",
    ] {
        let r = run(script).await;
        let err = stderr(&r);
        for banned in [
            "wasmtime",
            "wasm backtrace",
            "Caller",
            "GuestState",
            "_bashkit_boot",
            "bashkit_main",
        ] {
            assert!(!err.contains(banned), "{script}: leaked {banned}: {err}");
        }
    }
}

// --- bounded property-based fuzzing --------------------------------------------------------------

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
    })
}

fn fuzz_run(code: &str, args: &[&str]) -> bashkit::ExecResult {
    let mut bash = Bash::builder()
        .cpython_with_limits(CPythonLimits::default().max_duration(Duration::from_secs(2)))
        .limits(ExecutionLimits::new().max_stderr_bytes(1024))
        .build();
    runtime().block_on(async {
        bash.fs()
            .write_file(std::path::Path::new("/fuzz.py"), code.as_bytes())
            .await
            .unwrap();
        let mut script = String::from("python3");
        for a in args {
            script.push_str(" '");
            script.push_str(&a.replace('\'', "'\\''"));
            script.push('\'');
        }
        bash.exec(&script).await.expect("exec")
    })
}

const FRAGMENTS: &[&str] = &[
    "import os",
    "import sys",
    "open('/x','w').write('a')",
    "print(open('/x').read())",
    "os.listdir('/')",
    "os.chdir('..')",
    "os.makedirs('/a/b', exist_ok=True)",
    "sys.exit(3)",
    "raise ValueError('v')",
    "x = [1] * 1000",
    "def f(n): return f(n+1)",
    "f(0)",
    "while False: pass",
    "import json; json.loads('{')",
    "print('\\x00\\xff')",
    "os.remove('/x')",
    "open('../../etc/passwd')",
    "import socket",
    "sys.stdout.buffer.write(b'\\xff')",
    "os.rename('/x', '/usr/local/lib/python314.zip')",
    "1/0",
    "import time; time.sleep(0.001)",
    "print(*range(100))",
    "sys.setrecursionlimit(10**6)",
    "import random; random.random()",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Arbitrary text as a program never panics the host, never leaks
    /// internals and always yields a well-formed exit status.
    #[test]
    fn arbitrary_source_is_contained(code in "\\PC{0,200}") {
        prop_assume!(!input_echo_would_trip(&code));
        let r = fuzz_run(&code, &["/fuzz.py"]);
        assert_no_leak(&r, "cpython arbitrary_source", &[]);
        prop_assert!((0..=255).contains(&r.exit_code) || r.exit_code == 124);
    }

    /// Programs stitched from filesystem/os/sys fragments stay inside the
    /// VFS and the limits.
    #[test]
    fn fragment_programs_are_contained(idx in proptest::collection::vec(0..FRAGMENTS.len(), 1..8)) {
        let code: Vec<&str> = idx.iter().map(|i| FRAGMENTS[*i]).collect();
        let code = code.join("\n");
        let r = fuzz_run(&code, &["/fuzz.py"]);
        assert_no_leak(&r, "cpython fragments", &[]);
        prop_assert!(!r.stdout.to_string().contains("root:"));
    }

    /// Arbitrary command-line options never panic the CLI emulation.
    #[test]
    fn arbitrary_cli_args_are_contained(args in proptest::collection::vec("[-a-zA-Z0-9=/.]{0,6}", 0..5)) {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let r = fuzz_run("print(1)", &refs);
        assert_no_leak(&r, "cpython args", &[]);
    }
}
