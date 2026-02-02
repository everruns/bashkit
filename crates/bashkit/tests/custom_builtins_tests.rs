//! Integration tests for custom builtins
//!
//! Tests the public API for registering and using custom builtin commands.

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, FileSystem, InMemoryFs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Helper struct for testing - a simple echo with prefix
struct PrefixEcho {
    prefix: String,
}

#[async_trait]
impl Builtin for PrefixEcho {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let msg = ctx.args.join(" ");
        Ok(ExecResult::ok(format!("{}{}\n", self.prefix, msg)))
    }
}

/// Helper struct - transforms stdin
struct Transform {
    transform_fn: fn(&str) -> String,
}

#[async_trait]
impl Builtin for Transform {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let input = ctx.stdin.unwrap_or("");
        Ok(ExecResult::ok((self.transform_fn)(input)))
    }
}

/// Helper struct - reads from filesystem
struct FileReader;

#[async_trait]
impl Builtin for FileReader {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let path = match ctx.args.first() {
            Some(p) => std::path::Path::new(p),
            None => return Ok(ExecResult::err("Usage: readfile <path>\n".to_string(), 1)),
        };
        match ctx.fs.read_file(path).await {
            Ok(content) => Ok(ExecResult::ok(
                String::from_utf8_lossy(&content).to_string(),
            )),
            Err(e) => Ok(ExecResult::err(format!("Error: {}\n", e), 1)),
        }
    }
}

/// Helper struct - counter with shared state
struct Counter {
    count: Arc<AtomicU64>,
}

#[async_trait]
impl Builtin for Counter {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let value = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(ExecResult::ok(format!("{}\n", value)))
    }
}

/// Helper struct - returns error
struct Fail {
    message: String,
    code: i32,
}

#[async_trait]
impl Builtin for Fail {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::err(format!("{}\n", self.message), self.code))
    }
}

/// Helper struct - reads env vars
struct EnvDumper;

#[async_trait]
impl Builtin for EnvDumper {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let mut output = String::new();
        let mut keys: Vec<_> = ctx.env.keys().collect();
        keys.sort();
        for key in keys {
            if let Some(value) = ctx.env.get(key) {
                output.push_str(&format!("{}={}\n", key, value));
            }
        }
        Ok(ExecResult::ok(output))
    }
}

// =============================================================================
// Basic functionality tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_simple() {
    let mut bash = Bash::builder()
        .builtin(
            "prefix",
            Box::new(PrefixEcho {
                prefix: "[LOG] ".to_string(),
            }),
        )
        .build();

    let result = bash.exec("prefix hello world").await.unwrap();
    assert_eq!(result.stdout, "[LOG] hello world\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_custom_builtin_no_args() {
    let mut bash = Bash::builder()
        .builtin(
            "prefix",
            Box::new(PrefixEcho {
                prefix: ">>> ".to_string(),
            }),
        )
        .build();

    let result = bash.exec("prefix").await.unwrap();
    assert_eq!(result.stdout, ">>> \n");
}

// =============================================================================
// Pipeline tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_in_pipeline() {
    fn to_upper(s: &str) -> String {
        s.to_uppercase()
    }

    let mut bash = Bash::builder()
        .builtin(
            "upper",
            Box::new(Transform {
                transform_fn: to_upper,
            }),
        )
        .build();

    let result = bash.exec("echo hello | upper").await.unwrap();
    assert_eq!(result.stdout, "HELLO\n");
}

#[tokio::test]
async fn test_custom_builtin_pipeline_chain() {
    fn to_upper(s: &str) -> String {
        s.to_uppercase()
    }

    fn reverse(s: &str) -> String {
        s.chars().rev().collect()
    }

    let mut bash = Bash::builder()
        .builtin(
            "upper",
            Box::new(Transform {
                transform_fn: to_upper,
            }),
        )
        .builtin(
            "reverse",
            Box::new(Transform {
                transform_fn: reverse,
            }),
        )
        .build();

    let result = bash.exec("echo abc | upper | reverse").await.unwrap();
    // "abc\n" -> "ABC\n" -> "\nCBA"
    assert_eq!(result.stdout, "\nCBA");
}

// =============================================================================
// Filesystem access tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_filesystem_access() {
    let fs = Arc::new(InMemoryFs::new());
    // Create parent directory first
    fs.mkdir(std::path::Path::new("/data"), false)
        .await
        .unwrap();
    fs.write_file(
        std::path::Path::new("/data/test.txt"),
        b"custom content here",
    )
    .await
    .unwrap();

    let mut bash = Bash::builder()
        .fs(fs)
        .builtin("readfile", Box::new(FileReader))
        .build();

    let result = bash.exec("readfile /data/test.txt").await.unwrap();
    assert_eq!(result.stdout, "custom content here");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_custom_builtin_filesystem_error() {
    let mut bash = Bash::builder()
        .builtin("readfile", Box::new(FileReader))
        .build();

    let result = bash.exec("readfile /nonexistent").await.unwrap();
    assert!(result.stderr.contains("Error:"));
    assert_eq!(result.exit_code, 1);
}

// =============================================================================
// Stateful builtin tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_shared_state() {
    let counter = Arc::new(AtomicU64::new(0));

    let mut bash = Bash::builder()
        .builtin(
            "counter",
            Box::new(Counter {
                count: counter.clone(),
            }),
        )
        .build();

    let result = bash.exec("counter").await.unwrap();
    assert_eq!(result.stdout, "1\n");

    let result = bash.exec("counter").await.unwrap();
    assert_eq!(result.stdout, "2\n");

    let result = bash.exec("counter").await.unwrap();
    assert_eq!(result.stdout, "3\n");

    // Verify counter state
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

// =============================================================================
// Error handling tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_returns_error() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(Fail {
                message: "Something went wrong".to_string(),
                code: 42,
            }),
        )
        .build();

    let result = bash.exec("fail").await.unwrap();
    assert_eq!(result.stderr, "Something went wrong\n");
    assert_eq!(result.exit_code, 42);
}

#[tokio::test]
async fn test_custom_builtin_error_in_conditional() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(Fail {
                message: "error".to_string(),
                code: 1,
            }),
        )
        .builtin(
            "prefix",
            Box::new(PrefixEcho {
                prefix: "".to_string(),
            }),
        )
        .build();

    // fail || echo should run echo
    let result = bash.exec("fail || prefix success").await.unwrap();
    assert_eq!(result.stdout, "success\n");
    assert_eq!(result.exit_code, 0);
}

// =============================================================================
// Override default builtin tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_override_echo() {
    let mut bash = Bash::builder()
        .builtin(
            "echo",
            Box::new(PrefixEcho {
                prefix: "[CUSTOM] ".to_string(),
            }),
        )
        .build();

    let result = bash.exec("echo hello").await.unwrap();
    assert_eq!(result.stdout, "[CUSTOM] hello\n");
}

// =============================================================================
// Environment access tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_environment_access() {
    let mut bash = Bash::builder()
        .env("FOO", "bar")
        .env("BAZ", "qux")
        .builtin("dumpenv", Box::new(EnvDumper))
        .build();

    let result = bash.exec("dumpenv").await.unwrap();
    assert!(result.stdout.contains("FOO=bar"));
    assert!(result.stdout.contains("BAZ=qux"));
}

// =============================================================================
// Script integration tests
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_in_for_loop() {
    let mut bash = Bash::builder()
        .builtin(
            "prefix",
            Box::new(PrefixEcho {
                prefix: "- ".to_string(),
            }),
        )
        .build();

    let script = r#"
        for item in a b c; do
            prefix $item
        done
    "#;

    let result = bash.exec(script).await.unwrap();
    assert_eq!(result.stdout, "- a\n- b\n- c\n");
}

#[tokio::test]
async fn test_custom_builtin_in_if_condition() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(Fail {
                message: "".to_string(),
                code: 1,
            }),
        )
        .build();

    let script = r#"
        if fail; then
            echo "should not reach"
        else
            echo "correctly handled"
        fi
    "#;

    let result = bash.exec(script).await.unwrap();
    assert_eq!(result.stdout, "correctly handled\n");
}

#[tokio::test]
async fn test_custom_builtin_with_variable_expansion() {
    let mut bash = Bash::builder()
        .builtin(
            "prefix",
            Box::new(PrefixEcho {
                prefix: "".to_string(),
            }),
        )
        .build();

    let result = bash.exec("NAME=Alice; prefix Hello $NAME").await.unwrap();
    assert_eq!(result.stdout, "Hello Alice\n");
}

// =============================================================================
// Multiple custom builtins tests
// =============================================================================

#[tokio::test]
async fn test_multiple_custom_builtins() {
    fn to_upper(s: &str) -> String {
        s.to_uppercase()
    }

    let counter = Arc::new(AtomicU64::new(0));

    let mut bash = Bash::builder()
        .builtin(
            "prefix",
            Box::new(PrefixEcho {
                prefix: "[LOG] ".to_string(),
            }),
        )
        .builtin(
            "upper",
            Box::new(Transform {
                transform_fn: to_upper,
            }),
        )
        .builtin("counter", Box::new(Counter { count: counter }))
        .builtin(
            "fail",
            Box::new(Fail {
                message: "error".to_string(),
                code: 1,
            }),
        )
        .build();

    // Test all work independently
    let result = bash.exec("prefix test").await.unwrap();
    assert_eq!(result.stdout, "[LOG] test\n");

    let result = bash.exec("echo hello | upper").await.unwrap();
    assert_eq!(result.stdout, "HELLO\n");

    let result = bash.exec("counter").await.unwrap();
    assert_eq!(result.stdout, "1\n");

    let result = bash.exec("fail").await.unwrap();
    assert_eq!(result.exit_code, 1);
}

// =============================================================================
// Edge cases
// =============================================================================

#[tokio::test]
async fn test_custom_builtin_empty_name() {
    // Empty command names should work (though unusual)
    struct Empty;

    #[async_trait]
    impl Builtin for Empty {
        async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
            Ok(ExecResult::ok("empty\n".to_string()))
        }
    }

    let mut bash = Bash::builder().builtin("_", Box::new(Empty)).build();

    let result = bash.exec("_").await.unwrap();
    assert_eq!(result.stdout, "empty\n");
}

#[tokio::test]
async fn test_custom_builtin_special_characters_in_output() {
    struct SpecialOutput;

    #[async_trait]
    impl Builtin for SpecialOutput {
        async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
            Ok(ExecResult::ok("line1\nline2\ttab\n".to_string()))
        }
    }

    let mut bash = Bash::builder()
        .builtin("special", Box::new(SpecialOutput))
        .build();

    let result = bash.exec("special").await.unwrap();
    assert_eq!(result.stdout, "line1\nline2\ttab\n");
}
