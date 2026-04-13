# 004: Testing Strategy

## Status
Implemented

## Decision

Bashkit uses a multi-layer testing strategy:

1. **Unit tests** - Component-level tests in each module
2. **Spec tests** - Compatibility tests against bash behavior
3. **Security tests** - Threat model and failpoint tests
4. **Comparison tests** - Direct comparison with real bash
5. **Differential fuzzing** - Property-based testing against real bash

For current test counts and pass rates, see `specs/009-implementation-status.md`.

## Spec Test Framework

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

# Check spec tests match real bash
just check-bash-compat

# Generate comprehensive compatibility report
just compat-report
```

## Coverage

Coverage is tracked with cargo-tarpaulin and uploaded to Codecov.

```bash
cargo tarpaulin --features http_client --out html --output-dir coverage
```

## Adding New Tests

1. Create or edit `.test.sh` file in appropriate category
2. Use the standard format with `### test_name`, `### expect`, `### end`
3. Run `just check-bash-compat` to verify expected output matches real bash
4. If test fails due to unimplemented feature, add `### skip: reason`
5. If Bashkit intentionally differs from bash, add `### bash_diff: reason`
6. Update `specs/009-implementation-status.md` for skipped tests

### Checking Expected Outputs

```bash
# Check all tests match real bash
./scripts/update-spec-expected.sh

# Show detailed comparison for each test
./scripts/update-spec-expected.sh --verbose
```

## Comparison Testing

The `bash_comparison_tests` test runs in CI and compares Bashkit output against
real bash. Tests marked with `### bash_diff` are excluded from comparison.
Tests marked with `### skip` are excluded from both spec tests and comparison.

## Differential Fuzzing

Grammar-based property testing using proptest generates random valid bash scripts
and compares Bashkit output against real bash.

```bash
just fuzz-diff         # default 50 cases
just fuzz-diff-deep    # 1000 cases
```

Known exclusions: `pwd` (path differs), `wc` (formatting), filesystem ops (VFS).

## JavaScript Runtime Compatibility Tests

The NAPI-RS JS bindings must work across Node.js, Bun, and Deno. A separate
**runtime-compat** test suite using only `node:test` and `node:assert` validates
cross-runtime compatibility.

| Runtime | Versions | ava tests | runtime-compat | Examples |
|---------|----------|-----------|----------------|----------|
| Node    | 20, 22, 24, latest | Yes | Yes | Yes |
| Bun     | latest, canary | No | Yes | Yes |
| Deno    | 2.x, canary | No | Yes | Yes |

### Maintenance Rules

1. New ava tests covering new API surface → add runtime-compat counterpart
2. runtime-compat tests use only `node:test`, `node:assert`, `node:module`
3. Files are plain `.mjs` (no TypeScript)
4. Keep files focused — one file per concern area

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

# Run differential fuzzing
cargo test --test proptest_differential -- --nocapture
```
