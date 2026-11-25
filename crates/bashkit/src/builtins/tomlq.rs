//! TOML query builtin
//!
//! Non-standard builtin for querying TOML data using dot-separated paths.
//!
//! Usage:
//!   tomlq server.port config.toml
//!   cat config.toml | tomlq server.port
//!   tomlq -r dependencies.serde.version Cargo.toml

use async_trait::async_trait;

use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// tomlq builtin - TOML query tool
pub struct Tomlq;

/// Represents a parsed TOML value.
#[derive(Debug, Clone)]
enum TomlValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Table(Vec<(String, TomlValue)>),
}

impl TomlValue {
    /// Format value for display.
    fn display(&self, raw: bool) -> String {
        match self {
            TomlValue::String(s) => {
                if raw {
                    s.clone()
                } else {
                    format!("\"{}\"", s)
                }
            }
            TomlValue::Integer(n) => n.to_string(),
            TomlValue::Boolean(b) => b.to_string(),
            TomlValue::Table(entries) => {
                // Display as TOML fragment
                let mut out = String::new();
                for (k, v) in entries {
                    match v {
                        TomlValue::Table(sub) => {
                            out.push_str(&format!("[{}]\n", k));
                            for (sk, sv) in sub {
                                out.push_str(&format!("{} = {}\n", sk, sv.to_toml()));
                            }
                        }
                        _ => {
                            out.push_str(&format!("{} = {}\n", k, v.to_toml()));
                        }
                    }
                }
                out
            }
        }
    }

    /// Format value as TOML syntax.
    fn to_toml(&self) -> String {
        match self {
            TomlValue::String(s) => format!("\"{}\"", s),
            TomlValue::Integer(n) => n.to_string(),
            TomlValue::Boolean(b) => b.to_string(),
            TomlValue::Table(entries) => {
                let mut out = String::new();
                for (k, v) in entries {
                    out.push_str(&format!("{} = {}\n", k, v.to_toml()));
                }
                out
            }
        }
    }

    /// Look up a value by dot-separated path.
    fn query(&self, path: &str) -> Option<&TomlValue> {
        if path.is_empty() {
            return Some(self);
        }
        let parts: Vec<&str> = path.splitn(2, '.').collect();
        let key = parts[0];
        let rest = if parts.len() > 1 { parts[1] } else { "" };

        match self {
            TomlValue::Table(entries) => {
                for (k, v) in entries {
                    if k == key {
                        if rest.is_empty() {
                            return Some(v);
                        }
                        return v.query(rest);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// Parse a TOML value from a raw string (right side of `=`).
fn parse_toml_value(raw: &str) -> TomlValue {
    let s = raw.trim();

    // Boolean
    if s == "true" {
        return TomlValue::Boolean(true);
    }
    if s == "false" {
        return TomlValue::Boolean(false);
    }

    // String (double-quoted)
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return TomlValue::String(inner.to_string());
    }

    // String (single-quoted / literal)
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return TomlValue::String(inner.to_string());
    }

    // Integer
    if let Ok(n) = s.parse::<i64>() {
        return TomlValue::Integer(n);
    }

    // Fallback: bare string
    TomlValue::String(s.to_string())
}

/// Parse TOML content into a root table.
fn parse_toml(content: &str) -> TomlValue {
    let mut root: Vec<(String, TomlValue)> = Vec::new();
    // Stack of section path components, e.g. ["server"] or ["database", "pool"]
    let mut current_section: Vec<String> = Vec::new();
    // Entries for the current section
    let mut current_entries: Vec<(String, TomlValue)> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Section header: [section] or [section.subsection]
        if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.starts_with("[[") {
            // Flush current section
            flush_section(&mut root, &current_section, &mut current_entries);

            let section_path = &trimmed[1..trimmed.len() - 1].trim();
            current_section = section_path
                .split('.')
                .map(|s| s.trim().to_string())
                .collect();
            current_entries = Vec::new();
            continue;
        }

        // Key = value
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
            let value_str = trimmed[eq_pos + 1..].trim();
            // Strip inline comments (not inside strings)
            let value_str = strip_inline_comment(value_str);
            let value = parse_toml_value(&value_str);
            current_entries.push((key, value));
        }
    }

    // Flush remaining section
    flush_section(&mut root, &current_section, &mut current_entries);

    TomlValue::Table(root)
}

/// Strip inline comment from a value string (respecting quotes).
///
/// THREAT[TM-UNI-001]: `char_indices` because the offset is used to slice
/// `s` by bytes. Enumerating `s.chars()` gave a *char* index, so a value
/// holding any multi-byte character before the `#` sliced inside that
/// character and panicked -- `k='\u{f8ff}\u{f8ff}\u{f8ff}' #c` reached
/// `end byte index 6 is not a char boundary` and surfaced as
/// `tomlq: builtin failed unexpectedly` (nightly `Fuzz Testing`,
/// `tomlq_fuzz`). Same class as the awk option-parser split fixed on
/// 2026-11-24. It also drops the `Vec<char>` allocation.
fn strip_inline_comment(s: &str) -> String {
    let mut in_quotes = false;
    let mut quote_char = ' ';
    for (i, c) in s.char_indices() {
        if in_quotes {
            if c == quote_char {
                in_quotes = false;
            }
        } else if c == '"' || c == '\'' {
            in_quotes = true;
            quote_char = c;
        } else if c == '#' {
            return s[..i].trim().to_string();
        }
    }
    s.to_string()
}

/// Flush current section entries into the root table at the given path.
fn flush_section(
    root: &mut Vec<(String, TomlValue)>,
    section_path: &[String],
    entries: &mut Vec<(String, TomlValue)>,
) {
    if entries.is_empty() && section_path.is_empty() {
        return;
    }

    if section_path.is_empty() {
        // Top-level entries
        root.append(entries);
        return;
    }

    let section_value = TomlValue::Table(std::mem::take(entries));

    // Build nested table from path, e.g. ["a", "b"] -> Table("a" -> Table("b" -> entries))
    // For simplicity: merge into root at first key, nesting deeper keys.
    insert_at_path(root, section_path, section_value);
}

/// Insert a value at a nested path in the table entries.
fn insert_at_path(entries: &mut Vec<(String, TomlValue)>, path: &[String], value: TomlValue) {
    if path.is_empty() {
        return;
    }

    let key = &path[0];

    if path.len() == 1 {
        // Check if key already exists (merge tables)
        for entry in entries.iter_mut() {
            if &entry.0 == key
                && let (TomlValue::Table(existing), TomlValue::Table(new)) = (&mut entry.1, &value)
            {
                existing.extend(new.iter().cloned());
                return;
            }
        }
        entries.push((key.clone(), value));
        return;
    }

    // Nested: find or create intermediate table
    for entry in entries.iter_mut() {
        if &entry.0 == key
            && let TomlValue::Table(ref mut sub) = entry.1
        {
            insert_at_path(sub, &path[1..], value);
            return;
        }
    }

    // Create intermediate table
    let mut sub = Vec::new();
    insert_at_path(&mut sub, &path[1..], value);
    entries.push((key.clone(), TomlValue::Table(sub)));
}

#[async_trait]
impl Builtin for Tomlq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "tomlq: usage: tomlq [-r] [-t] QUERY [FILE]\n".to_string(),
                1,
            ));
        }

        let mut raw = false;
        let mut as_toml = false;
        let mut query: Option<String> = None;
        let mut file_arg: Option<String> = None;

        for arg in ctx.args {
            match arg.as_str() {
                "-r" => raw = true,
                "-t" => as_toml = true,
                _ => {
                    if query.is_none() {
                        query = Some(arg.clone());
                    } else {
                        file_arg = Some(arg.clone());
                    }
                }
            }
        }

        let query = match query {
            Some(q) => q,
            None => {
                return Ok(ExecResult::err("tomlq: missing query\n".to_string(), 1));
            }
        };

        let content = if let Some(path_str) = &file_arg {
            let path = resolve_path(ctx.cwd, path_str);
            match read_text_file(ctx.fs.as_ref(), &path, "tomlq").await {
                Ok(text) => text,
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("tomlq: cannot read '{}'\n", path_str),
                        1,
                    ));
                }
            }
        } else if let Some(stdin) = ctx.stdin {
            stdin.to_string()
        } else {
            return Ok(ExecResult::err("tomlq: no input\n".to_string(), 1));
        };

        let root = parse_toml(&content);

        match root.query(&query) {
            Some(val) => {
                let output = if as_toml {
                    val.to_toml()
                } else {
                    val.display(raw)
                };
                Ok(ExecResult::ok(format!("{}\n", output.trim_end())))
            }
            None => Ok(ExecResult::err(
                format!("tomlq: path '{}' not found\n", query),
                1,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offset `strip_inline_comment` slices with is a byte offset, so a
    /// multi-byte character before the `#` must not land inside it.
    #[test]
    fn strip_inline_comment_handles_multibyte_before_the_hash() {
        // Private-use U+F8FF is three bytes: a char index would point inside it.
        assert_eq!(
            strip_inline_comment("'\u{f8ff}\u{f8ff}\u{f8ff}' #c"),
            "'\u{f8ff}\u{f8ff}\u{f8ff}'"
        );
        assert_eq!(strip_inline_comment("\u{f8ff}#c"), "\u{f8ff}");
        assert_eq!(
            strip_inline_comment("\u{1f600}\u{1f600} # tail"),
            "\u{1f600}\u{1f600}"
        );
        // A `#` inside quotes is still part of the value.
        assert_eq!(
            strip_inline_comment("'\u{f8ff}#not a comment'"),
            "'\u{f8ff}#not a comment'"
        );
        // ASCII behavior is unchanged.
        assert_eq!(strip_inline_comment("1 #c"), "1");
        assert_eq!(strip_inline_comment("1"), "1");
        assert_eq!(strip_inline_comment("#c"), "");
    }
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Tomlq.execute(ctx).await.unwrap()
    }

    async fn run_with_file(args: &[&str], filename: &str, content: &str) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(&PathBuf::from(filename), content.as_bytes())
            .await
            .unwrap();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Tomlq.execute(ctx).await.unwrap()
    }

    const SAMPLE_TOML: &str = r#"
title = "My Config"
debug = true
max_retries = 3

[server]
host = "localhost"
port = 8080

[database]
url = "postgres://localhost/mydb"
pool_size = 5

[database.options]
timeout = 30
"#;

    #[tokio::test]
    async fn test_no_args() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_query_top_level_string() {
        let r = run(&["title"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "\"My Config\"");
    }

    #[tokio::test]
    async fn test_query_top_level_string_raw() {
        let r = run(&["-r", "title"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "My Config");
    }

    #[tokio::test]
    async fn test_query_top_level_boolean() {
        let r = run(&["debug"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "true");
    }

    #[tokio::test]
    async fn test_query_top_level_integer() {
        let r = run(&["max_retries"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "3");
    }

    #[tokio::test]
    async fn test_query_section_value() {
        let r = run(&["server.port"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "8080");
    }

    #[tokio::test]
    async fn test_query_nested_section() {
        let r = run(&["database.options.timeout"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "30");
    }

    #[tokio::test]
    async fn test_query_not_found() {
        let r = run(&["nonexistent"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("not found"));
    }

    #[tokio::test]
    async fn test_query_section_as_table() {
        let r = run(&["-t", "server"], Some(SAMPLE_TOML)).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("host"));
        assert!(r.stdout.contains("port"));
    }

    #[tokio::test]
    async fn test_read_from_file() {
        let r = run_with_file(
            &["server.host", "/config.toml"],
            "/config.toml",
            SAMPLE_TOML,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("localhost"));
    }

    #[tokio::test]
    async fn test_no_input() {
        let r = run(&["key"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("no input"));
    }

    #[tokio::test]
    async fn test_inline_comment_stripped() {
        let toml = "port = 8080 # the port\n";
        let r = run(&["port"], Some(toml)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "8080");
    }

    #[tokio::test]
    async fn test_comment_inside_string_preserved() {
        let toml = "msg = \"hello # world\"\n";
        let r = run(&["-r", "msg"], Some(toml)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "hello # world");
    }

    // TM-INF-022: malformed-input stderr must not leak `toml` crate Debug.
    #[tokio::test]
    async fn no_leak_malformed_input() {
        let r =
            crate::builtins::debug_leak_check::run(r#"echo 'not = toml = nope' | tomlq '.foo'"#)
                .await;
        crate::builtins::debug_leak_check::assert_no_leak(
            &r,
            "tomlq_malformed_input",
            &["toml::de::Error", "Error { line:"],
        );
    }
}
