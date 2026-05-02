//! Top-level integration tests for the jq builtin.
//!
//! Submodules also have their own unit tests (args parsing, format,
//! errors, conversion). This module tests the wired-up behavior end-to-end
//! through `Jq::execute` — covering positive, negative, and security cases.

use super::*;
use crate::builtins::Context;
use crate::error::Error;
use crate::fs::{FileSystem, InMemoryFs};
use crate::interpreter::ExecResult;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

async fn run_jq(filter: &str, input: &str) -> Result<String> {
    run_jq_with_args(&[filter], input).await
}

async fn run_jq_with_args(args: &[&str], input: &str) -> Result<String> {
    let result = run_jq_result_with_args(args, input).await?;
    Ok(result.stdout)
}

async fn run_jq_result(filter: &str, input: &str) -> Result<ExecResult> {
    run_jq_result_with_args(&[filter], input).await
}

async fn run_jq_result_with_args(args: &[&str], input: &str) -> Result<ExecResult> {
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
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    jq.execute(ctx).await
}

async fn run_jq_with_files(
    args: &[&str],
    files: &[(&str, &str)],
) -> std::result::Result<ExecResult, Error> {
    let jq = Jq;
    let fs = Arc::new(InMemoryFs::new());
    for (path, content) in files {
        let p = std::path::Path::new(path);
        if let Some(parent) = p.parent()
            && parent != std::path::Path::new("/")
        {
            fs.mkdir(parent, true).await.unwrap();
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
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    jq.execute(ctx).await
}

// =========================================================================
// Identity / basic filter tests
// =========================================================================

#[tokio::test]
async fn identity_pretty_prints_object() {
    let result = run_jq(".", r#"{"name":"test"}"#).await.unwrap();
    assert_eq!(result.trim(), "{\n  \"name\": \"test\"\n}");
}

#[tokio::test]
async fn field_access() {
    let result = run_jq(".name", r#"{"name":"foo","id":42}"#).await.unwrap();
    assert_eq!(result.trim(), r#""foo""#);
}

#[tokio::test]
async fn array_index() {
    let result = run_jq(".[1]", r#"["a","b","c"]"#).await.unwrap();
    assert_eq!(result.trim(), r#""b""#);
}

#[tokio::test]
async fn nested_field() {
    let result = run_jq(".user.name", r#"{"user":{"name":"alice"}}"#)
        .await
        .unwrap();
    assert_eq!(result.trim(), r#""alice""#);
}

#[tokio::test]
async fn keys_pretty_prints_array() {
    let result = run_jq("keys", r#"{"b":1,"a":2}"#).await.unwrap();
    assert_eq!(result.trim(), "[\n  \"a\",\n  \"b\"\n]");
}

#[tokio::test]
async fn length_returns_number() {
    let result = run_jq("length", r#"[1,2,3,4,5]"#).await.unwrap();
    assert_eq!(result.trim(), "5");
}

// =========================================================================
// Tier 1: --indent N
// =========================================================================

#[tokio::test]
async fn indent_4_uses_4_spaces() {
    let result = run_jq_with_args(&["--indent", "4", "."], r#"{"a":1,"b":[1,2]}"#)
        .await
        .unwrap();
    // Each nested line should use 4 spaces.
    assert!(result.contains("\n    \"a\": 1"), "stdout: {result}");
    assert!(result.contains("\n        1"), "nested array: {result}");
}

#[tokio::test]
async fn indent_0_renders_compact() {
    // Real jq: --indent 0 is equivalent to -c.
    let result = run_jq_with_args(&["--indent", "0", "."], r#"{"a":1}"#)
        .await
        .unwrap();
    assert_eq!(result.trim(), r#"{"a":1}"#);
}

#[tokio::test]
async fn indent_too_large_rejected() {
    let result = run_jq_result_with_args(&["--indent", "8", "."], r#"{"a":1}"#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("--indent must be"));
}

#[tokio::test]
async fn indent_non_numeric_rejected() {
    let result = run_jq_result_with_args(&["--indent", "abc", "."], r#"{"a":1}"#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("expected a number"));
}

#[tokio::test]
async fn tab_uses_tab_characters() {
    let result = run_jq_with_args(&["--tab", "."], r#"{"a":1}"#)
        .await
        .unwrap();
    assert!(result.contains("\n\t\"a\": 1"), "stdout: {result}");
}

// =========================================================================
// Tier 1: -e exit codes (1 vs 4 vs 5)
// =========================================================================

#[tokio::test]
async fn exit_status_false_returns_1() {
    let result = run_jq_result_with_args(&["-e", "."], "false")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
}

#[tokio::test]
async fn exit_status_null_returns_1() {
    let result = run_jq_result_with_args(&["-e", "."], "null").await.unwrap();
    assert_eq!(result.exit_code, 1);
}

#[tokio::test]
async fn exit_status_no_output_returns_4() {
    // `empty` produces no output values at all — real jq returns 4 with -e.
    let result = run_jq_result_with_args(&["-e", "empty"], "1")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 4);
}

#[tokio::test]
async fn exit_status_truthy_returns_0() {
    let result = run_jq_result_with_args(&["-e", "."], "42").await.unwrap();
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn exit_status_mixed_truthy_wins() {
    // If any output is non-null/non-false, exit is 0 (unless no output).
    let result = run_jq_result_with_args(&["-e", ".[]"], "[null, 1, false]")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
}

// =========================================================================
// Tier 1: unknown flags rejected
// =========================================================================

#[tokio::test]
async fn unknown_long_flag_errors() {
    let result = run_jq_result_with_args(&["--xyzzy", "."], r#"{"a":1}"#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("Unknown option --xyzzy"));
}

#[tokio::test]
async fn unknown_short_flag_errors() {
    let result = run_jq_result_with_args(&["-Z", "."], r#"{"a":1}"#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("Unknown option -Z"));
}

// =========================================================================
// Tier 1: scan / @tsv / @csv compat
// =========================================================================

#[tokio::test]
async fn scan_default_global_no_double_g() {
    // Real jq's scan(re) is global by default. Compat-def must not produce
    // "gg" when user passes "g" explicitly (was a regex-compile bug).
    let result = run_jq(r#"scan("\\d"; "g")"#, r#""1 2 3""#).await.unwrap();
    let lines: Vec<&str> = result.trim().split('\n').collect();
    assert_eq!(lines.len(), 3, "expected 3 matches: {result}");
}

#[tokio::test]
async fn tsv_rejects_arrays() {
    let result = run_jq_result(r#"[[1,2], "x"] | @tsv"#, "null")
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(
        result.stderr.contains("not valid in a tsv row"),
        "stderr: {}",
        result.stderr
    );
}

#[tokio::test]
async fn tsv_rejects_objects() {
    let result = run_jq_result(r#"[{"a":1}] | @tsv"#, "null").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("not valid in a tsv row"));
}

#[tokio::test]
async fn csv_rejects_arrays() {
    let result = run_jq_result(r#"[[1,2]] | @csv"#, "null").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("not valid in a csv row"));
}

#[tokio::test]
async fn tsv_accepts_scalars() {
    let result = run_jq(r#"["a", 1, true, null] | @tsv"#, "null")
        .await
        .unwrap();
    assert_eq!(result.trim(), r#""a\t1\ttrue\t""#);
}

#[tokio::test]
async fn csv_accepts_scalars() {
    // -r so the @csv string output is unquoted, making contains-checks readable.
    let result = run_jq_with_args(&["-r", r#"["a", 1, true, null] | @csv"#], "null")
        .await
        .unwrap();
    assert_eq!(result.trim(), r#""a",1,true,"#);
}

// =========================================================================
// Tier 2: --slurpfile / --rawfile
// =========================================================================

#[tokio::test]
async fn slurpfile_binds_array_of_values() {
    let result = run_jq_with_files(
        &["--slurpfile", "data", "/x.json", "-n", r#"$data | length"#],
        &[("/x.json", "1\n2\n3")],
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "3");
}

#[tokio::test]
async fn slurpfile_binds_single_value() {
    let result = run_jq_with_files(
        &["--slurpfile", "obj", "/x.json", "-n", r#"$obj[0].name"#],
        &[("/x.json", r#"{"name":"alice"}"#)],
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), r#""alice""#);
}

#[tokio::test]
async fn slurpfile_rejects_invalid_json() {
    let result = run_jq_with_files(
        &["--slurpfile", "x", "/bad.json", "-n", r#"$x"#],
        &[("/bad.json", "not json {")],
    )
    .await
    .unwrap();
    assert_ne!(result.exit_code, 0);
}

#[tokio::test]
async fn slurpfile_missing_file_errors() {
    let result = run_jq_with_files(&["--slurpfile", "x", "/missing.json", "-n", r#"$x"#], &[])
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("/missing.json"));
}

#[tokio::test]
async fn rawfile_binds_string_contents() {
    let result = run_jq_with_files(
        &["--rawfile", "txt", "/note.txt", "-n", r#"$txt"#],
        &[("/note.txt", "hello\nworld")],
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    // Output is JSON-quoted.
    assert_eq!(result.stdout.trim(), r#""hello\nworld""#);
}

#[tokio::test]
async fn rawfile_with_raw_output_emits_unquoted() {
    let result = run_jq_with_files(
        &["--rawfile", "txt", "/note.txt", "-rn", r#"$txt"#],
        &[("/note.txt", "hello")],
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello");
}

// =========================================================================
// Tier 2: --args / --jsonargs / $ARGS
// =========================================================================

#[tokio::test]
async fn args_positional_are_strings() {
    let result = run_jq_with_args(&["-n", "$ARGS.positional", "--args", "a", "b", "c"], "")
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!(["a", "b", "c"]));
}

#[tokio::test]
async fn jsonargs_positional_are_parsed_json() {
    let result = run_jq_with_args(
        &[
            "-n",
            "$ARGS.positional",
            "--jsonargs",
            "1",
            "true",
            r#"{"a":1}"#,
        ],
        "",
    )
    .await
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!([1, true, {"a": 1}]));
}

#[tokio::test]
async fn args_named_populated_from_arg() {
    let result = run_jq_with_args(
        &[
            "--arg",
            "name",
            "world",
            "--arg",
            "x",
            "1",
            "-n",
            "$ARGS.named",
        ],
        "",
    )
    .await
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!({"name": "world", "x": "1"}));
}

#[tokio::test]
async fn args_named_combines_with_argjson() {
    let result = run_jq_with_args(
        &[
            "--arg",
            "name",
            "world",
            "--argjson",
            "count",
            "5",
            "-n",
            "$ARGS.named",
        ],
        "",
    )
    .await
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!({"name": "world", "count": 5}));
}

#[tokio::test]
async fn jsonargs_invalid_json_rejected() {
    let result = run_jq_result_with_args(&["-n", "$ARGS.positional", "--jsonargs", "not json"], "")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
}

// =========================================================================
// Tier 2: number-format parity (1.0 stays 1.0)
// =========================================================================

#[tokio::test]
async fn float_zero_decimal_preserved_compact() {
    let result = run_jq_with_args(&["-c", "."], "1.0").await.unwrap();
    assert_eq!(result.trim(), "1.0");
}

#[tokio::test]
async fn float_zero_decimal_preserved_pretty() {
    let result = run_jq(".", "1.0").await.unwrap();
    assert_eq!(result.trim(), "1.0");
}

#[tokio::test]
async fn float_in_array_preserved() {
    let result = run_jq_with_args(&["-c", "."], "[1.0, 2.5, 3]")
        .await
        .unwrap();
    assert_eq!(result.trim(), "[1.0,2.5,3]");
}

#[tokio::test]
async fn integer_stays_integer() {
    let result = run_jq(".", "42").await.unwrap();
    assert_eq!(result.trim(), "42");
}

// =========================================================================
// Tier 2: input_filename / input_line_number
// =========================================================================

#[tokio::test]
async fn input_filename_null_for_stdin() {
    let result = run_jq("input_filename", "1").await.unwrap();
    assert_eq!(result.trim(), "null");
}

#[tokio::test]
async fn input_filename_returns_path_when_file_given() {
    let result = run_jq_with_files(&["input_filename", "/data.json"], &[("/data.json", "1")])
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), r#""/data.json""#);
}

#[tokio::test]
async fn input_line_number_increments_per_input() {
    // For NDJSON stdin, each value advances the line counter by 1.
    let result = run_jq("input_line_number", "1\n2\n3").await.unwrap();
    let lines: Vec<&str> = result.trim().split('\n').collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[tokio::test]
async fn input_line_number_zero_for_null_input() {
    let result = run_jq_with_args(&["-n", "input_line_number"], "")
        .await
        .unwrap();
    assert_eq!(result.trim(), "0");
}

#[tokio::test]
async fn input_filename_with_alternative_compiles() {
    // Reproduces the LLM idiom from the original bug report.
    let result = run_jq_result(r#"input_filename // "stdin""#, "1")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), r#""stdin""#);
}

// =========================================================================
// File input behavior
// =========================================================================

#[tokio::test]
async fn read_single_file() {
    let result = run_jq_with_files(&[".", "/data.json"], &[("/data.json", r#"{"a":1}"#)])
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "{\n  \"a\": 1\n}");
}

#[tokio::test]
async fn read_multiple_files() {
    let result = run_jq_with_files(
        &[".", "/a.json", "/b.json"],
        &[("/a.json", r#"{"x":1}"#), ("/b.json", r#"{"y":2}"#)],
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("\"x\": 1"));
    assert!(result.stdout.contains("\"y\": 2"));
}

#[tokio::test]
async fn slurp_combines_multiple_files() {
    let result = run_jq_with_files(
        &["-s", ".", "/a.json", "/b.json"],
        &[("/a.json", r#"{"x":1}"#), ("/b.json", r#"{"y":2}"#)],
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    let parsed: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn missing_file_errors() {
    let result = run_jq_with_files(&[".", "/missing.json"], &[])
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("jq: /missing.json:"));
}

// =========================================================================
// Help / version
// =========================================================================

#[tokio::test]
async fn help_long() {
    let result = run_jq_with_args(&["--help"], "").await.unwrap();
    assert!(result.contains("Usage:"));
    assert!(result.contains("--slurpfile"));
    assert!(result.contains("--indent"));
}

#[tokio::test]
async fn help_short() {
    let result = run_jq_with_args(&["-h"], "").await.unwrap();
    assert!(result.contains("Usage:"));
}

#[tokio::test]
async fn version_long() {
    let result = run_jq_with_args(&["--version"], "").await.unwrap();
    assert!(result.starts_with("jq-"));
}

#[tokio::test]
async fn version_short() {
    let result = run_jq_with_args(&["-V"], "").await.unwrap();
    assert!(result.starts_with("jq-"));
}

// =========================================================================
// Raw input / slurp combinations
// =========================================================================

#[tokio::test]
async fn raw_input_per_line() {
    let result = run_jq_with_args(&["-R", "."], "hello\nworld\n")
        .await
        .unwrap();
    assert_eq!(result.trim(), "\"hello\"\n\"world\"");
}

#[tokio::test]
async fn raw_input_slurp_single_string() {
    let result = run_jq_with_args(&["-Rs", "."], "hello\nworld\n")
        .await
        .unwrap();
    assert_eq!(result.trim(), "\"hello\\nworld\\n\"");
}

#[tokio::test]
async fn raw_input_slurp_empty_stdin_emits_empty_string() {
    let result = run_jq_with_args(&["-Rs", "."], "").await.unwrap();
    assert_eq!(result.trim(), "\"\"");
}

// =========================================================================
// Error formatting (TM-INF-022 regression — must be jq-shaped, not Debug)
// =========================================================================

#[tokio::test]
async fn unknown_filter_error_is_jq_shaped() {
    let result = run_jq_result("totally_made_up_filter", "1").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert_eq!(
        result.stderr,
        "jq: error: totally_made_up_filter/0 is not defined\n"
    );
}

#[tokio::test]
async fn compile_error_does_not_leak_internals() {
    let result = run_jq_result("totally_made_up_filter", "1").await.unwrap();
    assert!(!result.stderr.contains("File {"));
    assert!(!result.stderr.contains("Filter("));
    assert!(!result.stderr.contains("setpath"));
    assert!(!result.stderr.contains("__bashkit_env__"));
    assert!(!result.stderr.contains("__bashkit_filename__"));
    assert!(!result.stderr.contains("__bashkit_lineno__"));
}

#[tokio::test]
async fn parse_error_short() {
    let result = run_jq_result("[", "1").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.starts_with("jq: error: "));
    assert!(result.stderr.len() < 300);
}

#[tokio::test]
async fn runtime_error_summarizes_index_operands() {
    let result = run_jq_result(".product_name", r#"[{"product_name":"x"}]"#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 5);
    assert_eq!(
        result.stderr,
        "jq: error: Cannot index array with string \"product_name\"\n"
    );
}

#[tokio::test]
async fn runtime_error_iterate_over_null() {
    let result = run_jq_result(".[]", "null").await.unwrap();
    assert_eq!(result.exit_code, 5);
    assert_eq!(
        result.stderr,
        "jq: error: Cannot iterate over null (null)\n"
    );
}

// =========================================================================
// Security: env isolation (TM-INF-013)
// =========================================================================

#[tokio::test]
async fn env_access_returns_shell_var() {
    let jq = Jq;
    let fs = Arc::new(InMemoryFs::new());
    let mut vars = HashMap::new();
    let mut cwd = PathBuf::from("/");
    let mut env = HashMap::new();
    env.insert("TESTVAR".to_string(), "hello".to_string());
    let args = vec!["-n".to_string(), "env.TESTVAR".to_string()];

    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut vars,
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

    let result = jq.execute(ctx).await.unwrap();
    assert_eq!(result.stdout.trim(), "\"hello\"");
}

#[tokio::test]
async fn dollar_env_returns_shell_env() {
    let jq = Jq;
    let fs = Arc::new(InMemoryFs::new());
    let mut vars = HashMap::new();
    let mut cwd = PathBuf::from("/");
    let mut env = HashMap::new();
    env.insert("MY_VAR".to_string(), "hello".to_string());
    let args = vec!["$ENV.MY_VAR".to_string()];

    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut vars,
        cwd: &mut cwd,
        fs,
        stdin: Some("null"),
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = jq.execute(ctx).await.unwrap();
    assert_eq!(result.stdout.trim(), r#""hello""#);
}

#[tokio::test]
async fn env_does_not_pollute_process_env() {
    // TM-INF-013: shell variables passed via ctx.env must NOT leak into
    // the host process environment.
    let unique_key = "BASHKIT_TEST_JQ_NO_POLLUTE_PARITY";
    assert!(std::env::var(unique_key).is_err());

    let jq = Jq;
    let fs = Arc::new(InMemoryFs::new());
    let mut vars = HashMap::new();
    let mut cwd = PathBuf::from("/");
    let mut env = HashMap::new();
    env.insert(unique_key.to_string(), "leaked".to_string());
    let args = vec!["-n".to_string(), format!("env.{unique_key}")];

    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut vars,
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

    let result = jq.execute(ctx).await.unwrap();
    assert_eq!(result.stdout.trim(), "\"leaked\"");
    assert!(std::env::var(unique_key).is_err());
}

// =========================================================================
// Security: depth limits (TM-DOS-027)
// =========================================================================

#[tokio::test]
async fn deep_array_input_rejected() {
    let depth = 150;
    let input = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let result = run_jq_result(".", &input).await.unwrap();
    assert!(result.exit_code != 0);
    assert!(
        result.stderr.contains("nesting too deep") || result.stderr.contains("recursion limit"),
        "stderr: {}",
        result.stderr
    );
}

#[tokio::test]
async fn deep_object_input_rejected() {
    let depth = 150;
    let mut input = String::from("1");
    for _ in 0..depth {
        input = format!(r#"{{"a":{input}}}"#);
    }
    let result = run_jq_result(".", &input).await.unwrap();
    assert!(result.exit_code != 0);
}

#[tokio::test]
async fn moderate_nesting_works() {
    let result = run_jq(".", "[[[[1]]]]").await.unwrap();
    assert!(result.contains('1'));
}

#[tokio::test]
async fn argjson_rejects_deep_nesting() {
    let deep = format!("{}0{}", "[".repeat(150), "]".repeat(150));
    let result = run_jq_result_with_args(&["--argjson", "x", &deep, "-n", "$x"], "")
        .await
        .unwrap();
    assert!(result.exit_code != 0);
}

// =========================================================================
// Negative tests
// =========================================================================

#[tokio::test]
async fn invalid_json_input() {
    let result = run_jq_result(".", "not valid json").await.unwrap();
    assert!(result.exit_code != 0);
    assert!(result.stderr.contains("jq:"));
}

#[tokio::test]
async fn invalid_filter_syntax() {
    let result = run_jq_result(".[", r#"{"a":1}"#).await.unwrap();
    assert!(result.exit_code != 0);
    assert!(result.stderr.contains("jq:"));
}

#[tokio::test]
async fn empty_stdin_no_null_input_returns_empty() {
    let result = run_jq(".", "").await.unwrap();
    assert_eq!(result, "");
}

#[tokio::test]
async fn whitespace_only_stdin_returns_empty() {
    let result = run_jq(".", "   \n\t  ").await.unwrap();
    assert_eq!(result, "");
}

#[tokio::test]
async fn ndjson_multi_value_processed() {
    let result = run_jq(".a", "{\"a\":1}\n{\"a\":2}").await.unwrap();
    assert_eq!(result.trim(), "1\n2");
}

// =========================================================================
// jq 1.8 builtins — abs, trim, ltrim, rtrim, if-without-else, paths(filter)
// =========================================================================

#[tokio::test]
async fn abs_negative_to_positive() {
    assert_eq!(run_jq("abs", "-42").await.unwrap().trim(), "42");
    assert_eq!(run_jq("abs", "-0.5").await.unwrap().trim(), "0.5");
}

#[tokio::test]
async fn trim_strips_whitespace() {
    assert_eq!(
        run_jq("trim", r#""  hello  ""#).await.unwrap().trim(),
        r#""hello""#
    );
}

#[tokio::test]
async fn ltrim_only_left() {
    assert_eq!(
        run_jq("ltrim", r#""  hello  ""#).await.unwrap().trim(),
        r#""hello  ""#
    );
}

#[tokio::test]
async fn rtrim_only_right() {
    assert_eq!(
        run_jq("rtrim", r#""  hello  ""#).await.unwrap().trim(),
        r#""  hello""#
    );
}

#[tokio::test]
async fn if_without_else_uses_identity() {
    assert_eq!(
        run_jq("if . > 0 then . * 2 end", "5").await.unwrap().trim(),
        "10"
    );
    assert_eq!(
        run_jq("if . > 0 then . * 2 end", "-1")
            .await
            .unwrap()
            .trim(),
        "-1"
    );
}

#[tokio::test]
async fn paths_with_filter() {
    let result = run_jq("[paths(numbers)]", r#"{"a":1,"b":{"c":2},"d":"x"}"#)
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    let arr = parsed.as_array().unwrap();
    assert!(arr.iter().any(|v| v == &serde_json::json!(["a"])));
    assert!(arr.iter().any(|v| v == &serde_json::json!(["b", "c"])));
}

#[tokio::test]
async fn getpath_returns_value() {
    assert_eq!(
        run_jq(r#"getpath(["a","b"])"#, r#"{"a":{"b":42}}"#)
            .await
            .unwrap()
            .trim(),
        "42"
    );
}

// =========================================================================
// input / inputs across NDJSON
// =========================================================================

#[tokio::test]
async fn input_reads_next_value() {
    let result = run_jq_with_args(&["input"], "1\n2").await.unwrap();
    assert_eq!(result.trim(), "2");
}

#[tokio::test]
async fn inputs_collects_remaining() {
    let result = run_jq_with_args(&["-c", "[inputs]"], "1\n2\n3")
        .await
        .unwrap();
    assert_eq!(result.trim(), "[2,3]");
}

// =========================================================================
// TM-INF-022: per-tool no-leak guard against malformed inputs
// =========================================================================

const JQ_BANNED: &[&str] = &[
    "__bashkit_env__",
    "__bashkit_filename__",
    "__bashkit_lineno__",
    "JQ_COMPAT_DEFS",
    "def setpath",
    "def leaf_paths",
    "def @tsv:",
    "def @csv:",
    "def env:",
    "def input_filename:",
    "def input_line_number:",
    "Filter(0)",
    "Filter(1)",
    "Filter(2)",
    "Var,",
    "Mod,",
];

macro_rules! jq_no_leak {
    ($name:ident, $script:expr) => {
        #[tokio::test]
        async fn $name() {
            let r = crate::builtins::debug_leak_check::run($script).await;
            crate::builtins::debug_leak_check::assert_no_leak(&r, stringify!($name), JQ_BANNED);
        }
    };
}

jq_no_leak!(
    no_leak_undefined_filter_zero_arity,
    "echo 1 | jq totally_made_up"
);
jq_no_leak!(
    no_leak_undefined_filter_with_arity,
    "echo 1 | jq 'totally_made_up(1; 2)'"
);
jq_no_leak!(no_leak_undefined_variable, "echo 1 | jq '$nope'");
jq_no_leak!(no_leak_undefined_format, "echo '[1]' | jq '@xyzzy'");

jq_no_leak!(no_leak_unbalanced_bracket, "echo 1 | jq '['");
jq_no_leak!(no_leak_unbalanced_paren, "echo 1 | jq '('");
jq_no_leak!(no_leak_stray_pipe, "echo 1 | jq '|'");
jq_no_leak!(no_leak_unterminated_string, r#"echo 1 | jq '"abc'"#);
jq_no_leak!(no_leak_if_without_then, "echo 1 | jq 'if . then'");
jq_no_leak!(no_leak_reduce_without_as, "echo 1 | jq 'reduce . '");
jq_no_leak!(no_leak_def_without_body, "echo 1 | jq 'def f:'");

jq_no_leak!(no_leak_malformed_json_input, "echo 'not json {' | jq '.'");

#[tokio::test]
async fn no_leak_deeply_nested_input() {
    let script = format!("echo '{}{}' | jq '.'", "[".repeat(200), "]".repeat(200));
    let r = crate::builtins::debug_leak_check::run(&script).await;
    crate::builtins::debug_leak_check::assert_no_leak(&r, "no_leak_deeply_nested_input", JQ_BANNED);
}

jq_no_leak!(
    no_leak_index_array_with_string,
    r#"echo '[1,2]' | jq '.foo'"#
);
jq_no_leak!(no_leak_iterate_over_null, r#"echo 'null' | jq '.[]'"#);
jq_no_leak!(no_leak_add_array_and_number, r#"echo '[1,2]' | jq '. + 1'"#);

// New: malformed $ARGS / file flags
jq_no_leak!(no_leak_invalid_jsonargs, "jq -n . --jsonargs 'not json'");
jq_no_leak!(no_leak_invalid_argjson, "jq --argjson x 'not json' -n '$x'");
jq_no_leak!(no_leak_indent_too_large, "echo '{}' | jq --indent 99 '.'");
jq_no_leak!(no_leak_indent_negative, "echo '{}' | jq --indent -1 '.'");
jq_no_leak!(
    no_leak_slurpfile_missing,
    "jq --slurpfile x /no-such-file.json -n '$x'"
);

// Positive regression for the bug-report filter
#[tokio::test]
async fn harness_tsv_filter_compiles_and_runs() {
    let filter = r#"
        if (.data | length) == 0 then
          "No harnesses found."
        else
          (.data[] | [(.id // ""), (.name // ""), (.description // ""), (.parent_harness_id // ""), ((.capabilities // []) | length | tostring), (.created_at // "")] | @tsv)
        end
    "#;
    let input = r#"{"data":[{"id":"h1","name":"alpha","description":"d","parent_harness_id":null,"capabilities":["a","b"],"created_at":"2024-01-01"}]}"#;
    let result = run_jq_result(filter, input).await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), r#""h1\talpha\td\t\t2\t2024-01-01""#);
}

// Combined short flag regressions
#[tokio::test]
async fn combined_short_flags_rn() {
    let result = run_jq_with_args(&["-rn", "1+1"], "").await.unwrap();
    assert_eq!(result.trim(), "2");
}

#[tokio::test]
async fn combined_short_flags_sc_add() {
    let result = run_jq_with_args(&["-sc", "add"], "1\n2\n3\n")
        .await
        .unwrap();
    assert_eq!(result.trim(), "6");
}

#[tokio::test]
async fn combined_short_flags_snr() {
    let result = run_jq_with_args(&["-snr", r#""hello""#], "").await.unwrap();
    assert_eq!(result.trim(), "hello");
}

#[tokio::test]
async fn double_dash_separator() {
    let result = run_jq_with_args(&["-n", "--", "1+1"], "").await.unwrap();
    assert_eq!(result.trim(), "2");
}

// =========================================================================
// Differential tests vs real jq binary (when present in $PATH)
// =========================================================================

mod differential {
    //! These tests compare bashkit's jq output against the real `jq`
    //! binary when it's available. Skipped silently if no `jq` is found.
    //! They guard against regressions in jq compatibility for the most
    //! common LLM-emitted filter shapes.

    use super::*;

    fn real_jq_available() -> bool {
        std::process::Command::new("jq")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn run_real_jq(args: &[&str], input: &str) -> Option<(String, i32)> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("jq")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        // Ignore write errors: jq may exit before reading stdin (e.g. on
        // unknown flags), causing a broken pipe. Drop stdin to signal EOF.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        let out = child.wait_with_output().ok()?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        Some((stdout, out.status.code().unwrap_or(-1)))
    }

    /// Compare bashkit and real-jq output for the same args+input.
    /// Stdout must match byte-for-byte; exit codes must match.
    async fn assert_matches(args: &[&str], input: &str) {
        if !real_jq_available() {
            eprintln!("real jq not present; skipping differential test");
            return;
        }
        let (real_out, real_code) = run_real_jq(args, input).expect("real jq run should succeed");
        let bashkit = run_jq_result_with_args(args, input).await.unwrap();
        let args_label = args.join(" ");
        assert_eq!(
            bashkit.stdout, real_out,
            "stdout mismatch for {args_label}\nbashkit:\n{}\nreal jq:\n{}",
            bashkit.stdout, real_out
        );
        assert_eq!(
            bashkit.exit_code, real_code,
            "exit-code mismatch for {args_label}: bashkit={}, real={}",
            bashkit.exit_code, real_code
        );
    }

    #[tokio::test]
    async fn diff_identity_object() {
        assert_matches(&["."], r#"{"a":1,"b":[1,2]}"#).await;
    }

    #[tokio::test]
    async fn diff_compact_array() {
        assert_matches(&["-c", "."], r#"[1,2,3]"#).await;
    }

    #[tokio::test]
    async fn diff_indent_4() {
        assert_matches(&["--indent", "4", "."], r#"{"a":1,"b":2}"#).await;
    }

    #[tokio::test]
    async fn diff_indent_0() {
        assert_matches(&["--indent", "0", "."], r#"{"a":1}"#).await;
    }

    #[tokio::test]
    async fn diff_sort_keys() {
        assert_matches(&["-S", "."], r#"{"b":1,"a":2,"c":3}"#).await;
    }

    #[tokio::test]
    async fn diff_raw_output_string() {
        assert_matches(&["-r", ".name"], r#"{"name":"alice"}"#).await;
    }

    #[tokio::test]
    async fn diff_keys() {
        assert_matches(&["keys"], r#"{"b":1,"a":2}"#).await;
    }

    #[tokio::test]
    async fn diff_length_array() {
        assert_matches(&["length"], r#"[1,2,3,4,5]"#).await;
    }

    #[tokio::test]
    async fn diff_array_iter() {
        assert_matches(&[".[]"], r#"[1,2,3]"#).await;
    }

    #[tokio::test]
    async fn diff_select_filter() {
        assert_matches(&[".[] | select(. > 1)"], r#"[1,2,3]"#).await;
    }

    #[tokio::test]
    async fn diff_pipe_chain() {
        assert_matches(&[".items | map(.id)"], r#"{"items":[{"id":1},{"id":2}]}"#).await;
    }

    #[tokio::test]
    async fn diff_float_zero_decimal() {
        assert_matches(&["-c", "."], "1.0").await;
    }

    #[tokio::test]
    async fn diff_args_positional() {
        assert_matches(&["-n", "$ARGS.positional", "--args", "a", "b"], "").await;
    }

    #[tokio::test]
    async fn diff_argjson_named() {
        assert_matches(&["--argjson", "n", "5", "-n", "$ARGS.named"], "").await;
    }

    #[tokio::test]
    async fn diff_exit_status_truthy() {
        let (_, code) = run_real_jq(&["-e", "."], "42").unwrap_or((String::new(), -1));
        let bk = run_jq_result_with_args(&["-e", "."], "42").await.unwrap();
        if real_jq_available() {
            assert_eq!(bk.exit_code, code);
        }
    }

    #[tokio::test]
    async fn diff_exit_status_null() {
        if !real_jq_available() {
            return;
        }
        let (_, code) = run_real_jq(&["-e", "."], "null").unwrap_or((String::new(), -1));
        let bk = run_jq_result_with_args(&["-e", "."], "null").await.unwrap();
        assert_eq!(bk.exit_code, code);
    }

    #[tokio::test]
    async fn diff_exit_status_no_output() {
        if !real_jq_available() {
            return;
        }
        let (_, code) = run_real_jq(&["-e", "empty"], "1").unwrap_or((String::new(), -1));
        let bk = run_jq_result_with_args(&["-e", "empty"], "1")
            .await
            .unwrap();
        assert_eq!(bk.exit_code, code);
    }

    #[tokio::test]
    async fn diff_unknown_flag_exit_code() {
        if !real_jq_available() {
            return;
        }
        let (_, code) = run_real_jq(&["--xyzzy", "."], "1").unwrap_or((String::new(), -1));
        let bk = run_jq_result_with_args(&["--xyzzy", "."], "1")
            .await
            .unwrap();
        assert_eq!(
            bk.exit_code, code,
            "real jq exit={code}, bashkit={}",
            bk.exit_code
        );
    }
}
