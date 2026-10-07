// Integration tests for the CPython (WASI) python3 builtin (`cpython` feature).
//
// Bash -> CPython pipeline: CLI modes, sys.argv, stdio, exit codes, env and
// cwd, VFS bridging, per-call isolation, and interop with pipelines,
// substitutions, heredocs and conditionals. Stdlib coverage lives in
// `cpython_capability_tests.rs`; sandbox and limits in
// `cpython_security_tests.rs`.

#![cfg(feature = "cpython")]

use bashkit::{Bash, CPythonLimits, FileSystem, InMemoryFs};
use std::path::Path;
use std::sync::Arc;

fn bash() -> Bash {
    Bash::builder().cpython().build()
}

async fn run(script: &str) -> bashkit::ExecResult {
    bash().exec(script).await.expect("exec")
}

async fn ok(script: &str) -> String {
    let r = run(script).await;
    assert_eq!(r.exit_code, 0, "script: {script}\nstderr: {}", r.stderr);
    r.stdout.to_string()
}

// --- CLI modes ---------------------------------------------------------------

#[tokio::test]
async fn dash_c_prints() {
    assert_eq!(ok("python3 -c 'print(1 + 1)'").await, "2\n");
}

#[tokio::test]
async fn python_alias_registered() {
    assert_eq!(ok("python -c 'print(\"alias\")'").await, "alias\n");
}

#[tokio::test]
async fn dash_c_argv() {
    assert_eq!(
        ok("python3 -c 'import sys; print(sys.argv)' x 'y z'").await,
        "['-c', 'x', 'y z']\n"
    );
}

#[tokio::test]
async fn dash_c_attached_value() {
    assert_eq!(ok("python3 -c'print(7)'").await, "7\n");
}

#[tokio::test]
async fn script_file_and_argv() {
    let out = ok("echo 'import sys; print(sys.argv)' > /s.py; python3 /s.py a b").await;
    assert_eq!(out, "['/s.py', 'a', 'b']\n");
}

#[tokio::test]
async fn script_relative_to_cwd() {
    let out = ok("mkdir -p /proj; cd /proj; echo 'print(__file__)' > run.py; python3 run.py").await;
    assert_eq!(out, "run.py\n");
}

#[tokio::test]
async fn script_dir_on_sys_path() {
    let out = ok("mkdir -p /pkg; echo 'X = 5' > /pkg/helper.py; \
         printf 'import helper\\nprint(helper.X)\\n' > /pkg/main.py; python3 /pkg/main.py")
    .await;
    assert_eq!(out, "5\n");
}

#[tokio::test]
async fn main_guard_runs() {
    let out =
        ok("printf 'if __name__ == \"__main__\":\\n    print(\"main\")\\n' > /m.py; python3 /m.py")
            .await;
    assert_eq!(out, "main\n");
}

#[tokio::test]
async fn stdin_program_when_no_args() {
    assert_eq!(ok("echo 'print(40 + 2)' | python3").await, "42\n");
}

#[tokio::test]
async fn dash_reads_program_from_stdin_with_args() {
    let out = ok("python3 - a b <<'EOF'\nimport sys\nprint(sys.argv)\nEOF").await;
    assert_eq!(out, "['-', 'a', 'b']\n");
}

#[tokio::test]
async fn dash_m_stdlib_module() {
    let out = ok("echo '{\"b\":1,\"a\":2}' | python3 -m json.tool --sort-keys --compact").await;
    assert_eq!(out, "{\"a\":2,\"b\":1}\n");
}

#[tokio::test]
async fn dash_m_module_from_cwd() {
    let out = ok("mkdir -p /mm; cd /mm; echo 'import sys; print(\"mod\", sys.argv[1:])' > tool.py; python3 -m tool q").await;
    assert_eq!(out, "mod ['q']\n");
}

#[tokio::test]
async fn directory_with_dunder_main() {
    let out =
        ok("mkdir -p /app; echo 'print(\"from main\")' > /app/__main__.py; python3 /app").await;
    assert_eq!(out, "from main\n");
}

#[tokio::test]
async fn dash_x_skips_first_line() {
    let out = ok("printf 'not python\\nprint(\"ok\")\\n' > /x.py; python3 -x /x.py").await;
    assert_eq!(out, "ok\n");
}

#[tokio::test]
async fn ignored_compat_flags() {
    assert_eq!(ok("python3 -u -B -E -s -I -c 'print(1)'").await, "1\n");
}

#[tokio::test]
async fn version_flags() {
    assert_eq!(ok("python3 -V").await, "Python 3.14.8\n");
    assert_eq!(ok("python3 --version").await, "Python 3.14.8\n");
    assert!(ok("python3 -VV").await.starts_with("Python 3.14.8 "));
}

#[tokio::test]
async fn help_flag() {
    assert!(ok("python3 -h").await.starts_with("usage: python3"));
}

#[tokio::test]
async fn unknown_option_exits_2() {
    let r = run("python3 -Z").await;
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.to_string().contains("Unknown option: -Z"));
}

#[tokio::test]
async fn dash_c_without_value_exits_2() {
    let r = run("python3 -c").await;
    assert_eq!(r.exit_code, 2);
    assert!(
        r.stderr
            .to_string()
            .contains("Argument expected for the -c option")
    );
}

#[tokio::test]
async fn missing_script_exits_2() {
    let r = run("python3 /missing.py").await;
    assert_eq!(r.exit_code, 2);
    assert!(
        r.stderr
            .to_string()
            .starts_with("python3: can't open file '/missing.py'")
    );
}

#[tokio::test]
async fn warning_option_turns_warning_into_error() {
    let r = run("python3 -W error -c 'import warnings; warnings.warn(\"w\")'").await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.to_string().contains("UserWarning: w"));
}

// --- exit status and errors --------------------------------------------------

#[tokio::test]
async fn sys_exit_codes() {
    assert_eq!(
        run("python3 -c 'import sys; sys.exit(3)'").await.exit_code,
        3
    );
    assert_eq!(
        run("python3 -c 'import sys; sys.exit()'").await.exit_code,
        0
    );
    assert_eq!(
        run("python3 -c 'raise SystemExit(True)'").await.exit_code,
        1
    );
    assert_eq!(run("python3 -c 'exit(300)'").await.exit_code, 300 & 0xff);
}

#[tokio::test]
async fn sys_exit_message_goes_to_stderr() {
    let r = run("python3 -c 'import sys; sys.exit(\"bye\")'").await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stderr, "bye\n");
}

#[tokio::test]
async fn os_exit_keeps_flushed_output() {
    let r = run("python3 -c 'import os; print(\"a\", flush=True); os._exit(5)'").await;
    assert_eq!(r.exit_code, 5);
    assert_eq!(r.stdout, "a\n");
}

#[tokio::test]
async fn uncaught_exception_traceback_matches_cpython() {
    let r = run("python3 -c 'def f(): 1/0\nf()'").await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(
        r.stderr,
        "Traceback (most recent call last):\n  File \"<string>\", line 2, in <module>\n  \
         File \"<string>\", line 1, in f\nZeroDivisionError: division by zero\n"
    );
}

#[tokio::test]
async fn traceback_hides_driver_frames_for_scripts() {
    let r = run("echo 'raise ValueError(\"boom\")' > /e.py; python3 /e.py").await;
    let err = r.stderr.to_string();
    assert!(err.contains("File \"/e.py\", line 1"), "{err}");
    assert!(!err.contains("_bashkit_boot"), "{err}");
    assert!(err.ends_with("ValueError: boom\n"));
}

#[tokio::test]
async fn syntax_error_exit_1() {
    let r = run("python3 -c 'def ('").await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.to_string().contains("SyntaxError"));
}

#[tokio::test]
async fn atexit_handlers_run() {
    let out =
        ok("python3 -c 'import atexit; atexit.register(print, \"bye\"); print(\"hi\")'").await;
    assert_eq!(out, "hi\nbye\n");
}

// --- stdio ---------------------------------------------------------------------

#[tokio::test]
async fn stdin_read_and_iterate() {
    assert_eq!(
        ok("echo hello | python3 -c 'import sys; print(sys.stdin.read().upper(), end=\"\")'").await,
        "HELLO\n"
    );
    assert_eq!(
        ok(
            "printf '1\\n2\\n3\\n' | python3 -c 'import sys; print(sum(int(l) for l in sys.stdin))'"
        )
        .await,
        "6\n"
    );
}

#[tokio::test]
async fn input_builtin() {
    assert_eq!(
        ok("printf 'bob\\n' | python3 -c 'print(\"hi\", input())'").await,
        "hi bob\n"
    );
}

#[tokio::test]
async fn input_eof_raises() {
    let r = run("python3 -c 'input()' < /dev/null").await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.to_string().contains("EOFError"));
}

#[tokio::test]
async fn stderr_separate_from_stdout() {
    let r = run("python3 -c 'import sys; print(\"o\"); print(\"e\", file=sys.stderr)'").await;
    assert_eq!(r.stdout, "o\n");
    assert_eq!(r.stderr, "e\n");
}

#[tokio::test]
async fn binary_stdout_roundtrip() {
    let r = run("python3 -c 'import sys; sys.stdout.buffer.write(bytes(range(256)))'").await;
    assert_eq!(
        r.stdout.as_bytes(),
        (0u8..=255).collect::<Vec<_>>().as_slice()
    );
}

#[tokio::test]
async fn binary_stdin() {
    let out = ok(
        "python3 -c 'open(\"/b.bin\",\"wb\").write(b\"\\x00\\xff\\x10\")'; \
         python3 -c 'import sys; print(list(sys.stdin.buffer.read()))' < /b.bin",
    )
    .await;
    assert_eq!(out, "[0, 255, 16]\n");
}

#[tokio::test]
async fn unicode_output_is_utf8() {
    assert_eq!(
        ok("python3 -c 'print(\"héllo ✓ 日本\")'").await,
        "héllo ✓ 日本\n"
    );
    assert_eq!(
        ok("python3 -c 'import sys; print(sys.stdout.encoding, sys.getfilesystemencoding())'")
            .await,
        "utf-8 utf-8\n"
    );
}

// --- environment and cwd -------------------------------------------------------

#[tokio::test]
async fn only_exported_variables_visible() {
    let out = ok("FOO=1; export BAR=2; python3 -c 'import os; print(os.environ.get(\"FOO\"), os.environ.get(\"BAR\"))'").await;
    assert_eq!(out, "None 2\n");
}

#[tokio::test]
async fn prefix_assignment_visible() {
    let out = ok("GREETING=hey python3 -c 'import os; print(os.environ[\"GREETING\"])'").await;
    assert_eq!(out, "hey\n");
}

#[tokio::test]
async fn internal_variables_not_visible() {
    let out = ok("python3 -c 'import os; print(sorted(k for k in os.environ if k.startswith(\"__BASHKIT\")))'").await;
    assert_eq!(out, "[]\n");
}

#[tokio::test]
async fn environ_changes_do_not_leak_to_shell() {
    let out = ok("export A=1; python3 -c 'import os; os.environ[\"A\"]=\"2\"; os.environ[\"B\"]=\"3\"'; echo \"$A-${B:-unset}\"").await;
    assert_eq!(out, "1-unset\n");
}

#[tokio::test]
async fn cwd_follows_shell_cd() {
    let out = ok("mkdir -p /w/sub; cd /w/sub; python3 -c 'import os; print(os.getcwd())'").await;
    assert_eq!(out, "/w/sub\n");
}

#[tokio::test]
async fn python_chdir_does_not_move_shell() {
    let out = ok("mkdir -p /a /b; cd /a; python3 -c 'import os; os.chdir(\"/b\")'; pwd").await;
    assert_eq!(out, "/a\n");
}

// --- VFS bridging --------------------------------------------------------------

#[tokio::test]
async fn python_writes_shell_reads() {
    let out =
        ok("python3 -c 'open(\"/out.txt\", \"w\").write(\"from py\\n\")'; cat /out.txt").await;
    assert_eq!(out, "from py\n");
}

#[tokio::test]
async fn shell_writes_python_reads() {
    let out =
        ok("echo 'from sh' > /in.txt; python3 -c 'print(open(\"/in.txt\").read().strip())'").await;
    assert_eq!(out, "from sh\n");
}

#[tokio::test]
async fn relative_write_lands_in_cwd() {
    let out =
        ok("mkdir -p /w; cd /w; python3 -c 'open(\"rel.txt\",\"w\").write(\"r\")'; cat /w/rel.txt")
            .await;
    assert_eq!(out, "r");
}

#[tokio::test]
async fn unclosed_file_is_flushed_at_exit() {
    let out = ok("python3 -c 'f = open(\"/u.txt\", \"w\"); f.write(\"data\")'; cat /u.txt").await;
    assert_eq!(out, "data");
}

#[tokio::test]
async fn append_mode() {
    let out = ok(
        "echo one > /ap.txt; python3 -c 'open(\"/ap.txt\",\"a\").write(\"two\\n\")'; cat /ap.txt",
    )
    .await;
    assert_eq!(out, "one\ntwo\n");
}

#[tokio::test]
async fn read_write_seek() {
    let out = ok("python3 -c '
with open(\"/rw.bin\", \"w+b\") as f:
    f.write(b\"hello world\")
    f.seek(6)
    print(f.read())
    f.seek(0)
    f.write(b\"J\")
print(open(\"/rw.bin\",\"rb\").read())
'")
    .await;
    assert_eq!(out, "b'world'\nb'Jello world'\n");
}

#[tokio::test]
async fn binary_file_roundtrip() {
    let out =
        ok("python3 -c 'open(\"/b.bin\",\"wb\").write(bytes(range(256)))'; wc -c < /b.bin").await;
    assert_eq!(out.trim(), "256");
}

#[tokio::test]
async fn directory_operations() {
    let out = ok("python3 -c '
import os, shutil
os.makedirs(\"/d/x/y\", exist_ok=True)
open(\"/d/x/y/f.txt\",\"w\").write(\"1\")
os.rename(\"/d/x/y/f.txt\", \"/d/x/g.txt\")
print(sorted(os.listdir(\"/d/x\")))
shutil.copy(\"/d/x/g.txt\", \"/d/copy.txt\")
os.remove(\"/d/x/g.txt\")
os.rmdir(\"/d/x/y\")
print(sorted(os.listdir(\"/d\")), sorted(os.listdir(\"/d/x\")))
'; cat /d/copy.txt")
    .await;
    assert_eq!(out, "['g.txt', 'y']\n['copy.txt', 'x'] []\n1");
}

#[tokio::test]
async fn walk_glob_and_stat() {
    let out = ok(
        "mkdir -p /t/a /t/b; echo 12345 > /t/a/1.txt; echo x > /t/b/2.log; python3 -c '
import os, glob
print(sorted(glob.glob(\"/t/*/*.txt\")))
print(sum(len(fs) for _, _, fs in os.walk(\"/t\")))
st = os.stat(\"/t/a/1.txt\")
print(st.st_size, os.path.isfile(\"/t/a/1.txt\"), os.path.isdir(\"/t/a\"))
'",
    )
    .await;
    assert_eq!(out, "['/t/a/1.txt']\n2\n6 True True\n");
}

#[tokio::test]
async fn pathlib_operations() {
    let out = ok("python3 -c '
from pathlib import Path
p = Path(\"/pl/dir\")
p.mkdir(parents=True)
(p / \"a.txt\").write_text(\"alpha\")
print((p / \"a.txt\").read_text(), [c.name for c in p.iterdir()], (p / \"a.txt\").exists())
'")
    .await;
    assert_eq!(out, "alpha ['a.txt'] True\n");
}

#[tokio::test]
async fn missing_file_raises_file_not_found() {
    let r = run("python3 -c 'open(\"/nope.txt\")'").await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.to_string().contains("FileNotFoundError"));
}

#[tokio::test]
async fn unicode_file_names() {
    let out = ok("python3 -c 'open(\"/données.txt\",\"w\").write(\"ü\")'; cat /données.txt").await;
    assert_eq!(out, "ü");
}

#[tokio::test]
async fn sqlite_database_on_vfs_persists() {
    let out = ok("python3 -c '
import sqlite3
c = sqlite3.connect(\"/db.sqlite\")
c.execute(\"create table t(x)\")
c.execute(\"insert into t values (42)\")
c.commit(); c.close()
'; python3 -c 'import sqlite3; print(sqlite3.connect(\"/db.sqlite\").execute(\"select x from t\").fetchone())'")
    .await;
    assert_eq!(out, "(42,)\n");
}

#[tokio::test]
async fn custom_filesystem_is_used() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/seed.txt"), b"seeded")
        .await
        .unwrap();
    let mut bash = Bash::builder().fs(fs.clone()).cpython().build();
    let r = bash
        .exec("python3 -c 'print(open(\"/seed.txt\").read()); open(\"/made.txt\",\"w\").write(\"m\")'")
        .await
        .unwrap();
    assert_eq!(r.stdout, "seeded\n");
    assert_eq!(fs.read_file(Path::new("/made.txt")).await.unwrap(), b"m");
}

#[tokio::test]
async fn stdlib_zip_is_not_in_the_vfs() {
    let out = ok("python3 -c 'import os; print(os.listdir(\"/usr/local/lib\"))'; ls /usr/local/lib 2>/dev/null || echo absent").await;
    assert_eq!(out, "['python314.zip']\nabsent\n");
}

// --- per-call isolation ----------------------------------------------------------

#[tokio::test]
async fn interpreter_state_does_not_persist_between_calls() {
    let out = ok("python3 -c 'import builtins, json, sys; builtins.LEAK = 1; json.LEAK = 2; sys.modules[\"evil\"] = 1'; \
         python3 -c 'import builtins, json, sys; print(hasattr(builtins, \"LEAK\"), hasattr(json, \"LEAK\"), \"evil\" in sys.modules)'")
    .await;
    assert_eq!(out, "False False False\n");
}

#[tokio::test]
async fn preloaded_modules_stay_mutable_and_reimportable() {
    // Snapshot objects are immortal (no refcount writes); mutating, deleting
    // and re-importing them must still behave like regular CPython.
    let out = ok(
        "python3 -c 'import json, sys; json.dumps = None; del sys.modules[\"json\"]; \
         import json as j2; print(j2.dumps([1]), j2 is not json)'",
    )
    .await;
    assert_eq!(out, "[1] True\n");
}

#[tokio::test]
async fn environ_is_replaced_each_call() {
    let out = ok("export A=1; python3 -c 'import os; os.environ[\"B\"] = \"x\"; print(os.environ[\"A\"])'; \
         unset A; export C=3; python3 -c 'import os; print(\"A\" in os.environ, \"B\" in os.environ, os.environ[\"C\"], os.getenv(\"C\"))'")
    .await;
    assert_eq!(out, "1\nFalse False 3 3\n");
}

#[tokio::test]
async fn local_modules_are_reimported_each_call() {
    let out = ok(
        "mkdir -p /m; cd /m; echo 'V = 1' > ver.py; python3 -c 'import ver; print(ver.V)'; \
         echo 'V = 2' > ver.py; python3 -c 'import ver; print(ver.V)'",
    )
    .await;
    assert_eq!(out, "1\n2\n");
}

#[tokio::test]
async fn random_is_reseeded_per_call() {
    let out = ok("a=$(python3 -c 'import random; print(random.random())'); \
         b=$(python3 -c 'import random; print(random.random())'); [ \"$a\" != \"$b\" ] && echo differ")
    .await;
    assert_eq!(out, "differ\n");
}

#[tokio::test]
async fn separate_bash_instances_share_nothing() {
    let mut a = bash();
    let mut b = bash();
    a.exec("python3 -c 'open(\"/only-a.txt\",\"w\").write(\"a\")'")
        .await
        .unwrap();
    let r = b
        .exec("python3 -c 'import os; print(os.path.exists(\"/only-a.txt\"))'")
        .await
        .unwrap();
    assert_eq!(r.stdout, "False\n");
}

// --- bash interop ----------------------------------------------------------------

#[tokio::test]
async fn pipeline_into_shell_tools() {
    let out = ok("python3 -c 'for i in range(5): print(i)' | tail -n 2 | tr '\\n' ,").await;
    assert_eq!(out, "3,4,");
}

#[tokio::test]
async fn pipeline_between_python_calls() {
    let out = ok("python3 -c 'print(\"a b c\")' | python3 -c 'import sys; print(len(sys.stdin.read().split()))'").await;
    assert_eq!(out, "3\n");
}

#[tokio::test]
async fn command_substitution() {
    let out = ok("n=$(python3 -c 'print(6*7)'); echo \"n=$n\"").await;
    assert_eq!(out, "n=42\n");
}

#[tokio::test]
async fn conditionals_use_exit_status() {
    let out = ok(
        "if python3 -c 'import sys; sys.exit(1)'; then echo yes; else echo no; fi; \
         python3 -c 'pass' && echo and; python3 -c 'raise SystemExit(2)' || echo \"or $?\"",
    )
    .await;
    assert_eq!(out, "no\nand\nor 2\n");
}

#[tokio::test]
async fn errexit_stops_on_python_failure() {
    let r = run("set -e; python3 -c 'raise SystemExit(4)'; echo unreachable").await;
    assert_eq!(r.exit_code, 4);
    assert!(!r.stdout.to_string().contains("unreachable"));
}

#[tokio::test]
async fn heredoc_script() {
    let out = ok("python3 <<'EOF'\nimport json\nprint(json.dumps({\"k\": [1, 2]}))\nEOF").await;
    assert_eq!(out, "{\"k\": [1, 2]}\n");
}

#[tokio::test]
async fn redirect_python_output_to_file() {
    let out = ok("python3 -c 'print(\"to file\")' > /r.txt 2>/r.err; cat /r.txt").await;
    assert_eq!(out, "to file\n");
}

#[tokio::test]
async fn loop_of_calls() {
    let out = ok("for i in 1 2 3; do python3 -c \"print($i * 10)\"; done").await;
    assert_eq!(out, "10\n20\n30\n");
}

#[tokio::test]
async fn custom_limits_builder() {
    let mut bash = Bash::builder()
        .cpython_with_limits(CPythonLimits::default().max_recursion(120))
        .build();
    let r = bash
        .exec("python3 -c 'import sys; print(sys.getrecursionlimit())'")
        .await
        .unwrap();
    assert_eq!(r.stdout, "120\n");
}
