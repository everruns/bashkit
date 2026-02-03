//! Grammar-based differential fuzzing for BashKit
//!
//! These tests generate random valid bash scripts using proptest strategies
//! and compare BashKit output against real bash to find compatibility issues.
//!
//! Run with: cargo test --test proptest_differential -- --nocapture
//! Run more iterations: PROPTEST_CASES=1000 cargo test --test proptest_differential

use bashkit::Bash;
use proptest::prelude::*;
use std::process::Command;

/// Run script in real bash and capture output
fn run_real_bash(script: &str) -> (String, i32) {
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("Failed to run bash");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code().unwrap_or(1);

    (stdout, exit_code)
}

/// Run script in BashKit and capture output
async fn run_bashkit(script: &str) -> (String, i32) {
    let mut bash = Bash::new();
    match bash.exec(script).await {
        Ok(result) => (result.stdout, result.exit_code),
        Err(_) => (String::new(), 1),
    }
}

// === Grammar-based script generators ===

/// Generate a simple variable name (lowercase letters only for safety)
fn var_name_strategy() -> impl Strategy<Value = String> {
    "[a-z]{1,8}".prop_map(|s| s.to_uppercase())
}

/// Generate a safe literal value (no special chars that could cause issues)
/// Note: at least 1 char to avoid empty strings which cause syntax issues
fn safe_value_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{1,20}"
}

/// Generate a simple echo command
fn echo_command_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // echo literal
        safe_value_strategy().prop_map(|v| format!("echo {}", v)),
        // echo quoted
        safe_value_strategy().prop_map(|v| format!("echo \"{}\"", v)),
        // echo single quoted
        safe_value_strategy().prop_map(|v| format!("echo '{}'", v)),
        // echo -n (no newline)
        safe_value_strategy().prop_map(|v| format!("echo -n {}", v)),
        // echo multiple args
        (safe_value_strategy(), safe_value_strategy())
            .prop_map(|(a, b)| format!("echo {} {}", a, b)),
    ]
}

/// Generate a variable assignment
fn assignment_strategy() -> impl Strategy<Value = String> {
    (var_name_strategy(), safe_value_strategy())
        .prop_map(|(name, value)| format!("{}={}", name, value))
}

/// Generate an arithmetic expression
fn arithmetic_strategy() -> impl Strategy<Value = String> {
    let num = 0i32..100;
    prop_oneof![
        // Simple number
        num.clone().prop_map(|n| format!("echo $(({}))", n)),
        // Addition
        (num.clone(), num.clone()).prop_map(|(a, b)| format!("echo $(({} + {}))", a, b)),
        // Subtraction
        (num.clone(), num.clone()).prop_map(|(a, b)| format!("echo $(({} - {}))", a, b)),
        // Multiplication
        (num.clone(), num.clone()).prop_map(|(a, b)| format!("echo $(({} * {}))", a, b)),
        // Division (avoid div by zero)
        (num.clone(), 1i32..100).prop_map(|(a, b)| format!("echo $(({} / {}))", a, b)),
        // Modulo (avoid mod by zero)
        (num.clone(), 1i32..100).prop_map(|(a, b)| format!("echo $(({} % {}))", a, b)),
    ]
}

/// Generate an if statement
fn if_statement_strategy() -> impl Strategy<Value = String> {
    let condition = prop_oneof![
        Just("true".to_string()),
        Just("false".to_string()),
        (0i32..10, 0i32..10).prop_map(|(a, b)| format!("[ {} -eq {} ]", a, b)),
        (0i32..10, 0i32..10).prop_map(|(a, b)| format!("[ {} -lt {} ]", a, b)),
        (0i32..10, 0i32..10).prop_map(|(a, b)| format!("[ {} -gt {} ]", a, b)),
    ];

    (condition, safe_value_strategy(), safe_value_strategy()).prop_map(
        |(cond, then_val, else_val)| {
            format!(
                "if {}; then echo {}; else echo {}; fi",
                cond, then_val, else_val
            )
        },
    )
}

/// Generate a for loop
fn for_loop_strategy() -> impl Strategy<Value = String> {
    let items = proptest::collection::vec(safe_value_strategy(), 1..5);
    (var_name_strategy(), items).prop_map(|(var, items)| {
        format!(
            "for {} in {}; do echo ${}; done",
            var.to_lowercase(),
            items.join(" "),
            var.to_lowercase()
        )
    })
}

/// Generate a while loop (with limited iterations)
fn while_loop_strategy() -> impl Strategy<Value = String> {
    (1i32..5, safe_value_strategy()).prop_map(|(count, val)| {
        format!(
            "i=0; while [ $i -lt {} ]; do echo {}; i=$((i + 1)); done",
            count, val
        )
    })
}

/// Generate a case statement
fn case_statement_strategy() -> impl Strategy<Value = String> {
    (
        safe_value_strategy(),
        safe_value_strategy(),
        safe_value_strategy(),
    )
        .prop_map(|(input, pattern, result)| {
            format!(
                "case {} in {}) echo {};; *) echo default;; esac",
                input, pattern, result
            )
        })
}

/// Generate a pipeline
fn pipeline_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // echo | cat
        safe_value_strategy().prop_map(|v| format!("echo {} | cat", v)),
        // echo | head (skip wc - has known formatting differences)
        safe_value_strategy().prop_map(|v| format!("echo {} | head -n 1", v)),
        // printf | cat
        safe_value_strategy().prop_map(|v| format!("printf '{}\\n' | cat", v)),
        // Multiple pipes
        safe_value_strategy().prop_map(|v| format!("echo {} | cat | cat", v)),
    ]
}

/// Generate a command substitution
fn command_subst_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        safe_value_strategy().prop_map(|v| format!("echo $(echo {})", v)),
        Just("echo $(pwd)".to_string()),
        Just("X=$(echo hello); echo $X".to_string()),
    ]
}

/// Generate logical operators
fn logical_ops_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // && (and)
        (safe_value_strategy(), safe_value_strategy())
            .prop_map(|(a, b)| format!("echo {} && echo {}", a, b)),
        // || (or)
        safe_value_strategy().prop_map(|v| format!("false || echo {}", v)),
        // Combined
        (safe_value_strategy(), safe_value_strategy())
            .prop_map(|(a, b)| format!("true && echo {} || echo {}", a, b)),
    ]
}

/// Bash reserved words that cannot be used as function names
const BASH_RESERVED_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "case", "esac", "for", "select", "while", "until", "do",
    "done", "in", "function", "time", "coproc",
];

/// Generate a function definition and call
fn function_strategy() -> impl Strategy<Value = String> {
    (var_name_strategy(), safe_value_strategy()).prop_filter_map("reserved word", |(name, body)| {
        let lower = name.to_lowercase();
        if BASH_RESERVED_WORDS.contains(&lower.as_str()) {
            None
        } else {
            Some(format!("{}() {{ echo {}; }}; {}", lower, body, lower))
        }
    })
}

/// Generate a complete valid bash script
fn valid_script_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        10 => echo_command_strategy(),
        5 => assignment_strategy().prop_map(|a| format!("{}; echo done", a)),
        5 => arithmetic_strategy(),
        3 => if_statement_strategy(),
        3 => for_loop_strategy(),
        2 => while_loop_strategy(),
        2 => case_statement_strategy(),
        3 => pipeline_strategy(),
        3 => command_subst_strategy(),
        3 => logical_ops_strategy(),
        2 => function_strategy(),
    ]
}

/// Generate multi-statement scripts
fn multi_statement_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(valid_script_strategy(), 1..4).prop_map(|scripts| scripts.join("; "))
}

// === Property tests ===

proptest! {
    // Default to 50 cases, can override with PROPTEST_CASES env var
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Echo commands should produce identical output
    #[test]
    fn echo_matches_bash(script in echo_command_strategy()) {
        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}\nBashKit: {}\nBash: {}",
            script, bashkit_exit, bash_exit
        );
    }

    /// Arithmetic expressions should produce identical output
    #[test]
    fn arithmetic_matches_bash(script in arithmetic_strategy()) {
        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }

    /// Control flow should produce identical output
    #[test]
    fn control_flow_matches_bash(script in prop_oneof![
        if_statement_strategy(),
        for_loop_strategy(),
        while_loop_strategy(),
        case_statement_strategy(),
    ]) {
        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }

    /// Pipelines should produce identical output
    #[test]
    fn pipelines_match_bash(script in pipeline_strategy()) {
        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }

    /// Logical operators should produce identical output
    #[test]
    fn logical_ops_match_bash(script in logical_ops_strategy()) {
        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }

    /// Command substitutions should produce identical output
    #[test]
    fn command_subst_matches_bash(script in command_subst_strategy()) {
        // Skip pwd-related tests as paths differ between BashKit VFS and real fs
        prop_assume!(!script.contains("pwd"));

        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }

    /// Functions should produce identical output
    #[test]
    fn functions_match_bash(script in function_strategy()) {
        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }

    /// Valid scripts should produce identical output
    #[test]
    fn valid_scripts_match_bash(script in valid_script_strategy()) {
        // Skip tests with pwd as paths differ
        prop_assume!(!script.contains("pwd"));

        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }

    /// Multi-statement scripts should produce identical output
    #[test]
    fn multi_statement_matches_bash(script in multi_statement_strategy()) {
        // Skip tests with pwd as paths differ
        prop_assume!(!script.contains("pwd"));

        let (bash_out, bash_exit) = run_real_bash(&script);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (bashkit_out, bashkit_exit) = rt.block_on(run_bashkit(&script));

        prop_assert_eq!(
            &bashkit_out, &bash_out,
            "Output mismatch for script: {}\nBashKit: {:?}\nBash: {:?}",
            script, bashkit_out, bash_out
        );
        prop_assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for script: {}",
            script
        );
    }
}

// === Targeted differential tests ===

/// Test specific edge cases that have caused issues
#[tokio::test]
async fn differential_edge_cases() {
    let test_cases = [
        // Basic echo variations
        ("echo", "echo"),
        ("echo hello", "echo hello"),
        ("echo hello world", "echo hello world"),
        ("echo -n hello", "echo -n hello"),
        // Quoting
        ("echo \"hello world\"", "echo \"hello world\""),
        ("echo 'hello world'", "echo 'hello world'"),
        // Variables
        ("X=hello; echo $X", "X=hello; echo $X"),
        (
            "X=hello; Y=world; echo $X $Y",
            "X=hello; Y=world; echo $X $Y",
        ),
        // Arithmetic
        ("echo $((1 + 2))", "echo $((1 + 2))"),
        ("echo $((10 - 3))", "echo $((10 - 3))"),
        ("echo $((4 * 5))", "echo $((4 * 5))"),
        ("echo $((10 / 2))", "echo $((10 / 2))"),
        // Control flow
        ("if true; then echo yes; fi", "if true; then echo yes; fi"),
        (
            "if false; then echo yes; else echo no; fi",
            "if false; then echo yes; else echo no; fi",
        ),
        (
            "for i in a b c; do echo $i; done",
            "for i in a b c; do echo $i; done",
        ),
        // Logical operators
        ("true && echo yes", "true && echo yes"),
        ("false || echo no", "false || echo no"),
        // Pipelines
        ("echo hello | cat", "echo hello | cat"),
        // Command substitution
        ("echo $(echo hello)", "echo $(echo hello)"),
    ];

    for (name, script) in test_cases {
        let (bash_out, bash_exit) = run_real_bash(script);
        let (bashkit_out, bashkit_exit) = run_bashkit(script).await;

        assert_eq!(
            bashkit_out, bash_out,
            "Output mismatch for '{}'\nScript: {}\nBashKit: {:?}\nBash: {:?}",
            name, script, bashkit_out, bash_out
        );
        assert_eq!(
            bashkit_exit, bash_exit,
            "Exit code mismatch for '{}'\nScript: {}",
            name, script
        );
    }
}
