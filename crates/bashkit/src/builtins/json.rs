//! json builtin - simplified JSON processor
//!
//! A simpler alternative to jq for common JSON operations.
//! Uses serde_json (already a dependency) for parsing.
//!
//! Usage:
//!   echo '{"a":1}' | json get .a
//!   echo '{"a":1}' | json set .b 2
//!   echo '{"a":1}' | json keys
//!   echo '[1,2,3]' | json length
//!   echo '"hello"' | json type
//!   echo '{"a":1}' | json format

use async_trait::async_trait;
use serde_json::Value;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// json builtin - simplified JSON processor.
pub struct Json;

/// Parse a dot-separated JSON path like ".foo.bar.0" into path segments.
/// Leading dot is optional. Array indices are numeric segments.
fn parse_path(path: &str) -> Vec<String> {
    let s = path.strip_prefix('.').unwrap_or(path);
    if s.is_empty() {
        return Vec::new();
    }
    s.split('.').map(|seg| seg.to_string()).collect()
}

/// Get a value at a dot-separated path.
fn get_at_path(value: &Value, segments: &[String]) -> Option<Value> {
    let mut current = value;
    for seg in segments {
        match current {
            Value::Object(map) => {
                current = map.get(seg.as_str())?;
            }
            Value::Array(arr) => {
                let idx: usize = seg.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current.clone())
}

/// Set a value at a dot-separated path, returning the modified root.
fn set_at_path(value: &mut Value, segments: &[String], new_val: Value) -> bool {
    if segments.is_empty() {
        *value = new_val;
        return true;
    }

    let seg = &segments[0];
    let rest = &segments[1..];

    if rest.is_empty() {
        // Terminal segment - set directly
        match value {
            Value::Object(map) => {
                map.insert(seg.clone(), new_val);
                true
            }
            Value::Array(arr) => {
                if let Ok(idx) = seg.parse::<usize>() {
                    if idx < arr.len() {
                        arr[idx] = new_val;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    } else {
        // Intermediate segment - recurse
        match value {
            Value::Object(map) => {
                let entry = map
                    .entry(seg.clone())
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                set_at_path(entry, rest, new_val)
            }
            Value::Array(arr) => {
                if let Ok(idx) = seg.parse::<usize>() {
                    if let Some(elem) = arr.get_mut(idx) {
                        set_at_path(elem, rest, new_val)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// Format a JSON value for output. Strings are unquoted, others use JSON repr.
fn format_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => v.to_string(),
    }
}

/// Read JSON input from stdin or file argument.
async fn read_json_input(
    ctx: &Context<'_>,
    file_arg: Option<&str>,
) -> std::result::Result<String, ExecResult> {
    if let Some(file) = file_arg {
        let path = resolve_path(ctx.cwd, file);
        match ctx.fs.read_file(&path).await {
            Ok(bytes) => String::from_utf8(bytes)
                .map_err(|e| ExecResult::err(format!("json: invalid UTF-8 in {file}: {e}\n"), 1)),
            Err(e) => Err(ExecResult::err(format!("json: {file}: {e}\n"), 1)),
        }
    } else if let Some(stdin) = ctx.stdin {
        Ok(stdin.to_string())
    } else {
        Err(ExecResult::err(
            "json: no input (provide file or pipe stdin)\n".to_string(),
            1,
        ))
    }
}

#[async_trait]
impl Builtin for Json {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "json: usage: json <subcommand> [args...]\nSubcommands: get, set, keys, length, type, format, pretty\n".to_string(),
                1,
            ));
        }

        let subcmd = ctx.args[0].as_str();
        let rest = &ctx.args[1..];

        match subcmd {
            "get" => {
                if rest.is_empty() {
                    return Ok(ExecResult::err(
                        "json: get requires a path argument\n".to_string(),
                        1,
                    ));
                }
                let path_str = &rest[0];
                let file_arg = rest.get(1).map(|s| s.as_str());
                let input = match read_json_input(&ctx, file_arg).await {
                    Ok(s) => s,
                    Err(e) => return Ok(e),
                };
                let value: Value = match serde_json::from_str(input.trim()) {
                    Ok(v) => v,
                    Err(e) => return Ok(ExecResult::err(format!("json: invalid JSON: {e}\n"), 1)),
                };
                let segments = parse_path(path_str);
                match get_at_path(&value, &segments) {
                    Some(v) => Ok(ExecResult::ok(format!("{}\n", format_value(&v)))),
                    None => Ok(ExecResult::err(
                        format!("json: path '{}' not found\n", path_str),
                        1,
                    )),
                }
            }
            "set" => {
                if rest.len() < 2 {
                    return Ok(ExecResult::err(
                        "json: set requires PATH and VALUE arguments\n".to_string(),
                        1,
                    ));
                }
                let path_str = &rest[0];
                let raw_value = &rest[1];
                let file_arg = rest.get(2).map(|s| s.as_str());
                let input = match read_json_input(&ctx, file_arg).await {
                    Ok(s) => s,
                    Err(e) => return Ok(e),
                };
                let mut value: Value = match serde_json::from_str(input.trim()) {
                    Ok(v) => v,
                    Err(e) => return Ok(ExecResult::err(format!("json: invalid JSON: {e}\n"), 1)),
                };
                // Parse the new value as JSON, fallback to string
                let new_val: Value = serde_json::from_str(raw_value)
                    .unwrap_or_else(|_| Value::String(raw_value.clone()));
                let segments = parse_path(path_str);
                if set_at_path(&mut value, &segments, new_val) {
                    Ok(ExecResult::ok(format!("{}\n", value)))
                } else {
                    Ok(ExecResult::err(
                        format!("json: cannot set path '{}'\n", path_str),
                        1,
                    ))
                }
            }
            "keys" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let input = match read_json_input(&ctx, file_arg).await {
                    Ok(s) => s,
                    Err(e) => return Ok(e),
                };
                let value: Value = match serde_json::from_str(input.trim()) {
                    Ok(v) => v,
                    Err(e) => return Ok(ExecResult::err(format!("json: invalid JSON: {e}\n"), 1)),
                };
                match value {
                    Value::Object(map) => {
                        let mut out = String::new();
                        for key in map.keys() {
                            out.push_str(key);
                            out.push('\n');
                        }
                        Ok(ExecResult::ok(out))
                    }
                    _ => Ok(ExecResult::err(
                        "json: keys requires an object\n".to_string(),
                        1,
                    )),
                }
            }
            "length" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let input = match read_json_input(&ctx, file_arg).await {
                    Ok(s) => s,
                    Err(e) => return Ok(e),
                };
                let value: Value = match serde_json::from_str(input.trim()) {
                    Ok(v) => v,
                    Err(e) => return Ok(ExecResult::err(format!("json: invalid JSON: {e}\n"), 1)),
                };
                let len = match &value {
                    Value::Array(arr) => arr.len(),
                    Value::Object(map) => map.len(),
                    Value::String(s) => s.len(),
                    _ => {
                        return Ok(ExecResult::err(
                            "json: length requires array, object, or string\n".to_string(),
                            1,
                        ));
                    }
                };
                Ok(ExecResult::ok(format!("{len}\n")))
            }
            "type" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let input = match read_json_input(&ctx, file_arg).await {
                    Ok(s) => s,
                    Err(e) => return Ok(e),
                };
                let value: Value = match serde_json::from_str(input.trim()) {
                    Ok(v) => v,
                    Err(e) => return Ok(ExecResult::err(format!("json: invalid JSON: {e}\n"), 1)),
                };
                let type_name = match &value {
                    Value::Object(_) => "object",
                    Value::Array(_) => "array",
                    Value::String(_) => "string",
                    Value::Number(_) => "number",
                    Value::Bool(_) => "boolean",
                    Value::Null => "null",
                };
                Ok(ExecResult::ok(format!("{type_name}\n")))
            }
            "format" | "pretty" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let input = match read_json_input(&ctx, file_arg).await {
                    Ok(s) => s,
                    Err(e) => return Ok(e),
                };
                let value: Value = match serde_json::from_str(input.trim()) {
                    Ok(v) => v,
                    Err(e) => return Ok(ExecResult::err(format!("json: invalid JSON: {e}\n"), 1)),
                };
                match serde_json::to_string_pretty(&value) {
                    Ok(s) => Ok(ExecResult::ok(format!("{s}\n"))),
                    Err(e) => Ok(ExecResult::err(format!("json: format error: {e}\n"), 1)),
                }
            }
            _ => Ok(ExecResult::err(
                format!("json: unknown subcommand '{subcmd}'\n"),
                1,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run(args: &[&str], stdin: Option<&str>, fs: Option<Arc<InMemoryFs>>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));
        let fs_dyn = fs as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_dyn,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Json.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_no_args() {
        let r = run(&[], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_get_simple() {
        let r = run(&["get", ".name"], Some(r#"{"name":"alice"}"#), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "alice");
    }

    #[tokio::test]
    async fn test_get_nested() {
        let r = run(&["get", ".a.b"], Some(r#"{"a":{"b":42}}"#), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "42");
    }

    #[tokio::test]
    async fn test_get_array_index() {
        let r = run(&["get", ".1"], Some(r#"["a","b","c"]"#), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "b");
    }

    #[tokio::test]
    async fn test_get_not_found() {
        let r = run(&["get", ".missing"], Some(r#"{"a":1}"#), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("not found"));
    }

    #[tokio::test]
    async fn test_set_value() {
        let r = run(
            &["set", ".name", "\"bob\""],
            Some(r#"{"name":"alice"}"#),
            None,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        let output: Value = serde_json::from_str(r.stdout.trim()).unwrap();
        assert_eq!(output["name"], "bob");
    }

    #[tokio::test]
    async fn test_set_new_key() {
        let r = run(&["set", ".age", "30"], Some(r#"{"name":"alice"}"#), None).await;
        assert_eq!(r.exit_code, 0);
        let output: Value = serde_json::from_str(r.stdout.trim()).unwrap();
        assert_eq!(output["age"], 30);
    }

    #[tokio::test]
    async fn test_keys() {
        let r = run(&["keys"], Some(r#"{"b":2,"a":1}"#), None).await;
        assert_eq!(r.exit_code, 0);
        let lines: Vec<&str> = r.stdout.trim().lines().collect();
        assert!(lines.contains(&"a"));
        assert!(lines.contains(&"b"));
    }

    #[tokio::test]
    async fn test_keys_non_object() {
        let r = run(&["keys"], Some("[1,2,3]"), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("requires an object"));
    }

    #[tokio::test]
    async fn test_length_array() {
        let r = run(&["length"], Some("[1,2,3]"), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "3");
    }

    #[tokio::test]
    async fn test_length_object() {
        let r = run(&["length"], Some(r#"{"a":1,"b":2}"#), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_type_object() {
        let r = run(&["type"], Some("{}"), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "object");
    }

    #[tokio::test]
    async fn test_type_string() {
        let r = run(&["type"], Some(r#""hello""#), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "string");
    }

    #[tokio::test]
    async fn test_type_null() {
        let r = run(&["type"], Some("null"), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "null");
    }

    #[tokio::test]
    async fn test_pretty() {
        let r = run(&["pretty"], Some(r#"{"a":1}"#), None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("  \"a\": 1"));
    }

    #[tokio::test]
    async fn test_invalid_json() {
        let r = run(&["keys"], Some("not json"), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("invalid JSON"));
    }

    #[tokio::test]
    async fn test_unknown_subcommand() {
        let r = run(&["nope"], Some("{}"), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unknown subcommand"));
    }

    #[tokio::test]
    async fn test_read_from_file() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn
            .write_file(std::path::Path::new("/data.json"), br#"{"x":99}"#)
            .await
            .unwrap();

        let r = run(&["get", ".x", "/data.json"], None, Some(fs)).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "99");
    }

    #[tokio::test]
    async fn test_no_input() {
        let r = run(&["keys"], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("no input"));
    }
}
