//! Spec test integration - runs all .test.sh files against BashKit
//!
//! Run with: cargo test --test spec_tests
//! Run with comparison: cargo test --test spec_tests -- --include-ignored
//!
//! Test files are in tests/spec_cases/{bash,awk,grep,sed,jq}/

mod spec_runner;

use spec_runner::{load_spec_tests, run_spec_test, run_spec_test_with_comparison, TestSummary};
use std::path::PathBuf;

fn spec_cases_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/spec_cases")
}

/// Run all bash spec tests
#[tokio::test]
async fn bash_spec_tests() {
    let dir = spec_cases_dir().join("bash");
    let all_tests = load_spec_tests(&dir);

    if all_tests.is_empty() {
        println!("No bash spec tests found in {:?}", dir);
        return;
    }

    let mut summary = TestSummary::default();
    let mut failures = Vec::new();

    for (file, tests) in &all_tests {
        for test in tests {
            if test.skip {
                summary.add(
                    &spec_runner::TestResult {
                        name: test.name.clone(),
                        passed: false,
                        bashkit_stdout: String::new(),
                        bashkit_exit_code: 0,
                        expected_stdout: String::new(),
                        expected_exit_code: None,
                        real_bash_stdout: None,
                        real_bash_exit_code: None,
                        error: None,
                    },
                    true,
                );
                continue;
            }

            let result = run_spec_test(test).await;
            summary.add(&result, false);

            if !result.passed {
                failures.push((file.clone(), result));
            }
        }
    }

    // Print summary
    println!("\n=== Bash Spec Tests ===");
    println!(
        "Total: {} | Passed: {} | Failed: {} | Skipped: {}",
        summary.total, summary.passed, summary.failed, summary.skipped
    );
    println!("Pass rate: {:.1}%", summary.pass_rate());

    // Print failures
    if !failures.is_empty() {
        println!("\n=== Failures ===");
        for (file, result) in &failures {
            println!("\n[{}] {}", file, result.name);
            if let Some(ref err) = result.error {
                println!("  Error: {}", err);
            }
            println!("  Expected stdout: {:?}", result.expected_stdout);
            println!("  Got stdout:      {:?}", result.bashkit_stdout);
            if let Some(expected) = result.expected_exit_code {
                println!(
                    "  Expected exit:   {} | Got: {}",
                    expected, result.bashkit_exit_code
                );
            }
        }
    }

    assert!(failures.is_empty(), "{} spec tests failed", failures.len());
}

/// Run all awk spec tests
#[tokio::test]
async fn awk_spec_tests() {
    let dir = spec_cases_dir().join("awk");
    let all_tests = load_spec_tests(&dir);

    if all_tests.is_empty() {
        return;
    }

    run_category_tests("awk", all_tests).await;
}

/// Run all grep spec tests
#[tokio::test]
async fn grep_spec_tests() {
    let dir = spec_cases_dir().join("grep");
    let all_tests = load_spec_tests(&dir);

    if all_tests.is_empty() {
        return;
    }

    run_category_tests("grep", all_tests).await;
}

/// Run all sed spec tests
#[tokio::test]
async fn sed_spec_tests() {
    let dir = spec_cases_dir().join("sed");
    let all_tests = load_spec_tests(&dir);

    if all_tests.is_empty() {
        return;
    }

    run_category_tests("sed", all_tests).await;
}

/// Run all jq spec tests
#[tokio::test]
async fn jq_spec_tests() {
    let dir = spec_cases_dir().join("jq");
    let all_tests = load_spec_tests(&dir);

    if all_tests.is_empty() {
        return;
    }

    run_category_tests("jq", all_tests).await;
}

async fn run_category_tests(
    name: &str,
    all_tests: std::collections::HashMap<String, Vec<spec_runner::SpecTest>>,
) {
    let mut summary = TestSummary::default();
    let mut failures = Vec::new();

    for (file, tests) in &all_tests {
        for test in tests {
            if test.skip {
                summary.add(
                    &spec_runner::TestResult {
                        name: test.name.clone(),
                        passed: false,
                        bashkit_stdout: String::new(),
                        bashkit_exit_code: 0,
                        expected_stdout: String::new(),
                        expected_exit_code: None,
                        real_bash_stdout: None,
                        real_bash_exit_code: None,
                        error: None,
                    },
                    true,
                );
                continue;
            }

            let result = run_spec_test(test).await;
            summary.add(&result, false);

            if !result.passed {
                failures.push((file.clone(), result));
            }
        }
    }

    println!("\n=== {} Spec Tests ===", name.to_uppercase());
    println!(
        "Total: {} | Passed: {} | Failed: {} | Skipped: {}",
        summary.total, summary.passed, summary.failed, summary.skipped
    );

    if !failures.is_empty() {
        println!("\n=== Failures ===");
        for (file, result) in &failures {
            println!("\n[{}] {}", file, result.name);
            if let Some(ref err) = result.error {
                println!("  Error: {}", err);
            }
            println!("  Expected: {:?}", result.expected_stdout);
            println!("  Got:      {:?}", result.bashkit_stdout);
        }
    }

    assert!(
        failures.is_empty(),
        "{} {} tests failed",
        failures.len(),
        name
    );
}

/// Run external test suites (imported from bash/shellcheck)
#[tokio::test]
async fn external_spec_tests() {
    let dir = spec_cases_dir().join("external");
    let all_tests = load_spec_tests(&dir);

    if all_tests.is_empty() {
        println!("No external spec tests found in {:?}", dir);
        return;
    }

    // External tests are informational - we expect some failures
    // since these test features we haven't implemented yet
    let mut summary = TestSummary::default();
    let mut failures = Vec::new();

    for (file, tests) in &all_tests {
        for test in tests {
            if test.skip {
                summary.add(
                    &spec_runner::TestResult {
                        name: test.name.clone(),
                        passed: false,
                        bashkit_stdout: String::new(),
                        bashkit_exit_code: 0,
                        expected_stdout: String::new(),
                        expected_exit_code: None,
                        real_bash_stdout: None,
                        real_bash_exit_code: None,
                        error: None,
                    },
                    true,
                );
                continue;
            }

            let result = run_spec_test(test).await;
            summary.add(&result, false);

            if !result.passed {
                failures.push((file.clone(), result));
            }
        }
    }

    println!("\n=== External Spec Tests ===");
    println!(
        "Total: {} | Passed: {} | Failed: {} | Skipped: {}",
        summary.total, summary.passed, summary.failed, summary.skipped
    );
    println!("Pass rate: {:.1}%", summary.pass_rate());

    // Don't fail the test - external tests are informational
    if !failures.is_empty() {
        println!("\n=== Non-passing tests (expected for unimplemented features) ===");
        for (file, result) in &failures[..failures.len().min(5)] {
            println!("  [{}] {}", file, result.name);
        }
        if failures.len() > 5 {
            println!("  ... and {} more", failures.len() - 5);
        }
    }
}

/// Comparison test - runs against real bash to find divergences
#[tokio::test]
async fn bash_comparison_tests() {
    let dir = spec_cases_dir().join("bash");
    let all_tests = load_spec_tests(&dir);

    println!("\n=== Bash Comparison Tests ===");
    println!("Comparing BashKit output against real bash\n");

    let mut mismatches = Vec::new();

    for (file, tests) in &all_tests {
        for test in tests {
            if test.skip {
                continue;
            }

            let result = run_spec_test_with_comparison(test).await;

            let real_stdout = result.real_bash_stdout.as_deref().unwrap_or("");
            let real_exit = result.real_bash_exit_code.unwrap_or(-1);

            let stdout_matches = result.bashkit_stdout == real_stdout;
            let exit_matches = result.bashkit_exit_code == real_exit;

            if !stdout_matches || !exit_matches {
                mismatches.push((file.clone(), test.name.clone(), result));
            }
        }
    }

    if !mismatches.is_empty() {
        println!("=== Mismatches with real bash ===\n");
        for (file, name, result) in &mismatches {
            println!("[{}] {}", file, name);
            println!("  BashKit stdout: {:?}", result.bashkit_stdout);
            println!(
                "  Real bash stdout: {:?}",
                result.real_bash_stdout.as_deref().unwrap_or("")
            );
            println!("  BashKit exit: {}", result.bashkit_exit_code);
            println!(
                "  Real bash exit: {}",
                result.real_bash_exit_code.unwrap_or(-1)
            );
            println!();
        }
    }

    println!("Comparison complete: {} mismatches found", mismatches.len());
}
