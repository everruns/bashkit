//! Spec test integration - runs all .test.sh files against Bashkit
//!
//! Run with: cargo test --test spec_tests
//!
//! Test files are in tests/spec_cases/{bash,awk,grep,sed,jq}/
//!
//! ## Test Directives
//! - `### skip: reason` - Skip test entirely (not run in any test)
//! - `### bash_diff: reason` - Known difference from real bash (runs in spec tests, excluded from comparison)
//!
//! ## Skipped Tests (15 total)
//!
//! Actual `### skip:` markers across spec test files:
//!
//! ### date.test.sh (2 skipped)
//! - [ ] date -s (set time) not implemented and requires privileges
//! - [ ] timezone abbreviation format varies
//!
//! ### hextools.test.sh (3 skipped)
//! - [ ] xxd output format varies across platforms
//! - [ ] od output format varies
//! - [ ] hexdump -C output format varies
//!
//! ### jq.test.sh (5 skipped)
//! - [ ] jaq errors on .foo applied to null instead of returning null for //
//! - [ ] setpath not available in jaq standard library
//! - [ ] leaf_paths not available in jaq standard library
//! - [ ] jaq omits capture name field (real jq includes "name":null)
//! - [ ] jaq scan requires explicit "g" flag for global match
//!
//! ### python.test.sh (8 skipped)
//! - [ ] Monty does not support set & and | operators yet
//! - [ ] Monty does not have filter() builtin yet
//! - [ ] Monty does not support dict comprehension with enumerate yet
//! - [ ] Monty does not support str.format() method yet
//! - [ ] Monty does not support sorted() key parameter yet
//! - [ ] Monty does not support chain assignment (a = b = c = 0) yet
//! - [ ] Monty dict literal in bash quoting needs single-quote support
//! - [ ] export propagation to ctx.env may not work in spec test runner

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

/// Run all python spec tests (requires python feature)
#[cfg(feature = "python")]
#[tokio::test]
async fn python_spec_tests() {
    use bashkit::Bash;
    use spec_runner::run_spec_test_with;

    let dir = spec_cases_dir().join("python");
    let all_tests = load_spec_tests(&dir);

    if all_tests.is_empty() {
        println!("No python spec tests found in {:?}", dir);
        return;
    }

    // Python tests need the python builtin registered via builder
    let make_bash = || Bash::builder().python().build();

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

            let result = run_spec_test_with(test, make_bash).await;
            summary.add(&result, false);

            if !result.passed {
                failures.push((file.clone(), result));
            }
        }
    }

    println!("\n=== PYTHON Spec Tests ===");
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
        "{} python tests failed",
        failures.len()
    );
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

/// Comparison test - runs against real bash in CI
/// This test compares Bashkit output against real bash for all non-skipped tests.
/// It fails if any mismatch is found, ensuring Bashkit stays compatible with bash.
#[tokio::test]
async fn bash_comparison_tests() {
    let dir = spec_cases_dir().join("bash");
    let all_tests = load_spec_tests(&dir);

    println!("\n=== Bash Comparison Tests ===");
    println!("Comparing Bashkit output against real bash\n");

    let mut total = 0;
    let mut matched = 0;
    let mut mismatches = Vec::new();

    for (file, tests) in &all_tests {
        for test in tests {
            // Skip tests marked as skip or bash_diff (known differences)
            if test.skip || test.bash_diff {
                continue;
            }

            total += 1;
            let result = run_spec_test_with_comparison(test).await;

            let real_stdout = result.real_bash_stdout.as_deref().unwrap_or("");
            let real_exit = result.real_bash_exit_code.unwrap_or(-1);

            let stdout_matches = result.bashkit_stdout == real_stdout;
            let exit_matches = result.bashkit_exit_code == real_exit;

            if stdout_matches && exit_matches {
                matched += 1;
            } else {
                mismatches.push((file.clone(), test.name.clone(), result));
            }
        }
    }

    // Print summary
    println!(
        "Comparison: {}/{} tests match real bash ({:.1}%)",
        matched,
        total,
        if total > 0 {
            (matched as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    );

    if !mismatches.is_empty() {
        println!(
            "\n=== Mismatches with real bash ({}) ===\n",
            mismatches.len()
        );
        for (file, name, result) in &mismatches {
            println!("[{}] {}", file, name);
            println!("  Bashkit stdout: {:?}", result.bashkit_stdout);
            println!(
                "  Real bash stdout: {:?}",
                result.real_bash_stdout.as_deref().unwrap_or("")
            );
            println!("  Bashkit exit: {}", result.bashkit_exit_code);
            println!(
                "  Real bash exit: {}",
                result.real_bash_exit_code.unwrap_or(-1)
            );
            println!();
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} tests have mismatches with real bash. Bashkit must produce identical output.",
        mismatches.len()
    );
}
