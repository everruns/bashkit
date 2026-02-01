//! Differential testing: compare BashKit output against real bash
//!
//! Generates random scripts and runs them against both BashKit and the system's
//! bash interpreter, logging any mismatches for analysis.
//!
//! Run with: cargo test --test differential -- --nocapture

use bashkit::Bash;
use proptest::prelude::*;
use std::process::Command;

/// Run a script in the system's bash and capture output
fn run_real_bash(script: &str) -> Option<(String, i32)> {
    let output = Command::new("bash").arg("-c").arg(script).output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code().unwrap_or(-1);
    Some((stdout, exit_code))
}

/// Run a script in BashKit and capture output
async fn run_bashkit(script: &str) -> Option<(String, i32)> {
    let mut bash = Bash::new();
    match bash.exec(script).await {
        Ok(result) => Some((result.stdout, result.exit_code)),
        Err(_) => None,
    }
}

/// Compare outputs and return mismatch info if different
fn compare_outputs(
    script: &str,
    bashkit: Option<(String, i32)>,
    real: Option<(String, i32)>,
) -> Option<String> {
    match (bashkit, real) {
        (Some((bk_out, bk_code)), Some((real_out, real_code))) => {
            // Normalize outputs (trim trailing whitespace)
            let bk_out = bk_out.trim_end();
            let real_out = real_out.trim_end();

            if bk_out != real_out || bk_code != real_code {
                Some(format!(
                    "MISMATCH:\n  Script: {:?}\n  BashKit: {:?} (exit {})\n  Real:    {:?} (exit {})",
                    script, bk_out, bk_code, real_out, real_code
                ))
            } else {
                None
            }
        }
        (None, Some(_)) => Some(format!(
            "BASHKIT_FAILED:\n  Script: {:?}\n  BashKit failed but real bash succeeded",
            script
        )),
        (Some(_), None) => Some(format!(
            "REAL_FAILED:\n  Script: {:?}\n  Real bash failed but BashKit succeeded",
            script
        )),
        (None, None) => None, // Both failed, consider it a match
    }
}

/// Strategies for generating bash scripts
mod strategies {
    use proptest::prelude::*;

    /// Generate a simple integer
    pub fn number() -> impl Strategy<Value = i64> {
        -100i64..100
    }

    /// Generate a valid bash identifier
    pub fn identifier() -> impl Strategy<Value = String> {
        prop::string::string_regex("[a-z][a-z0-9]{0,8}").unwrap()
    }

    /// Generate an echo command
    pub fn echo_command() -> impl Strategy<Value = String> {
        prop_oneof![
            // Simple echo
            prop::string::string_regex("[a-zA-Z0-9 ]{1,20}")
                .unwrap()
                .prop_map(|s| format!("echo {}", s)),
            // Echo with variable
            identifier().prop_map(|v| format!("{}=hello; echo ${}", v, v)),
            // Echo arithmetic
            (number(), number()).prop_map(|(a, b)| format!("echo $(({}+{}))", a, b)),
        ]
    }

    /// Generate a variable assignment and use
    pub fn variable_script() -> impl Strategy<Value = String> {
        (
            identifier(),
            prop::string::string_regex("[a-z0-9]{1,10}").unwrap(),
        )
            .prop_map(|(name, value)| format!("{}={}; echo ${}", name, value, name))
    }

    /// Generate an arithmetic expression
    pub fn arithmetic_script() -> impl Strategy<Value = String> {
        prop_oneof![
            // Simple binary ops
            (number(), number()).prop_map(|(a, b)| format!("echo $(({}+{}))", a, b)),
            (number(), number()).prop_map(|(a, b)| format!("echo $(({}-{}))", a, b)),
            (number(), number()).prop_map(|(a, b)| format!("echo $(({}*{}))", a, b)),
            // Division (avoid div by zero)
            (number(), 1i64..100).prop_map(|(a, b)| format!("echo $(({}/{}))", a, b)),
            // Modulo (avoid mod by zero)
            (number(), 1i64..100).prop_map(|(a, b)| format!("echo $(({}%{}))", a, b)),
            // Comparisons
            (number(), number()).prop_map(|(a, b)| format!("echo $(({}<{}))", a, b)),
            (number(), number()).prop_map(|(a, b)| format!("echo $(({}>{}))", a, b)),
            (number(), number()).prop_map(|(a, b)| format!("echo $(({}=={}))", a, b)),
        ]
    }

    /// Generate a conditional
    pub fn conditional_script() -> impl Strategy<Value = String> {
        prop_oneof![
            // Simple if
            Just("if true; then echo yes; fi".to_string()),
            Just("if false; then echo yes; else echo no; fi".to_string()),
            // Numeric test
            (number(), number()).prop_map(|(a, b)| {
                format!(
                    "if [ {} -gt {} ]; then echo greater; else echo not; fi",
                    a, b
                )
            }),
        ]
    }

    /// Generate a loop
    pub fn loop_script() -> impl Strategy<Value = String> {
        prop_oneof![
            // For loop with word list
            Just("for i in a b c; do echo $i; done".to_string()),
            // For loop with numbers
            Just("for i in 1 2 3; do echo $i; done".to_string()),
            // While loop
            Just("x=0; while [ $x -lt 3 ]; do echo $x; x=$((x+1)); done".to_string()),
        ]
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Compare echo commands
    #[test]
    fn differential_echo(script in strategies::echo_command()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bashkit_result = rt.block_on(run_bashkit(&script));
        let real_result = run_real_bash(&script);

        if let Some(mismatch) = compare_outputs(&script, bashkit_result, real_result) {
            eprintln!("{}", mismatch);
            // Don't fail the test, just log mismatches
        }
    }

    /// Compare variable scripts
    #[test]
    fn differential_variables(script in strategies::variable_script()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bashkit_result = rt.block_on(run_bashkit(&script));
        let real_result = run_real_bash(&script);

        if let Some(mismatch) = compare_outputs(&script, bashkit_result, real_result) {
            eprintln!("{}", mismatch);
        }
    }

    /// Compare arithmetic scripts
    #[test]
    fn differential_arithmetic(script in strategies::arithmetic_script()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bashkit_result = rt.block_on(run_bashkit(&script));
        let real_result = run_real_bash(&script);

        if let Some(mismatch) = compare_outputs(&script, bashkit_result, real_result) {
            eprintln!("{}", mismatch);
        }
    }

    /// Compare conditional scripts
    #[test]
    fn differential_conditionals(script in strategies::conditional_script()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bashkit_result = rt.block_on(run_bashkit(&script));
        let real_result = run_real_bash(&script);

        if let Some(mismatch) = compare_outputs(&script, bashkit_result, real_result) {
            eprintln!("{}", mismatch);
        }
    }

    /// Compare loop scripts
    #[test]
    fn differential_loops(script in strategies::loop_script()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bashkit_result = rt.block_on(run_bashkit(&script));
        let real_result = run_real_bash(&script);

        if let Some(mismatch) = compare_outputs(&script, bashkit_result, real_result) {
            eprintln!("{}", mismatch);
        }
    }
}

#[cfg(test)]
mod known_issues {
    use super::*;

    /// Track known differences between BashKit and real bash
    /// These tests document expected mismatches, not pass/fail
    #[tokio::test]
    async fn document_known_differences() {
        let test_cases = vec![
            // Add known differences here as they're discovered
            ("echo -n hello", "echo -n flag may differ"),
            ("echo -e 'a\\nb'", "escape sequences differ"),
        ];

        println!("\n=== Known Differences ===");
        for (script, reason) in test_cases {
            let bashkit = run_bashkit(script).await;
            let real = run_real_bash(script);

            if let Some(mismatch) = compare_outputs(script, bashkit, real) {
                println!("\n{}\nReason: {}", mismatch, reason);
            } else {
                println!("\nSCRIPT NOW MATCHES: {:?}", script);
            }
        }
        println!("\n=========================\n");
    }
}
