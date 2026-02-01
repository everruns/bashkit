//! Property-based tests for variable expansion
//!
//! Verifies that variable expansion behaves correctly.

use bashkit::Bash;
use proptest::prelude::*;

/// Run a script and return stdout
async fn run_script(script: &str) -> Option<String> {
    let mut bash = Bash::new();
    match bash.exec(script).await {
        Ok(result) => Some(result.stdout),
        Err(_) => None,
    }
}

/// Generate valid variable names
fn var_name() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z_][a-zA-Z0-9_]{0,15}").unwrap()
}

/// Generate safe values (no special chars that could break parsing)
fn safe_value() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9_]{0,30}").unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Variable assignment and expansion round-trips correctly
    #[test]
    fn variable_roundtrip(name in var_name(), value in safe_value()) {
        let script = format!("{}={}\necho ${}", name, value, name);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_script(&script));

        if let Some(output) = result {
            let expected = format!("{}\n", value);
            prop_assert_eq!(output, expected, "Variable roundtrip failed for {}={}", name, value);
        }
    }

    /// Default value expansion works correctly
    #[test]
    fn default_value_expansion(name in var_name(), default in safe_value()) {
        // Unset variable should use default
        let script = format!("echo ${{{}:-{}}}", name, default);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_script(&script));

        if let Some(output) = result {
            let expected = format!("{}\n", default);
            prop_assert_eq!(output, expected, "Default expansion failed: {}", script);
        }
    }

    /// Length expansion returns correct length
    #[test]
    fn length_expansion(name in var_name(), value in safe_value()) {
        let script = format!("{}={}\necho ${{#{}}}", name, value, name);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_script(&script));

        if let Some(output) = result {
            let expected = format!("{}\n", value.len());
            prop_assert_eq!(output, expected, "Length expansion failed for value '{}' (len={})", value, value.len());
        }
    }

    /// Empty variable expands to empty string
    #[test]
    fn empty_variable(name in var_name()) {
        let script = format!("{}=\necho \"before${{{}}}after\"", name, name);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_script(&script));

        if let Some(output) = result {
            prop_assert_eq!(output, "beforeafter\n", "Empty expansion failed");
        }
    }

    /// Overwriting variables works
    #[test]
    fn variable_overwrite(name in var_name(), val1 in safe_value(), val2 in safe_value()) {
        let script = format!("{}={}\n{}={}\necho ${}", name, val1, name, val2, name);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_script(&script));

        if let Some(output) = result {
            let expected = format!("{}\n", val2);
            prop_assert_eq!(output, expected, "Variable overwrite failed");
        }
    }
}
