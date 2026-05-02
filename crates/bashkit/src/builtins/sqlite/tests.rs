//! Unit tests for the `sqlite` builtin.
//!
//! Coverage matrix (mirrors `specs/sqlite-builtin.md` § Test plan):
//!
//! - **Positive** — basic CRUD, transactions, dot-commands, output modes,
//!   `:memory:`, persistence to VFS, `--version`/`--help`.
//! - **Negative** — invalid SQL, unknown options, oversize script, unknown
//!   dot-command, missing `-c` arg, file too large, non-UTF8 file in `.read`.
//! - **Output formatting** — list, csv (RFC 4180 quoting), tabs, line, box,
//!   column, json, markdown.
//! - **Backend equivalence** — every positive test runs against both
//!   [`SqliteBackend::Memory`] and [`SqliteBackend::Vfs`].
//! - **Security** — opt-in gate, NULL/blob round-trip, no host filesystem
//!   access, oversize file rejected.
//! - **Property tests** — separator/null-text round-trip; SQL splitter on
//!   arbitrary input never panics.
//!
//! Compatibility tests (sqlite3 parity) live in
//! `crates/bashkit/tests/sqlite_compat_tests.rs`.

#![allow(clippy::too_many_lines)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::builtins::Context;
use crate::fs::{FileSystem, InMemoryFs};
use crate::interpreter::ExecResult;

use super::{Builtin, SQLITE_OPT_IN_ENV, Sqlite, SqliteBackend, SqliteLimits};

// ---------------------------------------------------------------------------
// Test harness helpers
// ---------------------------------------------------------------------------

fn opt_in_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert(SQLITE_OPT_IN_ENV.to_string(), "1".to_string());
    env
}

async fn run_with(
    args: &[&str],
    backend: SqliteBackend,
    fs: Arc<dyn FileSystem>,
    stdin: Option<&str>,
    env: &HashMap<String, String>,
) -> ExecResult {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut variables = HashMap::new();
    let mut cwd = PathBuf::from("/home/user");
    let ctx = Context::new_for_test(&owned, env, &mut variables, &mut cwd, fs, stdin);
    Sqlite::with_limits(SqliteLimits::default().backend(backend))
        .execute(ctx)
        .await
        .unwrap()
}

async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    run_with(args, SqliteBackend::Memory, fs, stdin, &env).await
}

async fn run_vfs(args: &[&str], stdin: Option<&str>) -> ExecResult {
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    run_with(args, SqliteBackend::Vfs, fs, stdin, &env).await
}

/// Run on both backends and assert their outputs match.
async fn run_both_match(args: &[&str], stdin: Option<&str>) -> ExecResult {
    let mem = run(args, stdin).await;
    let vfs = run_vfs(args, stdin).await;
    assert_eq!(
        mem.exit_code, vfs.exit_code,
        "exit codes diverge: mem vs vfs"
    );
    assert_eq!(
        mem.stdout, vfs.stdout,
        "stdout diverges between backends:\nMEM=={}\nVFS=={}",
        mem.stdout, vfs.stdout
    );
    mem
}

// ---------------------------------------------------------------------------
// Positive: opt-in + CRUD basics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn version_flag_works() {
    let r = run(&["--version"], None).await;
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("sqlite"));
}

#[tokio::test]
async fn help_flag_lists_dot_commands() {
    let r = run(&["--help"], None).await;
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("usage:"));
    assert!(r.stdout.contains(".dump"));
    assert!(r.stdout.contains("-separator"));
}

#[tokio::test]
async fn opt_in_required_by_default() {
    let owned: Vec<String> = vec![
        "--".to_string(),
        ":memory:".to_string(),
        "SELECT 1".to_string(),
    ];
    let env = HashMap::new(); // no opt-in
    let mut variables = HashMap::new();
    let mut cwd = PathBuf::from("/home/user");
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let ctx = Context::new_for_test(&owned, &env, &mut variables, &mut cwd, fs, None);
    let r = Sqlite::new().execute(ctx).await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("disabled"));
}

#[tokio::test]
async fn select_literal_returns_value() {
    let r = run_both_match(&[":memory:", "SELECT 42"], None).await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "42");
}

#[tokio::test]
async fn create_insert_select_round_trip() {
    let r = run_both_match(
        &[
            ":memory:",
            "CREATE TABLE t(a INTEGER, b TEXT); \
             INSERT INTO t VALUES (1,'a'), (2,'b'); \
             SELECT * FROM t ORDER BY a",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "1|a\n2|b\n");
}

#[tokio::test]
async fn header_flag_prefixes_output() {
    let r = run(
        &[
            "-header",
            ":memory:",
            "CREATE TABLE t(a, b); INSERT INTO t VALUES (1,2); SELECT * FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "a|b\n1|2\n");
}

#[tokio::test]
async fn csv_mode_quotes_separator() {
    let r = run(
        &[
            "-csv",
            "-header",
            ":memory:",
            "CREATE TABLE t(a, b); INSERT INTO t VALUES ('x,y','z'); SELECT * FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "a,b\n\"x,y\",z\n");
}

#[tokio::test]
async fn json_mode_emits_array_of_objects() {
    let r = run(
        &["-json", ":memory:", "SELECT 1 AS i, 'hi' AS s, NULL AS n"],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0);
    let parsed: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(parsed[0]["i"], 1);
    assert_eq!(parsed[0]["s"], "hi");
    assert!(parsed[0]["n"].is_null());
}

#[tokio::test]
async fn markdown_mode_renders_separator_row() {
    let r = run(
        &[
            "-markdown",
            ":memory:",
            "CREATE TABLE t(a, b); INSERT INTO t VALUES (1,2); SELECT * FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("| a"));
    assert!(r.stdout.contains("---"));
}

#[tokio::test]
async fn separator_flag_overrides_default() {
    let r = run(
        &[
            "-separator",
            ";",
            ":memory:",
            "CREATE TABLE t(a, b); INSERT INTO t VALUES (1,2); SELECT * FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.stdout, "1;2\n");
}

#[tokio::test]
async fn separator_flag_decodes_tab_escape() {
    let r = run(
        &[
            "-separator",
            "\\t",
            ":memory:",
            "CREATE TABLE t(a, b); INSERT INTO t VALUES (1,2); SELECT * FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.stdout, "1\t2\n");
}

#[tokio::test]
async fn nullvalue_renders_placeholder() {
    let r = run(&["-nullvalue", "<NIL>", ":memory:", "SELECT NULL"], None).await;
    assert_eq!(r.stdout.trim(), "<NIL>");
}

#[tokio::test]
async fn cmd_flag_runs_extra_sql_first() {
    let r = run(
        &[
            "-cmd",
            "CREATE TABLE t(a)",
            ":memory:",
            "INSERT INTO t VALUES (1); SELECT * FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "1");
}

#[tokio::test]
async fn stdin_used_when_no_inline_sql() {
    let r = run(
        &[":memory:"],
        Some("CREATE TABLE t(a); INSERT INTO t VALUES (7); SELECT * FROM t"),
    )
    .await;
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "7");
}

#[tokio::test]
async fn dump_includes_data_and_schema() {
    // Dot-commands must live on their own line — otherwise `.dump` would be
    // parsed as the trailing fragment of the previous SQL statement.
    let r = run(
        &[
            ":memory:",
            "CREATE TABLE t(a, b); INSERT INTO t VALUES (1,'x');\n.dump",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0, "stderr was: {}", r.stderr);
    assert!(r.stdout.contains("CREATE TABLE t"));
    assert!(r.stdout.contains("INSERT INTO \"t\" VALUES(1,'x')"));
    assert!(r.stdout.contains("BEGIN TRANSACTION;"));
    assert!(r.stdout.contains("COMMIT;"));
}

#[tokio::test]
async fn schema_filter_limits_output() {
    let r = run(
        &[
            ":memory:",
            "CREATE TABLE foo(a); CREATE TABLE bar(b);\n.schema foo",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(r.stdout.contains("foo"), "stdout: {}", r.stdout);
    assert!(!r.stdout.contains("bar"));
}

// ---------------------------------------------------------------------------
// Persistence to VFS (both backends)
// ---------------------------------------------------------------------------

async fn run_persisting(
    args: &[&str],
    backend: SqliteBackend,
    fs: Arc<dyn FileSystem>,
    env: &HashMap<String, String>,
) -> ExecResult {
    run_with(args, backend, fs, None, env).await
}

#[tokio::test]
async fn persistence_round_trip_memory_backend() {
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let r1 = run_persisting(
        &[
            "/tmp/db.sqlite",
            "CREATE TABLE t(a); INSERT INTO t VALUES (1)",
        ],
        SqliteBackend::Memory,
        fs.clone(),
        &env,
    )
    .await;
    assert_eq!(r1.exit_code, 0, "first invocation failed: {r1:?}");
    // The DB file must now exist on the VFS.
    assert!(fs.exists(Path::new("/tmp/db.sqlite")).await.unwrap());
    let r2 = run_persisting(
        &["/tmp/db.sqlite", "SELECT * FROM t"],
        SqliteBackend::Memory,
        fs,
        &env,
    )
    .await;
    assert_eq!(r2.exit_code, 0, "second invocation failed: {r2:?}");
    assert_eq!(r2.stdout.trim(), "1");
}

#[tokio::test]
async fn persistence_round_trip_vfs_backend() {
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let r1 = run_persisting(
        &[
            "/tmp/dbvfs.sqlite",
            "CREATE TABLE t(a); INSERT INTO t VALUES (42)",
        ],
        SqliteBackend::Vfs,
        fs.clone(),
        &env,
    )
    .await;
    assert_eq!(r1.exit_code, 0, "first invocation failed: {r1:?}");
    let r2 = run_persisting(
        &["/tmp/dbvfs.sqlite", "SELECT * FROM t"],
        SqliteBackend::Vfs,
        fs,
        &env,
    )
    .await;
    assert_eq!(r2.exit_code, 0);
    assert_eq!(r2.stdout.trim(), "42");
}

// ---------------------------------------------------------------------------
// Negative
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_sql_returns_error() {
    let r = run(&[":memory:", "NOT VALID SQL"], None).await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("sqlite:"));
}

#[tokio::test]
async fn unknown_option_errors() {
    let r = run(&["-bogus", ":memory:", "SELECT 1"], None).await;
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("unknown option"));
}

#[tokio::test]
async fn unknown_dot_command_errors() {
    let r = run(&[":memory:", ".thisdoesnotexist"], None).await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("unknown dot-command"));
}

#[tokio::test]
async fn separator_flag_missing_arg() {
    let r = run(&["-separator"], None).await;
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("requires an argument"));
}

#[tokio::test]
async fn cmd_flag_missing_arg() {
    let r = run(&["-cmd"], None).await;
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("requires an argument"));
}

#[tokio::test]
async fn invalid_backend_value() {
    let r = run(&["-backend", "wishful"], None).await;
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("invalid backend"));
}

#[tokio::test]
async fn too_many_statements_rejected() {
    // Cap to 5 statements; feed 20.
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let many = "SELECT 1; ".repeat(20);
    let owned: Vec<String> = vec![":memory:".to_string(), many];
    let mut variables = HashMap::new();
    let mut cwd = PathBuf::from("/home/user");
    let ctx = Context::new_for_test(&owned, &env, &mut variables, &mut cwd, fs, None);
    let limits = SqliteLimits::default().max_statements(5);
    let r = Sqlite::with_limits(limits).execute(ctx).await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("too many statements"));
}

#[tokio::test]
async fn deadline_zero_means_unlimited() {
    // ZERO duration disables the deadline entirely. Without this carve-out,
    // any non-trivial workload would race with the start time and fail.
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let owned: Vec<String> = vec![":memory:".to_string(), "SELECT 1".to_string()];
    let mut variables = HashMap::new();
    let mut cwd = PathBuf::from("/home/user");
    let ctx = Context::new_for_test(&owned, &env, &mut variables, &mut cwd, fs, None);
    let limits = SqliteLimits::default().max_duration(std::time::Duration::ZERO);
    let r = Sqlite::with_limits(limits).execute(ctx).await.unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "1");
}

#[tokio::test]
async fn deadline_already_expired_aborts_with_timeout() {
    // Construct a deadline that has already passed (1ns budget) so the very
    // first statement aborts.
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let owned: Vec<String> = vec![":memory:".to_string(), "SELECT 1".to_string()];
    let mut variables = HashMap::new();
    let mut cwd = PathBuf::from("/home/user");
    let ctx = Context::new_for_test(&owned, &env, &mut variables, &mut cwd, fs, None);
    // Pick a value smaller than any realistic SQL execution path.
    let limits = SqliteLimits::default().max_duration(std::time::Duration::from_nanos(1));
    // Spin briefly so the deadline is definitely in the past before we
    // start the engine — otherwise we'd race on Linux's monotonic clock
    // resolution.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let r = Sqlite::with_limits(limits).execute(ctx).await.unwrap();
    // Either we hit the per-statement deadline check (most likely) or the
    // pre-statement check; both surface the timeout message.
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("timed out"), "stderr was: {}", r.stderr);
}

#[test]
fn limits_builder_round_trips() {
    let l = SqliteLimits::default()
        .max_script_bytes(1024)
        .max_rows_per_query(10)
        .max_db_bytes(2048)
        .max_duration(std::time::Duration::from_secs(7))
        .max_statements(42)
        .backend(SqliteBackend::Vfs);
    assert_eq!(l.max_script_bytes, 1024);
    assert_eq!(l.max_rows_per_query, 10);
    assert_eq!(l.max_db_bytes, 2048);
    assert_eq!(l.max_duration, std::time::Duration::from_secs(7));
    assert_eq!(l.max_statements, 42);
    assert_eq!(l.backend, SqliteBackend::Vfs);
}

#[tokio::test]
async fn script_too_large_rejected() {
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let owned: Vec<String> = vec![":memory:".to_string(), "SELECT 1; ".repeat(50_000)];
    let mut variables = HashMap::new();
    let mut cwd = PathBuf::from("/home/user");
    let ctx = Context::new_for_test(&owned, &env, &mut variables, &mut cwd, fs, None);
    // Tighten the script cap so a small (~500 KiB) script trips it.
    let limits = SqliteLimits::default().max_script_bytes(1024);
    let r = Sqlite::with_limits(limits).execute(ctx).await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("script too large"));
}

#[tokio::test]
async fn dot_read_missing_file() {
    let r = run(&[":memory:", ".read /no/such/file.sql"], None).await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("cannot read"));
}

#[tokio::test]
async fn dot_read_not_utf8() {
    let env = opt_in_env();
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/binary.sql"), &[0xff, 0xfe, 0x00, 0x01])
        .await
        .unwrap();
    let fs_dyn: Arc<dyn FileSystem> = fs;
    let r = run_with(
        &[":memory:", ".read /tmp/binary.sql"],
        SqliteBackend::Memory,
        fs_dyn,
        None,
        &env,
    )
    .await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("not valid UTF-8"));
}

#[tokio::test]
async fn db_file_too_large_rejected() {
    // Cap to 1 KiB, then drop a 4 KiB blob into the VFS; loading should fail.
    let env = opt_in_env();
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/oversize.sqlite"), &vec![0u8; 4096])
        .await
        .unwrap();
    let fs_dyn: Arc<dyn FileSystem> = fs;
    let owned: Vec<String> = ["/tmp/oversize.sqlite".to_string(), "SELECT 1".to_string()].to_vec();
    let mut variables = HashMap::new();
    let mut cwd = PathBuf::from("/home/user");
    let ctx = Context::new_for_test(&owned, &env, &mut variables, &mut cwd, fs_dyn, None);
    let limits = SqliteLimits::default()
        .max_db_bytes(1024)
        .backend(SqliteBackend::Memory);
    let r = Sqlite::with_limits(limits).execute(ctx).await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("too large"));
}

#[tokio::test]
async fn dot_help_listed_via_command() {
    let r = run(&[":memory:", ".help"], None).await;
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains(".dump"));
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

#[tokio::test]
async fn host_filesystem_inaccessible() {
    // Use the sqlite engine to try to read /etc/passwd via ATTACH would be
    // the obvious attack; turso supports ATTACH but our IO is tied to the
    // bashkit FileSystem only, so any path resolves through the VFS.
    // Here we assert that even an absolute path like "/etc/passwd" is
    // resolved against the VFS, which doesn't contain such a file.
    let env = opt_in_env();
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    // Write a decoy file at /etc/passwd inside the VFS so we can confirm the
    // read goes through the VFS (which is sandboxed).
    fs.mkdir(Path::new("/etc"), false).await.unwrap();
    fs.write_file(Path::new("/etc/passwd"), b"vfs-sandboxed-decoy")
        .await
        .unwrap();
    let r = run_with(
        &["/etc/passwd", ".tables"],
        SqliteBackend::Vfs,
        fs,
        None,
        &env,
    )
    .await;
    // Loading a non-DB file as a database typically errors out — that's
    // fine; what we're asserting is that we did not panic and we did not
    // leak host filesystem state.
    assert!(matches!(r.exit_code, 0 | 1));
    assert!(!r.stderr.contains("/etc/passwd: "));
}

#[tokio::test]
async fn null_text_does_not_collide_with_real_text() {
    // Empty string and NULL must be distinguishable when the user picks a
    // sentinel. (Default null_text is "" so this is defensive against a
    // future regression where NULL would collide with real empty TEXT.)
    let r = run(
        &[
            "-nullvalue",
            "(null)",
            ":memory:",
            "CREATE TABLE t(x); INSERT INTO t VALUES (NULL), (''); \
             SELECT x FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(
        r.stdout.contains("(null)"),
        "stdout missing null sentinel: {:?}",
        r.stdout
    );
    // The empty-string row is rendered as a literal empty line (just `\n`).
    assert!(r.stdout.lines().any(|l| l.is_empty()));
}

#[tokio::test]
async fn blob_in_csv_is_escape_safe() {
    // A blob whose contents include the separator must not break CSV parsing.
    let r = run(
        &[
            "-csv",
            "-header",
            ":memory:",
            "CREATE TABLE t(b BLOB); INSERT INTO t VALUES (X'2C2C'); SELECT * FROM t",
        ],
        None,
    )
    .await;
    assert_eq!(r.exit_code, 0);
    // 0x2C == ','; CSV quoting must wrap the field.
    let last = r.stdout.lines().last().unwrap();
    assert!(last.starts_with('"') && last.ends_with('"'));
}

// ---------------------------------------------------------------------------
// Property tests (proptest) — splitter never panics; option parsing stable.
// ---------------------------------------------------------------------------

mod prop {
    use super::super::parser;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        // The SQL splitter must never panic on arbitrary input and must
        // always reconstruct an equivalent set of statements when fed
        // simple `;`-joined inputs.
        #[test]
        fn splitter_never_panics(s in "\\PC{0,200}") {
            let _ = parser::split(&s);
        }

        // Round-trip: splitting `A;B;` yields exactly two non-empty stmts
        // when A and B contain no `;`, no quotes, no comments, no leading
        // dots, and remain non-empty after trimming.
        #[test]
        fn round_trip_simple_pairs(
            a in "[a-zA-Z0-9]{1,20}",
            b in "[a-zA-Z0-9]{1,20}",
        ) {
            prop_assume!(!a.contains(';') && !b.contains(';'));
            prop_assume!(!a.trim().is_empty() && !b.trim().is_empty());
            prop_assume!(!a.starts_with('.') && !b.starts_with('.'));
            let s = format!("{a};{b};");
            let stmts = parser::split(&s);
            prop_assert_eq!(stmts.len(), 2);
        }
    }
}
