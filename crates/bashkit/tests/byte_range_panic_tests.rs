// Regression tests for byte range panics in the interpreter.
// Issue #1090: WordPart::Length handler panicked with "byte range starts at 43
// but ends at 8" when malformed input caused ']' to appear before '[' in the
// name string passed to ${#...}.

use bashkit::{Bash, ExecutionLimits};

/// Fuzz crash input from issue #1090 (decoded from base64).
/// Original artifact: crash-3c5c6ff235787b4ba345b870d35590436d6bc2c1
#[tokio::test]
async fn fuzz_crash_1090_byte_range_panic() {
    use base64::Engine;
    let b64 = "JCgAanEkeyMAAAAAAAAAAF0AAAAAADMAAAAAAAAAACQmAAAAAAAAAAAAAAAAAAAAAABbWz0m";
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap_or_default();
    let input = String::from_utf8_lossy(&decoded);
    let script = format!("echo $(({})) 2>/dev/null", input);

    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(100)
                .max_function_depth(10)
                .max_subst_depth(5)
                .max_stdout_bytes(4096)
                .max_stderr_bytes(4096)
                .timeout(std::time::Duration::from_millis(500)),
        )
        .build();

    // Must not panic — errors are acceptable
    let _ = bash.exec(&script).await;
}

/// Direct test: ${#name} where name contains ']' before '['.
#[tokio::test]
async fn length_with_bracket_before_open() {
    let mut bash = Bash::builder().build();

    // This creates a ${#...} expression where the parser might produce
    // a Length node with malformed name containing ']' before '['
    let result = bash.exec("x=']foo[bar'; echo ${#x}").await.unwrap();
    // Should not panic — just returns the length of $x
    assert!(result.exit_code == 0);
}

/// Edge case: ${#arr[idx]} with empty array name.
#[tokio::test]
async fn length_empty_array_name() {
    let mut bash = Bash::builder().build();
    let result = bash.exec("echo ${#[0]} 2>/dev/null").await;
    // Should not panic — error or empty is acceptable
    let _ = result;
}
