//! YAML query builtin
//!
//! Non-standard builtin for querying YAML data using dot-separated paths.
//!
//! Usage:
//!   yaml get server.port config.yml
//!   yaml keys config.yml
//!   yaml length config.yml
//!   yaml type server.port config.yml
//!   cat config.yml | yaml get server.port

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// yaml builtin - YAML query tool
pub struct Yaml;

/// Represents a parsed YAML value.
#[derive(Debug, Clone)]
enum YamlValue {
    Null,
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<YamlValue>),
    Map(Vec<(String, YamlValue)>),
}

impl YamlValue {
    /// Format value for display.
    fn display(&self, raw: bool) -> String {
        match self {
            YamlValue::Null => "null".to_string(),
            YamlValue::String(s) => {
                if raw {
                    s.clone()
                } else {
                    format!("\"{}\"", s)
                }
            }
            YamlValue::Integer(n) => n.to_string(),
            YamlValue::Float(f) => f.to_string(),
            YamlValue::Boolean(b) => b.to_string(),
            YamlValue::List(items) => {
                let mut out = String::new();
                for item in items {
                    out.push_str(&format!("- {}\n", item.display(raw)));
                }
                out
            }
            YamlValue::Map(entries) => {
                let mut out = String::new();
                for (k, v) in entries {
                    match v {
                        YamlValue::Map(_) | YamlValue::List(_) => {
                            out.push_str(&format!("{}:\n", k));
                            for line in v.display(raw).lines() {
                                out.push_str(&format!("  {}\n", line));
                            }
                        }
                        _ => {
                            out.push_str(&format!("{}: {}\n", k, v.display(raw)));
                        }
                    }
                }
                out
            }
        }
    }

    /// Look up a value by dot-separated path.
    fn query(&self, path: &str) -> Option<&YamlValue> {
        if path.is_empty() {
            return Some(self);
        }
        let parts: Vec<&str> = path.splitn(2, '.').collect();
        let key = parts[0];
        let rest = if parts.len() > 1 { parts[1] } else { "" };

        match self {
            YamlValue::Map(entries) => {
                for (k, v) in entries {
                    if k == key {
                        if rest.is_empty() {
                            return Some(v);
                        }
                        return v.query(rest);
                    }
                }
                // Try numeric index on map (unlikely but harmless)
                None
            }
            YamlValue::List(items) => {
                if let Ok(idx) = key.parse::<usize>() {
                    let item = items.get(idx)?;
                    if rest.is_empty() {
                        return Some(item);
                    }
                    return item.query(rest);
                }
                None
            }
            _ => None,
        }
    }

    /// Type name for the `type` subcommand.
    fn type_name(&self) -> &'static str {
        match self {
            YamlValue::Null => "null",
            YamlValue::String(_) => "string",
            YamlValue::Integer(_) => "integer",
            YamlValue::Float(_) => "float",
            YamlValue::Boolean(_) => "boolean",
            YamlValue::List(_) => "list",
            YamlValue::Map(_) => "map",
        }
    }

    /// Length: number of items in list/map, string length, or 1 for scalars.
    fn length(&self) -> usize {
        match self {
            YamlValue::List(items) => items.len(),
            YamlValue::Map(entries) => entries.len(),
            YamlValue::String(s) => s.len(),
            YamlValue::Null => 0,
            _ => 1,
        }
    }

    /// Top-level keys (map only).
    fn keys(&self) -> Option<Vec<&str>> {
        match self {
            YamlValue::Map(entries) => Some(entries.iter().map(|(k, _)| k.as_str()).collect()),
            _ => None,
        }
    }
}

/// Parse a raw YAML scalar value.
fn parse_yaml_scalar(s: &str) -> YamlValue {
    let trimmed = s.trim();

    if trimmed.is_empty() || trimmed == "~" || trimmed == "null" {
        return YamlValue::Null;
    }

    if trimmed == "true" || trimmed == "yes" {
        return YamlValue::Boolean(true);
    }
    if trimmed == "false" || trimmed == "no" {
        return YamlValue::Boolean(false);
    }

    // Quoted string
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        if trimmed.len() >= 2 {
            return YamlValue::String(trimmed[1..trimmed.len() - 1].to_string());
        }
        return YamlValue::String(String::new());
    }

    // Integer
    if let Ok(n) = trimmed.parse::<i64>() {
        return YamlValue::Integer(n);
    }

    // Float
    if let Ok(f) = trimmed.parse::<f64>() {
        return YamlValue::Float(f);
    }

    YamlValue::String(trimmed.to_string())
}

/// Indentation level (number of leading spaces).
fn indent_level(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

// THREAT[TM-DOS-051]: Maximum recursion depth for YAML parsing.
// Prevents stack overflow from deeply nested YAML documents.
const MAX_YAML_DEPTH: usize = 100;

/// Parse YAML content into a value.
/// Supports maps, lists, and scalars with 2-space indentation.
fn parse_yaml(content: &str) -> YamlValue {
    let lines: Vec<&str> = content.lines().collect();
    let (val, _) = parse_yaml_block(&lines, 0, 0, 0);
    val
}

/// Parse a block of YAML lines starting at `start` with expected `base_indent`.
/// Returns the parsed value and the index of the next unprocessed line.
fn parse_yaml_block(
    lines: &[&str],
    start: usize,
    base_indent: usize,
    depth: usize,
) -> (YamlValue, usize) {
    // THREAT[TM-DOS-051]: Prevent stack overflow from deeply nested YAML
    if depth > MAX_YAML_DEPTH {
        return (
            YamlValue::String(format!(
                "ERROR: maximum nesting depth exceeded ({})",
                MAX_YAML_DEPTH
            )),
            start,
        );
    }
    if start >= lines.len() {
        return (YamlValue::Null, start);
    }

    // Skip blank lines and comments to find first meaningful line
    let mut i = start;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            break;
        }
        i += 1;
    }

    if i >= lines.len() {
        return (YamlValue::Null, i);
    }

    let first_indent = indent_level(lines[i]);
    if first_indent < base_indent {
        return (YamlValue::Null, i);
    }

    let first_trimmed = lines[i].trim();

    // Check if this is a list
    if first_trimmed.starts_with("- ") || first_trimmed == "-" {
        return parse_yaml_list(lines, i, first_indent, depth);
    }

    // Otherwise it's a map (key: value)
    if first_trimmed.contains(": ") || first_trimmed.ends_with(':') {
        return parse_yaml_map(lines, i, first_indent, depth);
    }

    // Bare scalar
    (parse_yaml_scalar(first_trimmed), i + 1)
}

/// Parse a YAML map starting at line `start` with given `base_indent`.
fn parse_yaml_map(
    lines: &[&str],
    start: usize,
    base_indent: usize,
    depth: usize,
) -> (YamlValue, usize) {
    let mut entries: Vec<(String, YamlValue)> = Vec::new();
    let mut i = start;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Skip blanks and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }

        let cur_indent = indent_level(lines[i]);
        if cur_indent < base_indent {
            break;
        }
        if cur_indent > base_indent {
            // Belongs to a nested block already consumed; skip
            i += 1;
            continue;
        }

        // Must be a key line at base_indent
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim().to_string();
            let after_colon = trimmed[colon_pos + 1..].trim();

            if after_colon.is_empty() {
                // Value is a nested block on subsequent lines
                let (val, next) = parse_yaml_block(lines, i + 1, base_indent + 2, depth + 1);
                entries.push((key, val));
                i = next;
            } else {
                // Inline value
                entries.push((key, parse_yaml_scalar(after_colon)));
                i += 1;
            }
        } else {
            // Not a valid map entry at this indent; done
            break;
        }
    }

    (YamlValue::Map(entries), i)
}

/// Parse a YAML list starting at line `start` with given `base_indent`.
fn parse_yaml_list(
    lines: &[&str],
    start: usize,
    base_indent: usize,
    depth: usize,
) -> (YamlValue, usize) {
    let mut items: Vec<YamlValue> = Vec::new();
    let mut i = start;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }

        let cur_indent = indent_level(lines[i]);
        if cur_indent < base_indent {
            break;
        }
        if cur_indent > base_indent {
            i += 1;
            continue;
        }

        if !trimmed.starts_with("- ") && trimmed != "-" {
            break;
        }

        let after_dash = if trimmed == "-" { "" } else { &trimmed[2..] };

        if after_dash.is_empty() {
            // Nested block item
            let (val, next) = parse_yaml_block(lines, i + 1, base_indent + 2, depth + 1);
            items.push(val);
            i = next;
        } else if after_dash.contains(": ") || after_dash.ends_with(':') {
            // Inline map item starting on same line as dash.
            // Reconstruct as a map line with indent = base_indent + 2
            let fake_indent = " ".repeat(base_indent + 2);
            // Collect this line (without dash) plus subsequent indented lines
            let mut block_lines: Vec<String> = Vec::new();
            block_lines.push(format!("{}{}", fake_indent, after_dash));
            let mut j = i + 1;
            while j < lines.len() {
                let jt = lines[j].trim();
                if jt.is_empty() || jt.starts_with('#') {
                    j += 1;
                    continue;
                }
                let ji = indent_level(lines[j]);
                if ji <= base_indent {
                    break;
                }
                block_lines.push(lines[j].to_string());
                j += 1;
            }
            let block_strs: Vec<&str> = block_lines.iter().map(|s| s.as_str()).collect();
            let (val, _) = parse_yaml_map(&block_strs, 0, base_indent + 2, depth + 1);
            items.push(val);
            i = j;
        } else {
            items.push(parse_yaml_scalar(after_dash));
            i += 1;
        }
    }

    (YamlValue::List(items), i)
}

/// Read input from file arg or stdin.
async fn read_input<'a>(
    ctx: &Context<'a>,
    file_arg: Option<&str>,
) -> std::result::Result<String, ExecResult> {
    if let Some(path_str) = file_arg {
        let path = resolve_path(ctx.cwd, path_str);
        match ctx.fs.read_file(&path).await {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(_) => Err(ExecResult::err(
                format!("yaml: cannot read '{}'\n", path_str),
                1,
            )),
        }
    } else if let Some(stdin) = ctx.stdin {
        Ok(stdin.to_string())
    } else {
        Err(ExecResult::err("yaml: no input\n".to_string(), 1))
    }
}

#[async_trait]
impl Builtin for Yaml {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "yaml: usage: yaml <subcommand> [options] [file]\nSubcommands: get, keys, length, type\n"
                    .to_string(),
                1,
            ));
        }

        // Parse options and positional args
        let mut raw = false;
        let mut positional: Vec<String> = Vec::new();
        for arg in ctx.args {
            match arg.as_str() {
                "-r" => raw = true,
                _ => positional.push(arg.clone()),
            }
        }

        if positional.is_empty() {
            return Ok(ExecResult::err("yaml: missing subcommand\n".to_string(), 1));
        }

        let subcmd = positional[0].as_str();
        let rest = &positional[1..];

        match subcmd {
            "get" => {
                if rest.is_empty() {
                    return Ok(ExecResult::err(
                        "yaml: get requires a path argument\n".to_string(),
                        1,
                    ));
                }
                let query_path = &rest[0];
                let file_arg = rest.get(1).map(|s| s.as_str());
                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };

                let root = parse_yaml(&content);
                match root.query(query_path) {
                    Some(val) => {
                        let output = val.display(raw);
                        Ok(ExecResult::ok(format!("{}\n", output.trim_end())))
                    }
                    None => Ok(ExecResult::err(
                        format!("yaml: path '{}' not found\n", query_path),
                        1,
                    )),
                }
            }
            "keys" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };

                let root = parse_yaml(&content);
                match root.keys() {
                    Some(keys) => {
                        let out = keys.join("\n");
                        Ok(ExecResult::ok(format!("{out}\n")))
                    }
                    None => Ok(ExecResult::err("yaml: value is not a map\n".to_string(), 1)),
                }
            }
            "length" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };

                let root = parse_yaml(&content);
                Ok(ExecResult::ok(format!("{}\n", root.length())))
            }
            "type" => {
                if rest.is_empty() {
                    // Type of root
                    let file_arg: Option<&str> = None;
                    let content = match read_input(&ctx, file_arg).await {
                        Ok(c) => c,
                        Err(e) => return Ok(e),
                    };
                    let root = parse_yaml(&content);
                    Ok(ExecResult::ok(format!("{}\n", root.type_name())))
                } else {
                    let query_path = &rest[0];
                    let file_arg = rest.get(1).map(|s| s.as_str());
                    let content = match read_input(&ctx, file_arg).await {
                        Ok(c) => c,
                        Err(e) => return Ok(e),
                    };
                    let root = parse_yaml(&content);
                    match root.query(query_path) {
                        Some(val) => Ok(ExecResult::ok(format!("{}\n", val.type_name()))),
                        None => Ok(ExecResult::err(
                            format!("yaml: path '{}' not found\n", query_path),
                            1,
                        )),
                    }
                }
            }
            _ => Ok(ExecResult::err(
                format!("yaml: unknown subcommand '{}'\n", subcmd),
                1,
            )),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        Yaml.execute(ctx).await.unwrap()
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
            shell: None,
        };
        Yaml.execute(ctx).await.unwrap()
    }

    const SAMPLE_YAML: &str = "\
server:
  host: localhost
  port: 8080
database:
  url: postgres://localhost/mydb
  pool_size: 5
debug: true
name: my-app
";

    const LIST_YAML: &str = "\
fruits:
  - apple
  - banana
  - cherry
";

    #[tokio::test]
    async fn test_no_args() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_unknown_subcommand() {
        let r = run(&["bogus"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unknown subcommand"));
    }

    #[tokio::test]
    async fn test_get_top_level_string() {
        let r = run(&["get", "name"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "\"my-app\"");
    }

    #[tokio::test]
    async fn test_get_top_level_string_raw() {
        let r = run(&["-r", "get", "name"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "my-app");
    }

    #[tokio::test]
    async fn test_get_top_level_boolean() {
        let r = run(&["get", "debug"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "true");
    }

    #[tokio::test]
    async fn test_get_nested_value() {
        let r = run(&["get", "server.port"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "8080");
    }

    #[tokio::test]
    async fn test_get_nested_string() {
        let r = run(&["-r", "get", "server.host"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "localhost");
    }

    #[tokio::test]
    async fn test_get_not_found() {
        let r = run(&["get", "nonexistent"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("not found"));
    }

    #[tokio::test]
    async fn test_keys() {
        let r = run(&["keys"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("server"));
        assert!(r.stdout.contains("database"));
        assert!(r.stdout.contains("debug"));
        assert!(r.stdout.contains("name"));
    }

    #[tokio::test]
    async fn test_length() {
        let r = run(&["length"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "4");
    }

    #[tokio::test]
    async fn test_type_map() {
        let r = run(&["type", "server"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "map");
    }

    #[tokio::test]
    async fn test_type_integer() {
        let r = run(&["type", "server.port"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "integer");
    }

    #[tokio::test]
    async fn test_type_boolean() {
        let r = run(&["type", "debug"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "boolean");
    }

    #[tokio::test]
    async fn test_list_values() {
        let r = run(&["get", "fruits"], Some(LIST_YAML)).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("apple"));
        assert!(r.stdout.contains("banana"));
        assert!(r.stdout.contains("cherry"));
    }

    #[tokio::test]
    async fn test_list_length() {
        let r = run(&["length"], Some("- a\n- b\n- c\n")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "3");
    }

    #[tokio::test]
    async fn test_null_value() {
        let r = run(&["get", "val"], Some("val: null\n")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "null");
    }

    #[tokio::test]
    async fn test_read_from_file() {
        let r = run_with_file(&["get", "name", "/config.yml"], "/config.yml", SAMPLE_YAML).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("my-app"));
    }

    #[tokio::test]
    async fn test_no_input() {
        let r = run(&["get", "key"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("no input"));
    }

    #[tokio::test]
    async fn test_get_missing_path_arg() {
        let r = run(&["get"], Some(SAMPLE_YAML)).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("requires a path"));
    }

    #[tokio::test]
    async fn test_comments_ignored() {
        let yaml = "# comment\nkey: value\n# another comment\n";
        let r = run(&["-r", "get", "key"], Some(yaml)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "value");
    }
}
