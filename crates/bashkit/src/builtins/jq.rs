//! jq - JSON processor builtin
//!
//! Implements jq functionality using the jaq library.
//!
//! Usage:
//!   echo '{"name":"foo"}' | jq '.name'
//!   jq '.[] | .id' < data.json

use async_trait::async_trait;
use jaq_core::{load, Compiler, Ctx, RcIter};
use jaq_json::Val;
use std::path::Path;

use super::{resolve_path, Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// THREAT[TM-DOS-027]: Maximum nesting depth for JSON input values.
/// Prevents stack overflow when jaq evaluates deeply nested JSON structures
/// like `[[[[...]]]]` or `{"a":{"a":{"a":...}}}`.
/// Also limits filter complexity indirectly since deeply nested filters
/// produce deeply nested parse trees in jaq.
const MAX_JQ_JSON_DEPTH: usize = 100;

/// jq command - JSON processor
pub struct Jq;

impl Jq {
    /// Parse multiple JSON values from input using streaming deserializer.
    /// Handles multi-line JSON, NDJSON, and concatenated JSON values.
    fn parse_json_values(input: &str) -> Result<Vec<serde_json::Value>> {
        use serde_json::Deserializer;

        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let mut vals = Vec::new();
        let stream = Deserializer::from_str(trimmed).into_iter::<serde_json::Value>();
        for result in stream {
            let json_input =
                result.map_err(|e| Error::Execution(format!("jq: invalid JSON: {}", e)))?;
            // THREAT[TM-DOS-027]: Check nesting depth before evaluation
            check_json_depth(&json_input, MAX_JQ_JSON_DEPTH).map_err(Error::Execution)?;
            vals.push(json_input);
        }
        Ok(vals)
    }
}

/// THREAT[TM-DOS-027]: Check JSON nesting depth to prevent stack overflow
/// during jaq filter evaluation on deeply nested input.
fn check_json_depth(
    value: &serde_json::Value,
    max_depth: usize,
) -> std::result::Result<(), String> {
    fn measure_depth(
        v: &serde_json::Value,
        current: usize,
        max: usize,
    ) -> std::result::Result<(), String> {
        if current > max {
            return Err(format!(
                "jq: JSON nesting too deep ({} levels, max {})",
                current, max
            ));
        }
        match v {
            serde_json::Value::Array(arr) => {
                for item in arr {
                    measure_depth(item, current + 1, max)?;
                }
            }
            serde_json::Value::Object(map) => {
                for (_k, item) in map {
                    measure_depth(item, current + 1, max)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    measure_depth(value, 0, max_depth)
}

/// Recursively sort all object keys in a JSON value
fn sort_json_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<(String, serde_json::Value)> = map
                .into_iter()
                .map(|(k, v)| (k, sort_json_keys(v)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sort_json_keys).collect())
        }
        other => other,
    }
}

/// Format JSON with tabs instead of spaces
fn format_with_tabs(value: &serde_json::Value) -> String {
    let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
    // Replace 2-space indentation with tabs
    let mut result = String::new();
    for line in pretty.lines() {
        let spaces = line.len() - line.trim_start().len();
        let tabs = spaces / 2;
        result.push_str(&"\t".repeat(tabs));
        result.push_str(line.trim_start());
        result.push('\n');
    }
    // Remove trailing newline to match pattern
    result.truncate(result.trim_end_matches('\n').len());
    result
}

#[async_trait]
impl Builtin for Jq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Check for --version flag first
        for arg in ctx.args {
            if arg == "-V" || arg == "--version" {
                return Ok(ExecResult::ok("jq-1.7.1\n".to_string()));
            }
        }

        // Parse arguments for flags using index-based loop to support
        // multi-arg flags like --arg name value and --argjson name value.
        let mut raw_output = false;
        let mut compact_output = false;
        let mut null_input = false;
        let mut sort_keys = false;
        let mut slurp = false;
        let mut exit_status = false;
        let mut tab_indent = false;
        let mut join_output = false;
        let mut filter = ".";
        let mut file_args: Vec<&str> = Vec::new();
        // Store variable bindings as (name, serde_json::Value) to avoid
        // holding non-Send jaq Val across await points.
        let mut var_bindings: Vec<(String, serde_json::Value)> = Vec::new();

        let mut found_filter = false;
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if found_filter {
                // Everything after the filter is a file argument
                file_args.push(arg);
                i += 1;
                continue;
            }

            if arg == "--" {
                // End of options: next arg is filter, rest are files
                i += 1;
                if i < ctx.args.len() {
                    filter = &ctx.args[i];
                    found_filter = true;
                }
                i += 1;
                continue;
            }

            if arg == "--raw-output" {
                raw_output = true;
            } else if arg == "--compact-output" {
                compact_output = true;
            } else if arg == "--null-input" {
                null_input = true;
            } else if arg == "--sort-keys" {
                sort_keys = true;
            } else if arg == "--slurp" {
                slurp = true;
            } else if arg == "--exit-status" {
                exit_status = true;
            } else if arg == "--tab" {
                tab_indent = true;
            } else if arg == "--join-output" {
                join_output = true;
            } else if arg == "--arg" {
                // --arg name value: bind $name to string value
                if i + 2 < ctx.args.len() {
                    let name = format!("${}", &ctx.args[i + 1]);
                    let value = serde_json::Value::String(ctx.args[i + 2].to_string());
                    var_bindings.push((name, value));
                    i += 3;
                    continue;
                }
                i += 1;
                continue;
            } else if arg == "--argjson" {
                // --argjson name value: bind $name to parsed JSON value
                if i + 2 < ctx.args.len() {
                    let name = format!("${}", &ctx.args[i + 1]);
                    let json_val: serde_json::Value = serde_json::from_str(&ctx.args[i + 2])
                        .map_err(|e| {
                            Error::Execution(format!("jq: invalid JSON for --argjson: {}", e))
                        })?;
                    var_bindings.push((name, json_val));
                    i += 3;
                    continue;
                }
                i += 1;
                continue;
            } else if arg == "--indent" {
                // --indent N: skip the numeric argument (use default formatting)
                i += 2;
                continue;
            } else if arg == "--args" || arg == "--jsonargs" {
                // Remaining args are positional, not files; skip for now
                i += 1;
                continue;
            } else if arg.starts_with("--") {
                // Unknown long flag: skip
            } else if arg.starts_with('-') && arg.len() > 1 {
                // Short flag(s): may be combined like -rn, -sc, -snr
                for ch in arg[1..].chars() {
                    match ch {
                        'r' => raw_output = true,
                        'c' => compact_output = true,
                        'n' => null_input = true,
                        'S' => sort_keys = true,
                        's' => slurp = true,
                        'e' => exit_status = true,
                        'j' => join_output = true,
                        _ => {} // ignore unknown short flags
                    }
                }
            } else {
                // Non-flag argument: this is the filter
                filter = arg;
                found_filter = true;
            }
            i += 1;
        }

        // Build input: read from file arguments if provided, otherwise stdin
        let file_content: String;
        let input = if !file_args.is_empty() {
            let mut combined = String::new();
            for file_arg in &file_args {
                let path = resolve_path(ctx.cwd, file_arg);
                match ctx.fs.read_file(Path::new(&path)).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        if !combined.is_empty() && !combined.ends_with('\n') {
                            combined.push('\n');
                        }
                        combined.push_str(&text);
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(
                            format!("jq: Could not open file {}: {}\n", file_arg, e),
                            2,
                        ));
                    }
                }
            }
            file_content = combined;
            file_content.as_str()
        } else {
            ctx.stdin.unwrap_or("")
        };

        // If no input and not null_input mode, return empty
        if input.trim().is_empty() && !null_input {
            return Ok(ExecResult::ok(String::new()));
        }

        // Set up the loader with standard library definitions
        let loader = load::Loader::new(jaq_std::defs().chain(jaq_json::defs()));
        let arena = load::Arena::default();

        // Parse the filter
        let program = load::File {
            code: filter,
            path: (),
        };

        let modules = loader.load(&arena, program).map_err(|errs| {
            Error::Execution(format!(
                "jq: parse error: {}",
                errs.into_iter()
                    .map(|e| format!("{:?}", e))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

        // Compile the filter, registering any --arg/--argjson variable names
        let var_names: Vec<&str> = var_bindings.iter().map(|(n, _)| n.as_str()).collect();
        let compiler = Compiler::default().with_funs(jaq_std::funs().chain(jaq_json::funs()));
        let compiler = if var_names.is_empty() {
            compiler
        } else {
            compiler.with_global_vars(var_names.iter().copied())
        };
        let filter = compiler.compile(modules).map_err(|errs| {
            Error::Execution(format!(
                "jq: compile error: {}",
                errs.into_iter()
                    .map(|e| format!("{:?}", e))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

        // Process input as JSON
        let mut output = String::new();

        // Build list of inputs to process
        let inputs_to_process: Vec<Val> = if null_input {
            // -n flag: use null as input
            vec![Val::from(serde_json::Value::Null)]
        } else if slurp {
            // -s flag: read all inputs into a single array
            let vals = Self::parse_json_values(input)?;
            vec![Val::from(serde_json::Value::Array(vals))]
        } else {
            // Parse all JSON values from input (handles multi-line and NDJSON)
            let json_vals = Self::parse_json_values(input)?;
            json_vals.into_iter().map(Val::from).collect()
        };

        // Track for -e exit status
        let mut has_output = false;
        let mut all_null_or_false = true;

        for jaq_input in inputs_to_process {
            // Create empty inputs iterator
            let inputs = RcIter::new(core::iter::empty());

            // Run the filter, passing any --arg/--argjson variable values
            let var_vals: Vec<Val> = var_bindings
                .iter()
                .map(|(_, v)| Val::from(v.clone()))
                .collect();
            let ctx = Ctx::new(var_vals, &inputs);
            for result in filter.run((ctx, jaq_input)) {
                match result {
                    Ok(val) => {
                        has_output = true;
                        // Convert back to serde_json::Value and format
                        let json: serde_json::Value = val.into();

                        // Track for -e exit status
                        if !matches!(
                            json,
                            serde_json::Value::Null | serde_json::Value::Bool(false)
                        ) {
                            all_null_or_false = false;
                        }

                        // Apply sort_keys if -S flag is set
                        let json = if sort_keys {
                            sort_json_keys(json)
                        } else {
                            json
                        };

                        // -j implies raw output for strings
                        let effective_raw = raw_output || join_output;

                        // In raw mode or join mode, strings are output without quotes
                        if effective_raw {
                            if let serde_json::Value::String(s) = &json {
                                output.push_str(s);
                                if !join_output {
                                    output.push('\n');
                                }
                            } else {
                                // For non-strings, use appropriate formatting
                                let formatted = if compact_output {
                                    serde_json::to_string(&json)
                                } else if tab_indent {
                                    Ok(format_with_tabs(&json))
                                } else {
                                    match &json {
                                        serde_json::Value::Array(_)
                                        | serde_json::Value::Object(_) => {
                                            serde_json::to_string_pretty(&json)
                                        }
                                        _ => serde_json::to_string(&json),
                                    }
                                };
                                match formatted {
                                    Ok(s) => {
                                        output.push_str(&s);
                                        if !join_output {
                                            output.push('\n');
                                        }
                                    }
                                    Err(e) => {
                                        return Err(Error::Execution(format!(
                                            "jq: output error: {}",
                                            e
                                        )));
                                    }
                                }
                            }
                        } else if compact_output {
                            // Compact mode: no pretty-printing
                            match serde_json::to_string(&json) {
                                Ok(s) => {
                                    output.push_str(&s);
                                    output.push('\n');
                                }
                                Err(e) => {
                                    return Err(Error::Execution(format!(
                                        "jq: output error: {}",
                                        e
                                    )));
                                }
                            }
                        } else if tab_indent {
                            // Tab indentation mode
                            let formatted = format_with_tabs(&json);
                            output.push_str(&formatted);
                            output.push('\n');
                        } else {
                            // Use pretty-print for arrays/objects to match real jq behavior
                            let formatted = match &json {
                                serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                                    serde_json::to_string_pretty(&json)
                                }
                                _ => serde_json::to_string(&json),
                            };
                            match formatted {
                                Ok(s) => {
                                    output.push_str(&s);
                                    output.push('\n');
                                }
                                Err(e) => {
                                    return Err(Error::Execution(format!(
                                        "jq: output error: {}",
                                        e
                                    )));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(Error::Execution(format!("jq: runtime error: {:?}", e)));
                    }
                }
            }
        }

        // Ensure output ends with newline if there's output (for consistency)
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }

        // Handle -e exit status flag
        if exit_status && (!has_output || all_null_or_false) {
            return Ok(ExecResult::with_code(output, 1));
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_jq(filter: &str, input: &str) -> Result<String> {
        let jq = Jq;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args = vec![filter.to_string()];

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: Some(input),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = jq.execute(ctx).await?;
        Ok(result.stdout)
    }

    #[tokio::test]
    async fn test_jq_identity() {
        let result = run_jq(".", r#"{"name":"test"}"#).await.unwrap();
        // Pretty-printed output to match real jq behavior
        assert_eq!(result.trim(), "{\n  \"name\": \"test\"\n}");
    }

    #[tokio::test]
    async fn test_jq_field_access() {
        let result = run_jq(".name", r#"{"name":"foo","id":42}"#).await.unwrap();
        assert_eq!(result.trim(), r#""foo""#);
    }

    #[tokio::test]
    async fn test_jq_array_index() {
        let result = run_jq(".[1]", r#"["a","b","c"]"#).await.unwrap();
        assert_eq!(result.trim(), r#""b""#);
    }

    #[tokio::test]
    async fn test_jq_nested() {
        let result = run_jq(".user.name", r#"{"user":{"name":"alice"}}"#)
            .await
            .unwrap();
        assert_eq!(result.trim(), r#""alice""#);
    }

    #[tokio::test]
    async fn test_jq_keys() {
        let result = run_jq("keys", r#"{"b":1,"a":2}"#).await.unwrap();
        // Pretty-printed array output to match real jq behavior
        assert_eq!(result.trim(), "[\n  \"a\",\n  \"b\"\n]");
    }

    #[tokio::test]
    async fn test_jq_length() {
        let result = run_jq("length", r#"[1,2,3,4,5]"#).await.unwrap();
        assert_eq!(result.trim(), "5");
    }

    async fn run_jq_with_args(args: &[&str], input: &str) -> Result<String> {
        let jq = Jq;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: Some(input),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = jq.execute(ctx).await?;
        Ok(result.stdout)
    }

    #[tokio::test]
    async fn test_jq_raw_output() {
        let result = run_jq_with_args(&["-r", ".name"], r#"{"name":"test"}"#)
            .await
            .unwrap();
        assert_eq!(result.trim(), "test");
    }

    #[tokio::test]
    async fn test_jq_raw_output_long_flag() {
        let result = run_jq_with_args(&["--raw-output", ".name"], r#"{"name":"test"}"#)
            .await
            .unwrap();
        assert_eq!(result.trim(), "test");
    }

    #[tokio::test]
    async fn test_jq_version() {
        let result = run_jq_with_args(&["--version"], "").await.unwrap();
        assert!(result.starts_with("jq-"));
    }

    #[tokio::test]
    async fn test_jq_version_short() {
        let result = run_jq_with_args(&["-V"], "").await.unwrap();
        assert!(result.starts_with("jq-"));
    }

    /// TM-DOS-027: Deeply nested JSON arrays must be rejected
    /// Note: serde_json has a built-in recursion limit (~128 levels) that fires first.
    /// Our check_json_depth is defense-in-depth for values within serde's limit.
    #[tokio::test]
    async fn test_jq_json_depth_limit_arrays() {
        // Build 150-level nested JSON: [[[[....[1]....]]]]
        let depth = 150;
        let open = "[".repeat(depth);
        let close = "]".repeat(depth);
        let input = format!("{open}1{close}");

        let result = run_jq(".", &input).await;
        assert!(
            result.is_err(),
            "deeply nested JSON arrays must be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nesting too deep") || err.contains("recursion limit"),
            "error should mention nesting or recursion limit: {err}"
        );
    }

    /// TM-DOS-027: Deeply nested JSON objects must be rejected
    #[tokio::test]
    async fn test_jq_json_depth_limit_objects() {
        // Build 150-level nested JSON objects: {"a":{"a":{"a":...}}}
        let depth = 150;
        let mut input = String::from("1");
        for _ in 0..depth {
            input = format!(r#"{{"a":{input}}}"#);
        }

        let result = run_jq(".", &input).await;
        assert!(
            result.is_err(),
            "deeply nested JSON objects must be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nesting too deep") || err.contains("recursion limit"),
            "error should mention nesting or recursion limit: {err}"
        );
    }

    /// TM-DOS-027: Moderate JSON nesting within limit still works
    #[tokio::test]
    async fn test_jq_moderate_nesting_ok() {
        // 10 levels of nesting should be fine
        let depth = 10;
        let open = "[".repeat(depth);
        let close = "]".repeat(depth);
        let input = format!("{open}1{close}");

        let result = run_jq(".", &input).await;
        assert!(
            result.is_ok(),
            "moderate nesting should succeed: {:?}",
            result.err()
        );
    }

    /// TM-DOS-027: check_json_depth unit test
    #[test]
    fn test_check_json_depth() {
        // Flat value: ok
        let v = serde_json::json!(42);
        assert!(check_json_depth(&v, 100).is_ok());

        // 3-level nesting with limit 5: ok
        let v = serde_json::json!([[[1]]]);
        assert!(check_json_depth(&v, 5).is_ok());

        // 3-level nesting with limit 2: rejected
        assert!(check_json_depth(&v, 2).is_err());
    }

    /// Helper: run jq with file arguments on an in-memory filesystem
    async fn run_jq_with_files(
        args: &[&str],
        files: &[(&str, &str)],
    ) -> std::result::Result<ExecResult, Error> {
        let jq = Jq;
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            // Ensure parent directory exists
            let p = std::path::Path::new(path);
            if let Some(parent) = p.parent() {
                if parent != std::path::Path::new("/") {
                    fs.mkdir(parent, true).await.unwrap();
                }
            }
            fs.write_file(p, content.as_bytes()).await.unwrap();
        }
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        jq.execute(ctx).await
    }

    #[tokio::test]
    async fn test_jq_read_single_file() {
        let result = run_jq_with_files(&[".", "/data.json"], &[("/data.json", r#"{"a":1}"#)])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "{\n  \"a\": 1\n}");
    }

    #[tokio::test]
    async fn test_jq_read_multiple_files() {
        let result = run_jq_with_files(
            &[".", "/a.json", "/b.json"],
            &[("/a.json", r#"{"x":1}"#), ("/b.json", r#"{"y":2}"#)],
        )
        .await
        .unwrap();
        assert_eq!(result.exit_code, 0);
        // Each file produces its own output
        let lines: Vec<&str> = result.stdout.trim().split('\n').collect();
        assert!(result.stdout.contains("\"x\": 1"), "should contain x:1");
        assert!(result.stdout.contains("\"y\": 2"), "should contain y:2");
        // Two separate JSON objects
        assert!(
            lines.len() > 3,
            "should have multi-line output for two objects"
        );
    }

    #[tokio::test]
    async fn test_jq_slurp_files() {
        let result = run_jq_with_files(
            &["-s", ".", "/a.json", "/b.json"],
            &[("/a.json", r#"{"x":1}"#), ("/b.json", r#"{"y":2}"#)],
        )
        .await
        .unwrap();
        assert_eq!(result.exit_code, 0);
        // Slurp should wrap both objects in an array
        assert!(result.stdout.contains("\"x\": 1"), "should contain x:1");
        assert!(result.stdout.contains("\"y\": 2"), "should contain y:2");
        // Verify it's an array
        let parsed: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
        assert!(parsed.is_array(), "slurp output should be an array");
        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_jq_file_not_found() {
        let result = run_jq_with_files(&[".", "/missing.json"], &[])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("Could not open file"));
    }

    #[tokio::test]
    async fn test_jq_slurp_files_in_subdir() {
        // Matches the reported scenario: jq -s '.' /workspace/json_data/*.json
        let result = run_jq_with_files(
            &[
                "-s",
                ".",
                "/workspace/json_data/a.json",
                "/workspace/json_data/b.json",
            ],
            &[
                ("/workspace/json_data/a.json", r#"{"id":1}"#),
                ("/workspace/json_data/b.json", r#"{"id":2}"#),
            ],
        )
        .await
        .unwrap();
        assert_eq!(result.exit_code, 0);
        let parsed: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], 1);
        assert_eq!(arr[1]["id"], 2);
    }

    // --- Argument parsing bug regression tests ---

    #[tokio::test]
    async fn test_jq_combined_short_flags() {
        // -rn should be parsed as -r + -n
        let result = run_jq_with_args(&["-rn", "1+1"], "").await.unwrap();
        assert_eq!(result.trim(), "2");
    }

    #[tokio::test]
    async fn test_jq_combined_flags_sc() {
        // -sc should be parsed as -s + -c
        let result = run_jq_with_args(&["-sc", "add"], "1\n2\n3\n")
            .await
            .unwrap();
        assert_eq!(result.trim(), "6");
    }

    #[tokio::test]
    async fn test_jq_arg_flag() {
        // --arg name value: $name should resolve to "value" in the filter
        let result = run_jq_with_args(&["--arg", "name", "world", "-n", r#""hello \($name)""#], "")
            .await
            .unwrap();
        assert_eq!(result.trim(), r#""hello world""#);
    }

    #[tokio::test]
    async fn test_jq_argjson_flag() {
        // --argjson count 5: $count should resolve to 5 (number, not string)
        let result = run_jq_with_args(&["--argjson", "count", "5", "-n", "$count + 1"], "")
            .await
            .unwrap();
        assert_eq!(result.trim(), "6");
    }

    #[tokio::test]
    async fn test_jq_arg_does_not_eat_filter() {
        // --arg name value '.' should NOT treat '.' as a file
        let result = run_jq_with_args(&["--arg", "x", "hello", "."], r#"{"a":1}"#)
            .await
            .unwrap();
        assert!(result.contains("\"a\": 1"));
    }

    #[tokio::test]
    async fn test_jq_double_dash_separator() {
        // -- ends option parsing; next arg is filter
        let result = run_jq_with_args(&["-n", "--", "1+1"], "").await.unwrap();
        assert_eq!(result.trim(), "2");
    }

    #[tokio::test]
    async fn test_jq_indent_flag() {
        // --indent 4 should not eat the filter
        let result = run_jq_with_args(&["--indent", "4", "."], r#"{"a":1}"#)
            .await
            .unwrap();
        assert!(result.contains("\"a\""));
    }

    // --- Negative tests ---

    #[tokio::test]
    async fn test_jq_invalid_json_input() {
        let result = run_jq(".", "not valid json");
        assert!(result.await.is_err(), "invalid JSON input should error");
    }

    #[tokio::test]
    async fn test_jq_invalid_filter_syntax() {
        let result = run_jq(".[", r#"{"a":1}"#);
        assert!(result.await.is_err(), "invalid filter should error");
    }

    #[tokio::test]
    async fn test_jq_invalid_argjson_value() {
        let result = run_jq_with_args(&["--argjson", "x", "not json", "-n", "$x"], "");
        assert!(
            result.await.is_err(),
            "invalid JSON for --argjson should error"
        );
    }

    #[tokio::test]
    async fn test_jq_empty_input_no_null() {
        // Empty stdin without -n should produce empty output, not error
        let result = run_jq(".", "").await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_jq_whitespace_only_input() {
        let result = run_jq(".", "   \n\t  ").await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_jq_ndjson_multiple_values() {
        // Multiple JSON values on separate lines (NDJSON)
        let result = run_jq(".a", "{\"a\":1}\n{\"a\":2}\n").await.unwrap();
        assert_eq!(result.trim(), "1\n2");
    }

    #[tokio::test]
    async fn test_jq_exit_status_false() {
        // -e flag: false output -> exit code 1
        let jq = Jq;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args = vec!["-e".to_string(), ".".to_string()];
        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: Some("false"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        let result = jq.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1, "-e with false should exit 1");
    }

    #[tokio::test]
    async fn test_jq_exit_status_truthy() {
        // -e flag: truthy output -> exit code 0
        let jq = Jq;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args = vec!["-e".to_string(), ".".to_string()];
        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: Some("42"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        let result = jq.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0, "-e with truthy value should exit 0");
    }

    #[tokio::test]
    async fn test_jq_multiple_arg_bindings() {
        let result = run_jq_with_args(
            &[
                "--arg",
                "x",
                "hello",
                "--arg",
                "y",
                "world",
                "-n",
                r#""[\($x)] [\($y)]""#,
            ],
            "",
        )
        .await
        .unwrap();
        assert_eq!(result.trim(), r#""[hello] [world]""#);
    }

    #[tokio::test]
    async fn test_jq_combined_flags_snr() {
        // -snr: slurp + null-input + raw-output
        let result = run_jq_with_args(&["-snr", r#""hello""#], "").await.unwrap();
        assert_eq!(result.trim(), "hello");
    }
}
