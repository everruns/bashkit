//! `sqlite` / `sqlite3` builtin — embedded SQLite via [`turso_core`].
//!
//! See `specs/sqlite-builtin.md` for the design rationale, threat model, and
//! test plan. At a glance:
//!
//! - A single invocation opens a fresh database connection, runs every SQL
//!   statement and dot-command in the script, and persists changes back to
//!   the VFS on success.
//! - Two backends are wired up:
//!   - [`SqliteBackend::Memory`] — turso's `MemoryIO`, with whole-file load
//!     from / flush to the VFS at command boundaries (Phase 1).
//!   - [`SqliteBackend::Vfs`] — bashkit's `FileSystem` plugged into turso via
//!     a custom `IO` impl (Phase 2). Equivalent semantics, different code path.
//! - In-memory databases are spelled `:memory:` and bypass VFS entirely.
//! - Dot-commands implement a curated subset of `sqlite3` shell features
//!   (`.tables`, `.schema`, `.dump`, `.headers`, `.mode`, `.separator`,
//!   `.nullvalue`, `.read`, `.indexes`, `.help`, `.quit`/`.exit`).
//! - `BASHKIT_ALLOW_INPROCESS_SQLITE=1` (env or via builder) gates execution
//!   in case operators want to keep the BETA upstream code dormant.
//!
//! Limits enforced:
//! - SQL script length capped at [`SqliteLimits::max_script_bytes`].
//! - Per-result-set row count capped at [`SqliteLimits::max_rows_per_query`].
//! - Per-database file size capped at [`SqliteLimits::max_db_bytes`].

mod dot_commands;
mod engine;
mod formatter;
mod parser;
mod vfs_io;

#[cfg(test)]
mod tests;

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

use super::{Builtin, Context, check_help_version, resolve_path};
use dot_commands::{DotError, DotOutcome};
use engine::SqliteEngine;
use formatter::{OutputMode, OutputOpts, render};
use parser::Stmt;

const SQLITE_OPT_IN_ENV: &str = "BASHKIT_ALLOW_INPROCESS_SQLITE";

/// Default cap on raw SQL input (script size, summed across `-c` args, file
/// reads, and stdin). Mirrors `python`'s defensive limits.
const DEFAULT_MAX_SCRIPT_BYTES: usize = 4 * 1024 * 1024; // 4 MiB
/// Default cap on rows materialised per query, beyond which the query aborts.
const DEFAULT_MAX_ROWS_PER_QUERY: usize = 1_000_000;
/// Default cap on the size of a single database file when loaded from VFS.
const DEFAULT_MAX_DB_BYTES: usize = 256 * 1024 * 1024; // 256 MiB

/// Choice of `IO` backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SqliteBackend {
    /// Phase 1: load whole DB file into turso's `MemoryIO`, run, flush back.
    /// Default — most predictable and exposes the smallest surface of the
    /// upstream BETA codebase.
    #[default]
    Memory,
    /// Phase 2: turso talks to the VFS through a custom `IO` impl.
    /// Functionally equivalent for our use case but exercises the IO trait
    /// path; pick this if you want to test the full integration.
    Vfs,
}

impl SqliteBackend {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "memory" | "memio" | "mem" => Self::Memory,
            "vfs" | "vfsio" => Self::Vfs,
            _ => return None,
        })
    }
}

/// Resource limits for the embedded sqlite engine.
#[derive(Debug, Clone)]
pub struct SqliteLimits {
    /// Maximum raw SQL input size in bytes.
    pub max_script_bytes: usize,
    /// Maximum rows materialised per query before aborting.
    pub max_rows_per_query: usize,
    /// Maximum database file size loadable from the VFS.
    pub max_db_bytes: usize,
    /// Backend selection.
    pub backend: SqliteBackend,
}

impl Default for SqliteLimits {
    fn default() -> Self {
        Self {
            max_script_bytes: DEFAULT_MAX_SCRIPT_BYTES,
            max_rows_per_query: DEFAULT_MAX_ROWS_PER_QUERY,
            max_db_bytes: DEFAULT_MAX_DB_BYTES,
            backend: SqliteBackend::default(),
        }
    }
}

impl SqliteLimits {
    /// Set max script size.
    #[must_use]
    pub fn max_script_bytes(mut self, n: usize) -> Self {
        self.max_script_bytes = n;
        self
    }
    /// Set max rows materialised per query.
    #[must_use]
    pub fn max_rows_per_query(mut self, n: usize) -> Self {
        self.max_rows_per_query = n;
        self
    }
    /// Set max DB file bytes.
    #[must_use]
    pub fn max_db_bytes(mut self, n: usize) -> Self {
        self.max_db_bytes = n;
        self
    }
    /// Pick a backend.
    #[must_use]
    pub fn backend(mut self, backend: SqliteBackend) -> Self {
        self.backend = backend;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SqliteInprocessOptIn(pub bool);

fn sqlite_inprocess_enabled(ctx: &Context<'_>) -> bool {
    ctx.execution_extension::<SqliteInprocessOptIn>()
        .is_some_and(|opt_in| opt_in.0)
        || {
            #[cfg(test)]
            {
                let is_enabled = |v: &str| matches!(v, "1" | "true" | "TRUE" | "yes" | "YES");
                ctx.env
                    .get(SQLITE_OPT_IN_ENV)
                    .is_some_and(|v| is_enabled(v))
            }
            #[cfg(not(test))]
            {
                false
            }
        }
}

/// The `sqlite` / `sqlite3` builtin command.
pub struct Sqlite {
    /// Resource and backend configuration.
    pub limits: SqliteLimits,
}

impl Sqlite {
    /// Construct with default limits and the Memory backend.
    pub fn new() -> Self {
        Self {
            limits: SqliteLimits::default(),
        }
    }

    /// Construct with custom limits.
    pub fn with_limits(limits: SqliteLimits) -> Self {
        Self { limits }
    }
}

impl Default for Sqlite {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Builtin for Sqlite {
    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "sqlite/sqlite3: Embedded SQLite-compatible engine (Turso, BETA). \
             Usage: sqlite DB SQL... | sqlite DB <script | sqlite -separator , -header DB SELECT. \
             Dot-commands: .tables .schema .dump .headers .mode .separator .nullvalue .read .help. \
             Supports :memory:. No ATTACH/DETACH. \
             Set BASHKIT_ALLOW_INPROCESS_SQLITE=1 to enable.",
        )
    }

    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let invocation_args: Vec<String> = ctx.args.to_vec();
        if let Some(r) = check_help_version(&invocation_args, HELP_TEXT, Some("sqlite (turso 0.5)"))
        {
            return Ok(r);
        }

        if !sqlite_inprocess_enabled(&ctx) {
            return Ok(ExecResult::err(
                format!(
                    "sqlite: in-process SQLite disabled by default; set {SQLITE_OPT_IN_ENV}=1 to enable\n"
                ),
                1,
            ));
        }

        let parsed = match parse_args(&invocation_args, ctx.stdin) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ExecResult::err(format!("sqlite: {e}\n"), 2));
            }
        };

        // Resolve the database path early so error messages are deterministic.
        let db_target = resolve_db_target(&parsed.db_arg, ctx.cwd);

        // Apply per-invocation limits.
        let script_len = parsed.script.len();
        if script_len > self.limits.max_script_bytes {
            return Ok(ExecResult::err(
                format!(
                    "sqlite: script too large ({script_len} bytes; limit {})\n",
                    self.limits.max_script_bytes
                ),
                1,
            ));
        }

        // Resolve effective backend (CLI flag overrides builder default).
        let backend = parsed.backend.unwrap_or(self.limits.backend);

        // Open the engine, optionally seeded from VFS.
        let engine = match open_engine(&db_target, backend, &ctx.fs, &self.limits).await {
            Ok(e) => e,
            Err(msg) => return Ok(ExecResult::err(format!("sqlite: {msg}\n"), 1)),
        };

        // Initial output options come from the CLI flags; dot-commands may
        // mutate them as we go.
        let mut opts = parsed.output;

        // Run statements + dot-commands in order. We collect output into a
        // single buffer; errors short-circuit further execution but still
        // attempt a flush of any successful writes.
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0i32;
        let stmts = parser::split(&parsed.script);
        let exec_outcome = run_statements(
            &engine,
            stmts,
            &ctx.fs,
            ctx.cwd,
            &mut opts,
            &mut stdout,
            &self.limits,
            0,
        )
        .await;
        if let Err(e) = exec_outcome {
            stderr.push_str(&format!("sqlite: {e}\n"));
            exit_code = 1;
        }

        // Flush + persist.
        match &db_target {
            DbTarget::Memory => {}
            DbTarget::File { path } => match backend {
                SqliteBackend::Memory => {
                    if let Some(bytes) = engine.snapshot_bytes() {
                        // Even on error, try to persist anything that was
                        // flushed by turso so the caller doesn't lose work
                        // from earlier statements in the batch.
                        if let Err(e) = ctx.fs.write_file(path, &bytes).await {
                            stderr.push_str(&format!(
                                "sqlite: persist failed: {}: {e}\n",
                                path.display()
                            ));
                            exit_code = exit_code.max(1);
                        }
                    }
                }
                SqliteBackend::Vfs => {
                    if let Err(e) = engine.flush_dirty().await {
                        stderr.push_str(&format!("sqlite: flush failed: {e}\n"));
                        exit_code = exit_code.max(1);
                    }
                }
            },
        }

        let mut result = ExecResult {
            exit_code,
            ..Default::default()
        };
        result.stdout = stdout;
        result.stderr = stderr;
        Ok(result)
    }
}

#[derive(Debug)]
enum DbTarget {
    Memory,
    File { path: PathBuf },
}

fn resolve_db_target(arg: &str, cwd: &Path) -> DbTarget {
    if arg == ":memory:" || arg.is_empty() {
        return DbTarget::Memory;
    }
    DbTarget::File {
        path: resolve_path(cwd, arg),
    }
}

#[derive(Debug)]
struct ParsedArgs {
    db_arg: String,
    script: String,
    output: OutputOpts,
    backend: Option<SqliteBackend>,
}

fn parse_args(args: &[String], stdin: Option<&str>) -> std::result::Result<ParsedArgs, String> {
    let mut output = OutputOpts::default();
    let mut backend: Option<SqliteBackend> = None;
    let mut script_parts: Vec<String> = Vec::new();
    let mut db_arg: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            // sqlite3 historical flags use a single dash.
            "-header" | "-headers" | "--header" | "--headers" => {
                output.headers = true;
            }
            "-noheader" | "--noheader" => {
                output.headers = false;
            }
            "-csv" | "--csv" => {
                output.mode = OutputMode::Csv;
                output.separator = ",".to_string();
            }
            "-tabs" | "--tabs" => {
                output.mode = OutputMode::Tabs;
                output.separator = "\t".to_string();
            }
            "-line" | "--line" => {
                output.mode = OutputMode::Line;
            }
            "-list" | "--list" => {
                output.mode = OutputMode::List;
            }
            "-box" | "--box" => {
                output.mode = OutputMode::Box;
            }
            "-column" | "--column" => {
                output.mode = OutputMode::Column;
            }
            "-json" | "--json" => {
                output.mode = OutputMode::Json;
            }
            "-markdown" | "--markdown" => {
                output.mode = OutputMode::Markdown;
            }
            "-separator" | "--separator" => {
                let v = next_value(args, &mut i, "-separator")?;
                output.separator = decode_escapes(&v);
            }
            "-nullvalue" | "--nullvalue" => {
                let v = next_value(args, &mut i, "-nullvalue")?;
                output.null_text = v;
            }
            "-cmd" | "--cmd" => {
                let v = next_value(args, &mut i, "-cmd")?;
                script_parts.push(v);
            }
            "-backend" | "--backend" => {
                let v = next_value(args, &mut i, "-backend")?;
                let b = SqliteBackend::parse(&v)
                    .ok_or_else(|| format!("invalid backend '{v}' (memory|vfs)"))?;
                backend = Some(b);
            }
            "--" => {
                i += 1;
                // Everything after `--` is positional.
                while i < args.len() {
                    consume_positional(&args[i], &mut db_arg, &mut script_parts);
                    i += 1;
                }
                break;
            }
            arg if arg.starts_with('-') && arg != "-" => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                consume_positional(a, &mut db_arg, &mut script_parts);
            }
        }
        i += 1;
    }

    let db_arg = db_arg.unwrap_or_else(|| ":memory:".to_string());
    let mut script = script_parts.join(";\n");
    // Treat stdin as additional script text when no inline SQL was provided.
    if script.trim().is_empty()
        && let Some(input) = stdin
        && !input.is_empty()
    {
        script = input.to_string();
    }
    Ok(ParsedArgs {
        db_arg,
        script,
        output,
        backend,
    })
}

fn next_value(args: &[String], i: &mut usize, flag: &str) -> std::result::Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("option {flag} requires an argument"))
}

fn consume_positional(arg: &str, db: &mut Option<String>, script: &mut Vec<String>) {
    if db.is_none() {
        *db = Some(arg.to_string());
    } else {
        script.push(arg.to_string());
    }
}

fn decode_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
        {
            chars.next();
            match next {
                't' => out.push('\t'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

async fn open_engine(
    target: &DbTarget,
    backend: SqliteBackend,
    fs: &Arc<dyn FileSystem>,
    limits: &SqliteLimits,
) -> std::result::Result<SqliteEngine, String> {
    match target {
        DbTarget::Memory => SqliteEngine::open_pure_memory(),
        DbTarget::File { path } => match backend {
            SqliteBackend::Memory => {
                let initial = match fs.read_file(path).await {
                    Ok(bytes) => {
                        if bytes.len() > limits.max_db_bytes {
                            return Err(format!(
                                "database file too large ({} bytes; limit {})",
                                bytes.len(),
                                limits.max_db_bytes
                            ));
                        }
                        Some(bytes)
                    }
                    Err(_) => None,
                };
                SqliteEngine::open_memory(initial.as_deref())
            }
            SqliteBackend::Vfs => {
                let handle = vfs_io::current_handle_or_default();
                let io = vfs_io::BashkitVfsIO::new(fs.clone(), handle);
                let path_str = path.to_string_lossy().into_owned();
                SqliteEngine::open_vfs(io, &path_str)
            }
        },
    }
}

/// Hard cap on `.read` nesting depth. A self-referential script
/// (e.g. `echo '.read /tmp/loop.sql' > /tmp/loop.sql`) would otherwise blow
/// the stack; we bound it well under typical thread stack sizes.
const MAX_DOT_READ_DEPTH: usize = 16;

#[allow(clippy::too_many_arguments)]
async fn run_statements(
    engine: &SqliteEngine,
    stmts: Vec<Stmt>,
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
    opts: &mut OutputOpts,
    stdout: &mut String,
    limits: &SqliteLimits,
    depth: usize,
) -> std::result::Result<(), String> {
    if depth > MAX_DOT_READ_DEPTH {
        return Err(format!(
            ".read nesting too deep (limit {MAX_DOT_READ_DEPTH})"
        ));
    }
    for stmt in stmts {
        match stmt {
            Stmt::Sql(sql) => {
                let outcome = engine.execute(&sql).map_err(|e| sanitize(&e))?;
                if outcome.rows.len() > limits.max_rows_per_query {
                    return Err(format!(
                        "result set exceeds row cap ({} > {})",
                        outcome.rows.len(),
                        limits.max_rows_per_query
                    ));
                }
                let rendered = render(&outcome.columns, &outcome.rows, opts);
                stdout.push_str(&rendered);
            }
            Stmt::Dot(line) => {
                let result = dot_commands::dispatch(&line, engine, opts);
                match result {
                    Ok(DotOutcome::Stdout(s)) => stdout.push_str(&s),
                    Ok(DotOutcome::Configured) => {}
                    Ok(DotOutcome::Quit) => return Ok(()),
                    Ok(DotOutcome::Read(p)) => {
                        let abs = if p.is_absolute() { p } else { cwd.join(&p) };
                        let bytes = fs
                            .read_file(&abs)
                            .await
                            .map_err(|e| format!("cannot read {}: {e}", abs.display()))?;
                        let nested = String::from_utf8(bytes)
                            .map_err(|_| format!("{} is not valid UTF-8", abs.display()))?;
                        let nested_stmts = parser::split(&nested);
                        // Recurse via Box::pin to keep the future Send + size-bounded.
                        Box::pin(run_statements(
                            engine,
                            nested_stmts,
                            fs,
                            cwd,
                            opts,
                            stdout,
                            limits,
                            depth + 1,
                        ))
                        .await?;
                    }
                    Err(DotError::BadCommand(c)) => {
                        return Err(format!("unknown dot-command: .{c}"));
                    }
                    Err(e) => {
                        return Err(format!("{e}"));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Strip turso's internal location pointers from a message before showing it
/// to the user (defence-in-depth; the upstream messages occasionally include
/// crate-relative paths and pid info on assertion failures).
fn sanitize(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for line in msg.lines() {
        // Drop trailing `at <path>:<line>:<col>` annotations that some
        // libraries append. Conservative: keep everything up to ` at /`.
        let cleaned = match line.find(" at /") {
            Some(idx) => &line[..idx],
            None => line,
        };
        out.push_str(cleaned);
        out.push('\n');
    }
    out.trim_end().to_string()
}

const HELP_TEXT: &str = concat!(
    "usage: sqlite [OPTIONS] DB [SQL ...]\n",
    "       sqlite [OPTIONS] :memory: [SQL ...]\n",
    "Options:\n",
    "  -header, --header        Include column headers\n",
    "  -noheader, --noheader    Suppress column headers (default)\n",
    "  -csv, --csv              Output mode: CSV\n",
    "  -tabs, --tabs            Output mode: tabs\n",
    "  -line, --line            Output mode: name=value lines\n",
    "  -list, --list            Output mode: separator-joined (default)\n",
    "  -box, --box              Output mode: ASCII box table\n",
    "  -column, --column        Output mode: column-aligned\n",
    "  -json, --json            Output mode: JSON array of objects\n",
    "  -markdown, --markdown    Output mode: Markdown table\n",
    "  -separator SEP           Field separator (e.g. '|', ',', '\\t')\n",
    "  -nullvalue STR           Placeholder for NULL\n",
    "  -cmd SQL                 Run extra SQL before positional script\n",
    "  -backend memory|vfs      Pick the IO backend (default: memory)\n",
    "  --help                   Show this message\n",
    "  --version                Print engine version\n",
    "Dot-commands: .help .quit .exit .tables .schema .indexes\n",
    "              .headers .mode .separator .nullvalue\n",
    "              .dump .read PATH\n",
);
