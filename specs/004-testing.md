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
└── spec_cases/
    ├── bash/           # Core bash compatibility (19 files, 331 cases)
    │   ├── arithmetic.test.sh
    │   ├── arrays.test.sh
    │   ├── background.test.sh
    │   ├── command-subst.test.sh
    │   ├── control-flow.test.sh.skip  # Skipped - needs implementation
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
    ├── awk/            # AWK builtin tests (19 cases)
    ├── grep/           # Grep builtin tests (15 cases)
    ├── sed/            # Sed builtin tests (17 cases)
    └── jq/             # JQ builtin tests (21 cases)
```

### Spec Test Counts

| Category | Test Cases | In CI | Pass | Skip |
|----------|------------|-------|------|------|
| Bash | 331 | Yes | 223 | 108 |
| AWK | 19 | Yes | 17 | 2 |
| Grep | 15 | Yes | 15 | 0 |
| Sed | 17 | Yes | 17 | 0 |
| JQ | 21 | Yes | 21 | 0 |
| **Total** | **403** | **403** | 293 | 110 |

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

# Comparison against real bash (ignored by default)
cargo test --test spec_tests -- bash_comparison_tests --ignored
```

## Coverage

**Note:** No formal coverage tooling (codecov, tarpaulin) is currently configured.
Coverage is tracked manually via spec test pass rates.

### Current Status
- All spec tests: 73% pass rate (293/403 running in CI, 110 skipped)
- Text processing tools: 97% pass rate (70/72 running, 2 AWK skipped)
- Core bash specs: 100% pass rate (223/223 running, 108 skipped)

## TODO: Testing Gaps

The following items need attention:

- [x] **Enable bash_spec_tests in CI** - Done! 223/331 tests running
- [x] **Add bash_comparison_tests to CI** - Done! 275 tests compared against real bash
- [ ] **Fix control-flow.test.sh** - Currently skipped (.skip suffix)
- [ ] **Add coverage tooling** - Consider cargo-tarpaulin or codecov
- [ ] **Fix skipped spec tests** (110 total):
  - Bash: 108 skipped (various implementation gaps)
  - AWK: 2 skipped (blocked by multi-statement action parsing bug)
- [ ] **Fix bash_diff tests** (21 total):
  - wc: 14 tests (output formatting differs)
  - background: 2 tests (non-deterministic order)
  - globs: 2 tests (VFS vs real filesystem glob expansion)
  - timeout: 1 test (timeout 0 behavior)
  - brace-expansion: 1 test (empty item handling)

## Adding New Tests

1. Create or edit `.test.sh` file in appropriate category
2. Use the standard format with `### test_name`, `### expect`, `### end`
3. Run tests to verify
4. If test fails due to unimplemented feature, add `### skip: reason`
5. Update `KNOWN_LIMITATIONS.md` for skipped tests

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
