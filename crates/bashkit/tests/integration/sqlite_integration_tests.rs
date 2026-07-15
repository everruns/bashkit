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
async fn cached_engine_keeps_in_flight_transaction_across_exec_calls() {
    // Without the engine cache, every `sqlite` invocation opens a fresh
    // connection from the VFS bytes — so a `BEGIN` in one call would be
    // dropped before the next call's `COMMIT` could see it. With the
    // cache, the connection persists for the lifetime of the `Bash`
    // instance, and transactions span shell commands.
    let mut bash = make_bash();
    let r = bash
        .exec(
            r#"sqlite /tmp/tx.sqlite 'CREATE TABLE t(a INTEGER); BEGIN; INSERT INTO t VALUES (1)'"#,
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);

    // Mid-transaction: the row is visible to the same connection but not
    // yet committed. A follow-up call on the SAME `Bash` reuses the same
    // engine, so it sees the uncommitted row.
    let mid = bash
        .exec(r#"sqlite /tmp/tx.sqlite 'SELECT count(*) FROM t'"#)
        .await
        .unwrap();
    assert_eq!(mid.exit_code, 0, "stderr: {}", mid.stderr);
    assert_eq!(mid.stdout.trim(), "1");

    // Commit and verify with a final query.
    let commit = bash
        .exec(r#"sqlite /tmp/tx.sqlite 'COMMIT; SELECT count(*) FROM t'"#)
        .await
        .unwrap();
    assert_eq!(commit.exit_code, 0, "stderr: {}", commit.stderr);
    assert_eq!(commit.stdout.trim(), "1");
}

#[tokio::test]
async fn cached_engine_isolated_per_database_path() {
    // Two different DB paths must not collide in the cache. Schema
    // changes to `/tmp/a.sqlite` are invisible from `/tmp/b.sqlite`.
    let mut bash = make_bash();
    bash.exec(r#"sqlite /tmp/a.sqlite 'CREATE TABLE only_in_a(x)'"#)
        .await
        .unwrap();
    let r = bash
        .exec(r#"sqlite /tmp/b.sqlite '.tables'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(
        !r.stdout.contains("only_in_a"),
        "schema leaked across cache keys: {:?}",
        r.stdout
    );
}

#[tokio::test]
async fn memory_target_does_not_persist_across_exec_calls() {
    // `:memory:` databases are deliberately ephemeral — the cache only
    // holds file-backed engines. A schema created in one call must NOT
    // be visible to the next.
    let mut bash = make_bash();
    bash.exec(r#"sqlite :memory: 'CREATE TABLE ephemeral(x)'"#)
        .await
        .unwrap();
    let r = bash.exec(r#"sqlite :memory: '.tables'"#).await.unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(
        !r.stdout.contains("ephemeral"),
        ":memory: leaked schema between invocations: {:?}",
        r.stdout
    );
}

#[tokio::test]
async fn snapshot_and_restore_round_trips_sqlite_state() {
    // Prove the snapshot/restore story end-to-end: write a row in one
    // Bash, snapshot, drop it, restore in a fresh Bash, and read the
    // row back. The flush at command boundary keeps the on-disk image
    // current, so the snapshot picks up everything.
    let mut bash = make_bash();
    bash.exec(r#"sqlite /tmp/snap.sqlite 'CREATE TABLE t(v); INSERT INTO t VALUES ("preserved")'"#)
        .await
        .unwrap();
    let snap = bash.snapshot().expect("snapshot");

    // Brand new Bash, restored from the snapshot. The engine cache is
    // empty, so the first `sqlite` call re-opens from the VFS — and
    // sees the row because the previous Bash flushed before snapshot.
    let mut restored = make_bash();
    restored.restore_snapshot(&snap).expect("restore");
    let r = restored
        .exec(r#"sqlite /tmp/snap.sqlite 'SELECT v FROM t'"#)
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "preserved");
}

async fn assert_restore_into_existing_bash_clears_sqlite_cache(mut bash: Bash) {
    let clean = Bash::builder().sqlite().env(OPT_IN.0, OPT_IN.1).build();
    let clean_snap = clean.snapshot().expect("clean snapshot");

    let setup = bash
        .exec(r#"sqlite /tmp/leak.sqlite 'CREATE TABLE t(v); INSERT INTO t VALUES ("old-secret")'"#)
        .await
        .unwrap();
    assert_eq!(setup.exit_code, 0, "stderr: {}", setup.stderr);

    bash.restore_snapshot(&clean_snap)
        .expect("restore clean snapshot");

    let query = bash
        .exec(r#"sqlite /tmp/leak.sqlite 'SELECT v FROM t'"#)
        .await
        .unwrap();
    assert_ne!(
        query.exit_code, 0,
        "stale sqlite cache survived restore: stdout={} stderr={}",
        query.stdout, query.stderr
    );
    assert!(
        !query.stdout.contains("old-secret"),
        "restored Bash leaked stale sqlite row: {}",
        query.stdout
    );
}

#[tokio::test]
async fn snapshot_restore_into_existing_bash_clears_sqlite_cache_memory_backend() {
    assert_restore_into_existing_bash_clears_sqlite_cache(make_bash()).await;
}

#[tokio::test]
async fn snapshot_restore_into_existing_bash_clears_sqlite_cache_vfs_backend() {
    assert_restore_into_existing_bash_clears_sqlite_cache(make_bash_vfs()).await;
}

#[tokio::test]
async fn cached_engine_respects_deleted_db_between_exec_calls() {
    // Security invariant: once the backing file is deleted between calls, the
    // cached engine must not keep serving the deleted database's rows. On
    // deletion the builtin re-opens from the current VFS bytes; with the file
    // gone that yields a fresh, empty database — matching real `sqlite3`, which
    // creates a new database when the path does not exist. The property under
    // test is therefore "the old schema/data is no longer visible", NOT "the
    // query errors": turso 0.7 opens a missing file as an empty DB rather than
    // failing (0.6 errored), and either way no stale rows leak through.
    let mut bash = make_bash();
    let setup = bash
        .exec(r#"sqlite /tmp/deleted.sqlite 'CREATE TABLE t(v); INSERT INTO t VALUES ("secret")'"#)
        .await
        .unwrap();
    assert_eq!(setup.exit_code, 0, "stderr: {}", setup.stderr);

    let rm = bash.exec(r#"rm -f /tmp/deleted.sqlite"#).await.unwrap();
    assert_eq!(rm.exit_code, 0, "stderr: {}", rm.stderr);

    // A stale cached engine would still report the `t` table (count == 1); the
    // re-opened empty database reports an empty schema (count == 0).
    let schema = bash
        .exec(r#"sqlite /tmp/deleted.sqlite 'SELECT count(*) FROM sqlite_master'"#)
        .await
        .unwrap();
    assert_eq!(schema.exit_code, 0, "stderr: {}", schema.stderr);
    assert_eq!(
        schema.stdout.trim(),
        "0",
        "deleted db must re-open empty, not serve stale schema"
    );

    // The deleted table's rows must not be served: querying it fails with
    // "no such table" and the secret value never appears.
    let rows = bash
        .exec(r#"sqlite /tmp/deleted.sqlite 'SELECT v FROM t'"#)
        .await
        .unwrap();
    assert_ne!(
        rows.exit_code, 0,
        "querying the deleted table must fail; stdout: {}",
        rows.stdout
    );
    assert!(
        !rows.stdout.contains("secret"),
        "deleted row leaked: {}",
        rows.stdout
    );
}

#[tokio::test]
async fn cached_engine_respects_replaced_db_between_exec_calls() {
    let mut bash = make_bash();
    let setup = bash
        .exec(r#"sqlite /tmp/replaced.sqlite 'CREATE TABLE t(v); INSERT INTO t VALUES ("old")'"#)
        .await
        .unwrap();
    assert_eq!(setup.exit_code, 0, "stderr: {}", setup.stderr);

    let overwrite = bash
        .exec(
            r#"sqlite /tmp/new.sqlite 'CREATE TABLE t(v); INSERT INTO t VALUES ("new")'; cp /tmp/new.sqlite /tmp/replaced.sqlite"#,
        )
        .await
        .unwrap();
    assert_eq!(overwrite.exit_code, 0, "stderr: {}", overwrite.stderr);

    let query = bash
        .exec(r#"sqlite /tmp/replaced.sqlite 'SELECT v FROM t'"#)
        .await
        .unwrap();
    assert_eq!(query.exit_code, 0, "stderr: {}", query.stderr);
    assert_eq!(query.stdout.trim(), "new");
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
