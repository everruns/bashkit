# 004: Testing Strategy

## Status
Implemented

## Decision

BashKit uses a multi-layer testing strategy:

1. **Unit tests** - Component-level tests in each module
2. **Spec tests** - Compatibility tests against bash behavior
3. **Security tests** - Threat model and failpoint tests
4. **Comparison tests** - Direct comparison with real bash
5. **Differential fuzzing** - Property-based testing against real bash

## CI Test Summary

Tests run automatically on every PR via `cargo test --features http_client`:

| Test Suite | Test Functions | Notes |
|------------|---------------|-------|
| Unit tests (bashkit lib) | 286 | Core interpreter tests |
| limits.rs | 5 | Resource limit tests |
| spec_tests.rs | 10 (1 ignored) | Spec compatibility tests |
| threat_model_tests | 39 | Security tests |
| security_failpoint_tests | 14 | Fault injection tests |
| Doc tests | 2 | Documentation examples |
| **Total** | **356** | Plus 4 examples executed |

## Spec Test Framework

### Location
```
crates/bashkit/tests/
├── spec_runner.rs      # Test parser and runner
├── spec_tests.rs       # Integration test entry point
├── debug_spec.rs       # Debugging utilities
├── threat_model_tests.rs    # Security threat model tests
├── security_failpoint_tests.rs  # Fault injection tests
├── proptest_differential.rs # Grammar-based differential fuzzing
└── spec_cases/
    ├── bash/           # Core bash compatibility (20 files, 471 cases)
    │   ├── arithmetic.test.sh
    │   ├── arrays.test.sh
    │   ├── background.test.sh
    │   ├── command-subst.test.sh
    │   ├── control-flow.test.sh
    │   ├── cuttr.test.sh
    │   ├── date.test.sh
    │   ├── echo.test.sh
    │   ├── fileops.test.sh
    │   ├── functions.test.sh
    │   ├── globs.test.sh
    │   ├── headtail.test.sh
    │   ├── herestring.test.sh
    │   ├── path.test.sh
    │   ├── pipes-redirects.test.sh
    │   ├── procsub.test.sh
    │   ├── sleep.test.sh
    │   ├── sortuniq.test.sh
    │   ├── variables.test.sh
    │   └── wc.test.sh
    ├── awk/            # AWK builtin tests (89 cases)
    ├── grep/           # Grep builtin tests (70 cases)
    ├── sed/            # Sed builtin tests (65 cases)
    └── jq/             # JQ builtin tests (95 cases)
```

### Spec Test Counts

| Category | Test Cases | In CI | Pass | Skip |
|----------|------------|-------|------|------|
| Bash | 471 | Yes | 367 | 104 |
| AWK | 89 | Yes | 48 | 41 |
| Grep | 70 | Yes | 56 | 14 |
| Sed | 65 | Yes | 50 | 15 |
| JQ | 95 | Yes | 75 | 20 |
| **Total** | **790** | **790** | 596 | 194 |

### Test File Format

```sh
### test_name
# Optional description
script_to_execute
### expect
expected_output
### end

### another_test
### skip: reason for skipping
script_that_fails
### expect
expected_output
### end

### exit_code_test
false
### exit_code: 1
### expect
### end
```

### Directives
- `### test_name` - Start a new test
- `### expect` - Expected stdout follows
- `### end` - End of test case
- `### exit_code: N` - Expected exit code (optional)
- `### skip: reason` - Skip this test with reason
- `### bash_diff: reason` - Test has known difference from real bash (still runs in spec tests, excluded from bash comparison)
- `### paused_time` - Run with tokio paused time for deterministic timing tests

## Running Tests

```bash
# All spec tests
cargo test --test spec_tests

# Single category
cargo test --test spec_tests -- bash_spec_tests

# With output
cargo test --test spec_tests -- --nocapture

# Check spec tests match real bash
just check-bash-compat

# Check spec tests match real bash (verbose - shows each test)
just check-bash-compat-verbose

# Generate comprehensive compatibility report
just compat-report

# Or directly with cargo:
cargo test --test spec_tests -- bash_comparison_tests --nocapture
cargo test --test spec_tests -- compatibility_report --ignored --nocapture
```

## Coverage

Coverage is tracked with cargo-tarpaulin and uploaded to Codecov.

```bash
# Generate local coverage report
cargo tarpaulin --features http_client --out html --output-dir coverage

# View coverage report
open coverage/tarpaulin-report.html
```

The coverage workflow runs on every PR and push to main. Reports are uploaded
to Codecov and available as CI artifacts.

### Current Status
- All spec tests: 75% pass rate (596/790 running in CI, 194 skipped)
- Text processing tools: 72% pass rate (229/319 running, 90 skipped)
- Core bash specs: 78% pass rate (367/471 running, 104 skipped)

## TODO: Testing Gaps

The following items need attention:

- [x] **Enable bash_spec_tests in CI** - Done! 330/435 tests running
- [x] **Add bash_comparison_tests to CI** - Done! 309 tests compared against real bash
- [x] **Fix control-flow.test.sh** - Enabled! 31 tests now running
- [x] **Add coverage tooling** - cargo-tarpaulin + Codecov via `.github/workflows/coverage.yml`
- [ ] **Fix skipped spec tests** (194 total):
  - Bash: 104 skipped (various implementation gaps)
  - AWK: 41 skipped (operators, control flow, functions)
  - Grep: 14 skipped (flags, features)
  - Sed: 15 skipped (features)
  - JQ: 20 skipped (functions, flags)
- [ ] **Fix bash_diff tests** (21 total):
  - wc: 14 tests (output formatting differs)
  - background: 2 tests (non-deterministic order)
  - globs: 2 tests (VFS vs real filesystem glob expansion)
  - timeout: 1 test (timeout 0 behavior)
  - brace-expansion: 1 test (empty item handling)

## Adding New Tests

1. Create or edit `.test.sh` file in appropriate category
2. Use the standard format with `### test_name`, `### expect`, `### end`
3. Run `just check-bash-compat` to verify expected output matches real bash
4. If test fails due to unimplemented feature, add `### skip: reason`
5. If BashKit intentionally differs from bash, add `### bash_diff: reason`
6. Update `KNOWN_LIMITATIONS.md` for skipped tests

### Checking Expected Outputs

The `scripts/update-spec-expected.sh` script helps verify expected outputs:

```bash
# Check all tests match real bash
./scripts/update-spec-expected.sh

# Show detailed comparison for each test
./scripts/update-spec-expected.sh --verbose
```

If a test fails, either:
1. Fix the expected output to match real bash, or
2. Add `### bash_diff: reason` if the difference is intentional

## Comparison Testing

The `bash_comparison_tests` test runs in CI and compares BashKit output against real bash:

```rust
pub fn run_real_bash(script: &str) -> (String, i32) {
    Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
}
```

Tests marked with `### bash_diff` are excluded from comparison (known intentional differences).
Tests marked with `### skip` are excluded from both spec tests and comparison.

The test fails if any non-excluded test produces different output than real bash.

A verbose version `bash_comparison_tests_verbose` is available (ignored by default) for debugging.

## Compatibility Report

The `compatibility_report` test generates a comprehensive summary of BashKit's
compatibility with real bash. Run with:

```bash
just compat-report
```

Example output:
```
╔══════════════════════════════════════════════════════════════════╗
║                 BashKit Compatibility Report                     ║
╚══════════════════════════════════════════════════════════════════╝

┌─────────────┬───────┬────────┬─────────┬───────────┬─────────────────┐
│  Category   │ Total │ Passed │ Skipped │ BashDiff  │   Bash Compat   │
├─────────────┼───────┼────────┼─────────┼───────────┼─────────────────┤
│    bash     │  404  │ 294/294│   110   │    21     │ 273/273 (100.0%)│
│     awk     │  89   │  48/48 │   41    │     0     │  48/48  (100.0%)│
│    grep     │  55   │  34/34 │   21    │     0     │  33/34  ( 97.1%)│
│     sed     │  65   │  40/40 │   25    │     0     │  40/40  (100.0%)│
│     jq      │  95   │  58/58 │   37    │     0     │  58/58  (100.0%)│
└─────────────┴───────┴────────┴─────────┴───────────┴─────────────────┘

Summary:
  Bash compatibility: 452/453 (99.8%)
```

The report shows:
- **Passed**: Tests passing against expected output
- **Skipped**: Tests for unimplemented features
- **BashDiff**: Tests with known intentional differences from bash
- **Bash Compat**: Tests producing identical output to real bash

## Differential Fuzzing

Grammar-based property testing using proptest generates random valid bash scripts
and compares BashKit output against real bash. This helps find edge cases that
aren't covered by hand-written spec tests.

### Running Differential Fuzzing

```bash
# Run with default 50 cases per test
cargo test --test proptest_differential

# Run with more cases for deeper testing
PROPTEST_CASES=1000 cargo test --test proptest_differential

# Run with output to see generated scripts
cargo test --test proptest_differential -- --nocapture

# Using just commands
just fuzz-diff
just fuzz-diff-deep
```

### Script Generators

The fuzzer generates scripts in these categories:
- **Echo commands** - Various quoting styles, flags (-n), multiple args
- **Arithmetic** - Addition, subtraction, multiplication, division, modulo
- **Control flow** - if/else, for loops, while loops, case statements
- **Pipelines** - echo | cat, multi-stage pipes
- **Logical operators** - &&, ||, combined chains
- **Command substitution** - $() and backticks
- **Functions** - Definition and invocation

### Known Limitations

Some features are intentionally excluded from fuzzing:
- `pwd` - Path differs between BashKit VFS and real filesystem
- `wc` - Output formatting differs (column alignment)
- Filesystem operations - BashKit uses virtual filesystem

## Alternatives Considered

### Bash test suite
Rejected: Too complex, many tests for features we intentionally don't support.

### Traditional fuzzing (AFL, libFuzzer)
Future consideration: Would help find parser crashes via mutation.

## Verification

```bash
# Run what CI runs
cargo test --features http_client
cargo test --features failpoints --test security_failpoint_tests -- --test-threads=1

# Run ALL spec tests including ignored bash tests (manual)
cargo test --test spec_tests -- --include-ignored --nocapture

# Check pass rates for each category
cargo test --test spec_tests -- --nocapture 2>&1 | grep "Total:"

# Run differential fuzzing
cargo test --test proptest_differential -- --nocapture
```
