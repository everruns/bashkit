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
//! ## Skipped Tests TODO (87 total)
//!
//! The following tests are skipped and need fixes:
//!
//! ### date.test.sh (30 skipped) - grep output expectations wrong
//! - [ ] date_*_format tests - test expects empty but grep outputs matches
//! - [ ] date_rfc_format, date_nanoseconds, date_set_time - flags not implemented
//!
//! ### cuttr.test.sh (25 skipped) - cut/tr issues
//! - [ ] tr_* (5) - tr output missing trailing newline
//! - [ ] cut_char_* (5) - cut -c character mode not implemented
//! - [ ] cut_field_to_end - cut field-to-end range not fully implemented
//! - [ ] tr_squeeze, tr_complement - tr -s/-c not implemented
//! - [ ] tr_class_* (4) - tr character class handling differs
//! - [ ] tr_escape_* (2) - tr escape sequence handling differs
//! - [ ] tr_multiple_chars - tr output missing trailing newline
//! - [ ] cut_complement, cut_output_delimiter - not implemented
//! - [ ] tr_truncate_set2 - tr truncation behavior differs
//! - [ ] cut_only_delimited, cut_zero_terminated - not implemented
//!
//! ### sortuniq.test.sh (11 skipped) - sort/uniq flags
//! - [x] sort -f - case insensitive sort implemented
//! - [ ] sort -t, -k, -s, -c, -m, -h, -M, -o, -z - not implemented
//! - [x] uniq -d, -u - already implemented, tests unskipped
//! - [ ] uniq -i, -f - not implemented
//!
//! ### echo.test.sh (4 skipped)
//! - [x] echo_combined_en, echo_combined_ne - combined flag handling fixed
//! - [x] echo_E_flag - -E flag now works
//! - [x] echo_escape_hex, echo_escape_octal - hex/octal escapes work
//! - [ ] echo_empty, echo_no_newline - test format expects empty/newline mismatch
//! - [ ] echo_escape_r - carriage return handling differs
//! - [ ] echo_double_dash - -- to end options not implemented
//!
//! ### fileops.test.sh (5 skipped) - filesystem visibility
//! - [ ] mkdir_*, touch_*, mv_file - test conditionals not seeing fs changes
//!
//! ### wc.test.sh (0 skipped)
//! - [x] wc_chars_m_flag, wc_bytes_vs_chars - wc -m implemented
//! - [x] wc_max_line_length - wc -L implemented
//! - [x] wc_long_bytes - wc --bytes implemented
//! - [x] wc_unicode_chars - unicode character counting implemented
//!
//! ### sleep.test.sh (3 skipped)
//! - [ ] sleep_stderr_* - stderr redirect not implemented
//!
//! ### globs.test.sh (3 skipped)
//! - [ ] glob_bracket - bracket glob not fully implemented
//! - [ ] glob_recursive - recursive glob (**) not implemented
//! - [ ] brace_expansion - brace expansion not implemented
//!
//! ### timeout.test.sh (2 skipped)
//! - [ ] timeout_* - timing-dependent tests, verified manually
//!
//! ### pipes-redirects.test.sh (2 skipped)
//! - [ ] redirect_stderr - stderr redirect not fully implemented
//! - [ ] redirect_combined - combined redirects not implemented
//!
//! ### headtail.test.sh (2 skipped)
//! - [ ] head_default, tail_default - default line count not working with stdin
//!
//! ### path.test.sh (2 skipped)
//! - [ ] basename_no_args, dirname_no_args - error handling not implemented
//!
//! ### command-subst.test.sh (1 skipped)
//! - [x] subst_exit_code - exit code propagation implemented
//! - [ ] subst_backtick - backtick substitution not implemented
//!
//! ### arrays.test.sh (1 skipped)
//! - [ ] array_indices - ${!arr[@]} array indices expansion not implemented
//! - [x] array_slice - array slicing now implemented
//!
//! ### herestring.test.sh (0 skipped)
//! - [x] herestring_empty - test rewritten to verify newline behavior
//!
//! ### arithmetic.test.sh (0 skipped)
//! - [x] arith_assign - assignment inside $(()) implemented
//!
//! ### control-flow.test.sh (enabled)
//! - [x] Control flow tests enabled (31 tests passing)

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
