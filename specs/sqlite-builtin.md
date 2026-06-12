# `sqlite` Builtin

> **Experimental.** Backed by [Turso](https://github.com/tursodatabase/turso)
> (`turso_core`), a pure-Rust SQLite-compatible engine that is **BETA**
> upstream. Treat it as you would `python` — sandbox-safe by design but
> not yet hardened for arbitrary untrusted SQL workloads.

## Status
Implemented (experimental).

## Decision

Bashkit ships a `sqlite` (alias `sqlite3`) builtin behind the `sqlite` cargo
feature. It executes SQL against a database stored in the bashkit VFS (or
`:memory:`), formatted in any of the standard sqlite3 shell modes.

```bash
sqlite db.sqlite "SELECT * FROM users WHERE id = 1"
sqlite -csv -header db.sqlite "SELECT * FROM users" > /tmp/users.csv
sqlite db.sqlite '.tables' '.schema users' '.dump'
```

### Why Turso

- Pure Rust — no `libsqlite3-sys` C dependency, no toolchain coupling.
- Familiar API — Database / Connection / Statement, broadly SQLite-compatible.
- Plug-in IO via the `IO` / `File` traits, so we choose in-memory or
  VFS-backed storage without forking the engine.
- MIT licensed.

Risks acknowledged: BETA stability, larger transitive dep tree (~30 crates),
no public guarantee of file-format stability across turso releases. Feature
is opt-in at the cargo level **and** at runtime via
`BASHKIT_ALLOW_INPROCESS_SQLITE`.

## Architecture

Layers: `Sqlite` (Builtin impl, args + opt-in gate) → `parser` (;-aware,
comment/string-aware split of SQL & dot-commands) → `dot_commands` →
`engine::SqliteEngine` (thin turso wrapper with `Backend::Memory(MemoryIO)` /
`Backend::Vfs(BashkitVfsIO)`) → `formatter::render`.

### Phase 1 — `Backend::Memory` (default)

Read entire DB file from VFS into memory → fresh `MemoryIO`-backed turso
`Database` → run all SQL + dot-commands → checkpoint WAL, write bytes back
to VFS (on success or error).

Pros: simple, isolates BETA risk to the in-memory engine, matches the
"command-as-transaction-boundary" shell mental model. Cons: loads + saves the
whole file per invocation — practical for the KB–MB DBs bashkit users care
about; `SqliteLimits::max_db_bytes` (256 MB default) keeps it predictable.

### Phase 2 — `Backend::Vfs`

`vfs_io::BashkitVfsIO` implements `turso_core::IO`, holding a
`HashMap<String, Arc<VfsFile>>` of open files. `open_file` reads bytes from
`Arc<dyn FileSystem>` (bridged to async via a short-lived OS thread + tokio
`Handle::block_on`, so it works from any runtime flavour), wraps them in
`Mutex<Vec<u8>>` + `AtomicBool` dirty flag; `pread`/`pwrite`/`size`/`truncate`
operate purely in memory. After execution, `flush_dirty()` writes modified
buffers back via `FileSystem::write_file`.

Same observable semantics as Phase 1, but exercises the IO trait path
end-to-end. Select via `-backend vfs` (per-invocation) or
`SqliteLimits::backend(SqliteBackend::Vfs)` (per-builder). Backend
equivalence test `run_both_match` asserts identical output on both backends.

### `:memory:` databases

Detected ahead of backend selection; uses `SqliteEngine::open_pure_memory()`
(no file backing, no persistence). Common case for ad-hoc CTE / scratch
queries.

### Dot-commands

Standard sqlite3 dot-command subset — run `.help` for the list (`.tables`,
`.schema`, `.indexes`, `.headers`, `.mode`, `.separator`, `.nullvalue`,
`.dump`, `.read`, `.quit`/`.exit`). Bashkit-specific deviations:

- Unknown commands return `unknown dot-command: .xyz`.
- Dot-commands must appear at the **start of a line**; they cannot be mixed
  mid-statement with `;`.
- `.read PATH` executes a SQL script from the VFS.
- `.separator` supports escapes `\t`, `\n`, `\r`, `\0`, `\\`.

### Recursion guard

`.read` is bounded by `MAX_DOT_READ_DEPTH` (16) to prevent stack overflow on
self-referential scripts. Tested via `tm_sql_008`.

### Session-scoped engine cache

The `Sqlite` builtin holds an `Arc<Mutex<HashMap<(backend, path),
Arc<TokioMutex<Option<SqliteEngine>>>>>>` keyed by `(SqliteBackend, PathBuf)`.
First call against a file-backed path opens the engine and caches it; later
calls lock the per-key `TokioMutex` and reuse the connection. Concurrent
calls to the same DB serialise through that mutex; different DBs/backends run
independently.

Consequences:

- **Transactions span shell commands.** `BEGIN` in one
  `bash.exec("sqlite DB ...")` and `COMMIT` in the next work — the connection
  lives between calls. Tested by
  `cached_engine_keeps_in_flight_transaction_across_exec_calls`.
- **`:memory:` is intentionally NOT cached** — fresh ephemeral engine per
  invocation; use a VFS path for persistence within one `Bash` lifecycle.
  Tested by `memory_target_does_not_persist_across_exec_calls`.
- **Per-call flush is preserved.** After every successful or failing call,
  the builtin snapshots/flushes to the VFS, so snapshots between exec calls
  always pick up the latest committed state. Tested by
  `snapshot_and_restore_round_trips_sqlite_state`.
- **Lifecycle.** Cache drops with the owning `Bash`; each
  `Bash::builder().sqlite()` produces its own cache, so parallel `Bash`
  instances do not cross-contaminate.
- **Snapshot restore invalidates caches.** `Bash::restore_snapshot()` clears
  sqlite engine caches after restoring the VFS, so cached connections cannot
  leak rows from the previous VFS image or flush stale bytes over restored
  files. Tested by
  `snapshot_restore_into_existing_bash_clears_sqlite_cache_memory_backend` /
  `..._vfs_backend`.

Snapshot integration is automatic: per-exec flush keeps the in-VFS image
current at every legitimate `bash.snapshot()` point; restore into a fresh
`Bash` starts with an empty cache; the first `sqlite` call after restore
re-opens from the restored VFS bytes.

### SQL policy: ATTACH / DETACH / VACUUM and PRAGMA deny list

Before each statement reaches turso, `check_sql_policy()` inspects the
leading SQL keyword via the parser's lightweight tokeniser
(`leading_keyword`, comment- and case-aware):

- `ATTACH` and `DETACH` are unconditionally rejected. Cross-database access
  bypasses VFS isolation: ATTACH would let scripts open VFS paths the
  operator never staged, and on the VFS backend it would build new
  `MemoryIO`/`VfsIO` state outside our `:memory:bashkit-N` registry
  isolation.
- `VACUUM` (with or without `INTO`) is unconditionally rejected. Turso's
  `VACUUM INTO` opens the destination via `PlatformIO`, writing to the host
  filesystem rather than the configured `MemoryIO`/`BashkitVfsIO` — a
  sandbox escape. Plain `VACUUM` is denied for symmetry; there is no
  sandbox-safe way to express it today.
- `PRAGMA <name>` is checked against `SqliteLimits::pragma_deny`
  (case-insensitive, schema-prefix-aware so `PRAGMA main.cache_size`
  matches). Defaults block resource/FS-shaped knobs: `cache_size`,
  `mmap_size`, `page_size`, `max_page_count`, `temp_store_directory`,
  `data_store_directory`, `compile_options`, `locking_mode`, `shared_cache`.
  Pass `pragma_deny([])` to opt out or supply a custom set. Operational
  PRAGMAs (`user_version`, `wal_checkpoint`, `foreign_keys`, `journal_mode`)
  are intentionally **not** denied.

### Output formatting

`formatter::render(cols, rows, opts) -> String`. Modes: `list` (default,
`|` separator), `csv` (`,`, RFC 4180 quoting), `tabs`, `line`, `box`,
`column`, `json` (serde_json; NULL → `null`, blobs → lowercase hex, empty
rows → `[]\n`), `markdown`. Empty column list → empty string; empty row set
→ empty string in row-oriented modes.

## Trust Model & Threats

| ID            | Threat                                              | Mitigation                                                              |
|---------------|-----------------------------------------------------|-------------------------------------------------------------------------|
| TM-SQL-001    | Code execution via BETA upstream                    | Off by default (cargo feature) + runtime opt-in env var                 |
| TM-SQL-002    | Sandbox escape via host filesystem                  | All paths resolve through `Arc<dyn FileSystem>`; Phase 2 IO is bound to that FS only |
| TM-SQL-003    | DoS via large SQL input                              | `SqliteLimits::max_script_bytes` (4 MiB default)                        |
| TM-SQL-004    | DoS via huge result set                              | `SqliteLimits::max_rows_per_query` (1M default), checked before materialising each row |
| TM-SQL-005    | DoS via huge DB file                                 | `SqliteLimits::max_db_bytes` (256 MiB default) at load time and while growing DBs |
| TM-SQL-005a   | DoS via wall-clock burn (regex-style queries, CTEs)  | `SqliteLimits::max_duration` enforced via per-step deadline + `Statement::interrupt()` |
| TM-SQL-005b   | DoS via statement-flood (millions of `;`)            | `SqliteLimits::max_statements` checked after splitting                  |
| TM-SQL-006    | Binary corruption / truncation in BLOB round-trip    | Backed by `Vec<u8>`; tested via `tm_sql_006`                            |
| TM-SQL-007    | CSV escape failure with separator-bearing blobs      | Per-RFC-4180 quoting; tested via `tm_sql_007`                           |
| TM-SQL-008    | Stack overflow via recursive `.read`                 | `MAX_DOT_READ_DEPTH` cap; tested via `tm_sql_008`                       |
| TM-SQL-009    | Cross-database access via `ATTACH`/`DETACH`          | Policy rejects both keywords (case-insensitive, comment-aware); tested via `tm_sql_009` |
| TM-SQL-010    | DoS / fingerprinting via dangerous PRAGMAs           | `SqliteLimits::pragma_deny` defaults (see SQL policy above); parser handles comments plus quoted/schema-qualified names |
| TM-SQL-011    | Information leakage via host-side error strings      | `sanitize()` strips ` at /…:N:M` annotations from turso errors          |
| TM-SQL-012    | Sandbox escape via `VACUUM INTO` writing host files  | Policy rejects `VACUUM` (with/without `INTO`) at the keyword sniffer; tested via `vacuum_into_blocked`/`vacuum_plain_blocked`/`vacuum_blocked_with_leading_comment` |
| TM-SQL-013    | DoS via `.dump` cumulative output bypass            | `.dump` previously built the full string before `max_output_bytes` was applied; `bounded_append()` enforces the cap after each schema/row chunk with the remaining budget passed from `run_statements`; `THREAT[TM-DOS-091]`; tested via `dump_respects_output_cap` and `dump_output_cap_enforced_across_multiple_tables` |

## Test Plan

Unit tests in `crates/bashkit/src/builtins/sqlite/tests.rs` (flags,
dot-commands, output modes, opt-in gate, parser/policy/sanitizer, splitter
proptest). Integration / security (TM-SQL regressions) / compat /
differential (byte-equal vs host `sqlite3`; skips when absent, CI installs
it; `recursive_cte_unsupported_in_turso` pins a known Turso 0.6.0
divergence) / fuzz harnesses live in
`crates/bashkit/tests/integration/sqlite_{integration,security,compat,differential,fuzz}_tests.rs`.

Run: `cargo test --features sqlite -p bashkit -- sqlite`
(higher-cycle fuzzing: prefix `PROPTEST_CASES=2000`).

## Public API

```rust
// Builder: .sqlite() for defaults (Memory backend), or
Bash::builder()
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
```

### CLI

The `bashkit` CLI ships `sqlite` enabled by default (matching `python` and
`git`); `configure_bash` auto-injects the opt-in env var. `--no-sqlite`
removes it (`sqlite: command not found`). An LLM hint is auto-injected when
registered (usage, dot-commands, `:memory:`, no ATTACH/DETACH).

## Future Work (deferred)

ATTACH/DETACH support; page-streaming Phase 3 backend (real positional reads
against the VFS; needs `FsBackend::pread`); encryption key management
(turso supports it); track upstream turso — remove BETA caveat at 1.0.

## Alternatives Considered

| Option                              | Why rejected                                              |
|-------------------------------------|-----------------------------------------------------------|
| `rusqlite` + `sqlite-vfs` shim      | C dep (`libsqlite3-sys`) breaks the pure-Rust posture.    |
| `libsql` (Turso's SQLite fork)      | Still C-based; upstream is steering toward the Rust rewrite. |
| Whole-file shim only (no Phase 2)   | Acceptable, but exercising the IO trait flushes out integration bugs. Both phases coexist with negligible extra surface. |
| In-process REPL mode                | Out of scope for a non-interactive shell builtin.         |
