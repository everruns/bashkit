//! Dot-command dispatch.
//!
//! We support a deliberately small subset of `sqlite3` shell dot-commands —
//! the ones that actually make sense in a sandboxed, non-interactive context:
//!
//! | Command       | Semantics                                                |
//! |---------------|----------------------------------------------------------|
//! | `.help`       | List supported commands.                                 |
//! | `.quit`/`.exit` | End execution.                                         |
//! | `.tables`     | List tables in the main schema.                          |
//! | `.schema [t]` | Print CREATE TABLE statements (optional table filter).   |
//! | `.headers on|off` | Toggle column headers.                               |
//! | `.mode <m>`   | Switch output mode (list/csv/tabs/line/box/json/markdown/column). |
//! | `.separator <s>` | Set the field separator for list/csv modes.           |
//! | `.nullvalue <s>` | Set the placeholder for NULL values.                  |
//! | `.dump`       | Emit the schema + data as `INSERT` statements.           |
//! | `.read <path>`| Execute a script from the VFS.                           |
//! | `.indexes [t]`| List indexes (optionally filtered by table).             |
//!
//! Anything not in this table returns a `BadCommand` error so the user gets
//! actionable feedback rather than a silent no-op.

use std::path::PathBuf;

use turso_core::Value;

use super::engine::{Deadline, SqliteEngine};
use super::formatter::{OutputMode, OutputOpts};
use super::parser::tokenize_dot;

/// Outcome of running a dot-command.
#[derive(Debug)]
pub(super) enum DotOutcome {
    /// Command produced output for the user.
    Stdout(String),
    /// Command modified `OutputOpts` in place; no stdout.
    Configured,
    /// Command requests early termination (`.quit`, `.exit`).
    Quit,
    /// Command requests evaluation of additional script text from a VFS path.
    /// The caller (builtin) executes it against the same engine.
    Read(PathBuf),
}

#[derive(Debug, thiserror::Error)]
pub(super) enum DotError {
    #[error("unknown dot-command: .{0}")]
    BadCommand(String),
    #[error("usage: .{0} {1}")]
    Usage(&'static str, &'static str),
    #[error("invalid value for .{cmd}: {value}")]
    InvalidValue { cmd: &'static str, value: String },
    #[error("sqlite engine error: {0}")]
    Engine(String),
}

const HELP_TEXT: &str = concat!(
    ".help                   Show this message\n",
    ".quit / .exit           End execution\n",
    ".tables                 List tables\n",
    ".schema [TABLE]         Show CREATE statements\n",
    ".indexes [TABLE]        List indexes\n",
    ".headers on|off         Toggle column headers\n",
    ".mode MODE              Set output mode (list, csv, tabs, line, box,\n",
    "                        column, json, markdown)\n",
    ".separator SEP          Set output separator\n",
    ".nullvalue STR          Set NULL placeholder\n",
    ".dump                   Dump schema + data as SQL\n",
    ".read PATH              Execute SQL from a VFS file\n",
);

pub(super) fn dispatch(
    line: &str,
    engine: &SqliteEngine,
    opts: &mut OutputOpts,
    deadline: Deadline,
) -> Result<DotOutcome, DotError> {
    let (name, args) = tokenize_dot(line);
    match name.as_str() {
        "help" | "h" | "?" => Ok(DotOutcome::Stdout(HELP_TEXT.to_string())),
        "quit" | "exit" => Ok(DotOutcome::Quit),
        "headers" | "header" => set_headers(args, opts).map(|_| DotOutcome::Configured),
        "mode" => set_mode(args, opts).map(|_| DotOutcome::Configured),
        "separator" | "sep" => set_separator(args, opts).map(|_| DotOutcome::Configured),
        "nullvalue" | "null" => set_null(args, opts).map(|_| DotOutcome::Configured),
        "tables" => tables(args, engine, opts, deadline).map(DotOutcome::Stdout),
        "schema" => schema(args, engine, deadline).map(DotOutcome::Stdout),
        "indexes" | "indices" => indexes(args, engine, opts, deadline).map(DotOutcome::Stdout),
        "dump" => dump(engine, deadline).map(DotOutcome::Stdout),
        "read" => {
            let path = args
                .into_iter()
                .next()
                .ok_or(DotError::Usage("read", "PATH"))?;
            Ok(DotOutcome::Read(PathBuf::from(path)))
        }
        other => Err(DotError::BadCommand(other.to_string())),
    }
}

fn set_headers(args: Vec<String>, opts: &mut OutputOpts) -> Result<(), DotError> {
    let v = args
        .into_iter()
        .next()
        .ok_or(DotError::Usage("headers", "on|off"))?;
    let lower = v.to_ascii_lowercase();
    opts.headers = match lower.as_str() {
        "on" | "1" | "true" | "yes" => true,
        "off" | "0" | "false" | "no" => false,
        _ => {
            return Err(DotError::InvalidValue {
                cmd: "headers",
                value: v,
            });
        }
    };
    Ok(())
}

fn set_mode(args: Vec<String>, opts: &mut OutputOpts) -> Result<(), DotError> {
    let v = args
        .into_iter()
        .next()
        .ok_or(DotError::Usage("mode", "MODE"))?;
    let mode = OutputMode::parse(&v).ok_or(DotError::InvalidValue {
        cmd: "mode",
        value: v.clone(),
    })?;
    opts.mode = mode;
    // sqlite3 also flips the separator for csv/tabs to a sensible default,
    // and switching back to list mode restores `|` only if the previous mode
    // had clobbered it (otherwise we keep whatever the user picked).
    match mode {
        OutputMode::Csv => opts.separator = ",".to_string(),
        OutputMode::Tabs => opts.separator = "\t".to_string(),
        OutputMode::List if opts.separator == "," || opts.separator == "\t" => {
            opts.separator = "|".to_string();
        }
        _ => {}
    }
    Ok(())
}

fn set_separator(args: Vec<String>, opts: &mut OutputOpts) -> Result<(), DotError> {
    let v = args
        .into_iter()
        .next()
        .ok_or(DotError::Usage("separator", "SEP"))?;
    opts.separator = decode_escapes(&v);
    Ok(())
}

fn set_null(args: Vec<String>, opts: &mut OutputOpts) -> Result<(), DotError> {
    let v = args.into_iter().next().unwrap_or_default();
    opts.null_text = v;
    Ok(())
}

/// Decode backslash escapes in a separator (e.g. `\t`, `\n`).
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

fn tables(
    args: Vec<String>,
    engine: &SqliteEngine,
    opts: &OutputOpts,
    deadline: Deadline,
) -> Result<String, DotError> {
    let pattern = args.into_iter().next();
    let sql = match pattern {
        Some(p) => format!(
            "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '{}' ORDER BY name",
            p.replace('\'', "''")
        ),
        None => "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name".to_string(),
    };
    let outcome = engine.execute(&sql, deadline).map_err(DotError::Engine)?;
    let mut names = Vec::new();
    for row in &outcome.rows {
        if let Some(Value::Text(t)) = row.first() {
            names.push(t.as_str().to_string());
        }
    }
    if names.is_empty() {
        return Ok(String::new());
    }
    // sqlite3 prints tables in a column-wrapped layout. We emit them one per
    // line for ergonomics inside scripts; pipe to `column` if you want grids.
    let _ = opts; // keep signature stable for future formatting toggles
    let mut out = names.join("\n");
    out.push('\n');
    Ok(out)
}

fn schema(
    args: Vec<String>,
    engine: &SqliteEngine,
    deadline: Deadline,
) -> Result<String, DotError> {
    let pattern = args.into_iter().next();
    let sql = match pattern {
        Some(p) => format!(
            "SELECT sql FROM sqlite_master WHERE name LIKE '{}' AND sql IS NOT NULL ORDER BY name",
            p.replace('\'', "''")
        ),
        None => "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name".to_string(),
    };
    let outcome = engine.execute(&sql, deadline).map_err(DotError::Engine)?;
    let mut out = String::new();
    for row in &outcome.rows {
        if let Some(Value::Text(t)) = row.first() {
            out.push_str(t.as_str());
            out.push_str(";\n");
        }
    }
    Ok(out)
}

fn indexes(
    args: Vec<String>,
    engine: &SqliteEngine,
    _opts: &OutputOpts,
    deadline: Deadline,
) -> Result<String, DotError> {
    let pattern = args.into_iter().next();
    let sql = match pattern {
        Some(p) => format!(
            "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name LIKE '{}' ORDER BY name",
            p.replace('\'', "''")
        ),
        None => "SELECT name FROM sqlite_master WHERE type='index' ORDER BY name".to_string(),
    };
    let outcome = engine.execute(&sql, deadline).map_err(DotError::Engine)?;
    let mut out = String::new();
    for row in &outcome.rows {
        if let Some(Value::Text(t)) = row.first() {
            out.push_str(t.as_str());
            out.push('\n');
        }
    }
    Ok(out)
}

/// Emit `BEGIN; <CREATE TABLE>...; <INSERT INTO ... VALUES (...)>; COMMIT;`.
/// This matches sqlite3's `.dump` for tables; views/triggers/indexes only get
/// their CREATE statement, no rows. Blob literals are emitted as `X'..'`.
fn dump(engine: &SqliteEngine, deadline: Deadline) -> Result<String, DotError> {
    let mut out = String::from("PRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\n");

    // Schema first.
    let schema_outcome = engine
        .execute(
            "SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid",
            deadline,
        )
        .map_err(DotError::Engine)?;
    for row in &schema_outcome.rows {
        if let Some(Value::Text(sql)) = row.get(2) {
            out.push_str(sql.as_str());
            out.push_str(";\n");
        }
    }

    // Then data, table by table.
    let tables_outcome = engine
        .execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            deadline,
        )
        .map_err(DotError::Engine)?;
    for row in &tables_outcome.rows {
        let Some(Value::Text(t)) = row.first() else {
            continue;
        };
        let name = t.as_str().to_string();
        let quoted = name.replace('"', "\"\"");
        let sql = format!("SELECT * FROM \"{quoted}\"");
        let data = engine.execute(&sql, deadline).map_err(DotError::Engine)?;
        for data_row in &data.rows {
            let values: Vec<String> = data_row.iter().map(format_sql_literal).collect();
            out.push_str(&format!(
                "INSERT INTO \"{}\" VALUES({});\n",
                quoted,
                values.join(",")
            ));
        }
    }

    out.push_str("COMMIT;\n");
    Ok(out)
}

fn format_sql_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Numeric(_) => format!("{v}"),
        Value::Text(t) => {
            let escaped = t.as_str().replace('\'', "''");
            format!("'{escaped}'")
        }
        Value::Blob(b) => {
            let mut hex = String::with_capacity(b.len() * 2 + 3);
            hex.push_str("X'");
            for byte in b {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02X}");
            }
            hex.push('\'');
            hex
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> OutputOpts {
        OutputOpts::default()
    }

    fn mk_engine() -> SqliteEngine {
        SqliteEngine::open_pure_memory().expect("open in-mem")
    }

    fn no_deadline() -> Deadline {
        // Tests run instantly; an "unlimited" deadline is the only sensible
        // choice so a slow CI host doesn't flake.
        Deadline::new(std::time::Duration::ZERO)
    }

    fn dispatch_t(
        line: &str,
        engine: &SqliteEngine,
        opts: &mut OutputOpts,
    ) -> Result<DotOutcome, DotError> {
        dispatch(line, engine, opts, no_deadline())
    }

    #[test]
    fn help_returns_text() {
        let engine = mk_engine();
        let mut o = opts();
        let r = dispatch_t(".help", &engine, &mut o).unwrap();
        match r {
            DotOutcome::Stdout(s) => assert!(s.contains(".tables")),
            _ => panic!("expected stdout"),
        }
    }

    #[test]
    fn unknown_command_errors() {
        let engine = mk_engine();
        let mut o = opts();
        let err = dispatch_t(".doesnotexist", &engine, &mut o).unwrap_err();
        assert!(matches!(err, DotError::BadCommand(_)));
    }

    #[test]
    fn headers_toggle() {
        let engine = mk_engine();
        let mut o = opts();
        dispatch_t(".headers on", &engine, &mut o).unwrap();
        assert!(o.headers);
        dispatch_t(".headers off", &engine, &mut o).unwrap();
        assert!(!o.headers);
    }

    #[test]
    fn headers_invalid() {
        let engine = mk_engine();
        let mut o = opts();
        let err = dispatch_t(".headers maybe", &engine, &mut o).unwrap_err();
        assert!(matches!(err, DotError::InvalidValue { cmd: "headers", .. }));
    }

    #[test]
    fn headers_missing_arg() {
        let engine = mk_engine();
        let mut o = opts();
        let err = dispatch_t(".headers", &engine, &mut o).unwrap_err();
        assert!(matches!(err, DotError::Usage("headers", _)));
    }

    #[test]
    fn mode_changes_separator_for_csv() {
        let engine = mk_engine();
        let mut o = opts();
        dispatch_t(".mode csv", &engine, &mut o).unwrap();
        assert_eq!(o.separator, ",");
        dispatch_t(".mode tabs", &engine, &mut o).unwrap();
        assert_eq!(o.separator, "\t");
        dispatch_t(".mode list", &engine, &mut o).unwrap();
        assert_eq!(o.separator, "|");
    }

    #[test]
    fn mode_invalid() {
        let engine = mk_engine();
        let mut o = opts();
        let err = dispatch_t(".mode bogus", &engine, &mut o).unwrap_err();
        assert!(matches!(err, DotError::InvalidValue { cmd: "mode", .. }));
    }

    #[test]
    fn separator_decodes_escapes() {
        let engine = mk_engine();
        let mut o = opts();
        dispatch_t(".separator '\\t'", &engine, &mut o).unwrap();
        assert_eq!(o.separator, "\t");
        dispatch_t(".separator '\\n'", &engine, &mut o).unwrap();
        assert_eq!(o.separator, "\n");
    }

    #[test]
    fn nullvalue_sets_placeholder() {
        let engine = mk_engine();
        let mut o = opts();
        dispatch_t(".nullvalue NIL", &engine, &mut o).unwrap();
        assert_eq!(o.null_text, "NIL");
        // Empty arg → empty placeholder
        dispatch_t(".nullvalue", &engine, &mut o).unwrap();
        assert_eq!(o.null_text, "");
    }

    #[test]
    fn tables_lists_existing() {
        let engine = mk_engine();
        engine
            .execute("CREATE TABLE foo(a)", no_deadline())
            .unwrap();
        engine
            .execute("CREATE TABLE bar(b)", no_deadline())
            .unwrap();
        let mut o = opts();
        let DotOutcome::Stdout(s) = dispatch_t(".tables", &engine, &mut o).unwrap() else {
            panic!("expected stdout");
        };
        assert!(s.contains("foo"));
        assert!(s.contains("bar"));
    }

    #[test]
    fn tables_with_pattern() {
        let engine = mk_engine();
        engine
            .execute("CREATE TABLE foo(a)", no_deadline())
            .unwrap();
        engine
            .execute("CREATE TABLE bar(b)", no_deadline())
            .unwrap();
        let mut o = opts();
        let DotOutcome::Stdout(s) = dispatch_t(".tables foo", &engine, &mut o).unwrap() else {
            panic!("expected stdout");
        };
        assert!(s.contains("foo"));
        assert!(!s.contains("bar"));
    }

    #[test]
    fn tables_empty_db() {
        let engine = mk_engine();
        let mut o = opts();
        let DotOutcome::Stdout(s) = dispatch_t(".tables", &engine, &mut o).unwrap() else {
            panic!("expected stdout");
        };
        assert_eq!(s, "");
    }

    #[test]
    fn schema_returns_create() {
        let engine = mk_engine();
        engine
            .execute("CREATE TABLE foo(a INTEGER, b TEXT)", no_deadline())
            .unwrap();
        let mut o = opts();
        let DotOutcome::Stdout(s) = dispatch_t(".schema", &engine, &mut o).unwrap() else {
            panic!("expected stdout");
        };
        assert!(s.contains("CREATE TABLE foo"));
    }

    #[test]
    fn dump_round_trips() {
        let engine = mk_engine();
        engine
            .execute("CREATE TABLE t(x INTEGER, y TEXT)", no_deadline())
            .unwrap();
        engine
            .execute(
                "INSERT INTO t VALUES (1, 'hello'), (2, 'O''Brien')",
                no_deadline(),
            )
            .unwrap();
        let mut o = opts();
        let DotOutcome::Stdout(s) = dispatch_t(".dump", &engine, &mut o).unwrap() else {
            panic!("expected stdout");
        };
        assert!(s.contains("BEGIN TRANSACTION;"));
        assert!(s.contains("CREATE TABLE t"));
        assert!(s.contains("INSERT INTO \"t\" VALUES(1,'hello')"));
        assert!(s.contains("'O''Brien'"));
        assert!(s.contains("COMMIT;"));
    }

    #[test]
    fn read_returns_path() {
        let engine = mk_engine();
        let mut o = opts();
        let DotOutcome::Read(p) = dispatch_t(".read /tmp/x.sql", &engine, &mut o).unwrap() else {
            panic!("expected read");
        };
        assert_eq!(p.to_string_lossy(), "/tmp/x.sql");
    }

    #[test]
    fn read_without_path_errors() {
        let engine = mk_engine();
        let mut o = opts();
        let err = dispatch_t(".read", &engine, &mut o).unwrap_err();
        assert!(matches!(err, DotError::Usage("read", _)));
    }

    #[test]
    fn quit_signals_quit() {
        let engine = mk_engine();
        let mut o = opts();
        let r = dispatch_t(".quit", &engine, &mut o).unwrap();
        assert!(matches!(r, DotOutcome::Quit));
        let r = dispatch_t(".exit", &engine, &mut o).unwrap();
        assert!(matches!(r, DotOutcome::Quit));
    }
}
