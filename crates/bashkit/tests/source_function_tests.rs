//! Tests for source/. builtin function loading
//!
//! Verifies that functions defined in sourced files are registered
//! in the calling scope, PATH searching works, and positional
//! parameters are set correctly.

use bashkit::Bash;
use std::path::Path;

/// Source a file that defines a function, then call it
#[tokio::test]
async fn source_loads_function_into_scope() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"greet() { echo \"hello from lib\"; }",
    )
    .await
    .unwrap();

    let result = bash.exec("source /lib.sh\ngreet").await.unwrap();

    assert_eq!(result.stdout.trim(), "hello from lib");
    assert_eq!(result.exit_code, 0);
}

/// Dot command loads function into scope
#[tokio::test]
async fn dot_loads_function_into_scope() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"greet() { echo \"hello from dot\"; }",
    )
    .await
    .unwrap();

    let result = bash.exec(". /lib.sh\ngreet").await.unwrap();

    assert_eq!(result.stdout.trim(), "hello from dot");
    assert_eq!(result.exit_code, 0);
}

/// Source loads multiple functions
#[tokio::test]
async fn source_loads_multiple_functions() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"add() { echo $(( $1 + $2 )); }\nsub() { echo $(( $1 - $2 )); }",
    )
    .await
    .unwrap();

    let result = bash
        .exec("source /lib.sh\nadd 3 2\nsub 10 4")
        .await
        .unwrap();

    assert_eq!(result.stdout, "5\n6\n");
}

/// Source loads function that uses variables from caller's scope
#[tokio::test]
async fn source_function_sees_caller_variables() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"show_name() { echo \"name=$NAME\"; }",
    )
    .await
    .unwrap();

    let result = bash
        .exec("NAME=world\nsource /lib.sh\nshow_name")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "name=world");
}

/// Source sets variables visible in caller scope
#[tokio::test]
async fn source_variables_visible_in_caller() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/lib.sh"), b"LIB_VERSION=1.0")
        .await
        .unwrap();

    let result = bash
        .exec("source /lib.sh\necho $LIB_VERSION")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "1.0");
}

/// Function from source with keyword syntax
#[tokio::test]
async fn source_function_keyword_syntax() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"function myfunc { echo \"keyword style\"; }",
    )
    .await
    .unwrap();

    let result = bash.exec("source /lib.sh\nmyfunc").await.unwrap();

    assert_eq!(result.stdout.trim(), "keyword style");
}

/// Sourced function can call another sourced function
#[tokio::test]
async fn source_function_calls_another() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"inner() { echo \"inner\"; }\nouter() { inner; echo \"outer\"; }",
    )
    .await
    .unwrap();

    let result = bash.exec("source /lib.sh\nouter").await.unwrap();

    assert_eq!(result.stdout, "inner\nouter\n");
}

/// Source across multiple exec calls preserves functions
#[tokio::test]
async fn source_persists_across_exec_calls() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/lib.sh"), b"myfunc() { echo \"persisted\"; }")
        .await
        .unwrap();

    // First exec: source the file
    bash.exec("source /lib.sh").await.unwrap();

    // Second exec: call the function
    let result = bash.exec("myfunc").await.unwrap();

    assert_eq!(result.stdout.trim(), "persisted");
    assert_eq!(result.exit_code, 0);
}

/// Source a file created by the script itself (echo > file, then source)
#[tokio::test]
async fn source_script_created_file() {
    let mut bash = Bash::new();

    let result = bash
        .exec("echo 'myfunc() { echo created; }' > /tmp/lib.sh\nsource /tmp/lib.sh\nmyfunc")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "created");
    assert_eq!(result.exit_code, 0);
}

/// Source file with multi-line function body
#[tokio::test]
async fn source_multiline_function() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"greet() {\n  local name=$1\n  echo \"hello $name\"\n  return 0\n}",
    )
    .await
    .unwrap();

    let result = bash.exec("source /lib.sh\ngreet world").await.unwrap();

    assert_eq!(result.stdout.trim(), "hello world");
}

/// Source from within a function — sourced functions should be globally visible
#[tokio::test]
async fn source_from_within_function() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/lib.sh"), b"helper() { echo \"from helper\"; }")
        .await
        .unwrap();

    let result = bash
        .exec("load_lib() { source /lib.sh; }\nload_lib\nhelper")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "from helper");
    assert_eq!(result.exit_code, 0);
}

/// Source overwrites previously defined function
#[tokio::test]
async fn source_overwrites_function() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/lib.sh"), b"myfunc() { echo \"v2\"; }")
        .await
        .unwrap();

    let result = bash
        .exec("myfunc() { echo \"v1\"; }\nsource /lib.sh\nmyfunc")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "v2");
}

/// Chained source: A sources B, B defines function
#[tokio::test]
async fn source_chained() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/inner.sh"), b"deep_func() { echo \"deep\"; }")
        .await
        .unwrap();

    fs.write_file(Path::new("/outer.sh"), b"source /inner.sh")
        .await
        .unwrap();

    let result = bash.exec("source /outer.sh\ndeep_func").await.unwrap();

    assert_eq!(result.stdout.trim(), "deep");
}

/// Source with function that has return value
#[tokio::test]
async fn source_function_with_return() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"check() { if [ \"$1\" = \"ok\" ]; then return 0; else return 1; fi; }",
    )
    .await
    .unwrap();

    let result = bash
        .exec("source /lib.sh\ncheck ok\necho $?")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "0");

    let result2 = bash.exec("check fail\necho $?").await.unwrap();

    assert_eq!(result2.stdout.trim(), "1");
}

// === PATH searching tests ===

/// Source searches PATH when filename has no slashes
#[tokio::test]
async fn source_searches_path() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    // Put lib.sh in a PATH directory
    fs.mkdir(Path::new("/usr/lib"), true).await.unwrap();
    fs.write_file(
        Path::new("/usr/lib/mylib.sh"),
        b"from_path() { echo \"found via PATH\"; }",
    )
    .await
    .unwrap();

    let result = bash
        .exec("PATH=/usr/lib\nsource mylib.sh\nfrom_path")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "found via PATH");
}

/// Dot command searches PATH when filename has no slashes
#[tokio::test]
async fn dot_searches_path() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/scripts"), true).await.unwrap();
    fs.write_file(
        Path::new("/scripts/helpers.sh"),
        b"path_helper() { echo \"dot path\"; }",
    )
    .await
    .unwrap();

    let result = bash
        .exec("PATH=/scripts\n. helpers.sh\npath_helper")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "dot path");
}

/// Source prefers cwd over PATH for relative paths with slashes
#[tokio::test]
async fn source_relative_path_no_path_search() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/home"), true).await.unwrap();
    fs.write_file(
        Path::new("/home/lib.sh"),
        b"rel_func() { echo \"relative\"; }",
    )
    .await
    .unwrap();

    let result = bash
        .exec("cd /home\nsource ./lib.sh\nrel_func")
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "relative");
}

// === Positional parameters tests ===

/// Source with arguments sets positional parameters
#[tokio::test]
async fn source_positional_params() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/lib.sh"), b"echo \"arg1=$1 arg2=$2\"")
        .await
        .unwrap();

    let result = bash.exec("source /lib.sh hello world").await.unwrap();

    assert_eq!(result.stdout.trim(), "arg1=hello arg2=world");
}

/// Source positional params restore after source completes
#[tokio::test]
async fn source_restores_positional_params() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/lib.sh"), b"echo \"inside=$1\"")
        .await
        .unwrap();

    // Call from within a function that has its own positional params
    let result = bash
        .exec("wrapper() {\n  echo \"before=$1\"\n  source /lib.sh sourced_arg\n  echo \"after=$1\"\n}\nwrapper outer_arg")
        .await
        .unwrap();

    assert_eq!(
        result.stdout,
        "before=outer_arg\ninside=sourced_arg\nafter=outer_arg\n"
    );
}

/// Source with $# and individual positional params in sourced file
#[tokio::test]
async fn source_special_params() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/lib.sh"),
        b"echo \"count=$#\"\necho \"first=$1\"\necho \"second=$2\"\necho \"third=$3\"",
    )
    .await
    .unwrap();

    let result = bash.exec("source /lib.sh a b c").await.unwrap();

    assert_eq!(result.stdout, "count=3\nfirst=a\nsecond=b\nthird=c\n");
}

// === Negative tests (error handling) ===

/// Source with missing filename argument
#[tokio::test]
async fn source_missing_filename() {
    let mut bash = Bash::new();
    let result = bash.exec("source").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("filename argument required"));
}

/// Source with nonexistent file
#[tokio::test]
async fn source_nonexistent_file() {
    let mut bash = Bash::new();
    let result = bash.exec("source /nonexistent.sh").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("No such file"));
}

/// Dot with nonexistent file
#[tokio::test]
async fn dot_nonexistent_file() {
    let mut bash = Bash::new();
    let result = bash.exec(". /nonexistent.sh").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("No such file"));
}

/// Source with invalid syntax in file
#[tokio::test]
async fn source_syntax_error() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/bad.sh"), b"if then fi done")
        .await
        .unwrap();

    let result = bash.exec("source /bad.sh").await.unwrap();
    assert_ne!(result.exit_code, 0);
}

/// Source file not found via PATH search
#[tokio::test]
async fn source_not_in_path() {
    let mut bash = Bash::new();
    let result = bash
        .exec("PATH=/nonexistent\nsource nothere.sh")
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("No such file"));
}

/// Source empty file is valid (no-op)
#[tokio::test]
async fn source_empty_file() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/empty.sh"), b"").await.unwrap();

    let result = bash.exec("source /empty.sh\necho ok").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "ok");
}

/// Source file with only comments is valid
#[tokio::test]
async fn source_comments_only() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/comments.sh"),
        b"# just a comment\n# another one",
    )
    .await
    .unwrap();

    let result = bash.exec("source /comments.sh\necho ok").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "ok");
}
