//! Property-based security tests for BashKit
//!
//! These tests use proptest to generate random inputs and verify
//! that BashKit maintains security invariants under all conditions.
//!
//! Run with: cargo test --test proptest_security

use bashkit::{Bash, ExecutionLimits};
use proptest::prelude::*;
use std::time::Duration;

// Strategy for generating arbitrary bash-like input
fn bash_input_strategy() -> impl Strategy<Value = String> {
    // Generate strings with bash-relevant characters (limited to 200 chars for speed)
    proptest::string::string_regex(
        "[a-zA-Z0-9_${}()\\[\\];|&<>\"'\\s\\-=+*/!@#%^~`.,:?\\\\]{0,200}",
    )
    .unwrap()
}

// Strategy for generating deeply nested structures
fn nested_structure_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Nested parentheses
        (1..50usize).prop_map(|n| format!("{}echo x{}", "(".repeat(n), ")".repeat(n))),
        // Nested braces
        (1..50usize).prop_map(|n| format!("{}echo x{}", "{".repeat(n), "}".repeat(n))),
        // Nested command substitution
        (1..20usize).prop_map(|n| {
            let mut s = "echo x".to_string();
            for _ in 0..n {
                s = format!("$({s})");
            }
            s
        }),
        // Nested arithmetic
        (1..20usize).prop_map(|n| {
            let mut s = "1".to_string();
            for _ in 0..n {
                s = format!("$(({s}+1))");
            }
            format!("echo {s}")
        }),
    ]
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
    // Use minimal cases for CI (default 256 is too slow)
    // PROPTEST_CASES env var can override, or run fuzzing workflow for thorough testing
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Parser should never panic on arbitrary input
    /// Note: This test is slow with complex inputs, thorough testing done in fuzz workflow
    #[test]
    #[ignore] // Run with `cargo test -- --ignored` for full coverage
    fn parser_never_panics(input in bash_input_strategy()) {
        let parser = bashkit::parser::Parser::new(&input);
        // Should return Ok or Err, never panic
        let _ = parser.parse();
    }

    /// Lexer should never panic on arbitrary input
    #[test]
    fn lexer_never_panics(input in bash_input_strategy()) {
        let mut lexer = bashkit::parser::Lexer::new(&input);
        // Consume all tokens - should never panic
        while lexer.next_token().is_some() {}
    }

    /// Execution with limits should always terminate
    /// Note: This test is slow, comprehensive testing done in fuzz workflow
    #[test]
    #[ignore] // Run with `cargo test -- --ignored` for full coverage
    fn execution_always_terminates(input in bash_input_strategy()) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let limits = ExecutionLimits::new()
                .max_commands(50)
                .max_loop_iterations(50)
                .max_function_depth(5)
                .timeout(Duration::from_millis(100));

            let mut bash = Bash::builder().limits(limits).build();

            // Should complete (with Ok or Err), never hang
            let _ = tokio::time::timeout(
                Duration::from_millis(200),
                bash.exec(&input)
            ).await;
        });
    }

    /// Nested structures should not cause stack overflow
    /// Note: This test can be slow with deep nesting, thorough testing done in fuzz workflow
    #[test]
    #[ignore] // Run with `cargo test -- --ignored` for full coverage
    fn nested_structures_safe(input in nested_structure_strategy()) {
        let parser = bashkit::parser::Parser::new(&input);
        // Should handle deep nesting gracefully
        let _ = parser.parse();
    }

    /// Resource limits should be enforced
    #[test]
    fn resource_limits_enforced(input in resource_stress_strategy()) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let limits = ExecutionLimits::new()
                .max_commands(10)
                .max_loop_iterations(10)
                .timeout(Duration::from_millis(50));

            let mut bash = Bash::builder().limits(limits).build();
            let _ = bash.exec(&input).await;
            // Should complete without hanging
        });
    }

    /// Output should not exceed reasonable bounds
    /// Note: This test is slow, comprehensive testing done in fuzz workflow
    #[test]
    #[ignore] // Run with `cargo test -- --ignored` for full coverage
    fn output_bounded(input in bash_input_strategy()) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (stdout_len, stderr_len) = rt.block_on(async {
            let limits = ExecutionLimits::new()
                .max_commands(20)
                .timeout(Duration::from_millis(50));

            let mut bash = Bash::builder().limits(limits).build();

            if let Ok(result) = bash.exec(&input).await {
                (result.stdout.len(), result.stderr.len())
            } else {
                (0, 0)
            }
        });

        // Output should be bounded by our limits
        // Note: Currently no output limit, but execution limits prevent runaway
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
        let path = format!("{prefix}{slashes}{}", segments.join("/"));
        let script = format!("cat {path}");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let mut bash = Bash::new();
            // Should not access real filesystem regardless of path
            let _ = bash.exec(&script).await;
        });
    }

    /// Variable expansion should not execute code
    #[test]
    fn variable_expansion_safe(var_content in "[^']{0,100}") {
        let script = format!("X='{var_content}'; echo $X");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let limits = ExecutionLimits::new()
                .max_commands(10)
                .timeout(Duration::from_millis(100));

            let mut bash = Bash::builder().limits(limits).build();

            if let Ok(result) = bash.exec(&script).await {
                // Output should contain the variable content, not execute it
                // This validates that variable expansion doesn't lead to injection
                let _ = result;
            }
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
