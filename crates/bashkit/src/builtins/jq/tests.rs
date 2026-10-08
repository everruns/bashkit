//! Top-level integration tests for the jq builtin.
//!
//! Submodules also have their own unit tests (args parsing, format,
//! errors, conversion). This module tests the wired-up behavior end-to-end
//! through `Jq::execute` — covering positive, negative, and security cases.

use super::*;
use crate::builtins::{Context, test_stream};
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
    Ok(result.stdout.to_string())
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
        stdin: Some(test_stream(input)),
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

// jq deliberately accepts literal U+0000..U+001F bytes inside input strings.
// This compatibility exception belongs only to jq's input boundary; output is
// always valid JSON with the controls escaped.
#[tokio::test]
async fn literal_controls_inside_strings_are_escaped() {
    let controls: String = (0u8..=0x1f).map(char::from).collect();
    let input = format!("\"before{controls}after\"");
    let result = run_jq_with_args(&["-c", "."], &input).await.unwrap();
    assert_eq!(
        result,
        format!(
            "{}\n",
            serde_json::to_string(&format!("before{controls}after")).unwrap()
        )
    );
}

#[tokio::test]
async fn literal_controls_preserve_adjacent_existing_escapes() {
    let input = "\"left\\n\t\\t\rright\"";
    let result = run_jq_with_args(&["-c", "explode"], input).await.unwrap();
    assert_eq!(result, "[108,101,102,116,10,9,9,13,114,105,103,104,116]\n");
}

#[tokio::test]
async fn literal_controls_work_across_concatenated_values() {
    let result = run_jq_with_args(&["-c", "."], "\"a\nb\"{\"c\":\"d\te\"}")
        .await
        .unwrap();
    assert_eq!(result, "\"a\\nb\"\n{\"c\":\"d\\te\"}\n");
}

#[tokio::test]
async fn literal_controls_work_in_ndjson_after_escaped_quote() {
    let result = run_jq_with_args(&["-c", "."], "\"a\\\"\tb\"\n\"c\rd\"\n")
        .await
        .unwrap();
    assert_eq!(result, "\"a\\\"\\tb\"\n\"c\\rd\"\n");
}

#[tokio::test]
async fn structural_whitespace_remains_structural() {
    let result = run_jq_with_args(&["-c", "."], "\t1\r\n 2\n").await.unwrap();
    assert_eq!(result, "1\n2\n");
}

#[tokio::test]
async fn non_whitespace_control_outside_string_is_rejected() {
    let result = run_jq_result(".", "1\u{0000}2").await.unwrap();
    assert_eq!(result.exit_code, 5);
    assert!(
        result.stderr.contains("invalid JSON"),
        "stderr={}",
        result.stderr
    );
}

#[tokio::test]
async fn unterminated_string_with_control_is_rejected_deterministically() {
    let first = run_jq_result(".", "\"unterminated\n").await.unwrap();
    let second = run_jq_result(".", "\"unterminated\n").await.unwrap();
    assert_eq!(first.exit_code, 5);
    assert_eq!(first.stderr, second.stderr);
    assert!(first.stderr.contains("unterminated string"));
    crate::builtins::debug_leak_check::assert_no_leak(
        &first,
        "unterminated jq input string",
        JQ_BANNED,
    );
}

#[tokio::test]
async fn literal_controls_work_for_file_and_slurpfile_inputs() {
    let direct = run_jq_with_files(&["-c", ".", "/input.json"], &[("/input.json", "\"a\nb\"")])
        .await
        .unwrap();
    assert_eq!(direct.stdout, "\"a\\nb\"\n");

    let slurped = run_jq_with_files(
        &["-nc", "--slurpfile", "data", "/input.json", "$data"],
        &[("/input.json", "\"a\nb\"\"c\td\"")],
    )
    .await
    .unwrap();
    assert_eq!(slurped.stdout, "[\"a\\nb\",\"c\\td\"]\n");
}

#[tokio::test]
async fn jq_argument_json_boundaries_remain_strict() {
    let argjson = run_jq_result_with_args(&["--argjson", "x", "\"a\nb\"", "-n", "$x"], "")
        .await
        .unwrap();
    assert_eq!(argjson.exit_code, 2);

    let jsonargs = run_jq_result_with_args(&["-n", ".", "--jsonargs", "\"a\nb\""], "")
        .await
        .unwrap();
    assert_eq!(jsonargs.exit_code, 2);
}

/// TM-DOS-093: an unbounded generator must not grow output without limit or
/// hang the host. jaq's iterator is synchronous so the execution timeout
/// cannot preempt it; the output-byte cap (here the 1 MB default, since the
/// test harness supplies no shell/limits) must abort with a clean error.
#[tokio::test]
async fn unbounded_generator_is_capped() {
    let result = run_jq_result_with_args(&["-n", "repeat(1)"], "")
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0, "runaway generator should fail");
    assert!(
        result.stderr.contains("output limit exceeded"),
        "expected output-limit error, got stderr={:?} stdout_len={}", // debug-ok: test assertion
        result.stderr,
        result.stdout.len()
    );
    assert!(
        result.stdout.len() <= 1_048_576 + 64,
        "output must be bounded by the cap, got {} bytes",
        result.stdout.len()
    );
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
async fn exit_status_last_value_wins() {
    // jq -e: the last output decides (here false -> 1).
    let result = run_jq_result_with_args(&["-e", ".[]"], "[null, 1, false]")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    let result = run_jq_result_with_args(&["-e", ".[]"], "[null, false, 1]")
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

#[tokio::test]
async fn repeated_rawfile_bindings_are_byte_capped() {
    let payload = "x".repeat(1024 * 1024);
    let mut args = Vec::new();
    for i in 0..32 {
        args.push("--rawfile".to_string());
        args.push(format!("x{i}"));
        args.push("/big.txt".to_string());
    }
    args.push("-n".to_string());
    args.push(".".to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let result = run_jq_with_files(&refs, &[("/big.txt", &payload)])
        .await
        .unwrap();

    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("file bindings exceed"));
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
async fn input_line_number_counts_lines_read() {
    // jq reads a line at a time: the last value has no newline after it,
    // so it still reports line 2 (matches jq 1.7).
    let result = run_jq("input_line_number", "1\n2\n3").await.unwrap();
    let lines: Vec<&str> = result.trim().split('\n').collect();
    assert_eq!(lines, vec!["1", "2", "2"]);
    let result = run_jq("input_line_number", "1\n2\n\n3").await.unwrap();
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
    assert_eq!(result.exit_code, 2);
    assert_eq!(
        result.stderr,
        "jq: error: Could not open file /missing.json: No such file or directory\n"
    );
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
// fancy-regex backed natives — lookahead/lookbehind/backrefs/atomic groups
// (real jq uses Oniguruma; jaq's regex crate rejects these patterns)
// =========================================================================

#[tokio::test]
async fn regex_lookahead_compiles_and_matches() {
    // Positive lookahead: match "foo" only when followed by "bar"
    let result = run_jq(r#"test("foo(?=bar)")"#, r#""foobar""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "true");
}

#[tokio::test]
async fn regex_lookahead_negative_does_not_match() {
    let result = run_jq(r#"test("foo(?=bar)")"#, r#""foobaz""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "false");
}

#[tokio::test]
async fn regex_negative_lookahead() {
    let result = run_jq(r#"test("foo(?!bar)")"#, r#""foobaz""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "true");
}

#[tokio::test]
async fn regex_lookbehind_compiles_and_matches() {
    // Lookbehind: match "bar" only when preceded by "foo"
    let result = run_jq(r#"test("(?<=foo)bar")"#, r#""foobar""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "true");
}

#[tokio::test]
async fn regex_negative_lookbehind() {
    let result = run_jq(r#"test("(?<!foo)bar")"#, r#""xxbar""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "true");
}

#[tokio::test]
async fn regex_backreference() {
    // Backref \1: match repeated word
    let result = run_jq(r#"test("(\\w+) \\1")"#, r#""hello hello""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "true");
}

#[tokio::test]
async fn regex_backreference_no_match() {
    let result = run_jq(r#"test("(\\w+) \\1")"#, r#""hello world""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "false");
}

#[tokio::test]
async fn regex_atomic_group_compiles() {
    // Atomic group (?>...): no backtracking
    let result = run_jq(r#"test("(?>abc|abd)d")"#, r#""abcd""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "true");
}

#[tokio::test]
async fn regex_match_with_lookbehind_has_correct_offset() {
    // Lookbehind is zero-width: match offset is at "bar", not "foo"
    let result = run_jq(r#"match("(?<=foo)bar") | .offset"#, r#""foobar""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "3");
}

#[tokio::test]
async fn regex_capture_groups_with_lookahead_named() {
    // Named capture group with lookahead
    let result = run_jq(r#"capture("(?<word>\\w+)(?=,)")"#, r#""apple,banana""#)
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    assert_eq!(parsed["word"], serde_json::json!("apple"));
}

#[tokio::test]
async fn regex_basic_pattern_still_works() {
    // Regression — basic patterns must keep working under fancy-regex.
    let result = run_jq(r#"test("\\d+")"#, r#""abc123""#).await.unwrap();
    assert_eq!(result.trim(), "true");
}

#[tokio::test]
async fn regex_scan_returns_all_matches() {
    let result = run_jq(r#"[scan("\\d+")]"#, r#""a1 b22 c333""#)
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!(["1", "22", "333"]));
}

#[tokio::test]
async fn regex_scan_with_lookahead() {
    // scan with lookahead — extract digits before letters only
    let result = run_jq(r#"[scan("\\d+(?=[a-z])")]"#, r#""1a 2 3b 4""#)
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();
    assert_eq!(parsed, serde_json::json!(["1", "3"]));
}

#[tokio::test]
async fn regex_match_on_unicode_offset_in_chars() {
    // Offset must be in CHARS not bytes (matches jq docs).
    // "héllo" — byte 3 = char 2 (start of second 'l').
    let result = run_jq(r#"match("ll") | .offset"#, r#""héllo""#)
        .await
        .unwrap();
    assert_eq!(result.trim(), "2");
}

#[tokio::test]
async fn regex_invalid_pattern_yields_short_error() {
    let result = run_jq_result(r#"test("(unbalanced")"#, r#""x""#)
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.starts_with("jq: error (at <stdin>:0): "));
    assert!(result.stderr.len() < 300);
    // Must not leak fancy-regex internal Debug shapes
    assert!(!result.stderr.contains("ParseError"));
    assert!(!result.stderr.contains("CompileError"));
}

#[tokio::test]
async fn setpath_binds_dynamic_path_to_original_input() {
    let result = run_jq(r#"setpath(.path; 1)"#, r#"{"path":["a"]}"#)
        .await
        .unwrap();
    assert_eq!(
        result.trim(),
        "{\n  \"path\": [\n    \"a\"\n  ],\n  \"a\": 1\n}"
    );
}

#[tokio::test]
async fn setpath_binds_dynamic_value_to_original_input() {
    let result = run_jq(r#"setpath(["a"]; .path[0])"#, r#"{"path":["expected"]}"#)
        .await
        .unwrap();
    assert_eq!(
        result.trim(),
        "{\n  \"path\": [\n    \"expected\"\n  ],\n  \"a\": \"expected\"\n}"
    );
}

#[tokio::test]
async fn regex_invalid_flag_yields_short_error() {
    let result = run_jq_result(r#"match("x"; "z")"#, r#""x""#).await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("invalid regex flag"));
}

#[tokio::test]
async fn regex_swap_greed_flag_silently_accepted() {
    // 'l' (swap greed) is not exposed by fancy-regex; we silently accept
    // for jq-flag-string parity. The pattern still compiles.
    let result = run_jq(r#"test("a+"; "l")"#, r#""aaa""#).await.unwrap();
    assert_eq!(result.trim(), "true");
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
        "jq: error (at <stdin>:0): Cannot index array with string \"product_name\"\n"
    );
}

#[tokio::test]
async fn runtime_error_iterate_over_null() {
    let result = run_jq_result(".[]", "null").await.unwrap();
    assert_eq!(result.exit_code, 5);
    assert_eq!(
        result.stderr,
        "jq: error (at <stdin>:0): Cannot iterate over null (null)\n"
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
        stdin: Some(test_stream("null")),
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
async fn finite_user_recursion_stays_available() {
    let result = run_jq_result(
        "def f($n): if $n == 0 then 42 else f($n - 1) end; f(32)",
        "null",
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "42");
}

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
// Security: jq filters must not terminate the host process (#1571)
// =========================================================================

// The tests reaching their assertions at all proves the host wasn't killed:
// `halt` ends the jq command with its code, never the process.
#[tokio::test]
async fn halt_ends_the_command_not_the_host() {
    let result = run_jq_result_with_args(&["-n", "1, halt, 2"], "")
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n");
    assert_eq!(result.exit_code, 0);
    let result = run_jq_result_with_args(&["-n", "halt(7)"], "")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 7);
}

#[tokio::test]
async fn halt_stops_remaining_inputs() {
    let result = run_jq_result_with_args(&["-c", "if . == 2 then halt else . end"], "1 2 3")
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn halt_error_prints_message_and_exits() {
    let result = run_jq_result("halt_error", r#""boom\n""#).await.unwrap();
    assert_eq!(result.stderr, "boom\n");
    assert_eq!(result.exit_code, 5);
    let result = run_jq_result("halt_error(3)", r#"{"a":1}"#).await.unwrap();
    assert_eq!(result.stderr, "{\"a\":1}\n");
    assert_eq!(result.exit_code, 3);
    let result = run_jq_result("try halt_error catch 9", "null")
        .await
        .unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 5);
}

#[tokio::test]
async fn debug_and_stderr_write_to_jq_stderr() {
    let result = run_jq_result_with_args(&["-c", "debug, debug(\"m\"), stderr"], "[1]")
        .await
        .unwrap();
    assert_eq!(result.stdout, "[1]\n[1]\n[1]\n");
    assert_eq!(result.stderr, "[\"DEBUG:\",[1]]\n[\"DEBUG:\",\"m\"]\n[1]");
}

#[tokio::test]
async fn debug_output_is_capped() {
    let result = run_jq_result_with_args(&["-n", "[range(200000)] | debug | length"], "")
        .await
        .unwrap();
    assert_eq!(result.stdout, "200000\n");
    assert!(result.stderr.len() <= super::messages::MAX_MESSAGE_BYTES);
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

    /// Prefer a direct raw-input comparison. Older jq builds reject controls
    /// at the lexer, so fall back to equivalent escaped JSON on those versions
    /// while retaining real jq as the filter/output oracle.
    async fn assert_control_matches(args: &[&str], raw_input: &str, escaped_input: &str) {
        if !real_jq_available() {
            eprintln!("real jq not present; skipping differential test");
            return;
        }
        let direct = run_real_jq(args, raw_input).unwrap();
        let (real_out, real_code) = if direct.1 == 0 {
            direct
        } else {
            run_real_jq(args, escaped_input).unwrap()
        };
        let bashkit = run_jq_result_with_args(args, raw_input).await.unwrap();
        assert_eq!(bashkit.stdout, real_out);
        assert_eq!(bashkit.exit_code, real_code);
    }

    #[tokio::test]
    async fn diff_literal_newline_tab_and_cr() {
        for (raw, escaped) in [
            ("\"a\nb\"", "\"a\\nb\""),
            ("\"a\tb\"", "\"a\\tb\""),
            ("\"a\rb\"", "\"a\\rb\""),
        ] {
            assert_control_matches(&["-c", "."], raw, escaped).await;
        }
    }

    #[tokio::test]
    async fn diff_all_literal_controls() {
        let controls: String = (0u8..=0x1f).map(char::from).collect();
        let raw = format!("\"{controls}\"");
        let escaped = serde_json::to_string(&controls).unwrap();
        assert_control_matches(&["-c", "."], &raw, &escaped).await;
    }

    #[tokio::test]
    async fn diff_controls_adjacent_to_escapes_and_concatenated_values() {
        assert_control_matches(
            &["-c", "."],
            "\"a\\n\tb\"\"c\r\\td\"",
            "\"a\\n\\tb\"\"c\\r\\td\"",
        )
        .await;
    }

    #[tokio::test]
    async fn diff_control_normalization_boundary_size() {
        let content = "x".repeat(16 * 1024);
        let raw = format!("\"{content}\n\"");
        let escaped = format!("\"{content}\\n\"");
        assert_control_matches(&["-c", "length"], &raw, &escaped).await;
    }

    #[tokio::test]
    async fn diff_malformed_quote_and_control_outside_string() {
        assert_matches(&["-c", "."], "\"unterminated\n").await;
        assert_matches(&["-c", "."], "1\u{0001}2").await;
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

    // ----- Regex parity (fancy-regex vs Oniguruma) -----

    #[tokio::test]
    async fn diff_regex_basic_test() {
        assert_matches(&[r#"test("\\d+")"#], r#""abc123""#).await;
    }

    #[tokio::test]
    async fn diff_regex_lookahead_test() {
        assert_matches(&[r#"test("foo(?=bar)")"#], r#""foobar""#).await;
    }

    #[tokio::test]
    async fn diff_regex_lookbehind_test() {
        assert_matches(&[r#"test("(?<=foo)bar")"#], r#""foobar""#).await;
    }

    #[tokio::test]
    async fn diff_regex_backref_test() {
        assert_matches(&[r#"test("(\\w+) \\1")"#], r#""hello hello""#).await;
    }

    #[tokio::test]
    async fn diff_regex_match_offset() {
        assert_matches(&[r#"match("ll") | .offset"#], r#""hello""#).await;
    }

    #[tokio::test]
    async fn diff_regex_match_unicode_offset() {
        // Real jq reports char (not byte) offset; bashkit must match.
        assert_matches(&[r#"match("ll") | .offset"#], r#""héllo""#).await;
    }

    #[tokio::test]
    async fn diff_regex_scan_digits() {
        assert_matches(&[r#"[scan("\\d+")]"#], r#""a1 b22 c333""#).await;
    }

    #[tokio::test]
    async fn diff_regex_capture_named() {
        assert_matches(&[r#"capture("(?<word>\\w+),")"#], r#""apple,banana""#).await;
    }
}

#[tokio::test]
async fn filter_from_file_treats_positionals_as_inputs() {
    let files = [
        ("/f.jq", "# doubled\n.a | map(. * 2)\n"),
        ("/in.json", r#"{"a":[1,2]}"#),
    ];
    let result = run_jq_with_files(&["-c", "-f", "/f.jq", "/in.json"], &files)
        .await
        .expect("jq -f runs");
    assert_eq!(result.stdout, "[2,4]\n");
    let result = run_jq_with_files(&["-cf", "/missing.jq", "/in.json"], &files)
        .await
        .expect("jq -f runs");
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("Could not open /missing.jq"));
}

// =========================================================================
// Input stream: -n with input/inputs, per-file names, errors per input
// (behavior checked against jq 1.7)
// =========================================================================

#[tokio::test]
async fn null_input_leaves_stream_to_input_and_inputs() {
    assert_eq!(
        run_jq_with_args(&["-n", "input"], "41 42").await.unwrap(),
        "41\n"
    );
    assert_eq!(
        run_jq_with_args(&["-nc", "[inputs]"], "1 \"a\" [2]")
            .await
            .unwrap(),
        "[1,\"a\",[2]]\n"
    );
    assert_eq!(
        run_jq_with_args(&["-nc", "[limit(2; inputs)], input"], "1 2 3 4")
            .await
            .unwrap(),
        "[1,2]\n3\n"
    );
    assert_eq!(
        run_jq_with_args(&["-n", "reduce inputs as $x (0; . + $x)"], "1 2 39")
            .await
            .unwrap(),
        "42\n"
    );
}

#[tokio::test]
async fn slurp_of_empty_input_is_empty_array() {
    assert_eq!(run_jq_with_args(&["-s", "."], "").await.unwrap(), "[]\n");
    assert_eq!(run_jq_with_args(&["."], "").await.unwrap(), "");
}

#[tokio::test]
async fn input_fails_when_stream_is_exhausted() {
    let result = run_jq_result_with_args(&["-c", "[., input]"], "1 2 3")
        .await
        .unwrap();
    assert_eq!(result.stdout, "[1,2]\n");
    assert_eq!(result.stderr, "jq: error (at <stdin>:0): No more inputs\n");
    assert_eq!(result.exit_code, 5);
}

#[tokio::test]
async fn input_filename_follows_each_file() {
    let result = run_jq_with_files(
        &["-c", "[., input_filename]", "/a.json", "/b.json"],
        &[("/a.json", "{\"f\":\"a\"}"), ("/b.json", "1 2")],
    )
    .await
    .unwrap();
    assert_eq!(
        result.stdout,
        "[{\"f\":\"a\"},\"/a.json\"]\n[1,\"/b.json\"]\n[2,\"/b.json\"]\n"
    );
}

#[tokio::test]
async fn missing_file_among_others_is_skipped_with_exit_2() {
    let result = run_jq_with_files(
        &["-c", ".", "/a.json", "/nope.json"],
        &[("/a.json", "{\"ok\":true}")],
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "{\"ok\":true}\n");
    assert_eq!(
        result.stderr,
        "jq: error: Could not open file /nope.json: No such file or directory\n"
    );
    assert_eq!(result.exit_code, 2);
}

#[tokio::test]
async fn runtime_error_moves_on_to_next_input() {
    let result = run_jq_result_with_args(
        &["-c", ".a | ascii_downcase"],
        "{\"a\":\"X\"} 1 {\"a\":\"Y\"}",
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "\"x\"\n\"y\"\n");
    assert_eq!(
        result.stderr,
        "jq: error (at <stdin>:0): Cannot index number with string \"a\"\n"
    );
    // The last input succeeded.
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn runtime_error_location_names_file_and_line() {
    let result = run_jq_with_files(
        &[".[] | .n | ascii_downcase", "/d.json"],
        &[("/d.json", "[\n  {\"n\": \"A\"},\n  {\"n\": 5}\n]\n")],
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "\"a\"\n");
    assert!(
        result.stderr.starts_with("jq: error (at /d.json:4): "),
        "stderr={}",
        result.stderr
    );
    assert_eq!(result.exit_code, 5);
}

#[tokio::test]
async fn error_with_string_prints_message_unquoted() {
    let result = run_jq_result_with_args(&["-n", "error(\"boom\")"], "")
        .await
        .unwrap();
    assert_eq!(result.stderr, "jq: error (at <unknown>): boom\n");
    let result = run_jq_result_with_args(&["-n", "error({a:1})"], "")
        .await
        .unwrap();
    assert_eq!(
        result.stderr,
        "jq: error (at <unknown>) (not a string): {\"a\":1}\n"
    );
}

#[tokio::test]
async fn values_before_a_parse_error_are_processed() {
    let result = run_jq_result_with_args(&["-c", "."], "{\"a\":1}\n{a:2}")
        .await
        .unwrap();
    assert_eq!(result.stdout, "{\"a\":1}\n");
    assert!(
        result.stderr.contains("invalid JSON"),
        "stderr={}",
        result.stderr
    );
    assert_eq!(result.exit_code, 5);
    // A literal glued to junk is one bad token: nothing is emitted.
    let result = run_jq_result_with_args(&["-c", "."], "1\u{0001}2")
        .await
        .unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 5);
}

// =========================================================================
// Numbers and value errors (behavior checked against jq 1.7)
// =========================================================================

#[tokio::test]
async fn computed_floats_print_like_jq() {
    let out = run_jq_with_args(
        &[
            "-nc",
            "[pow(2;10), (17/5|floor), (1.5*2), 1e17*1, 0.00001*1, (0.1+0.2), (2.5*2.5)]",
        ],
        "",
    )
    .await
    .unwrap();
    assert_eq!(out, "[1024,3,3,1e+17,1e-05,0.30000000000000004,6.25]\n");
}

#[tokio::test]
async fn input_number_literals_keep_their_form_until_computed() {
    let out = run_jq_with_args(&["-c", "[.[], (.[0] + 1)]"], "[1.0, 2.50]")
        .await
        .unwrap();
    assert_eq!(out, "[1.0,2.5,2]\n");
}

#[tokio::test]
async fn nan_and_infinities_render_like_jq() {
    let out = run_jq_with_args(
        &[
            "-nc",
            "[nan, infinite, -infinite, (nan|isnan), (infinite|isinfinite)]",
        ],
        "",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        "[null,1.7976931348623157e+308,-1.7976931348623157e+308,true,true]\n"
    );
}

#[tokio::test]
async fn math_errors_use_jq_wording() {
    let out = run_jq_with_args(
        &[
            "-c",
            "[(try (\"a\"+1) catch .), (try ({\"aaaaaaa\":\"bbbbbbbbbbb\"}+1) catch .), (try ([1]*{}) catch .), (try (1/0) catch .), (try (5%0) catch .)]",
        ],
        "null",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        concat!(
            "[\"string (\\\"a\\\") and number (1) cannot be added\",",
            "\"object ({\\\"aaaaaaa\\\":...) and number (1) cannot be added\",",
            "\"array ([1]) and object ({}) cannot be multiplied\",",
            "\"number (1) and number (0) cannot be divided because the divisor is zero\",",
            "\"number (5) and number (0) cannot be divided (remainder) because the divisor is zero\"]\n"
        )
    );
}

#[tokio::test]
async fn division_by_zero_is_a_runtime_error() {
    let result = run_jq_result_with_args(&["-c", "1 / .x"], "{\"x\": 2} {\"x\": 0} {\"x\": 4}")
        .await
        .unwrap();
    assert_eq!(result.stdout, "0.5\n0.25\n");
    assert_eq!(
        result.stderr,
        "jq: error (at <stdin>:0): number (1) and number (0) cannot be divided because the divisor is zero\n"
    );
}

#[tokio::test]
async fn remainder_truncates_operands() {
    let out = run_jq_with_args(&["-nc", "[5 % 3, -5 % 3, 5 % -3, 5.9 % 3, 1e20 % 7]"], "")
        .await
        .unwrap();
    assert_eq!(out, "[2,-2,2,2,0]\n");
}

#[tokio::test]
async fn index_rules_follow_jq() {
    let out = run_jq_with_args(
        &["-c", ".[-1], .[-10], .[1.7], (try .[\"a\"] catch .)"],
        "[1, 2, 3]",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        "3\nnull\n2\n\"Cannot index array with string \\\"a\\\"\"\n"
    );
    let result = run_jq_result_with_args(&["getpath([\"a\",0])"], "{\"a\":{\"b\":1}}")
        .await
        .unwrap();
    assert_eq!(
        result.stderr,
        "jq: error (at <stdin>:0): Cannot index object with number\n"
    );
    assert_eq!(result.exit_code, 5);
}

#[tokio::test]
async fn gamma_is_log_gamma() {
    let out = run_jq_with_args(&["-nc", "[8 | gamma, significand, logb]"], "")
        .await
        .unwrap();
    assert_eq!(out, "[8.525161361065413,1,3]\n");
}

#[tokio::test]
async fn update_through_null_creates_containers() {
    let out = run_jq_with_args(
        &[
            "-nc",
            "(.a.b.c = 1), (.[2] = 1), (.[1:3] = [\"x\"]), (.a |= empty), \
             ([1] | .[3] = 9), ([1] | .[1.7] = 2), ([1] | .[5] |= empty)",
        ],
        "",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        "{\"a\":{\"b\":{\"c\":1}}}\n[null,null,1]\n[\"x\"]\nnull\n[1,null,null,9]\n[1,2]\n[1]\n"
    );
}

#[tokio::test]
async fn update_index_errors_follow_jq() {
    let out = run_jq_with_args(
        &[
            "-nc",
            "(try (.[-1] = 1) catch .), (try ({} | .[1] = 2) catch .), \
             (try (.[1e9] = 1) catch .), (try (.[] = 1) catch .)",
        ],
        "",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        "\"Out of bounds negative array index\"\n\"Cannot index object with number\"\n\
         \"Array index too large\"\n\"Cannot iterate over null (null)\"\n"
    );
}

#[tokio::test]
async fn string_builtins_follow_jq() {
    let out = run_jq_with_args(
        &[
            "-nc",
            "([1, null, \"a\", true] | join(\"-\")), ([null] | join(\",\")), \
             (try ([[1]] | join(\",\")) catch .), (1 | ltrimstr(\"a\")), \
             (\"ab\" | rtrimstr(1)), (\"a1b2\" | [scan(\"([a-z])([0-9])\")]), \
             (\"ab\" | capture(\"(?<x>a)(?<y>z)?\")), \"\\u007f\"",
        ],
        "",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        "\"1--a-true\"\n\"\"\n\"string (\\\"\\\") and array ([1]) cannot be added\"\n1\n\"ab\"\n\
         [[\"a\",\"1\"],[\"b\",\"2\"]]\n{\"x\":\"a\",\"y\":null}\n\"\\u007f\"\n"
    );
}

#[tokio::test]
async fn unmatched_group_is_reported_like_jq() {
    let out = run_jq_with_args(&["-c", "[match(\"(a)(z)?\").captures[]]"], "\"ab\"")
        .await
        .unwrap();
    assert_eq!(
        out,
        "[{\"offset\":0,\"length\":1,\"string\":\"a\",\"name\":null},\
         {\"offset\":-1,\"string\":null,\"length\":0,\"name\":null}]\n"
    );
}

#[tokio::test]
async fn from_entries_and_object_keys_follow_jq() {
    let out = run_jq_with_args(
        &["-c", "from_entries"],
        r#"[{"key":"a","value":1},{"name":"c","value":3},{"Name":"N","value":4},{"key":"d"},{"name":"f","Value":6}]"#,
    )
    .await
    .unwrap();
    assert_eq!(out, "{\"a\":1,\"c\":3,\"N\":4,\"d\":null,\"f\":6}\n");
    let result = run_jq_result_with_args(&["-c", "from_entries"], r#"[{"key":null,"value":4}]"#)
        .await
        .unwrap();
    assert_eq!(
        result.stderr,
        "jq: error (at <stdin>:0): Cannot use null (null) as object key\n"
    );
    assert_eq!(result.exit_code, 5);
}

#[tokio::test]
async fn tostream_round_trips() {
    let out = run_jq_with_args(
        &[
            "-c",
            "[tostream], fromstream(tostream), [1 | truncate_stream([[0], 1], [[1, 0], 2], [[1, 0]], [[1]])], IN(.[]; 1)",
        ],
        r#"[1,{"a":2},[]]"#,
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        "[[[0],1],[[1,\"a\"],2],[[1,\"a\"]],[[2],[]],[[2]]]\n[1,{\"a\":2},[]]\n[[[0],2],[[0]]]\ntrue\n"
    );
}

#[test]
fn run_loop_never_calls_unwrap_valr() {
    // THREAT[TM-INF-023]: `jaq_core::unwrap_valr` exits the process on halt.
    assert!(!include_str!("mod.rs").contains("unwrap_valr("));
}

#[tokio::test]
async fn ascii_output_escapes_non_ascii() {
    let out = run_jq_with_args(&["-ac", "."], r#"{"s":"olá ☃ 😀"}"#)
        .await
        .unwrap();
    assert_eq!(out, "{\"s\":\"ol\\u00e1 \\u2603 \\ud83d\\ude00\"}\n");
    // -a keeps strings JSON-encoded even under -r, like jq.
    let out = run_jq_with_args(&["-ra", "."], "\"é\"").await.unwrap();
    assert_eq!(out, "\"\\u00e9\"\n");
}

#[tokio::test]
async fn raw_output0_separates_with_nul() {
    let out = run_jq_with_args(&["--raw-output0", ".[]"], r#"[1,"x",{}]"#)
        .await
        .unwrap();
    assert_eq!(out, "1\0x\0{}\0");
    let result = run_jq_result_with_args(&["--raw-output0", ".[]"], r#"["a\u0000b","c"]"#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "");
    assert_eq!(
        result.stderr,
        "jq: error (at <stdin>:0): Cannot dump a string containing NUL with --raw-output0 option\n"
    );
    assert_eq!(result.exit_code, 5);
}

#[tokio::test]
async fn stream_flag_feeds_path_events() {
    let out = run_jq_with_args(&["-c", "--stream", "."], r#"{"a":[1,{"b":null}],"c":"d"}"#)
        .await
        .unwrap();
    assert_eq!(
        out,
        "[[\"a\",0],1]\n[[\"a\",1,\"b\"],null]\n[[\"a\",1,\"b\"]]\n[[\"a\",1]]\n[[\"c\"],\"d\"]\n[[\"c\"]]\n"
    );
    let out = run_jq_with_args(&["-c", "--stream", "."], "1 [] {}")
        .await
        .unwrap();
    assert_eq!(out, "[[],1]\n[[],[]]\n[[],{}]\n");
    let out = run_jq_with_args(&["-nc", "--stream", "fromstream(inputs)"], r#"{"a":[1,2]}"#)
        .await
        .unwrap();
    assert_eq!(out, "{\"a\":[1,2]}\n");
}

#[tokio::test]
async fn stream_slurp_and_metadata_preserve_event_order() {
    let result = run_jq_with_files(
        &[
            "-c",
            "--stream",
            "[input_filename, .]",
            "/first.json",
            "/second.json",
        ],
        &[("/first.json", "[1,[]]"), ("/second.json", "{}")],
    )
    .await
    .unwrap();
    assert_eq!(
        result.stdout,
        "[\"/first.json\",[[0],1]]\n[\"/first.json\",[[1],[]]]\n[\"/first.json\",[[1]]]\n[\"/second.json\",[[],{}]]\n"
    );
    let out = run_jq_with_args(&["-sc", "--stream", "."], "[1,[]] {}")
        .await
        .unwrap();
    assert_eq!(out, "[[[0],1],[[1],[]],[[1]],[[],{}]]\n");
    let out = run_jq_with_args(&["-nc", "--stream", "[limit(1; inputs)], input"], "[1,2]")
        .await
        .unwrap();
    assert_eq!(out, "[[[0],1]]\n[[1],2]\n");
}

#[tokio::test]
async fn seq_flag_uses_record_separators() {
    let out = run_jq_with_args(&["--seq", "-c", "."], "\x1e[1]\n\x1e2\n")
        .await
        .unwrap();
    assert_eq!(out, "\x1e[1]\n\x1e2\n");
}

#[tokio::test]
async fn destructuring_alternatives_follow_jq() {
    let out = run_jq_with_args(
        &["-c", ".[] as [$x, $y] ?// $x | [$x, $y]"],
        r#"[1, {"b": 2}, [4, 5]]"#,
    )
    .await
    .unwrap();
    assert_eq!(out, "[1,null]\n[{\"b\":2},null]\n[4,5]\n");
    // An error in the body moves on to the next pattern; the last one's
    // error propagates.
    let result = run_jq_result_with_args(
        &[
            "-c",
            ".[] as [$a] ?// $a | if $a == 1 then error(\"x\") else $a end",
        ],
        "[[1]]",
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "[1]\n");
    let result = run_jq_result_with_args(&["-c", ". as [$a] ?// $a | error(\"e\")"], "[1]")
        .await
        .unwrap();
    assert_eq!(result.stderr, "jq: error (at <stdin>:0): e\n");
    let result = run_jq_result_with_args(&["-c", ". as [$a] ?// {a: $a} | error(\"e\")"], "[1]")
        .await
        .unwrap();
    assert_eq!(
        result.stderr,
        "jq: error (at <stdin>:0): Cannot index array with string \"a\"\n"
    );
}

#[tokio::test]
async fn decoding_builtins_follow_jq() {
    let out = run_jq_with_args(
        &[
            "-nc",
            "([55296, 65] | implode), (\"YW\" | @base64d), (\"YQ\" | @base64d), \
             (try (\"!!\" | @base64d) catch .), (try (\"Y\" | @base64d) catch .), \
             (try (\"x\" | strptime(\"%Y\")) catch .), (try (\"2026-01-15\" | fromdate) catch .), \
             (\"2026-01-15T12:00:00Z\" | fromdate)",
        ],
        "",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        "\"\u{fffd}A\"\n\"a\"\n\"a\"\n\"string (\\\"!!\\\") is not valid base64 data\"\n\
         \"string (\\\"Y\\\") trailing base64 byte found\"\n\
         \"date \\\"x\\\" does not match format \\\"%Y\\\"\"\n\
         \"date \\\"2026-01-15\\\" does not match format \\\"%Y-%m-%dT%H:%M:%SZ\\\"\"\n1768478400\n"
    );
}
