//! End-to-end example: VFS-backed SQLite workflow.
//!
//! Demonstrates:
//! - Creating a database in the bashkit VFS
//! - Persistence across multiple `bash.exec()` invocations
//! - Output-mode flags (`-csv`, `-json`, `-markdown`, `-header`)
//! - Dot-commands (`.tables`, `.schema`, `.dump`, `.read`)
//! - Pipelining sqlite output into other builtins
//! - Custom resource limits via `SqliteLimits`
//!
//! Designed to be runnable in CI:
//!
//! ```bash
//! cargo run --example sqlite_workflow --features sqlite
//! ```
//!
//! Each step asserts on its observed output so a regression breaks the
//! example, not just a smoke test.

use bashkit::{Bash, SqliteBackend, SqliteLimits};
use std::time::Duration;

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    // Tighter limits than defaults — a config DB shouldn't ever be huge.
    let limits = SqliteLimits::default()
        .max_db_bytes(8 * 1024 * 1024)
        .max_rows_per_query(10_000)
        .max_duration(Duration::from_secs(5))
        .max_statements(1_000)
        .backend(SqliteBackend::Memory);

    let mut bash = Bash::builder()
        .sqlite_with_limits(limits)
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
        .build();

    // Step 1: Create schema and insert seed data.
    let r = bash
        .exec(concat!(
            "sqlite /tmp/notes.sqlite '",
            "CREATE TABLE notes(id INTEGER PRIMARY KEY, title TEXT, body TEXT);",
            "INSERT INTO notes(title, body) VALUES",
            "  (\"todo\", \"finish sqlite docs\"),",
            "  (\"shopping\", \"milk, eggs, bread\"),",
            "  (\"reading\", \"https://docs.rs/turso_core\")",
            "'",
        ))
        .await?;
    assert_eq!(r.exit_code, 0, "step 1 failed: {}", r.stderr);
    println!("step 1: seeded {} rows", 3);

    // Step 2: Read it back through a fresh connection — proves persistence
    // through the VFS at command boundary.
    let r = bash
        .exec("sqlite -header /tmp/notes.sqlite 'SELECT id, title FROM notes ORDER BY id'")
        .await?;
    assert_eq!(r.exit_code, 0, "step 2 failed: {}", r.stderr);
    println!("step 2: read-back\n{}", r.stdout);

    // Step 3: JSON for downstream tools (jq, etc.).
    let r = bash
        .exec(
            "sqlite -json /tmp/notes.sqlite 'SELECT title, body FROM notes WHERE id <= 2 ORDER BY id'",
        )
        .await?;
    assert_eq!(r.exit_code, 0, "step 3 failed: {}", r.stderr);
    let parsed: serde_json::Value = serde_json::from_str(r.stdout.trim()).expect("valid json");
    assert_eq!(parsed[0]["title"], "todo");
    assert_eq!(parsed[1]["title"], "shopping");
    println!("step 3: json mode parsed cleanly");

    // Step 4: CSV pipelined into `wc -l`.
    let r = bash
        .exec("sqlite -csv /tmp/notes.sqlite 'SELECT * FROM notes' | wc -l")
        .await?;
    assert_eq!(r.exit_code, 0, "step 4 failed: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "3");
    println!("step 4: csv | wc -l → 3");

    // Step 5: Dump and re-import into a second DB (round-trip).
    let r = bash
        .exec("sqlite /tmp/notes.sqlite '.dump' > /tmp/dump.sql")
        .await?;
    assert_eq!(r.exit_code, 0, "step 5 dump failed: {}", r.stderr);
    let r = bash
        .exec("sqlite /tmp/notes.copy.sqlite '.read /tmp/dump.sql' 'SELECT count(*) FROM notes'")
        .await?;
    assert_eq!(r.exit_code, 0, "step 5 reimport failed: {}", r.stderr);
    assert_eq!(r.stdout.trim(), "3");
    println!("step 5: .dump → .read round-trip preserved 3 rows");

    // Step 6: Markdown for human-friendly reporting.
    let r = bash
        .exec("sqlite -markdown -header /tmp/notes.sqlite 'SELECT title FROM notes ORDER BY id'")
        .await?;
    assert_eq!(r.exit_code, 0, "step 6 failed: {}", r.stderr);
    assert!(r.stdout.contains("| title"));
    assert!(r.stdout.contains("---"));
    println!("step 6: markdown table\n{}", r.stdout);

    // Step 7: Schema introspection via .tables / .schema.
    let r = bash.exec("sqlite /tmp/notes.sqlite '.tables'").await?;
    assert_eq!(r.stdout.trim(), "notes");
    let r = bash
        .exec("sqlite /tmp/notes.sqlite '.schema notes'")
        .await?;
    assert!(r.stdout.contains("CREATE TABLE notes"));
    println!("step 7: .tables / .schema OK");

    // Step 8: Demonstrate the wall-clock cap kicking in. We use an
    // intentionally cheap query that completes inside the 5s budget; the
    // negative path (timeout) is covered by tests, not the example.
    let r = bash
        .exec("sqlite :memory: 'SELECT count(*) FROM (VALUES (1),(2),(3),(4),(5))'")
        .await?;
    assert_eq!(r.stdout.trim(), "5");
    println!("step 8: limits in place, simple aggregate ran fine");

    println!("\nAll 8 steps OK.");
    Ok(())
}
