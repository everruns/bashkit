# Embedded SQLite (Turso)

Bashkit can embed a SQLite-compatible engine inside a sandbox by enabling
the `sqlite` feature. The engine is [Turso](https://github.com/tursodatabase/turso)
(`turso_core`), a pure-Rust rewrite of SQLite that exposes a pluggable
`IO` trait — letting us bind it to bashkit's virtual filesystem.

> ⚠️ Turso is **BETA** upstream. The feature is opt-in at the cargo
> level *and* at runtime via `BASHKIT_ALLOW_INPROCESS_SQLITE=1`. Enable it
> only for trusted scripts until upstream cuts a stable release.

## Quick start

```toml
# Cargo.toml
bashkit = { version = "0.2", features = ["sqlite"] }
```

```rust,ignore
use bashkit::Bash;

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let mut bash = Bash::builder()
        .sqlite()
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
        .build();

    let r = bash
        .exec(r#"sqlite /tmp/notes.sqlite '
            CREATE TABLE IF NOT EXISTS notes(id INTEGER PRIMARY KEY, body TEXT);
            INSERT INTO notes(body) VALUES (\"hello\");
        '"#)
        .await?;
    println!("write: {}", r.stderr);

    let r = bash.exec(r#"sqlite -header /tmp/notes.sqlite "SELECT * FROM notes""#).await?;
    print!("{}", r.stdout);
    Ok(())
}
```

## Two backends

`SqliteBackend::Memory` (default) loads the database file from the VFS into
turso's `MemoryIO`, runs the SQL, and flushes the resulting bytes back to
the VFS at command boundary. `SqliteBackend::Vfs` plugs the bashkit
`FileSystem` directly into turso via a custom `IO` impl; same observable
semantics, different code path.

```rust,ignore
use bashkit::{Bash, SqliteBackend, SqliteLimits};

let bash = Bash::builder()
    .sqlite_with_limits(
        SqliteLimits::default()
            .backend(SqliteBackend::Vfs)
            .max_db_bytes(8 * 1024 * 1024),
    )
    .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
    .build();
```

You can also flip the backend per-invocation with `-backend memory|vfs`.

## In-memory databases

Use `:memory:` as the database argument when you don't need persistence:

```bash
sqlite :memory: '
  WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM r WHERE n<10)
  SELECT sum(n) FROM r
'
```

## Output modes

| Flag         | Mode      | Notes                                  |
|--------------|-----------|----------------------------------------|
| (default)    | `list`    | `|`-separated, one row per line        |
| `-csv`       | `csv`     | RFC 4180 quoting                       |
| `-tabs`      | `tabs`    | Tab-separated                          |
| `-line`      | `line`    | `name = value` blocks                  |
| `-column`    | `column`  | Column-aligned with `--` separator     |
| `-box`       | `box`     | ASCII box-drawing borders              |
| `-json`      | `json`    | Array of objects via `serde_json`      |
| `-markdown`  | `markdown`| GitHub-flavoured table                 |

`.headers on` adds a header row; `-header` is its CLI equivalent.

## Dot-commands

```text
.help                       Show command list
.quit / .exit               Stop execution
.tables [PAT]               List tables (LIKE pattern)
.schema [PAT]               Print CREATE statements
.indexes [PAT]              List indexes
.headers on|off             Toggle column headers
.mode MODE                  Switch output mode
.separator SEP              Set field separator (escapes: \t \n \r \0)
.nullvalue STR              Set NULL placeholder
.dump                       Dump schema + data as SQL
.read PATH                  Execute SQL from a VFS file
```

Dot-commands must be on their own line. `.read` is bounded to 16 levels
of nesting to prevent stack overflow on self-referential scripts.

## Resource limits

`SqliteLimits` caps script size, result-set size, and database file size.
Defaults:

| Limit                  | Default     |
|------------------------|-------------|
| `max_script_bytes`     | 4 MiB       |
| `max_rows_per_query`   | 1,000,000   |
| `max_db_bytes`         | 256 MiB     |

These act as defence-in-depth against malicious or runaway SQL. Tune them
per workload via `SqliteLimits::default().max_*(...)`.

## Security

See `specs/sqlite-builtin.md` § "Trust Model & Threats" for the full
threat table. Highlights:

- **No host filesystem access.** All paths resolve through the bashkit
  VFS; even with `-backend vfs` and an absolute path like `/etc/passwd`,
  the engine reads from the VFS only.
- **Default-disabled at runtime.** Without
  `BASHKIT_ALLOW_INPROCESS_SQLITE=1`, the builtin refuses to execute.
- **No `ATTACH`/`DETACH` sandbox holes.** Cross-database access is left
  unsupported intentionally.
- **Bounded recursion in `.read`.**

## Compatibility with the `sqlite3` shell

The CLI surface is intentionally a subset of `sqlite3`'s. The pinned
parity tests live in `tests/sqlite_compat_tests.rs`. Notable differences:

- No interactive REPL.
- No `ATTACH`, `DETACH`, `.load`, `.eqp`, or `.fullschema`.
- Dot-commands must be on their own line (no inline `;` mixing).

See also:

- [`python_guide`](crate::python_guide) — the embedded Python builtin,
  same opt-in pattern.
- [`live_mounts_guide`](crate::live_mounts_guide) — mount points the
  sqlite builtin reads from / writes to.
- [`threat_model`](crate::threat_model) — overall security model.
