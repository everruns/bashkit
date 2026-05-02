//! Minimal example: open a database in the VFS, write a row, read it back.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example sqlite_basic --features sqlite
//! ```

use bashkit::Bash;

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let mut bash = Bash::builder()
        .sqlite()
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
        .build();

    // Create a table and insert a row in one invocation.
    let r = bash
        .exec(
            r#"
            sqlite /tmp/notes.sqlite "
              CREATE TABLE IF NOT EXISTS notes(id INTEGER PRIMARY KEY, body TEXT);
              INSERT INTO notes(body) VALUES ('hello world');
            "
            "#,
        )
        .await?;
    println!("write: exit={}, stderr={:?}", r.exit_code, r.stderr);

    // Read it back in a second invocation — proves persistence to the VFS.
    let r = bash
        .exec(r#"sqlite -header /tmp/notes.sqlite "SELECT id, body FROM notes ORDER BY id""#)
        .await?;
    print!("{}", r.stdout);
    Ok(())
}
