//! Tests for awk: parser limits, interpreter limits, end-to-end runs.

use super::parser::AwkParser;
use super::{Awk, csv_split_fields};
use crate::builtins::limits::{
    AWK_MAX_GETLINE_CACHE_BYTES as MAX_GETLINE_CACHE_BYTES,
    AWK_MAX_GETLINE_CACHED_FILES as MAX_GETLINE_CACHED_FILES,
    AWK_MAX_GETLINE_FILE_BYTES as MAX_GETLINE_FILE_BYTES, AWK_MAX_OUTPUT_BYTES as MAX_OUTPUT_BYTES,
    AWK_MAX_OUTPUT_TARGETS as MAX_OUTPUT_TARGETS,
};
use crate::builtins::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

use crate::fs::{FileSystem, InMemoryFs};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

async fn run_awk(args: &[&str], stdin: Option<&str>) -> Result<ExecResult> {
    let awk = Awk;
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
        stdin,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    awk.execute(ctx).await
}

#[tokio::test]
async fn test_awk_print_all() {
    let result = run_awk(&["{print}"], Some("hello\nworld")).await.unwrap();
    assert_eq!(result.stdout, "hello\nworld\n");
}

#[tokio::test]
async fn test_awk_print_field() {
    let result = run_awk(&["{print $1}"], Some("hello world\nfoo bar"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "hello\nfoo\n");
}

#[tokio::test]
async fn test_awk_print_multiple_fields() {
    let result = run_awk(&["{print $2, $1}"], Some("hello world"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "world hello\n");
}

#[tokio::test]
async fn test_awk_field_separator() {
    let result = run_awk(&["-F:", "{print $1}"], Some("root:x:0:0"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "root\n");
}

#[tokio::test]
async fn test_awk_nr() {
    let result = run_awk(&["{print NR, $0}"], Some("a\nb\nc")).await.unwrap();
    assert_eq!(result.stdout, "1 a\n2 b\n3 c\n");
}

#[tokio::test]
async fn test_awk_nf() {
    let result = run_awk(&["{print NF}"], Some("a b c\nd e")).await.unwrap();
    assert_eq!(result.stdout, "3\n2\n");
}

#[tokio::test]
async fn test_awk_begin_end() {
    let result = run_awk(
        &["BEGIN{print \"start\"} {print} END{print \"end\"}"],
        Some("middle"),
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "start\nmiddle\nend\n");
}

#[tokio::test]
async fn test_awk_pattern() {
    let result = run_awk(&["/hello/{print}"], Some("hello\nworld\nhello again"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "hello\nhello again\n");
}

#[tokio::test]
async fn test_awk_condition() {
    let result = run_awk(&["NR==2{print}"], Some("line1\nline2\nline3"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "line2\n");
}

#[tokio::test]
async fn test_awk_arithmetic() {
    let result = run_awk(&["{print $1 + $2}"], Some("1 2\n3 4"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "3\n7\n");
}

#[tokio::test]
async fn test_awk_variables() {
    let result = run_awk(&["{sum += $1} END{print sum}"], Some("1\n2\n3\n4"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "10\n");
}

#[tokio::test]
async fn test_awk_unicode_identifier_no_panic() {
    let result = run_awk(&["BEGIN{café=7; print café}"], None).await.unwrap();
    assert_eq!(result.stdout, "7\n");
}

#[tokio::test]
async fn test_awk_length() {
    let result = run_awk(&["{print length($0)}"], Some("hello\nhi"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "5\n2\n");
}

#[tokio::test]
async fn test_awk_substr() {
    let result = run_awk(&["{print substr($0, 2, 3)}"], Some("hello"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "ell\n");
}

#[tokio::test]
async fn test_awk_toupper() {
    let result = run_awk(&["{print toupper($0)}"], Some("hello"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "HELLO\n");
}

#[tokio::test]
async fn test_awk_multi_statement() {
    // Test multiple statements separated by semicolon
    let result = run_awk(&["{x=1; print x}"], Some("test")).await.unwrap();
    assert_eq!(result.stdout, "1\n");
}

#[tokio::test]
async fn test_awk_gsub_with_print() {
    // gsub with regex literal followed by print
    let result = run_awk(
        &[r#"{gsub(/hello/, "hi"); print}"#],
        Some("hello hello hello"),
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "hi hi hi\n");
}

#[tokio::test]
async fn test_awk_split_with_array_access() {
    // split with array indexing
    let result = run_awk(
        &[r#"{n = split($0, arr, ":"); print arr[2]}"#],
        Some("a:b:c"),
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "b\n");
}

/// TM-DOS-027: Deeply nested parenthesized expressions must be rejected
#[test]
fn test_awk_parser_depth_limit_parens() {
    // Build expression with 150 nested parens: (((((...(1)...))))
    let depth = 150;
    let open = "(".repeat(depth);
    let close = ")".repeat(depth);
    let program = format!("{{print {open}1{close}}}");

    let mut parser = AwkParser::new(&program);
    let result = parser.parse();
    assert!(result.is_err(), "deeply nested parens must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("nesting too deep"),
        "error should mention nesting: {err}"
    );
}

/// TM-DOS-027: Deeply chained unary operators must be rejected
#[test]
fn test_awk_parser_depth_limit_unary() {
    // Build expression with 200 chained negations: - - - ... - 1
    let depth = 200;
    let prefix = "- ".repeat(depth);
    let program = format!("{{print {prefix}1}}");

    let mut parser = AwkParser::new(&program);
    let result = parser.parse();
    assert!(result.is_err(), "deeply chained unary ops must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("nesting too deep"),
        "error should mention nesting: {err}"
    );
}

/// TM-DOS-027: Range patterns must not recursively parse comma chains.
#[test]
fn test_awk_parser_rejects_chained_range_pattern() {
    let operands = std::iter::repeat_n("1", 150).collect::<Vec<_>>().join(",");
    let program = format!("{operands}{{print}}");

    let mut parser = AwkParser::new(&program);
    let result = parser.parse();
    assert!(result.is_err(), "comma-chained ranges must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("unexpected ',' after range pattern"),
        "error should mention unexpected comma after range: {err}"
    );
}

/// TM-DOS-027: Moderate nesting within limit still works
#[test]
fn test_awk_parser_moderate_nesting_ok() {
    // 10 levels of parens should be fine
    let depth = 10;
    let open = "(".repeat(depth);
    let close = ")".repeat(depth);
    let program = format!("{{print {open}1{close}}}");

    let mut parser = AwkParser::new(&program);
    let result = parser.parse();
    assert!(
        result.is_ok(),
        "moderate nesting should succeed: {:?}", // debug-ok: assert-failure message
        result.err()
    );
}

// === New tests for added features ===

#[tokio::test]
async fn test_awk_for_c_style() {
    let result = run_awk(&["BEGIN{for(i=1;i<=5;i++) print i}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n2\n3\n4\n5\n");
}

#[tokio::test]
async fn test_awk_for_with_body_block() {
    let result = run_awk(&["BEGIN{for(i=0;i<3;i++){print i}}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "0\n1\n2\n");
}

#[tokio::test]
async fn test_awk_while_loop() {
    let result = run_awk(&["BEGIN{i=1; while(i<=3){print i; i++}}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n2\n3\n");
}

#[tokio::test]
async fn test_awk_do_while() {
    let result = run_awk(&["BEGIN{i=1; do{print i; i++}while(i<=3)}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n2\n3\n");
}

#[tokio::test]
async fn test_awk_post_increment() {
    let result = run_awk(&["{print i++}"], Some("a\nb\nc")).await.unwrap();
    assert_eq!(result.stdout, "0\n1\n2\n");
}

#[tokio::test]
async fn test_awk_pre_increment() {
    let result = run_awk(&["{print ++i}"], Some("a\nb\nc")).await.unwrap();
    assert_eq!(result.stdout, "1\n2\n3\n");
}

#[tokio::test]
async fn test_awk_post_decrement() {
    let result = run_awk(&["BEGIN{x=3; print x--; print x}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "3\n2\n");
}

#[tokio::test]
async fn test_awk_array_assign() {
    let result = run_awk(&[r#"BEGIN{a[1]="x"; a[2]="y"; print a[1], a[2]}"#], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "x y\n");
}

#[tokio::test]
async fn test_awk_array_in_operator() {
    let result = run_awk(
        &[r#"BEGIN{a["foo"]=1; if("foo" in a) print "yes"; if("bar" in a) print "no"}"#],
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "yes\n");
}

#[tokio::test]
async fn test_awk_for_in_loop() {
    let result = run_awk(
        &[r#"BEGIN{a[1]="x"; a[2]="y"; for(k in a) count++; print count}"#],
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "2\n");
}

#[tokio::test]
async fn test_awk_delete_array_element() {
    let result = run_awk(
        &[r#"BEGIN{a[1]=1; a[2]=2; delete a[1]; for(k in a) print k, a[k]}"#],
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "2 2\n");
}

#[tokio::test]
async fn test_awk_v_flag() {
    let result = run_awk(&["-v", "x=hello", "BEGIN{print x}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "hello\n");
}

#[tokio::test]
async fn test_awk_v_flag_numeric() {
    let result = run_awk(&["-v", "n=42", "BEGIN{print n+1}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "43\n");
}

#[tokio::test]
async fn test_awk_break_in_for() {
    let result = run_awk(&["BEGIN{for(i=1;i<=10;i++){if(i>3) break; print i}}"], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n2\n3\n");
}

#[tokio::test]
async fn test_awk_continue_in_for() {
    let result = run_awk(
        &["BEGIN{for(i=1;i<=5;i++){if(i==3) continue; print i}}"],
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "1\n2\n4\n5\n");
}

#[tokio::test]
async fn test_awk_ternary() {
    let result = run_awk(&[r#"{print ($1>2 ? "big" : "small")}"#], Some("1\n3\n2"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "small\nbig\nsmall\n");
}

#[tokio::test]
async fn test_awk_field_assignment() {
    let result = run_awk(&[r#"{$2="new"; print}"#], Some("one two three"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "one new three\n");
}

#[tokio::test]
async fn test_awk_csv_to_json_pattern() {
    // This is the pattern LLMs use for CSV→JSON conversion
    let result = run_awk(
        &[
            "-F,",
            r#"NR==1{for(i=1;i<=NF;i++) h[i]=$i; next} {for(i=1;i<=NF;i++) printf "%s=%s ", h[i], $i; print ""}"#,
        ],
        Some("name,age\nalice,30\nbob,25"),
    )
    .await
    .unwrap();
    assert!(result.stdout.contains("name=alice"));
    assert!(result.stdout.contains("age=30"));
    assert!(result.stdout.contains("name=bob"));
}

#[tokio::test]
async fn test_awk_compound_array_assign() {
    let result = run_awk(
        &[r#"{count[$1]++} END{for(k in count) print k, count[k]}"#],
        Some("a\nb\na\nc\nb\na"),
    )
    .await
    .unwrap();
    // Order may vary, so check contents
    assert!(result.stdout.contains("a 3"));
    assert!(result.stdout.contains("b 2"));
    assert!(result.stdout.contains("c 1"));
}

#[tokio::test]
async fn test_awk_next_statement() {
    let result = run_awk(&["NR==2{next} {print}"], Some("line1\nline2\nline3"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "line1\nline3\n");
}

#[tokio::test]
async fn test_awk_exit_statement() {
    let result = run_awk(&["NR==2{exit} {print}"], Some("line1\nline2\nline3"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "line1\n");
}

#[tokio::test]
async fn test_awk_getline_basic() {
    let result = run_awk(&["{getline; print}"], Some("line1\nline2"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "line2\n");
}

#[tokio::test]
async fn test_awk_getline_updates_fields() {
    let result = run_awk(&["{getline; print $1}"], Some("a b\nc d"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "c\n");
}

#[tokio::test]
async fn test_awk_getline_at_eof() {
    // getline at EOF should keep current $0
    let result = run_awk(&["{getline; print}"], Some("only")).await.unwrap();
    assert_eq!(result.stdout, "only\n");
}

#[tokio::test]
async fn test_awk_revenue_calculation() {
    // This is the exact eval task pattern
    let result = run_awk(
        &["-F,", "NR>1{total+=$2*$3} END{print total}"],
        Some("product,price,quantity\nwidget,10,5\ngadget,25,3\ndoohickey,7,12\nsprocket,15,8"),
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "329\n");
}

#[tokio::test]
async fn test_awk_printf_parens() {
    // printf with parenthesized syntax: printf("format", args)
    let result = run_awk(
        &[r#"BEGIN{printf("["); printf("%s", "x"); printf("]"); print ""}"#],
        Some(""),
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "[x]\n");
}

#[tokio::test]
async fn test_awk_printf_parens_csv() {
    // CSV to JSON pattern using printf with parens
    let result = run_awk(
        &[
            "-F,",
            r#"NR==1{for(i=1;i<=NF;i++) h[i]=$i; next} {printf("%s{", (NR>2?",":"")); for(i=1;i<=NF;i++){printf("%s\"%s\":\"%s\"", (i>1?",":""), h[i], $i)}; printf("}")} END{print ""}"#,
        ],
        Some("name,age\nalice,30\nbob,25\n"),
    )
    .await
    .unwrap();
    assert!(result.stdout.contains("alice"));
    assert!(result.stdout.contains("bob"));
}

#[tokio::test]
async fn test_awk_recursive_function_depth_limit() {
    // Recursive function should be limited, not stack overflow
    let result = run_awk(
        &[r#"function r(n) { return r(n+1) } BEGIN { print r(0) }"#],
        Some(""),
    )
    .await
    .unwrap();
    // Should complete without crashing (returns Uninitialized -> empty string)
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_awk_while_loop_limited() {
    // Infinite while loop should terminate via default max_loop_iterations
    let result = run_awk(
        &[r#"BEGIN { i=0; while(1) { i++; if(i>200000) break } print i }"#],
        Some(""),
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    let count: usize = result.stdout.trim().parse().unwrap();
    // Should be capped at default max_loop_iterations (10_000), not 200_000
    assert!(
        count <= 10_001,
        "loop ran {} times, expected <= 10001",
        count
    );
}

#[tokio::test]
async fn test_awk_unicode_in_comment() {
    // Issue #395: multi-byte Unicode chars in comments should not panic
    let result = run_awk(&["# ── header ──────\n{ print $1 }"], Some("hello world\n"))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello");
}

#[tokio::test]
async fn test_awk_unicode_in_string() {
    // Multi-byte chars in string literals should not panic
    let result = run_awk(&[r#"BEGIN { print "café" }"#], Some(""))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "café");
}

#[tokio::test]
async fn test_awk_array_assign_field_ref_subscript() {
    // Issue #396.1: arr[$1] = $3 should work with field refs as subscripts
    let result = run_awk(
        &["{ arr[$1] = $2 } END { print arr[\"hello\"] }"],
        Some("hello world\n"),
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "world");
}

#[tokio::test]
async fn test_awk_multi_subscript() {
    // Issue #396.2: a["x","y"] multi-subscript with SUBSEP
    let result = run_awk(&[r#"BEGIN { a["x","y"] = 1; print a["x","y"] }"#], Some(""))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "1");
}

#[tokio::test]
async fn test_awk_subsep_defined() {
    // Issue #396.3: SUBSEP should be defined as \034
    let result = run_awk(&[r#"BEGIN { print length(SUBSEP) }"#], Some(""))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "1");
}

#[tokio::test]
async fn test_awk_preincrement_array() {
    // Issue #396.4: ++arr[key] should work
    let result = run_awk(
        &["{ ++count[$1] } END { for (k in count) print k, count[k] }"],
        Some("a\nb\na\n"),
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("a 2"));
    assert!(result.stdout.contains("b 1"));
}

/// Helper that returns the VFS alongside the result for testing file output.
async fn run_awk_with_fs(
    args: &[&str],
    stdin: Option<&str>,
) -> (Result<ExecResult>, Arc<InMemoryFs>) {
    let awk = Awk;
    let fs = Arc::new(InMemoryFs::new());
    let mut vars = HashMap::new();
    let mut cwd = PathBuf::from("/");
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    let ctx = Context {
        args: &args,
        env: &HashMap::new(),
        variables: &mut vars,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = awk.execute(ctx).await;
    (result, fs)
}

#[tokio::test]
async fn test_awk_print_redirect_truncate() {
    // Issue #607: print ... > file should create file with content
    let (result, fs) = run_awk_with_fs(&[r#"BEGIN{print "hello" > "/tmp/out"}"#], None).await;
    let result = result.unwrap();
    assert_eq!(result.exit_code, 0);
    // stdout should be empty (output went to file)
    assert_eq!(result.stdout, "");
    let content = fs
        .read_file(std::path::Path::new("/tmp/out"))
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&content), "hello\n");
}

#[tokio::test]
async fn test_awk_printf_redirect_truncate() {
    // Issue #607: printf ... > file should create file with content
    let (result, fs) = run_awk_with_fs(&[r#"BEGIN{printf "hello" > "/tmp/out"}"#], None).await;
    let result = result.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "");
    let content = fs
        .read_file(std::path::Path::new("/tmp/out"))
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&content), "hello");
}

#[tokio::test]
async fn test_awk_print_redirect_append() {
    // Issue #607: print ... >> file should append
    let (result, fs) = run_awk_with_fs(
        &[r#"BEGIN{print "a" > "/tmp/out"; print "b" >> "/tmp/out"}"#],
        None,
    )
    .await;
    let result = result.unwrap();
    assert_eq!(result.exit_code, 0);
    let content = fs
        .read_file(std::path::Path::new("/tmp/out"))
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&content), "a\nb\n");
}

#[tokio::test]
async fn test_awk_print_redirect_multiple_to_same_file() {
    // Multiple prints to same file with > should accumulate (AWK keeps file open)
    let (result, fs) = run_awk_with_fs(
        &[r#"BEGIN{print "line1" > "/tmp/out"; print "line2" > "/tmp/out"}"#],
        None,
    )
    .await;
    let result = result.unwrap();
    assert_eq!(result.exit_code, 0);
    let content = fs
        .read_file(std::path::Path::new("/tmp/out"))
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&content), "line1\nline2\n");
}

#[tokio::test]
async fn test_awk_print_redirect_pipe_unsupported() {
    // Pipe output should return clear error
    let (result, _fs) = run_awk_with_fs(&[r#"BEGIN{print "hello" | "cat"}"#], None).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("pipe"),
        "expected pipe error, got: {err_msg}"
    );
}

// ========================================================================
// --csv flag (issues #617, #618)
// ========================================================================

#[tokio::test]
async fn test_awk_csv_basic() {
    let result = run_awk(&["--csv", "{print $2}"], Some("a,b,c\nd,e,f"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "b\ne\n");
}

#[tokio::test]
async fn test_awk_csv_quoted_fields() {
    // Quoted field with embedded comma
    let result = run_awk(&["--csv", "{print $2}"], Some(r#"1,"hello, world",3"#))
        .await
        .unwrap();
    assert_eq!(result.stdout, "hello, world\n");
}

#[tokio::test]
async fn test_awk_csv_escaped_quotes() {
    // Double-quote escaping per RFC 4180
    let result = run_awk(&["--csv", "{print $2}"], Some(r#"1,"she said ""hi""",3"#))
        .await
        .unwrap();
    assert_eq!(result.stdout, "she said \"hi\"\n");
}

#[tokio::test]
async fn test_awk_csv_nf() {
    let result = run_awk(&["--csv", "{print NF}"], Some("a,b,c"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "3\n");
}

#[tokio::test]
async fn test_awk_csv_ofs() {
    // In CSV mode, OFS defaults to comma
    let result = run_awk(&["--csv", "{print $1, $3}"], Some("a,b,c"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "a,c\n");
}

#[tokio::test]
async fn test_awk_csv_empty_fields() {
    let result = run_awk(&["--csv", "{print NF}"], Some("a,,c,"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "4\n");
}

#[tokio::test]
async fn test_awk_csv_k_flag_alias() {
    // -k is an alias for --csv
    let result = run_awk(&["-k", "{print $2}"], Some("a,b,c")).await.unwrap();
    assert_eq!(result.stdout, "b\n");
}

#[test]
fn test_csv_split_fields_unit() {
    assert_eq!(csv_split_fields("a,b,c"), vec!["a", "b", "c"]);
    assert_eq!(
        csv_split_fields(r#"1,"hello, world",3"#),
        vec!["1", "hello, world", "3"]
    );
    assert_eq!(csv_split_fields(r#""a""b",c"#), vec!["a\"b", "c"]);
    assert_eq!(csv_split_fields("a,,c,"), vec!["a", "", "c", ""]);
}

// ========================================================================
// gawk 5.3+ Unicode escape sequences (issue #617)
// ========================================================================

#[tokio::test]
async fn test_awk_unicode_escape_basic() {
    // \u followed by hex digits → Unicode character
    let result = run_awk(&[r#"BEGIN{print "\u0041"}"#], None).await.unwrap();
    assert_eq!(result.stdout, "A\n");
}

#[tokio::test]
async fn test_awk_unicode_escape_multibyte() {
    // \u00E9 → é (Latin small e with acute)
    let result = run_awk(&[r#"BEGIN{print "\u00E9"}"#], None).await.unwrap();
    assert_eq!(result.stdout, "é\n");
}

#[tokio::test]
async fn test_awk_unicode_escape_emoji() {
    // \u1F600 → 😀 (grinning face, 5 hex digits)
    let result = run_awk(&[r#"BEGIN{print "\u1F600"}"#], None).await.unwrap();
    assert_eq!(result.stdout, "😀\n");
}

#[tokio::test]
async fn test_awk_unicode_escape_bare_u() {
    // \u with no hex digits → literal \u
    let result = run_awk(&[r#"BEGIN{print "\u "}"#], None).await.unwrap();
    assert_eq!(result.stdout, "\\u \n");
}

#[tokio::test]
async fn test_awk_unicode_escape_mixed() {
    // Mix of Unicode escapes and regular text
    let result = run_awk(&[r#"BEGIN{print "caf\u00E9"}"#], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "café\n");
}

// ========================================================================
// getline from file (issue #796)
// ========================================================================

/// Helper: create an in-memory FS with a file, then run awk.
async fn run_awk_with_file(
    args: &[&str],
    stdin: Option<&str>,
    file_path: &str,
    file_content: &str,
) -> Result<ExecResult> {
    let awk = Awk;
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new(file_path), file_content.as_bytes())
        .await
        .unwrap();
    let mut vars = HashMap::new();
    let mut cwd = PathBuf::from("/");
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    let ctx = Context {
        args: &args,
        env: &HashMap::new(),
        variables: &mut vars,
        cwd: &mut cwd,
        fs,
        stdin,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    awk.execute(ctx).await
}

#[tokio::test]
async fn test_awk_getline_file_into_var() {
    // getline var < "file" reads lines one at a time
    let result = run_awk_with_file(
        &[r#"BEGIN{while((getline line < "/tmp/data.txt") > 0) print line}"#],
        None,
        "/tmp/data.txt",
        "alpha\nbeta\ngamma",
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "alpha\nbeta\ngamma\n");
}

#[tokio::test]
async fn test_awk_getline_file_no_var() {
    // getline < "file" without variable updates $0
    let result = run_awk_with_file(
        &[r#"BEGIN{getline < "/tmp/data.txt"; print}"#],
        None,
        "/tmp/data.txt",
        "first\nsecond",
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "first\n");
}

#[tokio::test]
async fn test_awk_getline_file_eof() {
    // getline at EOF keeps variable unchanged
    let result = run_awk_with_file(
        &[r#"BEGIN{getline x < "/tmp/f.txt"; getline x < "/tmp/f.txt"; print x}"#],
        None,
        "/tmp/f.txt",
        "only",
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "only\n");
}

#[tokio::test]
async fn test_awk_getline_file_missing() {
    // getline from non-existent file should not crash (returns -1 / empty)
    let result = run_awk(&[r#"BEGIN{getline x < "/no/such/file"; print "ok"}"#], None)
        .await
        .unwrap();
    assert_eq!(result.stdout, "ok\n");
}

#[tokio::test]
async fn test_awk_output_limit_exceeded() {
    // Each iteration prints a 1000-char line. 100k iters = ~100MB >> 10MB limit.
    let result = run_awk(
        &[r#"BEGIN { s = sprintf("%1000s", "x"); for(i=0;i<100000;i++) print s }"#],
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(
        result.stderr.contains("output limit exceeded"),
        "stderr should mention output limit: {}",
        result.stderr
    );
    assert!(
        result.stdout.len() <= 11_000_000,
        "stdout should be bounded: {} bytes",
        result.stdout.len()
    );
}

#[tokio::test]
async fn test_awk_single_write_over_limit_rejected() {
    // One oversized record must be rejected before buffering stdout.
    let input = "x".repeat(MAX_OUTPUT_BYTES);
    let result = run_awk(&["{ print }"], Some(&input)).await.unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(
        result.stderr.contains("output limit exceeded"),
        "stderr should mention output limit: {}",
        result.stderr
    );
    assert_eq!(result.stdout.len(), 0);
}

#[tokio::test]
async fn test_awk_output_under_limit_ok() {
    // Small output well under 10MB should succeed normally
    let result = run_awk(&[r#"BEGIN { for(i=0;i<100;i++) print "hello" }"#], None)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 100);
}

#[tokio::test]
async fn test_awk_file_redirect_output_limit() {
    // File redirect output should also be bounded
    let result = run_awk(
        &[r#"BEGIN { s = sprintf("%1000s", "x"); for(i=0;i<100000;i++) print s > "/tmp/out" }"#],
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(
        result.stderr.contains("output limit exceeded"),
        "stderr should mention output limit: {}",
        result.stderr
    );
}

#[tokio::test]
async fn test_awk_file_redirect_target_limit() {
    // Many tiny writes to unique paths must be capped without scanning all prior buffers.
    let program = format!(
        r#"BEGIN {{ for(i=0;i<{};i++) print "x" > ("/tmp/out" i) }}"#,
        MAX_OUTPUT_TARGETS + 1
    );
    let result = run_awk(&[&program], None).await.unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(
        result
            .stderr
            .contains("too many output redirection targets"),
        "stderr should mention target limit: {}",
        result.stderr
    );
}

/// Helper: run AWK with a caller-provided VFS.
async fn run_awk_with_custom_fs(
    args: &[&str],
    stdin: Option<&str>,
    fs: Arc<InMemoryFs>,
) -> Result<ExecResult> {
    let awk = Awk;
    let mut vars = HashMap::new();
    let mut cwd = PathBuf::from("/");
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    let ctx = Context {
        args: &args,
        env: &HashMap::new(),
        variables: &mut vars,
        cwd: &mut cwd,
        fs,
        stdin,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    awk.execute(ctx).await
}

#[tokio::test]
async fn test_awk_getline_file_cache_limit_exceeded() {
    // Opening more than MAX_GETLINE_CACHED_FILES distinct files must fail
    // gracefully (getline returns -1 for new files beyond the limit).
    use crate::fs::FsLimits;

    let limits = FsLimits {
        max_file_count: 200_000,
        max_total_bytes: 200_000_000,
        ..FsLimits::default()
    };
    let fs = Arc::new(InMemoryFs::with_limits(limits));
    let count = MAX_GETLINE_CACHED_FILES + 5;
    for i in 0..count {
        fs.write_file(
            std::path::Path::new(&format!("/tmp/f{i}.txt")),
            format!("line{i}").as_bytes(),
        )
        .await
        .unwrap();
    }

    // AWK program: read one line from each file, count successes
    let prog = format!(
        r#"BEGIN{{ ok=0; for(i=0;i<{count};i++) {{ f="/tmp/f"i".txt"; if((getline x < f)>0) ok++ }} print ok }}"#,
    );
    let result = run_awk_with_custom_fs(&[&prog], None, fs).await.unwrap();
    let ok: usize = result.stdout.trim().parse().unwrap();
    // Exactly MAX_GETLINE_CACHED_FILES should succeed, rest should fail
    assert_eq!(ok, MAX_GETLINE_CACHED_FILES);
}

#[tokio::test]
async fn test_awk_getline_file_cache_within_limit() {
    // Opening a reasonable number of files should all succeed.
    let fs = Arc::new(InMemoryFs::new());
    let count = 10;
    for i in 0..count {
        fs.write_file(
            std::path::Path::new(&format!("/tmp/f{i}.txt")),
            format!("data{i}").as_bytes(),
        )
        .await
        .unwrap();
    }

    let prog = format!(
        r#"BEGIN{{ ok=0; for(i=0;i<{count};i++) {{ f="/tmp/f"i".txt"; if((getline x < f)>0) ok++ }} print ok }}"#,
    );
    let result = run_awk_with_custom_fs(&[&prog], None, fs).await.unwrap();
    let ok: usize = result.stdout.trim().parse().unwrap();
    assert_eq!(ok, count);
}

#[tokio::test]
async fn test_awk_getline_file_size_limit() {
    // A file exceeding FsLimits::max_file_size is rejected by getline.
    // Defense-in-depth: VFS also enforces limits, so a file at exactly
    // the boundary is accepted while one over is rejected at VFS level.
    use crate::fs::FsLimits;

    let limits = FsLimits {
        max_file_size: 100,
        ..FsLimits::unlimited()
    };
    let fs = Arc::new(InMemoryFs::with_limits(limits));
    // Write a file within limits -- should be readable via getline.
    fs.write_file(std::path::Path::new("/tmp/ok.txt"), &[b'a'; 100])
        .await
        .unwrap();
    // Attempt to write an oversized file -- VFS rejects it, so getline
    // returns -1 (file not found).
    let _ = fs
        .write_file(std::path::Path::new("/tmp/big.txt"), &[b'x'; 101])
        .await;

    // Within-limit file succeeds
    let result = run_awk_with_custom_fs(
        &[r#"BEGIN{r=(getline x < "/tmp/ok.txt"); print r}"#],
        None,
        fs.clone(),
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "1\n");

    // Over-limit file fails (not stored by VFS)
    let result = run_awk_with_custom_fs(
        &[r#"BEGIN{r=(getline x < "/tmp/big.txt"); print r}"#],
        None,
        fs,
    )
    .await
    .unwrap();
    assert_eq!(result.stdout, "-1\n");
}

#[tokio::test]
async fn test_awk_getline_file_normalizes_cache_key() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/tmp/data.txt"), b"one\ntwo\n")
        .await
        .unwrap();

    let result = run_awk_with_custom_fs(
        &[r#"BEGIN{
            r1=(getline a < "/tmp/data.txt");
            r2=(getline b < "/tmp/./data.txt");
            r3=(getline c < "/tmp/././data.txt");
            print r1, a; print r2, b; print r3
        }"#],
        None,
        fs,
    )
    .await
    .unwrap();

    assert_eq!(result.stdout, "1 one\n1 two\n0\n");
}

#[tokio::test]
async fn test_awk_getline_file_builtin_size_limit() {
    let fs = Arc::new(InMemoryFs::with_limits(crate::fs::FsLimits::unlimited()));
    fs.write_file(
        std::path::Path::new("/tmp/big.txt"),
        &vec![b'x'; MAX_GETLINE_FILE_BYTES + 1],
    )
    .await
    .unwrap();

    let result = run_awk_with_custom_fs(
        &[r#"BEGIN{r=(getline x < "/tmp/big.txt"); print r}"#],
        None,
        fs,
    )
    .await
    .unwrap();

    assert_eq!(result.stdout, "-1\n");
}

#[tokio::test]
async fn test_awk_getline_file_total_cache_byte_limit() {
    let fs = Arc::new(InMemoryFs::with_limits(crate::fs::FsLimits::unlimited()));
    let chunk = MAX_GETLINE_CACHE_BYTES / 2 + 1;
    fs.write_file(std::path::Path::new("/tmp/a.txt"), &vec![b'a'; chunk])
        .await
        .unwrap();
    fs.write_file(std::path::Path::new("/tmp/b.txt"), &vec![b'b'; chunk])
        .await
        .unwrap();

    let result = run_awk_with_custom_fs(
        &[r#"BEGIN{r1=(getline a < "/tmp/a.txt"); r2=(getline b < "/tmp/b.txt"); print r1, r2}"#],
        None,
        fs,
    )
    .await
    .unwrap();

    assert_eq!(result.stdout, "1 -1\n");
}

// TM-INF-022: malformed-input corpus must not leak Debug shapes.
const AWK_BANNED: &[&str] = &["AwkError::", "ParseError {", "Token::"];

#[tokio::test]
async fn no_leak_unbalanced_brace() {
    let r = crate::builtins::debug_leak_check::run("echo 1 | awk 'BEGIN { print'").await;
    crate::builtins::debug_leak_check::assert_no_leak(&r, "awk_unbalanced_brace", AWK_BANNED);
}

#[tokio::test]
async fn no_leak_invalid_regex() {
    let r = crate::builtins::debug_leak_check::run("echo 1 | awk '/[/'").await;
    crate::builtins::debug_leak_check::assert_no_leak(&r, "awk_invalid_regex", AWK_BANNED);
}

#[tokio::test]
async fn no_leak_undefined_function_call() {
    let r =
        crate::builtins::debug_leak_check::run("echo 1 | awk 'BEGIN { totally_undefined_fn() }'")
            .await;
    crate::builtins::debug_leak_check::assert_no_leak(
        &r,
        "awk_undefined_function_call",
        AWK_BANNED,
    );
}
