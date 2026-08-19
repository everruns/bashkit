---
type: Test Strategy
title: Testing Strategy
description: Test organization, patterns, fixtures, differential testing, and CI expectations.
tags:
  - bashkit
  - testing
  - ci
---

# Testing Strategy

## Status
Implemented

## Decision

Multi-layer testing strategy:

1. **Unit tests** - Component-level tests in each module
2. **Spec tests** - Compatibility tests against bash behavior
3. **Security tests** - Threat model and failpoint tests
4. **Comparison tests** - Direct comparison with real bash
5. **Differential fuzzing** - Property-based testing against real bash

For current test counts and pass rates, see CI (`spec_tests::bash_spec_tests`).
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

- `tests/integration/main.rs`, declares every default integration test
  as a `mod`. Built once, linked once. New behavioral tests go here.
- `tests/integration/<name>.rs`, one module per concern area.
- `tests/<name>.rs`, **only** for tests that genuinely need their own
  binary. Today that list is:
  - `realfs_tests.rs`, `realfs` feature, runs in a dedicated CI job.
  - `security_failpoint_tests.rs`, `failpoints` global state, requires
    `--test-threads=1`.
  - `proptest_security.rs`, `--test-threads=1` and custom
    `PROPTEST_CASES` env.
  - `ssh_builtin_tests.rs`, `ssh_supabase_tests.rs`, feature-isolation
    sweeps that build bashkit with `--features ssh` only.
  - `logging_security_tests.rs`, mutates `BASHKIT_UNSAFE_LOGGING` in
    the process env; cannot share a binary with other tests.

When adding a new test file, default to placing it under
`tests/integration/` and adding a `pub mod foo;` line to
`tests/integration/main.rs`. Only promote to a top-level
`tests/<name>.rs` if the test trips one of the criteria above; document
the reason in the file's module docstring.

Filtering still works as usual: `cargo test --test integration -- foo`
matches `integration::*::foo*` test paths.

Filesystem implementations share a private conformance helper under
`tests/support/filesystem_security_conformance.rs`. The default adapters invoke
it from the consolidated integration binary; `realfs_tests.rs` invokes the same
contract under `--features realfs`. Keep backend-neutral invariants in the
helper and adapter-specific fault/atomicity cases in their owning test module.

The consolidation rule above covers the `bashkit` crate. `bashkit-cli`
has its own, much smaller test surface:

- `crates/bashkit-cli/src/main.rs` unit tests cover argument parsing and
  builder wiring in-process (`build_bash(&args, mode)` + `bash.exec(...)`).
- `crates/bashkit-cli/tests/cli_oneshot.rs` spawns the real binary via
  `env!("CARGO_BIN_EXE_bashkit")` and covers the parts of the CLI
  contract that only exist as a process: exit status propagation,
  stdout/stderr separation, the anyhow error path out of `main`,
  resource-limit flags, and argv shapes. Children run with
  `RUST_BACKTRACE` cleared so the anyhow output is deterministic.

Anything observable only through a subprocess belongs in
`cli_oneshot.rs`; anything about how flags configure the builder stays in
the `main.rs` unit tests, which are far cheaper.

## Coverage

Uploaded to Codecov from three sources: Rust unit/integration coverage via
`cargo tarpaulin`; Rust coverage exercised through Python and Node binding
tests via `cargo llvm-cov`.

## Third-Party Adoption Suite

`crates/bashkit/tests/integration/thirdparty_adoption_tests.rs` covers the
"agent bash tool" embedding shape end to end: a real workspace mounted
read-write, a `CommandResolver` bridging unresolved names to host processes,
and `HostMounts` mapping the VFS cwd back to a host directory.

Decision: this shape was only ever validated downstream, and the first adopter
(crabot) shipped two defects bashkit's own tests could not see, a harness that
emptied `PATH`, and a stdin pipe to a host process that never reached EOF and
hung the suite. Both are properties of the *composition*, not of any one API,
so they need a test that composes.

Rules for this file:

- **The bridge in it is the reference.** If bridging a host command needs more
  code than what is there, that is a gap in bashkit's API, not a reason to grow
  the test.
- **Use names bashkit does not implement** (`host-cat`, `host-echo`, …). The
  resolver runs last, so a test using `cat` or `echo` silently asserts on
  builtins and never reaches the bridge, verified by injecting a defect and
  confirming the test fails.
- **Never let a defect become a hang.** The bridge bounds its wait and drains
  pipes only after a clean exit; after a timeout kill a grandchild can still
  hold the write end, and reading would block forever. A hung job reports
  nothing; a bounded one names the cause.

`host_mounts_tests.rs` covers the mapping in isolation, including the
sibling-prefix trap (`/workspace2` must not resolve inside `/workspace`).

Both are `cfg(realfs)`, so the main `Run tests` step (which does not enable
`realfs`) compiles them out. They run in the dedicated `Run realfs integration
tests` step and in the `Test (Windows adoption shape)` job.

## Windows CI

`test-windows` is the only non-ubuntu job. It runs the adoption, host-mount,
and command-resolver suites on `windows-latest` and gates `Check`.

It is deliberately scoped rather than a full workspace port: the adoption shape
exists to work on Windows without a real `bash`, and that claim needs a Windows
runner to stay true. Everything else stays ubuntu-only.

## Third-Party API Steps in CI

The `Examples` job runs a few steps that call third-party LLM APIs (Anthropic
via `cargo run --example agent_tool`, OpenAI via `examples/harness-openai-joke.sh`).
Their credentials come from Doppler, so they only run on pushes to `main`.

Every such step must set `continue-on-error: true`. An upstream outage, quota
exhaustion, or billing lapse is not a bashkit regression, and letting it fail
the `Check` gate makes main red for a cause no commit can fix. The steps stay
in CI as smoke signals, read their logs when they fail rather than trusting
the job conclusion.

Corollary: these steps do not protect against a broken model id. Pin only model
ids that are current, and re-check them when a model is retired, a retired id
surfaces as a 404 in an already-green job.

## CI hang and gate invariants

Two rules on `.github/workflows/ci.yml`, both enforced by
`scripts/tests/test_release_workflow.py` (run in the `Lint` job under
`Test repo scripts`):

1. **Every job sets `timeout-minutes`.** Without one, GitHub's 6h job limit is
   the only bound, so a hung step burns a runner for six hours and reports
   `cancelled`. Budget roughly 2-3x the job's observed green run time.
2. **The `Check` gate `needs` every other job and tests every result.** `Check`
   is the branch-protection status, so a job missing from that list is a job
   nobody is gating on. On 2026-08-18 `wasm` and `wasm-web` were absent, and a
   `wasm-web` job cancelled at the 6h limit left the run red while `Check`
   reported success minutes earlier.

Corollary for CI tool installs: prefer a pinned, digest-verified download with a
bounded timeout over `apt-get`, which has no overall timeout and stalls
indefinitely on a bad mirror. See `scripts/install-binaryen-ci.sh` and
`scripts/install-ripgrep-ci.sh`.

## Adding New Tests

1. Create or edit `.test.sh` file in appropriate category, standard format
2. Run `just check-bash-compat` to verify expected output matches real bash
3. Unimplemented feature → `### skip: reason`; intentional difference →
   `### bash_diff: reason`
4. Record the limitation in [Known Limitations](limitations.md) (skip reason = evidence)

## Public capability parity contract

[`contracts/capability-parity.json`](../../contracts/capability-parity.json) is the
canonical matrix for Rust `BashBuilder`, `BashTool`, and `ScriptedTool`, plus the CLI,
Python, NAPI JavaScript, browser WASM, and C ABI. Every capability/surface cell is
either supported with a `path#test-selector` that the owning package test job executes,
or unsupported with a concrete reason. This keeps intentionally narrow wrappers honest
without implying that every core capability belongs on every surface.

`just check-capability-parity` rejects incomplete cells, stale selectors, and generated
inventory drift. `just regen-capability-parity` updates the generated
[Public Capability Parity](../status/capability-parity.md) inventory. A wrapper option
change must update the executable surface test and manifest in the same change.

## Comparison Testing

The `bash_comparison_tests` test is ignored by default for local `cargo test`
runs because it compares against the host shell environment. CI runs it
explicitly as a strict parity gate. Tests marked with `### bash_diff` are
excluded from comparison. Tests marked with `### skip` are excluded from both
spec tests and comparison.

### yq compatibility and fuzzing

The yq suite keeps a portable locked corpus for the jq-compatible mikefarah/yq
surface and replays the same cases against a real mikefarah/yq binary when one
is available. Set `MIKEFARAH_YQ=/path/to/yq` to make the live oracle explicit;
the locked cases always run, so CI does not silently lose all compatibility
coverage when the external tool is absent.

`yq_fuzz` varies YAML/JSON input, output format, jq expressions, and stdin versus
in-place execution. It must invoke `yq` directly, the removed legacy `yaml`
helper is not a valid fuzz oracle. The proptest layer pairs arbitrary YAML and
arbitrary filter text to enforce panic, diagnostic, host-path, and Debug-shape
invariants at the parser/evaluator boundary.

## Quarterly Competitor Regressions

Behavior fixes imported from peer shell interpreters live as checked-in JSON
under `crates/bashkit/tests/fixtures/competitor-regressions/`. Each quarterly
corpus records its source repository, inclusive review window, and expected
case count. Every case must include a stable kebab-case ID, full upstream
commit hash, commit date, script, locked stdout/stderr/status, oracle, optional
required Cargo features, and one classification:

- `pass`: Bashkit already matched the behavior when imported.
- `bug`: the import exposed a Bashkit defect; `resolution` must be `fixed` and
  the same corpus remains the regression test.
- `intentional_divergence`: `limitation_id` must resolve to a canonical `L-*`
  row in [Known Limitations](limitations.md).

Use `oracle: real_bash` only for portable shell behavior. The runner executes
those cases with the first `bash` on `PATH`, a cleared environment, and a fresh
temporary working directory. A case whose syntax requires a newer shell may
set `minimum_bash_major`; older host oracles skip only that comparison. Use
`locked` for platform-specific utilities, embedded runtimes, or HTTP; the
expected value must be established during review. HTTP fixtures use an injected
`HttpTransport`, never the network.

Quarterly import process:

1. Review upstream fixes since the prior corpus window; copy the smallest
   behavior reproducer and pin the full commit hash.
2. Add a new dated JSON corpus. Do not add a fetch step or generated executable
   source: fixture review is the trust boundary (TM-INF-032).
3. Run `just competitor-regressions`. Confirm every `real_bash` oracle and the
   Bashkit lane; fix tight regressions or classify a canonical limitation.
4. CI runs the same feature-complete, network-free lane. Schema, provenance,
   unique IDs, classifications, and limitation references are test-enforced.

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
4. Keep files focused, one file per concern area
