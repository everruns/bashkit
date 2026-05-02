# `sqlite` Builtin

> **Experimental.** Backed by [Turso](https://github.com/tursodatabase/turso)
> (`turso_core`), a pure-Rust SQLite-compatible engine that is **BETA**
> upstream. Treat it as you would `python` — sandbox-safe by design but
> not yet hardened for arbitrary untrusted SQL workloads.

## Status
Implemented (experimental).

## Decision

Bashkit ships a `sqlite` (alias `sqlite3`) builtin behind the `sqlite`
cargo feature. It executes SQL against a database stored in the bashkit
virtual filesystem (or `:memory:`), formatted in any of the standard
sqlite3 shell modes (`list`, `csv`, `tabs`, `line`, `box`, `column`,
`json`, `markdown`).

```bash
sqlite db.sqlite "SELECT * FROM users WHERE id = 1"
sqlite -csv -header db.sqlite "SELECT * FROM users" > /tmp/users.csv
sqlite :memory: <<'SQL'
  CREATE TABLE t(a, b);
  INSERT INTO t VALUES (1, 'x');
  SELECT * FROM t;
SQL
sqlite db.sqlite '.tables' '.schema users' '.dump'
```

### Why Turso

- Pure Rust — no `libsqlite3-sys` C dependency, no toolchain coupling.
- Familiar API — Database / Connection / Statement, broadly SQLite-compatible.
- Plug-in IO via the `IO` / `File` traits, so we can choose between
  in-memory or VFS-backed storage without forking the engine.
- MIT licensed.

Risks acknowledged: BETA stability, larger transitive dep tree (~30 crates),
no public guarantee of file-format stability across turso releases. The
feature is opt-in at the cargo level **and** at runtime via
`BASHKIT_ALLOW_INPROCESS_SQLITE`.

## Architecture

```
┌────────────────────────────────────┐
│ Sqlite (Builtin trait impl)        │  args parsing + opt-in gate
├────────────────────────────────────┤
│ parser ── splits SQL & dot-cmds    │  ;-aware, comment/string-aware
├────────────────────────────────────┤
│ dot_commands ── .tables/.dump/...  │  curated subset of sqlite3 shell
├────────────────────────────────────┤
│ engine::SqliteEngine               │  thin wrapper around turso
│ ├── Backend::Memory(MemoryIO)      │  Phase 1: load/flush whole file
│ └── Backend::Vfs(BashkitVfsIO)     │  Phase 2: turso talks to VFS
├────────────────────────────────────┤
│ formatter ── render(cols, rows)    │  list/csv/tabs/line/box/column
│                                    │  /json/markdown
└────────────────────────────────────┘
```

### Phase 1 — `Backend::Memory` (default)

1. On invocation, read the entire DB file from the VFS into memory.
2. Hand the bytes to turso via a fresh `MemoryIO`-backed `Database`.
3. Run all SQL + dot-commands in sequence.
4. After the script finishes (success or error), checkpoint the WAL,
   read the file bytes back out of `MemoryIO`, and write them to the VFS.

Pros: simple, isolates BETA risk to the in-memory engine, matches the
"command-as-transaction-boundary" mental model of the shell.

Cons: each invocation loads + saves the entire file. Practical for the
DBs bashkit users actually care about (KB to single-digit MB); the
`SqliteLimits::max_db_bytes` cap (256 MB default) keeps it predictable.

### Phase 2 — `Backend::Vfs`

`vfs_io::BashkitVfsIO` implements `turso_core::IO`. It holds a
`HashMap<String, Arc<VfsFile>>` of open files. On `open_file`:

1. Read the bytes from `Arc<dyn FileSystem>` (bridged to async via a
   short-lived OS thread + tokio `Handle::block_on`, so the call works
   from any tokio runtime flavour).
2. Wrap the bytes in a `Mutex<Vec<u8>>` and an `AtomicBool` dirty flag.
3. Subsequent `pread`/`pwrite`/`size`/`truncate` operate purely in memory.

After SQL execution finishes, the builtin calls `flush_dirty()`, which
writes any modified buffers back to the VFS via `FileSystem::write_file`.

Same observable semantics as Phase 1, but exercises the IO trait path
end-to-end. Use `-backend vfs` (per-invocation) or
`SqliteLimits::backend(SqliteBackend::Vfs)` (per-builder) to select it.
A backend equivalence test (`run_both_match`) runs the positive cases on
both backends and asserts identical output.

### `:memory:` databases

`:memory:` is detected ahead of any backend selection and uses
`SqliteEngine::open_pure_memory()` (no file backing, no persistence).
This is the common case for ad-hoc CTE / scratch queries.

### Dot-commands

Curated subset of the sqlite3 shell:

| Command         | Behaviour                                              |
|-----------------|--------------------------------------------------------|
| `.help`         | List supported commands.                               |
| `.quit`/`.exit` | Stop the script (subsequent stmts are not executed).   |
| `.tables [PAT]` | List tables, optionally filtered by `LIKE PAT`.        |
| `.schema [PAT]` | Print `CREATE …` statements.                           |
| `.indexes [PAT]`| List indexes.                                          |
| `.headers on|off` | Toggle column headers.                               |
| `.mode MODE`    | Switch output mode.                                    |
| `.separator S`  | Set separator (escapes: `\t`, `\n`, `\r`, `\0`, `\\`).|
| `.nullvalue S`  | Set NULL placeholder.                                  |
| `.dump`         | Emit schema + data as SQL INSERTs.                     |
| `.read PATH`    | Execute a SQL script from the VFS.                     |

Anything else returns an `unknown dot-command: .xyz` error. Dot-commands
must appear at the **start of a line**; they are not tokens that can be
mixed mid-statement with `;`.

### Recursion guard

`.read` is bounded by `MAX_DOT_READ_DEPTH` (16) to prevent stack overflow
on self-referential scripts. Tested via `tm_sql_008`.

### Output formatting

`formatter::render(cols, rows, opts) -> String`. Rules:

- Empty column list → empty string (CREATE / INSERT / UPDATE / DELETE).
- Empty row set → empty string in row-oriented modes; `[]\n` in `json`.
- `list` mode default separator is `|`; `csv` flips it to `,`; `tabs` to `\t`.
- `csv` quotes per RFC 4180 (separator, `"`, `\r`, `\n` trigger quoting).
- `json` uses `serde_json` for keys/strings; numbers unquoted; NULL → `null`;
  blobs → lowercase hex string.
- `markdown` emits a `|---|---|` separator row.

## Trust Model & Threats

| ID            | Threat                                              | Mitigation                                                              |
|---------------|-----------------------------------------------------|-------------------------------------------------------------------------|
| TM-SQL-001    | Code execution via BETA upstream                    | Off by default (cargo feature) + runtime opt-in env var                 |
| TM-SQL-002    | Sandbox escape via host filesystem                  | All paths resolve through `Arc<dyn FileSystem>`; Phase 2 IO is bound to that FS only |
| TM-SQL-003    | DoS via large SQL input                              | `SqliteLimits::max_script_bytes` (4 MiB default)                        |
| TM-SQL-004    | DoS via huge result set                              | `SqliteLimits::max_rows_per_query` (1M default)                         |
| TM-SQL-005    | DoS via huge DB file                                 | `SqliteLimits::max_db_bytes` (256 MiB default) at load time             |
| TM-SQL-005a   | DoS via wall-clock burn (regex-style queries, CTEs)  | `SqliteLimits::max_duration` enforced via per-step deadline + `Statement::interrupt()` |
| TM-SQL-005b   | DoS via statement-flood (millions of `;`)            | `SqliteLimits::max_statements` checked after splitting                  |
| TM-SQL-006    | Binary corruption / truncation in BLOB round-trip    | Backed by `Vec<u8>`; tested via `tm_sql_006`                            |
| TM-SQL-007    | CSV escape failure with separator-bearing blobs      | Per-RFC-4180 quoting; tested via `tm_sql_007`                           |
| TM-SQL-008    | Stack overflow via recursive `.read`                 | `MAX_DOT_READ_DEPTH` cap; tested via `tm_sql_008`                       |
| TM-SQL-009    | Information leakage via host-side error strings      | `sanitize()` strips ` at /…:N:M` annotations from turso errors          |

## Test Plan

Coverage lives in four layers (all cited tests are real):

- **Unit** — `crates/bashkit/src/builtins/sqlite/{tests.rs,…}`. 74 cases
  covering positive flow, every flag, every dot-command, every output
  mode, opt-in gate, recursion cap, oversize input, and proptest harness
  for the SQL splitter.
- **Integration** — `crates/bashkit/tests/sqlite_integration_tests.rs`.
  14 cases driving `Bash::exec` end-to-end (pipelines, redirection, env
  expansion, `.read` of a heredoc-built VFS file, `.dump`/`.read` round
  trip, both backends).
- **Security** — `crates/bashkit/tests/sqlite_security_tests.rs`. 9 cases,
  one per TM row above.
- **Compatibility** — `crates/bashkit/tests/sqlite_compat_tests.rs`. 8
  parity checks against the sqlite3 shell (separator, CSV escaping,
  `.tables` ordering, `.dump` brackets, PRAGMA round-trip, ORDER/LIMIT,
  aggregates).
- **Fuzz / property** — `crates/bashkit/tests/sqlite_fuzz_tests.rs`.
  4 proptest harnesses (no-panic on arbitrary SQL, no host file leak via
  random paths, CSV well-formedness, no `:memory:` artifacts on the VFS).

Run everything:

```bash
cargo test --features sqlite -p bashkit
```

## Public API

```rust
// Cargo.toml
bashkit = { version = "0.2", features = ["sqlite"] }

// Builder
let bash = Bash::builder()
    .sqlite()                                  // default limits, Memory backend
    .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
    .build();

// Custom limits / backend selection
use std::time::Duration;
let bash = Bash::builder()
    .sqlite_with_limits(
        SqliteLimits::default()
            .backend(SqliteBackend::Vfs)
            .max_db_bytes(8 * 1024 * 1024)
            .max_rows_per_query(10_000)
            .max_duration(Duration::from_secs(5))
            .max_statements(1_000),
    )
    .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
    .build();

bash.exec(r#"sqlite /tmp/cache.sqlite "SELECT * FROM cache""#).await?;
```

### CLI

The `bashkit` CLI ships `sqlite` enabled by default (matching `python` and
`git`). The runtime opt-in env var is auto-injected by `configure_bash`:

```bash
bashkit -c "sqlite :memory: 'SELECT 1'"           # works out of the box
bashkit --no-sqlite -c "sqlite :memory: 'SELECT 1'"
# → sqlite: command not found
```

LLM hint (auto-injected when `sqlite` is registered):

> sqlite/sqlite3: Embedded SQLite-compatible engine (Turso, BETA). Usage:
> `sqlite DB SQL...` | `sqlite DB <script` | `sqlite -separator , -header
> DB SELECT`. Dot-commands: `.tables .schema .dump .headers .mode .separator
> .nullvalue .read .help`. Supports `:memory:`. No `ATTACH`/`DETACH`. Set
> `BASHKIT_ALLOW_INPROCESS_SQLITE=1` to enable.

## Verification

```bash
# Build with sqlite feature
cargo build --features sqlite

# All sqlite tests
cargo test --features sqlite -p bashkit -- sqlite

# Targeted layers
cargo test --features sqlite -p bashkit --lib                       # unit
cargo test --features sqlite -p bashkit --test sqlite_integration_tests
cargo test --features sqlite -p bashkit --test sqlite_security_tests
cargo test --features sqlite -p bashkit --test sqlite_compat_tests
cargo test --features sqlite -p bashkit --test sqlite_fuzz_tests
```

Higher-cycle fuzzing:

```bash
PROPTEST_CASES=2000 cargo test --features sqlite -p bashkit --test sqlite_fuzz_tests
```

## Future Work (deferred)

- ATTACH / DETACH support (currently unsupported; isolation simpler without).
- Connection pooling across consecutive `sqlite` invocations within the
  same `Bash` so transactions can span commands.
- Page-streaming Phase 3 backend that uses real positional reads against
  the VFS rather than a whole-file load. Requires a `FsBackend::pread`
  extension.
- Encryption: turso supports it but we expose no key management story yet.
- Track upstream turso releases; remove the BETA caveat once they cut a
  1.0.

## Alternatives Considered

| Option                              | Why rejected                                              |
|-------------------------------------|-----------------------------------------------------------|
| `rusqlite` + `sqlite-vfs` shim      | C dep (`libsqlite3-sys`) breaks the pure-Rust posture.    |
| `libsql` (Turso's SQLite fork)      | Still C-based; upstream is steering toward the Rust rewrite. |
| Whole-file shim only (no Phase 2)   | Acceptable, but exercising the IO trait flushes out integration bugs. Both phases coexist with negligible extra surface. |
| In-process REPL mode                | Out of scope for a non-interactive shell builtin.         |
