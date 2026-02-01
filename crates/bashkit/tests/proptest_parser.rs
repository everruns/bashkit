//! Property-based tests for the parser
//!
//! Uses proptest to generate random inputs and verify the parser/interpreter never panics.

use bashkit::Bash;
use proptest::prelude::*;

/// Run a script and return whether it completed (didn't panic)
async fn try_exec(script: &str) -> bool {
    let mut bash = Bash::new();
    // We don't care about success/failure, just that it doesn't panic
    let _ = bash.exec(script).await;
    true
}

/// Strategies for generating bash-like input
mod strategies {
    use proptest::prelude::*;

    /// Generate arbitrary strings (may be invalid bash)
    pub fn arbitrary_string() -> impl Strategy<Value = String> {
        prop::string::string_regex(".{0,100}").unwrap()
    }

    /// Generate valid bash identifiers
    pub fn identifier() -> impl Strategy<Value = String> {
        prop::string::string_regex("[a-zA-Z_][a-zA-Z0-9_]{0,20}").unwrap()
    }

    /// Generate simple words (alphanumeric + some special chars)
    pub fn word() -> impl Strategy<Value = String> {
        prop::string::string_regex("[a-zA-Z0-9_./-]{1,30}").unwrap()
    }

    /// Generate a simple echo command
    pub fn echo_command() -> impl Strategy<Value = String> {
        word().prop_map(|w| format!("echo {}", w))
    }

    /// Generate a variable assignment
    pub fn assignment() -> impl Strategy<Value = String> {
        (identifier(), word()).prop_map(|(name, value)| format!("{}={}", name, value))
    }

    /// Generate an arithmetic expression
    pub fn arithmetic() -> impl Strategy<Value = String> {
        prop::string::string_regex("[0-9+\\-*/%() ]{1,30}").unwrap()
    }

    /// Generate a simple bash script (one or more commands)
    pub fn simple_script() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                echo_command(),
                assignment(),
                Just("true".to_string()),
                Just("false".to_string()),
            ],
            1..5,
        )
        .prop_map(|commands| commands.join("\n"))
    }
}

// Fuzz test with fewer cases (each requires a Tokio runtime, so very slow)
// Run with PROPTEST_CASES=500 for thorough testing locally
proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Parser/interpreter should never panic on arbitrary input
    #[test]
    fn never_panics_on_arbitrary_input(input in strategies::arbitrary_string()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let completed = rt.block_on(try_exec(&input));
        prop_assert!(completed, "Script execution failed to complete");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Valid identifiers work as assignments
    #[test]
    fn handles_valid_assignments(name in strategies::identifier(), value in strategies::word()) {
        let script = format!("{}={}", name, value);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let completed = rt.block_on(try_exec(&script));
        prop_assert!(completed, "Assignment failed: {}", script);
    }

    /// Echo commands work
    #[test]
    fn handles_echo_commands(word in strategies::word()) {
        let script = format!("echo {}", word);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut bash = Bash::new();
        let result = rt.block_on(bash.exec(&script));
        prop_assert!(result.is_ok(), "Echo failed: {}", script);
    }

    /// Arithmetic doesn't panic
    #[test]
    fn handles_arithmetic(expr in strategies::arithmetic()) {
        let script = format!("echo $(({}))", expr);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let completed = rt.block_on(try_exec(&script));
        prop_assert!(completed, "Arithmetic failed: {}", script);
    }

    /// Simple scripts execute successfully
    #[test]
    fn handles_simple_scripts(script in strategies::simple_script()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut bash = Bash::new();
        let result = rt.block_on(bash.exec(&script));
        prop_assert!(result.is_ok(), "Script failed:\n{}", script);
    }

    /// Variable expansions work
    #[test]
    fn handles_variable_expansion(name in strategies::identifier()) {
        // Set and then use the variable
        let script = format!("{}=test\necho ${}", name, name);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut bash = Bash::new();
        let result = rt.block_on(bash.exec(&script));
        prop_assert!(result.is_ok(), "Variable expansion failed: {}", script);
    }
}
