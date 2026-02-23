//! Property-based security tests for Bashkit
//!
//! These tests use proptest to generate random inputs and verify
//! that Bashkit maintains security invariants under all conditions.
//!
//! Run with: cargo test --test proptest_security

use bashkit::{Bash, ExecutionLimits};
use proptest::prelude::*;
use std::time::Duration;

// Strategy for generating arbitrary bash-like input
// Note: Limited character set and length to avoid parser pathological cases
fn bash_input_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-zA-Z0-9_ ;|$()]{0,50}").unwrap()
}

// Strategy for generating resource-intensive scripts
fn resource_stress_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Long pipelines
        (2..20usize).prop_map(|n| {
            let mut s = "echo x".to_string();
            for _ in 0..n {
                s.push_str(" | cat");
            }
            s
        }),
        // Many commands
        (2..50usize).prop_map(|n| { (0..n).map(|_| "echo x").collect::<Vec<_>>().join("; ") }),
        // Long variable names
        (1..100usize).prop_map(|n| format!("{}=value", "A".repeat(n))),
    ]
}

proptest! {
    // 16 cases per test - fast enough for CI
    // Parser fuzzing is done in nightly workflow due to potential hangs (threat model V3)
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Lexer should never panic on arbitrary input
    /// Note: Parser tests moved to fuzz workflow due to potential hangs (threat model V3)
    #[test]
    fn lexer_never_panics(input in bash_input_strategy()) {
        let mut lexer = bashkit::parser::Lexer::new(&input);
        // Consume all tokens - should never panic
        while lexer.next_token().is_some() {}
    }

    /// Resource limits should be enforced
    #[test]
    fn resource_limits_enforced(input in resource_stress_strategy()) {
        thread_local! {
            static RT: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
        }

        RT.with(|rt| {
            rt.block_on(async {
                let limits = ExecutionLimits::new()
                    .max_commands(10)
                    .max_loop_iterations(10)
                    .timeout(Duration::from_millis(20));

                let mut bash = Bash::builder().limits(limits).build();
                let _ = bash.exec(&input).await;
            });
        });
    }

    /// Output should not exceed reasonable bounds
    /// Uses resource_stress_strategy which generates valid bash (arbitrary input can hang parser)
    #[test]
    fn output_bounded(input in resource_stress_strategy()) {
        thread_local! {
            static RT: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
        }

        let (stdout_len, stderr_len) = RT.with(|rt| {
            rt.block_on(async {
                let limits = ExecutionLimits::new()
                    .max_commands(10)
                    .timeout(Duration::from_millis(20));

                let mut bash = Bash::builder().limits(limits).build();

                if let Ok(result) = bash.exec(&input).await {
                    (result.stdout.len(), result.stderr.len())
                } else {
                    (0, 0)
                }
            })
        });

        prop_assert!(stdout_len < 10_000_000);
        prop_assert!(stderr_len < 10_000_000);
    }

    /// Path traversal attempts should be contained
    #[test]
    fn path_traversal_contained(
        prefix in "[.]{0,10}",
        slashes in "[/]{1,10}",
        segments in proptest::collection::vec("[.]{0,3}", 0..10)
    ) {
        thread_local! {
            static RT: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
        }

        let path = format!("{prefix}{slashes}{}", segments.join("/"));
        let script = format!("cat {path}");

        RT.with(|rt| {
            rt.block_on(async {
                let mut bash = Bash::new();
                let _ = bash.exec(&script).await;
            });
        });
    }

    /// Variable expansion should not execute code
    #[test]
    fn variable_expansion_safe(var_content in "[^']{0,100}") {
        thread_local! {
            static RT: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
        }

        let script = format!("X='{var_content}'; echo $X");

        RT.with(|rt| {
            rt.block_on(async {
                let limits = ExecutionLimits::new()
                    .max_commands(10)
                    .timeout(Duration::from_millis(20));

                let mut bash = Bash::builder().limits(limits).build();
                let _ = bash.exec(&script).await;
            });
        });
    }
}

// Additional focused tests

#[test]
fn test_deeply_nested_parens() {
    // Test very deep nesting doesn't cause stack overflow
    let deep = format!("{}1{}", "(".repeat(500), ")".repeat(500));
    let parser = bashkit::parser::Parser::new(&deep);
    let _ = parser.parse();
}

#[test]
fn test_very_long_pipeline() {
    let pipeline = (0..100).map(|_| "cat").collect::<Vec<_>>().join(" | ");
    let script = format!("echo x | {pipeline}");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let limits = ExecutionLimits::new()
            .max_commands(200)
            .timeout(Duration::from_millis(500));

        let mut bash = Bash::builder().limits(limits).build();
        let _ = bash.exec(&script).await;
    });
}

#[test]
fn test_null_bytes_handled() {
    // Null bytes should not cause issues
    let input = "echo hello\x00world";
    let parser = bashkit::parser::Parser::new(input);
    let _ = parser.parse();
}

#[test]
fn test_unicode_handling() {
    let scripts = [
        "echo 你好世界",
        "echo مرحبا",
        "echo 🎉🚀",
        "VAR=émoji; echo $VAR",
        "echo '\u{0000}\u{FFFF}'",
    ];

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        for script in scripts {
            let mut bash = Bash::new();
            let _ = bash.exec(script).await;
        }
    });
}

/// Regression: proptest found multi-byte char panic in variable expansion
/// Input "${:¡%" caused byte index panic in substring/parameter expansion
#[test]
fn test_multibyte_in_variable_expansion() {
    let scripts = [
        "X='${:¡%'; echo $X",
        "X='¡%'; echo ${X:1}",
        "X='日本語'; echo ${X:1:2}",
        "X='émoji'; echo ${X:0:3}",
        "X='über'; echo ${#X}",
    ];

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        for script in scripts {
            let limits = ExecutionLimits::new()
                .max_commands(10)
                .timeout(Duration::from_millis(100));
            let mut bash = Bash::builder().limits(limits).build();
            let _ = bash.exec(script).await;
        }
    });
}
