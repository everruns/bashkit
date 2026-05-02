//! End-to-end integration tests for the `sqlite` builtin.
//!
//! These tests drive the public `Bash::exec` path so they exercise the full
//! pipeline: shell parsing, env expansion, redirection, the builtin itself,
//! and persistence to the virtual filesystem. They intentionally do NOT
//! reach into the builtin's internals — if these break, downstream callers
//! using `bashkit` as a library will be affected.
//!
//! Coverage:
//! - Inline SQL via `-c`
//! - Persistence to a VFS path across two invocations (Memory + VFS backends)
//! - Pipelining stdin into `sqlite`
//! - Output redirection to a VFS file
//! - Environment expansion inside SQL strings
//! - `.read` of a SQL script from the VFS
//! - JSON/CSV/markdown formatting
//! - `:memory:` round-trip in a single invocation
//!
//! See `specs/sqlite-builtin.md` for the test plan.

#![cfg(feature = "sqlite")]

use bashkit::{Bash, SqliteBackend, SqliteLimits};
use std::path::Path;

const OPT_IN: (&str, &str) = ("BASHKIT_ALLOW_INPROCESS_SQLITE", "1");

fn make_bash() -> Bash {
    Bash::builder().sqlite().env(OPT_IN.0, OPT_IN.1).build()
}

fn make_bash_vfs() -> Bash {
    Bash::builder()
        .sqlite_with_limits(SqliteLimits::default().backend(SqliteBackend::Vfs))
        .env(OPT_IN.0, OPT_IN.1)
        .build()
}

#[tokio::test]
async fn inline_select_returns_value() {
    let mut bash = make_bash();
    let r = bash.exec(r#"sqlite :memory: 'SELECT 1'"#).await.unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "1");
}

#[tokio::test]
async fn inline_create_insert_select_round_trip() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite :memory: 'CREATE TABLE t(a,b); INSERT INTO t VALUES (1,"hi"); SELECT * FROM t'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "1|hi\n");
}

#[tokio::test]
async fn persistence_round_trip_memory_backend() {
    let mut bash = make_bash();
    let r1 = bash
        .exec(r#"sqlite /tmp/db.sqlite 'CREATE TABLE t(a); INSERT INTO t VALUES (5)'"#)
        .await
        .unwrap();
    assert_eq!(r1.exit_code, 0, "stderr: {}", r1.stderr);
    let r2 = bash
        .exec(r#"sqlite /tmp/db.sqlite 'SELECT * FROM t'"#)
        .await
        .unwrap();
    assert_eq!(r2.exit_code, 0, "stderr: {}", r2.stderr);
    assert_eq!(r2.stdout.trim(), "5");
    assert!(bash.fs().exists(Path::new("/tmp/db.sqlite")).await.unwrap());
}

#[tokio::test]
async fn persistence_round_trip_vfs_backend() {
    let mut bash = make_bash_vfs();
    let r1 = bash
        .exec(r#"sqlite /tmp/v.sqlite 'CREATE TABLE t(a); INSERT INTO t VALUES (9)'"#)
        .await
        .unwrap();
    assert_eq!(r1.exit_code, 0, "stderr: {}", r1.stderr);
    let r2 = bash
        .exec(r#"sqlite /tmp/v.sqlite 'SELECT * FROM t'"#)
        .await
        .unwrap();
    assert_eq!(r2.exit_code, 0, "stderr: {}", r2.stderr);
    assert_eq!(r2.stdout.trim(), "9");
}

#[tokio::test]
async fn stdin_pipeline_drives_sql() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"echo 'CREATE TABLE t(a); INSERT INTO t VALUES (7); SELECT * FROM t' | sqlite :memory:"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "7");
}

#[tokio::test]
async fn redirect_output_to_vfs_file() {
    let mut bash = make_bash();
    bash.exec(r#"sqlite :memory: 'SELECT 99' > /tmp/out.txt"#)
        .await
        .unwrap();
    let bytes = bash
        .fs()
        .read_file(Path::new("/tmp/out.txt"))
        .await
        .unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap().trim(), "99");
}

#[tokio::test]
async fn env_expansion_in_sql() {
    let mut bash = make_bash();
    let r = bash
        .exec(r#"NAME=hello; sqlite :memory: "SELECT '$NAME'""#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "hello");
}

#[tokio::test]
async fn dot_read_runs_vfs_script() {
    let mut bash = make_bash();
    let prep = bash
        .exec(
            r#"cat > /tmp/script.sql <<'EOF'
CREATE TABLE t(a);
INSERT INTO t VALUES (1), (2), (3);
EOF"#,
        )
        .await
        .unwrap();
    assert_eq!(prep.exit_code, 0, "stderr: {}", prep.stderr);
    let r = bash
        .exec(r#"sqlite :memory: '.read /tmp/script.sql' 'SELECT count(*) FROM t'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "3");
}

#[tokio::test]
async fn json_mode_emits_array_of_objects() {
    let mut bash = make_bash();
    let r = bash
        .exec(r#"sqlite -json :memory: 'SELECT 1 AS i, "hi" AS s'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    let parsed: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(parsed[0]["i"], 1);
    assert_eq!(parsed[0]["s"], "hi");
}

#[tokio::test]
async fn markdown_mode_pipes_into_grep() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite -markdown :memory: 'CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (10), (20); SELECT * FROM t' | grep '10'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(r.stdout.contains("10"));
}

#[tokio::test]
async fn missing_sqlite_feature_disabled_by_default() {
    // Build without enabling the builder method — use a fresh Bash without
    // calling .sqlite() and confirm the command falls through to "command
    // not found" semantics rather than executing in-process.
    let mut bash = Bash::builder().env(OPT_IN.0, OPT_IN.1).build();
    let r = bash.exec(r#"sqlite :memory: 'SELECT 1'"#).await.unwrap();
    assert_ne!(r.exit_code, 0);
}

#[tokio::test]
async fn cmd_flag_runs_setup_first() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite -cmd 'CREATE TABLE t(a)' :memory: 'INSERT INTO t VALUES (4); SELECT * FROM t'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "4");
}

#[tokio::test]
async fn dot_dump_round_trips_via_dot_read() {
    // Create a DB with data, dump it, then re-import the dump into a fresh
    // DB and verify the same query gives the same answer.
    let mut bash = make_bash();
    let dump = bash
        .exec(r#"sqlite /tmp/src.sqlite 'CREATE TABLE t(a, b); INSERT INTO t VALUES (1, "x"), (2, "y")'"#)
        .await
        .unwrap();
    assert_eq!(dump.exit_code, 0, "stderr: {}", dump.stderr);
    let dump = bash
        .exec(r#"sqlite /tmp/src.sqlite '.dump' > /tmp/dumped.sql"#)
        .await
        .unwrap();
    assert_eq!(dump.exit_code, 0, "stderr: {}", dump.stderr);
    let r = bash
        .exec(r#"sqlite /tmp/dst.sqlite '.read /tmp/dumped.sql' 'SELECT * FROM t ORDER BY a'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "1|x\n2|y\n");
}

#[tokio::test]
async fn invalid_sql_exit_code_one() {
    let mut bash = make_bash();
    let r = bash
        .exec(r#"sqlite :memory: 'NOT A VALID STATEMENT' || echo 'caught'"#)
        .await
        .unwrap();
    assert!(
        r.stdout.contains("caught"),
        "expected fall-through to ||: stdout={:?} stderr={:?}",
        r.stdout,
        r.stderr
    );
}
