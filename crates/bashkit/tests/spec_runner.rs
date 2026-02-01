//! Spec test runner for BashKit compatibility testing
//!
//! Test file format (.test.sh):
//! ```
//! ### test_name
//! # Description of what this tests
//! echo hello
//! ### expect
//! hello
//! ### end
//!
//! ### error_test
//! ### expect_error
//! invalid syntax (((
//! ### expect
//! ### end
//! ```
//!
//! Directives:
//! - `### test_name` - Start a new test
//! - `### expect` - Expected stdout follows
//! - `### end` - End of test case
//! - `### exit_code: N` - Expected exit code
//! - `### skip: reason` - Skip this test
//! - `### expect_error` - Expect parse/execution error (test passes if error occurs)
//! - `### timeout: N` - Custom timeout in seconds (default: 5)
//!
//! Multiple tests per file supported. Tests are run against BashKit
//! and optionally compared against real bash.

use bashkit::Bash;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

/// Default timeout for spec tests in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 5;

/// A single test case parsed from a .test.sh file
#[derive(Debug, Clone)]
pub struct SpecTest {
    pub name: String,
    pub description: String,
    pub script: String,
    pub expected_stdout: String,
    pub expected_exit_code: Option<i32>,
    pub skip: bool,
    pub skip_reason: Option<String>,
    /// If true, test passes when an error occurs (for testing error handling)
    pub expect_error: bool,
    /// Timeout in seconds (default: 5)
    pub timeout_secs: u64,
}

/// Result of running a spec test
#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub bashkit_stdout: String,
    pub bashkit_exit_code: i32,
    pub expected_stdout: String,
    pub expected_exit_code: Option<i32>,
    pub real_bash_stdout: Option<String>,
    pub real_bash_exit_code: Option<i32>,
    pub error: Option<String>,
}

/// Parse test cases from a .test.sh file
pub fn parse_spec_file(content: &str) -> Vec<SpecTest> {
    let mut tests = Vec::new();
    let mut current_test: Option<SpecTest> = None;
    let mut in_script = false;
    let mut in_expect = false;
    let mut script_lines = Vec::new();
    let mut expect_lines = Vec::new();

    for line in content.lines() {
        if let Some(directive) = line.strip_prefix("### ") {
            let directive = directive.trim();

            if directive == "expect" {
                in_script = false;
                in_expect = true;
            } else if directive == "end" {
                // Finalize current test
                if let Some(mut test) = current_test.take() {
                    test.script = script_lines.join("\n");
                    test.expected_stdout = expect_lines.join("\n");
                    if !test.expected_stdout.is_empty() {
                        test.expected_stdout.push('\n');
                    }
                    tests.push(test);
                }
                script_lines.clear();
                expect_lines.clear();
                in_script = false;
                in_expect = false;
            } else if let Some(code_str) = directive.strip_prefix("exit_code:") {
                if let Some(ref mut test) = current_test {
                    if let Ok(code) = code_str.trim().parse() {
                        test.expected_exit_code = Some(code);
                    }
                }
            } else if let Some(reason) = directive.strip_prefix("skip:") {
                if let Some(ref mut test) = current_test {
                    test.skip = true;
                    test.skip_reason = Some(reason.trim().to_string());
                }
            } else if directive == "skip" {
                if let Some(ref mut test) = current_test {
                    test.skip = true;
                }
            } else if directive == "expect_error" {
                if let Some(ref mut test) = current_test {
                    test.expect_error = true;
                }
            } else if let Some(secs_str) = directive.strip_prefix("timeout:") {
                if let Some(ref mut test) = current_test {
                    if let Ok(secs) = secs_str.trim().parse() {
                        test.timeout_secs = secs;
                    }
                }
            } else {
                // New test name
                if let Some(mut test) = current_test.take() {
                    test.script = script_lines.join("\n");
                    test.expected_stdout = expect_lines.join("\n");
                    if !test.expected_stdout.is_empty() {
                        test.expected_stdout.push('\n');
                    }
                    tests.push(test);
                }
                script_lines.clear();
                expect_lines.clear();

                current_test = Some(SpecTest {
                    name: directive.to_string(),
                    description: String::new(),
                    script: String::new(),
                    expected_stdout: String::new(),
                    expected_exit_code: None,
                    skip: false,
                    skip_reason: None,
                    expect_error: false,
                    timeout_secs: DEFAULT_TIMEOUT_SECS,
                });
                in_script = true;
                in_expect = false;
            }
        } else if let Some(comment) = line.strip_prefix("# ") {
            if in_script && script_lines.is_empty() {
                // Description comment at start of script
                if let Some(ref mut test) = current_test {
                    if test.description.is_empty() {
                        test.description = comment.to_string();
                    } else {
                        script_lines.push(line.to_string());
                    }
                }
            } else if in_script {
                script_lines.push(line.to_string());
            }
        } else if in_script {
            script_lines.push(line.to_string());
        } else if in_expect {
            expect_lines.push(line.to_string());
        }
    }

    // Handle case where file doesn't end with ### end
    if let Some(mut test) = current_test.take() {
        test.script = script_lines.join("\n");
        test.expected_stdout = expect_lines.join("\n");
        if !test.expected_stdout.is_empty() && !test.expected_stdout.ends_with('\n') {
            test.expected_stdout.push('\n');
        }
        tests.push(test);
    }

    tests
}

/// Run a single spec test against BashKit with timeout
pub async fn run_spec_test(test: &SpecTest) -> TestResult {
    let mut bash = Bash::new();
    let timeout_duration = Duration::from_secs(test.timeout_secs);

    let exec_result = timeout(timeout_duration, bash.exec(&test.script)).await;

    let (bashkit_stdout, bashkit_exit_code, error) = match exec_result {
        Ok(Ok(result)) => (result.stdout, result.exit_code, None),
        Ok(Err(e)) => (String::new(), 1, Some(e.to_string())),
        Err(_) => (
            String::new(),
            1,
            Some(format!("Test timed out after {}s", test.timeout_secs)),
        ),
    };

    // Determine if test passed
    let passed = if test.expect_error {
        // For expect_error tests, we pass if there was an error
        error.is_some()
    } else {
        // Normal test: check stdout and exit code match
        let stdout_matches = bashkit_stdout == test.expected_stdout;
        let exit_code_matches = test
            .expected_exit_code
            .map(|expected| bashkit_exit_code == expected)
            .unwrap_or(true);
        stdout_matches && exit_code_matches && error.is_none()
    };

    TestResult {
        name: test.name.clone(),
        passed,
        bashkit_stdout,
        bashkit_exit_code,
        expected_stdout: test.expected_stdout.clone(),
        expected_exit_code: test.expected_exit_code,
        real_bash_stdout: None,
        real_bash_exit_code: None,
        error,
    }
}

/// Run a spec test against real bash for comparison
pub fn run_real_bash(script: &str) -> (String, i32) {
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("Failed to run bash");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code().unwrap_or(1);

    (stdout, exit_code)
}

/// Run spec test with real bash comparison
pub async fn run_spec_test_with_comparison(test: &SpecTest) -> TestResult {
    let mut result = run_spec_test(test).await;

    let (real_stdout, real_exit_code) = run_real_bash(&test.script);
    result.real_bash_stdout = Some(real_stdout);
    result.real_bash_exit_code = Some(real_exit_code);

    result
}

/// Load all spec tests from a directory
pub fn load_spec_tests(dir: &Path) -> HashMap<String, Vec<SpecTest>> {
    let mut all_tests = HashMap::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "sh") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let file_name = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let tests = parse_spec_file(&content);
                    if !tests.is_empty() {
                        all_tests.insert(file_name, tests);
                    }
                }
            }
        }
    }

    all_tests
}

/// Summary statistics for a test run
#[derive(Debug, Default)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl TestSummary {
    pub fn add(&mut self, result: &TestResult, was_skipped: bool) {
        self.total += 1;
        if was_skipped {
            self.skipped += 1;
        } else if result.passed {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
    }

    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / (self.total - self.skipped) as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_spec_file() {
        let content = r#"
### simple_echo
# Test basic echo
echo hello
### expect
hello
### end

### multi_line
echo one
echo two
### expect
one
two
### end
"#;

        let tests = parse_spec_file(content);
        assert_eq!(tests.len(), 2);

        assert_eq!(tests[0].name, "simple_echo");
        assert_eq!(tests[0].description, "Test basic echo");
        assert_eq!(tests[0].script, "echo hello");
        assert_eq!(tests[0].expected_stdout, "hello\n");

        assert_eq!(tests[1].name, "multi_line");
        assert_eq!(tests[1].script, "echo one\necho two");
        assert_eq!(tests[1].expected_stdout, "one\ntwo\n");
    }

    #[test]
    fn test_parse_with_exit_code() {
        let content = r#"
### exit_test
false
### exit_code: 1
### expect
### end
"#;

        let tests = parse_spec_file(content);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].expected_exit_code, Some(1));
    }

    #[test]
    fn test_parse_with_skip() {
        let content = r#"
### skipped_test
### skip: not implemented yet
echo hello
### expect
hello
### end
"#;

        let tests = parse_spec_file(content);
        assert_eq!(tests.len(), 1);
        assert!(tests[0].skip);
        assert_eq!(
            tests[0].skip_reason,
            Some("not implemented yet".to_string())
        );
    }

    #[tokio::test]
    async fn test_run_simple_spec() {
        let test = SpecTest {
            name: "echo_test".to_string(),
            description: "Test echo".to_string(),
            script: "echo hello".to_string(),
            expected_stdout: "hello\n".to_string(),
            expected_exit_code: None,
            skip: false,
            skip_reason: None,
            expect_error: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        };

        let result = run_spec_test(&test).await;
        assert!(result.passed, "Test should pass: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_failing_spec() {
        let test = SpecTest {
            name: "fail_test".to_string(),
            description: "Test that should fail".to_string(),
            script: "echo wrong".to_string(),
            expected_stdout: "right\n".to_string(),
            expected_exit_code: None,
            skip: false,
            skip_reason: None,
            expect_error: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        };

        let result = run_spec_test(&test).await;
        assert!(!result.passed, "Test should fail");
    }

    #[tokio::test]
    async fn test_expect_error() {
        let test = SpecTest {
            name: "error_test".to_string(),
            description: "Test that expects an error".to_string(),
            script: "(((".to_string(), // Invalid syntax
            expected_stdout: String::new(),
            expected_exit_code: None,
            skip: false,
            skip_reason: None,
            expect_error: true,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        };

        let result = run_spec_test(&test).await;
        assert!(
            result.passed,
            "Test should pass when error occurs: {:?}",
            result
        );
    }

    #[test]
    fn test_parse_expect_error() {
        let content = r#"
### syntax_error
### expect_error
(((
### expect
### end
"#;

        let tests = parse_spec_file(content);
        assert_eq!(tests.len(), 1);
        assert!(tests[0].expect_error);
    }
}
