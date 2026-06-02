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

/// Regression for #1414: `${arr[@]:offset:length}` with negative `length`
/// cast to usize used to overflow `start + len_val` and panic in release.
/// The fix uses `saturating_add().min(values.len())`, so slicing must
/// complete without panicking regardless of the signed length value.
#[tokio::test]
async fn array_slice_negative_length_no_panic() {
    let mut bash = Bash::builder().build();
    let result = bash
        .exec("arr=(a b c d e); echo \"${arr[@]:1:-1}\"")
        .await
        .expect("negative slice length must not panic");
    assert_eq!(result.exit_code, 0);
}

/// Regression for #1414: ensure `start + len_val` near `usize::MAX` does
/// not overflow — a very large length value must saturate, not wrap.
#[tokio::test]
async fn array_slice_huge_length_no_panic() {
    let mut bash = Bash::builder().build();
    let result = bash
        .exec("arr=(a b c); echo \"${arr[@]:1:9999999999999999999}\"")
        .await;
    // Should not panic — either Ok with clamped slice or a graceful error.
    let _ = result;
}

/// Regression: arithmetic_fuzz crash (Jun 1 2026) — null bytes inside
/// arithmetic context with command substitution caused a panic.
/// Artifact: crash-f87fe9f09e5a5a307ed625dfc86d1003031c70ae
#[tokio::test]
async fn arith_fuzz_crash_null_bytes_cmdsub_shopt() {
    // Raw bytes: )&)h$,\n\0\0$(shopt\t8\t\t\0$\0\0(shop\0t
    let input_bytes: &[u8] = b")&)h$,\n\x00\x00$(shopt\t8\t\t\x00$\x00\x00(shop\x00t";
    let input = String::from_utf8_lossy(input_bytes);
    let script = format!("echo $(({input})) 2>/dev/null");
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
    let _ = bash.exec(&script).await;
}

/// Regression: arithmetic_fuzz crash (May 28 2026) — null bytes and SOH in
/// arithmetic context with array subscript substitution caused a panic.
/// Artifact: crash-b9727354157e9576ec4380c5febb7168f8c1d999
#[tokio::test]
async fn arith_fuzz_crash_null_bytes_array_subscript() {
    // Raw bytes: \x01\0$(b=[${?\0Range\x01"e {D$PWD
    let input_bytes: &[u8] = b"\x01\x00$(b=[${?\x00Range\x01\"e {D$PWD";
    let input = String::from_utf8_lossy(input_bytes);
    let script = format!("echo $(({input})) 2>/dev/null");
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
    let _ = bash.exec(&script).await;
}
