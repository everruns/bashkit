//! Thin wrapper around `turso_core` that hides the choice of `IO` backend.
//!
//! The two backends correspond to the two phases described in
//! `specs/sqlite-builtin.md`:
//!
//! - **Phase 1** ([`Backend::Memory`]): use turso's `MemoryIO` and snapshot the
//!   raw database bytes for caller-driven persistence. This is what the
//!   builtin uses when the caller asks for `:memory:` databases or wants to
//!   load/flush the entire DB file from the VFS at command boundaries.
//!
//! - **Phase 2** ([`Backend::Vfs`]): use [`super::vfs_io::BashkitVfsIO`], which
//!   reads/writes through bashkit's `Arc<dyn FileSystem>` and persists dirty
//!   bytes back on `flush_dirty`.
//!
//! Both expose the same query API. The builtin layer above is agnostic to
//! which backend is active.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use turso_core::{Connection, Database, IO, MemoryIO, OpenFlags, StepResult, Value};

use super::vfs_io::BashkitVfsIO;

/// Result alias for engine operations. The error string is intended to be
/// shown directly to the user via `ExecResult::err`, so it should not include
/// host paths or other sensitive details.
pub(super) type EngineResult<T> = std::result::Result<T, String>;

/// Each engine creates its own unique in-memory path. Turso bypasses its
/// process-wide `DATABASE_MANAGER` registry for paths starting with `:memory:`,
/// so we prefix with that to keep concurrent engines isolated even when
/// multiple instances live in the same process (e.g. parallel tests).
fn unique_memory_path() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(":memory:bashkit-{n}")
}

/// Selects which `IO` impl backs the engine.
pub(super) enum Backend {
    /// Pure in-process `MemoryIO`. The owner of the engine is responsible for
    /// calling [`SqliteEngine::snapshot_bytes`] when it wants to persist.
    Memory(Arc<MemoryIO>),
    /// VFS-backed `BashkitVfsIO`. The owner calls `flush_dirty` on the IO
    /// when it wants the in-memory pages flushed back to the VFS.
    Vfs(Arc<BashkitVfsIO>),
}

/// Outcome of executing a single SQL statement.
#[derive(Debug, Default)]
pub(super) struct StatementOutcome {
    /// Column names if the statement produced a result set.
    pub columns: Vec<String>,
    /// Rows materialised from the result set. Empty for non-SELECT statements.
    pub rows: Vec<Vec<Value>>,
    /// Number of rows changed (for INSERT/UPDATE/DELETE; 0 otherwise).
    pub changes: i64,
}

/// Wraps a turso `Database`/`Connection` pair plus the backing `IO`.
pub(super) struct SqliteEngine {
    backend: Backend,
    _db: Arc<Database>,
    conn: Arc<Connection>,
    /// Path used to register the database file inside the IO. For
    /// [`Backend::Memory`] this is a unique `:memory:bashkit-N` string so
    /// concurrent engines never share state through turso's process-wide
    /// `DATABASE_MANAGER` registry.
    memory_path: Option<String>,
}

impl SqliteEngine {
    /// Open a fresh in-memory database. If `initial_bytes` is `Some`, the
    /// bytes are written into a temporary file inside the `MemoryIO` first
    /// so that turso opens an existing database rather than a blank one.
    ///
    /// We always route through a named in-memory file (rather than `:memory:`)
    /// so that [`SqliteEngine::snapshot_bytes`] can read the resulting
    /// database pages back. When there are no initial bytes we still seed an
    /// empty file to ensure the path exists.
    pub(super) fn open_memory(initial_bytes: Option<&[u8]>) -> EngineResult<Self> {
        let io: Arc<MemoryIO> = Arc::new(MemoryIO::new());
        let path = unique_memory_path();
        if let Some(bytes) = initial_bytes
            && !bytes.is_empty()
        {
            seed_memory_io(&io, &path, bytes).map_err(turso_msg)?;
        }
        let io_dyn: Arc<dyn IO> = io.clone();
        let db = Database::open_file(io_dyn, &path).map_err(turso_msg)?;
        let conn = db.connect().map_err(turso_msg)?;
        Ok(Self {
            backend: Backend::Memory(io),
            _db: db,
            conn,
            memory_path: Some(path),
        })
    }

    /// Open a true `:memory:` database (no file backing, no persistence).
    /// Use this when the caller never intends to extract bytes.
    pub(super) fn open_pure_memory() -> EngineResult<Self> {
        let io: Arc<MemoryIO> = Arc::new(MemoryIO::new());
        let io_dyn: Arc<dyn IO> = io.clone();
        let db = Database::open_file(io_dyn, ":memory:").map_err(turso_msg)?;
        let conn = db.connect().map_err(turso_msg)?;
        Ok(Self {
            backend: Backend::Memory(io),
            _db: db,
            conn,
            memory_path: None,
        })
    }

    /// Open a database backed by the bashkit VFS via [`BashkitVfsIO`].
    /// `path_in_io` is the path string passed verbatim to turso (and used as
    /// a key in the VFS).
    pub(super) fn open_vfs(io: Arc<BashkitVfsIO>, path_in_io: &str) -> EngineResult<Self> {
        let io_dyn: Arc<dyn IO> = io.clone();
        let db = Database::open_file(io_dyn, path_in_io).map_err(turso_msg)?;
        let conn = db.connect().map_err(turso_msg)?;
        Ok(Self {
            backend: Backend::Vfs(io),
            _db: db,
            conn,
            memory_path: None,
        })
    }

    /// Execute a single statement, materialising rows up-front so that the
    /// caller doesn't need to drive the step loop.
    pub(super) fn execute(&self, sql: &str) -> EngineResult<StatementOutcome> {
        let mut stmt = self.conn.prepare(sql).map_err(turso_msg)?;
        let mut outcome = StatementOutcome::default();
        for idx in 0..stmt.num_columns() {
            outcome.columns.push(stmt.get_column_name(idx).to_string());
        }
        loop {
            match stmt.step().map_err(turso_msg)? {
                StepResult::Row => {
                    if let Some(row) = stmt.row() {
                        let values: Vec<Value> = (0..stmt.num_columns())
                            .map(|idx| row.get_value(idx).clone())
                            .collect();
                        outcome.rows.push(values);
                    }
                }
                StepResult::Done => break,
                StepResult::IO => {
                    self.io_step()?;
                }
                StepResult::Busy | StepResult::Interrupt => {
                    return Err("query was interrupted or database is busy".to_string());
                }
            }
        }
        outcome.changes = self.conn.changes();
        Ok(outcome)
    }

    fn io_step(&self) -> EngineResult<()> {
        match &self.backend {
            Backend::Memory(io) => io.step().map_err(turso_msg),
            Backend::Vfs(io) => io.step().map_err(turso_msg),
        }
    }

    /// Snapshot the database file bytes. Only meaningful for the memory
    /// backend (which is what the Phase 1 path uses). Returns `None` for
    /// the VFS backend, since persistence happens via the IO directly.
    ///
    /// We force a TRUNCATE-mode checkpoint before reading so that any pages
    /// still in the WAL are folded into the main file. Without this step the
    /// snapshot would be missing the just-written transaction.
    pub(super) fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        let Backend::Memory(io) = &self.backend else {
            return None;
        };
        let path = self.memory_path.as_deref()?;
        let _ = self.conn.checkpoint(turso_core::CheckpointMode::Truncate {
            upper_bound_inclusive: None,
        });
        let file = io.open_file(path, OpenFlags::None, false).ok()?;
        let size = file.size().ok()? as usize;
        if size == 0 {
            return Some(Vec::new());
        }
        Some(read_all(&file, size))
    }

    /// For the VFS backend, flush any pages dirtied in memory back to the
    /// underlying `FileSystem`. Returns the number of files persisted.
    pub(super) async fn flush_dirty(&self) -> EngineResult<usize> {
        match &self.backend {
            Backend::Memory(_) => Ok(0),
            Backend::Vfs(io) => io.flush_dirty().await,
        }
    }

    /// Close the connection, releasing any cached pages. Best-effort.
    pub(super) fn close(&self) {
        let _ = self.conn.close();
    }
}

impl Drop for SqliteEngine {
    fn drop(&mut self) {
        // turso's Connection has its own Drop, but we want to be explicit
        // about checkpoints to keep the on-disk image consistent.
        self.close();
    }
}

fn read_all(file: &Arc<dyn turso_core::File>, size: usize) -> Vec<u8> {
    use turso_core::{Buffer, Completion};
    let mut out = vec![0u8; size];
    let chunk_size: usize = 4096;
    let mut pos = 0usize;
    while pos < size {
        let remaining = size - pos;
        let take = remaining.min(chunk_size);
        let chunk = Arc::new(Buffer::new(vec![0u8; take]));
        // The completion runs synchronously for MemoryIO; the closure receives
        // the buffer back via the Result tuple. We copy bytes after pread()
        // returns rather than from the closure, since the closure has to be
        // 'static and copying from there is awkward.
        let completion = Completion::new_read(chunk.clone(), |_res| None);
        let _ = file.pread(pos as u64, completion);
        out[pos..pos + take].copy_from_slice(&chunk.as_slice()[..take]);
        pos += take;
    }
    out
}

/// Pre-seed a `MemoryIO`-backed file with bytes by writing them as a single
/// `pwrite` operation. This is how we hand turso an existing database image.
fn seed_memory_io(
    io: &Arc<MemoryIO>,
    path: &str,
    bytes: &[u8],
) -> std::result::Result<(), turso_core::LimboError> {
    use turso_core::{Buffer, Completion, OpenFlags};
    let file = io.open_file(path, OpenFlags::Create, false)?;
    if bytes.is_empty() {
        return Ok(());
    }
    let buf = Arc::new(Buffer::new(bytes.to_vec()));
    let completion = Completion::new_write(|_| {});
    let _completion = file.pwrite(0, buf, completion)?;
    Ok(())
}

/// Map a turso error to a sanitised user-facing string.
fn turso_msg(e: turso_core::LimboError) -> String {
    e.to_string()
}
