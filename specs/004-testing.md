# 004: Testing Strategy

## Status
Implemented

## Decision

BashKit uses a multi-layer testing strategy:

1. **Unit tests** - Component-level tests in each module
2. **Spec tests** - Compatibility tests against bash behavior
3. **Security tests** - Threat model and failpoint tests
4. **Comparison tests** - Direct comparison with real bash (manual)

## CI Test Summary

Tests run automatically on every PR via `cargo test --features network`:

| Test Suite | Test Functions | Notes |
|------------|---------------|-------|
| Unit tests (bashkit lib) | 286 | Core interpreter tests |
| limits.rs | 5 | Resource limit tests |
| spec_tests.rs | 9 (2 ignored) | Spec compatibility tests |
| threat_model_tests | 39 | Security tests |
| security_failpoint_tests | 14 | Fault injection tests |
| Doc tests | 2 | Documentation examples |
| **Total** | **355** | Plus 4 examples executed |

## Spec Test Framework

### Location
```
crates/bashkit/tests/
├── spec_runner.rs      # Test parser and runner
├── spec_tests.rs       # Integration test entry point
├── debug_spec.rs       # Debugging utilities
├── threat_model_tests.rs    # Security threat model tests
├── security_failpoint_tests.rs  # Fault injection tests
└── spec_cases/
    ├── bash/           # Core bash compatibility (19 files, 209 cases)
    │   ├── arithmetic.test.sh (22)
    │   ├── arrays.test.sh (14)
    │   ├── background.test.sh (2)
    │   ├── command-subst.test.sh (14)
    │   ├── control-flow.test.sh.skip  # Skipped - needs implementation
    │   ├── cuttr.test.sh (10)
    │   ├── date.test.sh (4)
    │   ├── echo.test.sh (10)
    │   ├── fileops.test.sh (15)
    │   ├── functions.test.sh (14)
    │   ├── globs.test.sh (7)
    │   ├── headtail.test.sh (14)
    │   ├── herestring.test.sh (8)
    │   ├── path.test.sh (14)
    │   ├── pipes-redirects.test.sh (13)
    │   ├── procsub.test.sh (6)
    │   ├── sleep.test.sh (6)
    │   ├── sortuniq.test.sh (12)
    │   ├── variables.test.sh (20)
    │   └── wc.test.sh (4)
    ├── awk/            # AWK builtin tests (19 cases)
    ├── grep/           # Grep builtin tests (15 cases)
    ├── sed/            # Sed builtin tests (17 cases)
    └── jq/             # JQ builtin tests (21 cases)
```

### Spec Test Counts

| Category | Test Cases | In CI | Pass | Skip |
|----------|------------|-------|------|------|
| Bash | 209 | **NO** (ignored) | - | - |
| AWK | 19 | Yes | 17 | 2 |
| Grep | 15 | Yes | 13 | 2 |
| Sed | 17 | Yes | 13 | 4 |
| JQ | 21 | Yes | 20 | 1 |
| **Total** | **281** | **72** | 63 | 9 |

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

## Running Tests

```bash
# All spec tests
cargo test --test spec_tests

# Single category
cargo test --test spec_tests -- bash_spec_tests

# With output
cargo test --test spec_tests -- --nocapture

# Comparison against real bash (ignored by default)
cargo test --test spec_tests -- bash_comparison_tests --ignored
```

## Coverage

**Note:** No formal coverage tooling (codecov, tarpaulin) is currently configured.
Coverage is tracked manually via spec test pass rates.

### Current Status
- Text processing tools: 88% pass rate (63/72 running in CI)
- Core bash specs: Not running in CI (209 cases ignored)

## TODO: Testing Gaps

The following items need attention:

- [ ] **Enable bash_spec_tests in CI** - 209 test cases currently ignored
- [ ] **Fix control-flow.test.sh** - Currently skipped (.skip suffix)
- [ ] **Add coverage tooling** - Consider cargo-tarpaulin or codecov
- [ ] **Fix skipped spec tests** (9 total):
  - AWK: 2 skipped
  - Grep: 2 skipped
  - Sed: 4 skipped
  - JQ: 1 skipped
- [ ] **Add bash_comparison_tests to CI** - Currently ignored, runs manually

## Adding New Tests

1. Create or edit `.test.sh` file in appropriate category
2. Use the standard format with `### test_name`, `### expect`, `### end`
3. Run tests to verify
4. If test fails due to unimplemented feature, add `### skip: reason`
5. Update `KNOWN_LIMITATIONS.md` for skipped tests

## Comparison Testing

The `bash_comparison_tests` test (ignored by default) runs each spec test against both BashKit and real bash:

```rust
pub fn run_real_bash(script: &str) -> (String, i32) {
    Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
}
```

This helps identify behavioral differences.

## Alternatives Considered

### Bash test suite
Rejected: Too complex, many tests for features we intentionally don't support.

### Property-based testing
Future consideration: Would help find edge cases in parser.

### Fuzzing
Future consideration: Would help find parser crashes.

## Verification

```bash
# Run what CI runs
cargo test --features network
cargo test --features failpoints --test security_failpoint_tests -- --test-threads=1

# Run ALL spec tests including ignored bash tests (manual)
cargo test --test spec_tests -- --include-ignored --nocapture

# Check pass rates for each category
cargo test --test spec_tests -- --nocapture 2>&1 | grep "Total:"
```
