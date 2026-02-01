# 004: Testing Strategy

## Status
Implemented

## Decision

BashKit uses a multi-layer testing strategy:

1. **Unit tests** - Component-level tests in each module
2. **Spec tests** - Compatibility tests against bash behavior
3. **Comparison tests** - Direct comparison with real bash

## Honest Metrics

We track **spec test pass rate**, not bash compatibility percentage:
- Pass rate = tests passing / tests running (excludes skipped)
- Skipped tests are documented limitations, not hidden failures
- Comparison tests against real bash run separately (not in pass rate)

This matters because:
1. 100% of curated tests ≠ bash compatible
2. We test what we implement, not what bash does
3. Real compatibility requires differential testing against bash

## Spec Test Framework

### Location
```
crates/bashkit/tests/
├── spec_runner.rs      # Test parser and runner
├── spec_tests.rs       # Integration test entry point
├── debug_spec.rs       # Debugging utilities
└── spec_cases/
    ├── bash/           # Core bash compatibility
    │   ├── echo.test.sh
    │   ├── variables.test.sh
    │   ├── control-flow.test.sh.skip  # Needs implementation
    │   ├── functions.test.sh
    │   ├── arithmetic.test.sh
    │   ├── arrays.test.sh
    │   ├── globs.test.sh
    │   ├── pipes-redirects.test.sh
    │   └── command-subst.test.sh
    ├── awk/            # AWK builtin tests
    ├── grep/           # Grep builtin tests
    ├── sed/            # Sed builtin tests
    └── jq/             # JQ builtin tests
```

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

## Coverage Goals

| Category | Tests | Passing | Skipped | Status |
|----------|-------|---------|---------|--------|
| Core shell | 145 | 100 | 45 | In progress |
| Text processing | 72 | 62 | 10 | Good |

Note: "Passing" means passing against expected output. For real bash compatibility, run comparison tests.

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
# All tests pass
cargo test --test spec_tests

# Check coverage percentage
cargo test --test spec_tests -- bash_spec_tests --nocapture 2>&1 | grep "Pass rate"
```
