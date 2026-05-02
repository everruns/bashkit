//! Cross-tool guard: stderr from any builtin must never leak Rust Debug
//! representations of internal types.
//!
//! Real shell tools print short, opaque error messages. When a builtin wraps
//! a third-party library (jaq, regex, serde_json, semver, …) and formats the
//! library's error with `{:?}`, the result is `File { code: "…800 chars of
//! prepended source…", path: () }, [("@tsv", Filter(0))]` — internal struct
//! shapes leaking into stderr where LLM agents see them.
//!
//! This test fires curated malformed inputs at the high-risk builtins (the
//! ones that wrap external libraries with `Debug`-ful error types) and
//! asserts stderr:
//!   - is bounded in length (≤ 1 KB)
//!   - starts with a tool name (`<tool>: …`)
//!   - contains none of the banned Debug-shape substrings
//!   - contains none of the tool-specific internal markers
//!
//! Static counterpart: `scripts/check-no-debug-fmt.sh` forbids `{:?}` in
//! builtin source via grep.

use bashkit::{Bash, ControlFlow, ExecResult};

/// Universal Debug-shape substrings. Any of these in stderr means a Rust
/// Debug formatter reached the agent. Keep this list general — tool-specific
/// internals are checked separately.
const UNIVERSAL_BANNED: &[&str] = &[
    // jaq internals
    "File {",
    "path: ()",
    "Token(",
    "Tok::",
    "Undefined::",
    "Errors {",
    // generic Debug-of-struct/enum shapes
    "Vec [",
    " { code:",
    "Some([",
    "Span {",
    "Range {",
    // unrendered format directives (catches `format!("{:?}", x)` where x
    // is a String — the format string itself doesn't appear, but the
    // explicit literal does in poorly-written error paths)
    // (intentionally NOT included as a substring — it's too easy to match
    // legitimate stderr like `error: expected '{' got ...`. The static grep
    // tripwire covers this case.)
];

const MAX_STDERR_BYTES: usize = 1024;

#[track_caller]
fn assert_clean(result: &ExecResult, ctx: &str, tool_banned: &[&str]) {
    let stderr = &result.stderr;
    assert!(
        stderr.len() <= MAX_STDERR_BYTES,
        "[{ctx}] stderr exceeds {MAX_STDERR_BYTES} bytes ({} bytes):\n---\n{stderr}\n---",
        stderr.len()
    );
    for pat in UNIVERSAL_BANNED.iter().chain(tool_banned.iter()) {
        assert!(
            !stderr.contains(pat),
            "[{ctx}] stderr leaks banned shape `{pat}`:\n---\n{stderr}\n---"
        );
    }
}

async fn run(script: &str) -> ExecResult {
    // Both Ok(ExecResult { exit_code: !=0, stderr: ... }) and the hard
    // Err(execution error) paths must produce clean diagnostics — we treat
    // them uniformly.
    let mut bash = Bash::new();
    bash.exec(script).await.unwrap_or_else(|e| ExecResult {
        stdout: String::new(),
        stderr: e.to_string(),
        exit_code: 1,
        control_flow: ControlFlow::None,
        ..Default::default()
    })
}

// =============================================================================
// jq — wraps jaq, the densest source of Debug-leak risk in the codebase
// =============================================================================

#[cfg(feature = "jq")]
mod jq {
    use super::*;

    /// jq-specific internals that must never reach stderr — they would reveal
    /// the prepended compat-defs source we splice into every filter.
    const JQ_BANNED: &[&str] = &[
        "__bashkit_env__",
        "JQ_COMPAT_DEFS",
        "def setpath",
        "def leaf_paths",
        "def @tsv:",
        "def @csv:",
        "def env:",
        // Undefined::* variant Debug spellings (post-formatter these become
        // `name/arity is not defined`; the raw variant tag must not appear)
        "Filter(0)",
        "Filter(1)",
        "Filter(2)",
        "Var,",
        "Mod,",
    ];

    macro_rules! jq_case {
        ($name:ident, $script:expr) => {
            #[tokio::test]
            async fn $name() {
                let r = run($script).await;
                assert_clean(&r, stringify!($name), JQ_BANNED);
            }
        };
    }

    // --- compile errors: every Undefined variant ---
    jq_case!(undefined_filter_zero_arity, "echo 1 | jq totally_made_up");
    jq_case!(
        undefined_filter_with_arity,
        "echo 1 | jq 'totally_made_up(1; 2)'"
    );
    jq_case!(undefined_variable, "echo 1 | jq '$nope'");
    jq_case!(undefined_format, "echo '[1]' | jq '@xyzzy'");

    // --- the exact filter from the original bug report ---
    jq_case!(
        harness_tsv_filter_with_undefined_inner,
        r#"echo '{"data":[]}' | jq '
          if (.data | length) == 0 then
            "No harnesses found."
          else
            (.data[] | [(.id // ""), totally_undefined_helper] | @tsv)
          end
        '"#
    );

    // --- parse / lex errors ---
    jq_case!(unbalanced_bracket, "echo 1 | jq '['");
    jq_case!(unbalanced_paren, "echo 1 | jq '('");
    jq_case!(stray_pipe, "echo 1 | jq '|'");
    jq_case!(unterminated_string, r#"echo 1 | jq '"abc'"#);
    jq_case!(if_without_then, "echo 1 | jq 'if . then'");
    jq_case!(reduce_without_as, "echo 1 | jq 'reduce . '");
    jq_case!(def_without_body, "echo 1 | jq 'def f:'");
    jq_case!(empty_brace_expr, "echo 1 | jq '{(.)}'");

    // --- input errors ---
    jq_case!(malformed_json_input, "echo 'not json {' | jq '.'");
    jq_case!(
        deeply_nested_input,
        &("echo '".to_owned() + &"[".repeat(200) + &"]".repeat(200) + "' | jq '.'")
    );

    // --- runtime errors ---
    jq_case!(index_array_with_string, r#"echo '[1,2]' | jq '.foo'"#);
    jq_case!(iterate_over_null, r#"echo 'null' | jq '.[]'"#);
    jq_case!(add_array_and_number, r#"echo '[1,2]' | jq '. + 1'"#);

    // --- @tsv / @csv positive checks (regression: must compile cleanly) ---
    #[tokio::test]
    async fn tsv_compiles_for_user_filter() {
        let r = run(
            r#"echo '{"data":[{"id":"h1","name":"a","description":"d","parent_harness_id":null,"capabilities":["x"],"created_at":"t"}]}' | jq -r '
                .data[] | [(.id // ""), (.name // ""), (.description // ""), (.parent_harness_id // ""), ((.capabilities // []) | length | tostring), (.created_at // "")] | @tsv
            '"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
        assert!(r.stdout.contains("h1"), "stdout: {}", r.stdout);
        assert!(r.stdout.contains('\t'), "tab not present: {}", r.stdout);
    }

    #[tokio::test]
    async fn csv_compiles_basic() {
        let r = run(r#"echo 'null' | jq -r '["a","b"] | @csv'"#).await;
        assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
        assert!(r.stdout.contains("\"a\",\"b\""), "stdout: {}", r.stdout);
    }
}

// =============================================================================
// awk — wraps a bespoke parser whose error type derives Debug
// =============================================================================

mod awk {
    use super::*;

    const AWK_BANNED: &[&str] = &["AwkError::", "ParseError {", "Token::"];

    #[tokio::test]
    async fn unbalanced_brace() {
        let r = run("echo 1 | awk 'BEGIN { print'").await;
        assert_clean(&r, "awk_unbalanced_brace", AWK_BANNED);
    }

    #[tokio::test]
    async fn invalid_regex() {
        let r = run("echo 1 | awk '/[/'").await;
        assert_clean(&r, "awk_invalid_regex", AWK_BANNED);
    }

    #[tokio::test]
    async fn undefined_function_call() {
        let r = run("echo 1 | awk 'BEGIN { totally_undefined_fn() }'").await;
        assert_clean(&r, "awk_undefined_function_call", AWK_BANNED);
    }
}

// =============================================================================
// regex-based tools
// =============================================================================

mod regex_tools {
    use super::*;

    const REGEX_BANNED: &[&str] = &["regex::Error", "ParseError {"];

    #[tokio::test]
    async fn grep_invalid_regex() {
        let r = run(r"echo 1 | grep -E '['").await;
        assert_clean(&r, "grep_invalid_regex", REGEX_BANNED);
    }

    #[tokio::test]
    async fn sed_invalid_regex() {
        let r = run(r"echo 1 | sed -E 's/[//'").await;
        assert_clean(&r, "sed_invalid_regex", REGEX_BANNED);
    }
}

// =============================================================================
// data-format tools — wrap serde_json / toml / serde_yaml
// =============================================================================

mod data_tools {
    use super::*;

    const SERDE_BANNED: &[&str] = &[
        "serde_json::Error",
        "serde_yaml::Error",
        "toml::de::Error",
        "Error { line:",
    ];

    #[tokio::test]
    async fn json_malformed_input() {
        let r = run(r#"echo 'not json' | json get .foo"#).await;
        assert_clean(&r, "json_malformed_input", SERDE_BANNED);
    }

    #[tokio::test]
    async fn yaml_malformed_input() {
        let r = run(r#"echo ':' | yaml get .foo"#).await;
        assert_clean(&r, "yaml_malformed_input", SERDE_BANNED);
    }

    #[tokio::test]
    async fn tomlq_malformed_input() {
        let r = run(r#"echo 'not = toml = nope' | tomlq '.foo'"#).await;
        assert_clean(&r, "tomlq_malformed_input", SERDE_BANNED);
    }

    #[tokio::test]
    async fn csv_malformed_input() {
        let r = run(r#"printf 'a,"unterm\n' | csv to-json"#).await;
        assert_clean(&r, "csv_malformed_input", SERDE_BANNED);
    }
}

// =============================================================================
// number / version / date — wrap chrono, semver, num parsers
// =============================================================================

mod numeric_tools {
    use super::*;

    const PARSER_BANNED: &[&str] = &[
        "ParseFloatError",
        "ParseIntError",
        "chrono::ParseError",
        "semver::Error",
    ];

    #[tokio::test]
    async fn bc_garbage() {
        let r = run(r#"echo 'not an expression {{{' | bc"#).await;
        assert_clean(&r, "bc_garbage", PARSER_BANNED);
    }

    #[tokio::test]
    async fn expr_garbage() {
        let r = run(r#"expr garbage / 0"#).await;
        assert_clean(&r, "expr_garbage", PARSER_BANNED);
    }

    #[tokio::test]
    async fn semver_invalid() {
        let r = run(r#"semver lt not.a.version 1.0.0"#).await;
        assert_clean(&r, "semver_invalid", PARSER_BANNED);
    }

    #[tokio::test]
    async fn date_invalid_format() {
        let r = run(r#"date -d 'not a date in any format'"#).await;
        assert_clean(&r, "date_invalid_format", PARSER_BANNED);
    }

    #[tokio::test]
    async fn numfmt_garbage() {
        let r = run(r#"echo 'not a number' | numfmt --from=iec"#).await;
        assert_clean(&r, "numfmt_garbage", PARSER_BANNED);
    }
}

// =============================================================================
// Generic per-tool sweep: every common builtin called with a bogus flag must
// produce a clean error. This is a coarse net — it won't catch subtle
// per-feature errors, but it instantly catches the most common leak vector
// (a tool that derives Debug on its arg-parsing error and forwards via {:?}).
// =============================================================================

mod bogus_flag_sweep {
    use super::*;

    /// Common builtins that accept flags. Tools without flag-parsing
    /// (true, false, :) and tools that take a full file path instead of
    /// flags (cd, source) are excluded.
    const TOOLS: &[&str] = &[
        "cat",
        "ls",
        "wc",
        "head",
        "tail",
        "sort",
        "uniq",
        "cut",
        "tr",
        "grep",
        "sed",
        "awk",
        "find",
        "tree",
        "diff",
        "comm",
        "paste",
        "column",
        "join",
        "split",
        "fold",
        "expand",
        "unexpand",
        "nl",
        "tac",
        "rev",
        "strings",
        "od",
        "xxd",
        "hexdump",
        "base64",
        "md5sum",
        "sha1sum",
        "sha256sum",
        "tar",
        "gzip",
        "gunzip",
        "zip",
        "unzip",
        "seq",
        "expr",
        "bc",
        "numfmt",
        "test",
        "printf",
        "echo",
        "env",
        "printenv",
        "stat",
        "file",
        "basename",
        "dirname",
        "realpath",
        "csv",
        "json",
        "yaml",
        "tomlq",
        "semver",
        "envsubst",
        "template",
        "patch",
    ];

    #[tokio::test]
    async fn every_builtin_handles_bogus_flag_cleanly() {
        for tool in TOOLS {
            let r = run(&format!("{tool} --xyzzy-not-a-real-flag </dev/null")).await;
            assert_clean(&r, &format!("{tool}_bogus_flag"), &[]);
        }
    }
}
