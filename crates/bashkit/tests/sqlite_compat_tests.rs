//! sqlite3 compatibility / parity tests.
//!
//! These pin behaviour we want to keep in lockstep with the reference
//! `sqlite3` CLI (which most agents and humans already know), so that swapping
//! `sqlite3 db.sqlite` for `sqlite db.sqlite` is a no-op for typical
//! workflows.
//!
//! Scope:
//! - List mode default separator (`|`) and value rendering.
//! - CSV escaping per RFC 4180.
//! - `.headers on` enables a single header row before data.
//! - `.tables` returns table names sorted, one per line.
//! - `.dump` emits `BEGIN TRANSACTION;` / `COMMIT;` brackets.
//! - PRAGMA `user_version` round-trips.
//! - `ORDER BY` / `LIMIT` / `OFFSET` syntax accepted.
//!
//! These are NOT exhaustive sqlite parity tests — they pin the shapes that
//! drive day-to-day usage. Add a row to `specs/sqlite-builtin.md` if you
//! intentionally diverge from sqlite3 here.

#![cfg(feature = "sqlite")]

use bashkit::Bash;

fn make_bash() -> Bash {
    Bash::builder()
        .sqlite()
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
        .build()
}

#[tokio::test]
async fn list_mode_uses_pipe_separator() {
    let mut bash = make_bash();
    let r = bash
        .exec(r#"sqlite :memory: 'SELECT 1, "two", 3.5'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "1|two|3.5\n");
}

#[tokio::test]
async fn csv_mode_quotes_per_rfc4180() {
    let mut bash = make_bash();
    let r = bash
        .exec(r#"sqlite -csv :memory: 'SELECT "she said ""hi""" AS msg, "a,b" AS list'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "\"she said \"\"hi\"\"\",\"a,b\"");
}

#[tokio::test]
async fn header_flag_emits_one_header_row() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite -header :memory: '
            CREATE TABLE t(x, y);
            INSERT INTO t VALUES (1, "a"), (2, "b");
            SELECT * FROM t ORDER BY x'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "x|y\n1|a\n2|b\n");
}

#[tokio::test]
async fn dot_tables_returns_sorted_names() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite :memory: '
            CREATE TABLE zebra(a);
            CREATE TABLE alpha(b);
            CREATE TABLE mango(c);
            ' '
.tables'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    let lines: Vec<&str> = r.stdout.trim().lines().collect();
    assert_eq!(lines, vec!["alpha", "mango", "zebra"]);
}

#[tokio::test]
async fn dot_dump_brackets_with_begin_commit() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite :memory: 'CREATE TABLE t(a)' '
.dump'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    let trimmed = r.stdout.trim_end();
    assert!(trimmed.starts_with("PRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;"));
    assert!(trimmed.ends_with("COMMIT;"));
}

#[tokio::test]
async fn pragma_user_version_round_trips() {
    let mut bash = make_bash();
    let r = bash
        .exec(r#"sqlite /tmp/uv.sqlite 'PRAGMA user_version=42; PRAGMA user_version'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(r.stdout.contains("42"));
    let r = bash
        .exec(r#"sqlite /tmp/uv.sqlite 'PRAGMA user_version'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "42");
}

#[tokio::test]
async fn order_by_limit_offset_accepted() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite :memory: '
            CREATE TABLE t(x);
            INSERT INTO t VALUES (5), (1), (3), (4), (2);
            SELECT x FROM t ORDER BY x LIMIT 2 OFFSET 1'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "2\n3\n");
}

#[tokio::test]
async fn aggregate_functions_work() {
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite :memory: '
            CREATE TABLE t(x);
            INSERT INTO t VALUES (1), (2), (3);
            SELECT COUNT(*), SUM(x), AVG(x), MIN(x), MAX(x) FROM t'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    let parts: Vec<&str> = r.stdout.trim().split('|').collect();
    assert_eq!(parts[0], "3"); // COUNT
    assert_eq!(parts[1], "6"); // SUM
    // AVG(1,2,3) is 2.0 — turso renders integers as `2.0` for floats from AVG
    assert!(parts[2].starts_with('2'), "AVG was {:?}", parts[2]);
    assert_eq!(parts[3], "1"); // MIN
    assert_eq!(parts[4], "3"); // MAX
}
