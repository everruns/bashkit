# Testing Strategy

## Decision

Multi-layer testing strategy:

1. **Unit tests** - Component-level tests in each module
2. **Spec tests** - Compatibility tests against bash behavior
3. **Security tests** - Threat model and failpoint tests
4. **Comparison tests** - Direct comparison with real bash
5. **Differential fuzzing** - Property-based testing against real bash

For current test counts and pass rates, see `specs/implementation-status.md`.
For run commands, see AGENTS.md "Local Dev" and `.github/workflows/ci.yml`.

## Spec Test Framework

### Test File Format

```sh
### test_name
# Optional description
script_to_execute
### expect
expected_output
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

Spec tests live inside the consolidated `integration` binary:
`cargo test --test integration -- spec_tests::` (or a category like
`spec_tests::bash_spec_tests`). `just check-bash-compat` verifies expectations
against real bash; `just compat-report` generates the compatibility report;
`./scripts/update-spec-expected.sh [--verbose]` updates expected outputs.

## Integration Test Binary Layout

Cargo treats every file in `crates/bashkit/tests/*.rs` as its own
integration-test binary, statically linking the whole interpreter (monty,
zapcode, turso, russh, jaq, reqwest+rustls, ed25519-dalek) into each.
With ~80 such files the link step alone exceeded the CI runner's disk
(`rustc-LLVM ERROR: IO failure on output stream: No space left on
device`). Bashkit consolidates those into one binary:

- `tests/integration/main.rs` — declares every default integration test
  as a `mod`. Built once, linked once. New behavioral tests go here.
- `tests/integration/<name>.rs` — one module per concern area.
- `tests/<name>.rs` — **only** for tests that genuinely need their own
  binary. Today that list is:
  - `realfs_tests.rs` — `realfs` feature, runs in a dedicated CI job.
  - `security_failpoint_tests.rs` — `failpoints` global state, requires
    `--test-threads=1`.
  - `proptest_security.rs` — `--test-threads=1` and custom
    `PROPTEST_CASES` env.
  - `ssh_builtin_tests.rs`, `ssh_supabase_tests.rs` — feature-isolation
    sweeps that build bashkit with `--features ssh` only.
  - `logging_security_tests.rs` — mutates `BASHKIT_UNSAFE_LOGGING` in
    the process env; cannot share a binary with other tests.

When adding a new test file, default to placing it under
`tests/integration/` and adding a `pub mod foo;` line to
`tests/integration/main.rs`. Only promote to a top-level
`tests/<name>.rs` if the test trips one of the criteria above; document
the reason in the file's module docstring.

Filtering still works as usual: `cargo test --test integration -- foo`
matches `integration::*::foo*` test paths.

## Coverage

Uploaded to Codecov from three sources: Rust unit/integration coverage via
`cargo tarpaulin`; Rust coverage exercised through Python and Node binding
tests via `cargo llvm-cov`.

## Adding New Tests

1. Create or edit `.test.sh` file in appropriate category, standard format
2. Run `just check-bash-compat` to verify expected output matches real bash
3. Unimplemented feature → `### skip: reason`; intentional difference →
   `### bash_diff: reason`
4. Update `specs/implementation-status.md` for skipped tests

## Comparison Testing

The `bash_comparison_tests` test is ignored by default for local `cargo test`
runs because it compares against the host shell environment. CI runs it
explicitly as a strict parity gate. Tests marked with `### bash_diff` are
excluded from comparison. Tests marked with `### skip` are excluded from both
spec tests and comparison.

## Differential Fuzzing

Grammar-based property testing using proptest generates random valid bash
scripts and compares Bashkit output against real bash. `just fuzz-diff`
(50 cases), `just fuzz-diff-deep` (1000). Part of the consolidated binary:
`cargo test --test integration -- proptest_differential::`.

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
