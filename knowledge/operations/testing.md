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
  - `headtail_allocation_tests.rs`, installs a counting
    `#[global_allocator]` to assert a memory bound (TM-DOS-117); the
    allocator is process-wide, so unrelated tests sharing the binary
    would land in the measurement.

When adding a new test file, default to placing it under
`tests/integration/` and adding a `pub mod foo;` line to
`tests/integration/main.rs`. Only promote to a top-level
`tests/<name>.rs` if the test trips one of the criteria above; document
the reason in the file's module docstring.

`cdpath_allocation_tests.rs` also needs its own binary: its process-wide
counting allocator proves CDPATH peak heap usage stays below 256 KiB for a
bounded 1,024-entry / 1,024-byte-target request that previously retained over
1 MiB of candidate paths. It complements the shared-budget integration tests
rather than substituting budget counters for an actual allocation measurement.

`tar_files_from_allocation_tests.rs` also needs its own binary for its
process-wide counting allocator. It uses a 40 KB newline-dense list and a
64 KB live budget through `Bash::exec`, rejecting the former >1 MB eager
collector peak while asserting bounded rejection and subsequent session use.
The consolidated `tar_files_from_budget_tests` covers live/input/work limits,
stdin, repeated lists, blank/CRLF/lossy names, and GNU tar differential behavior.

`pr_allocation_tests.rs` also needs its own binary: its counting allocator
proves repeated headers stay within three times a 128 KiB live budget,
including transient reallocation overlap and interpreter dispatch storage.
The bounded production request uses a 1 KiB header and 256 empty lines.
It also checks 256 empty file operands with a 4 KiB date format: no pages
means no retained per-file date strings.

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

### The `bashkit` suite spawns the CLI, and must not guess its path

One exception runs the CLI from the `bashkit` crate: the bounded-child
harness in `threat_model_tests.rs` (TM-DOS-135) spawns the `bashkit`
binary so hostile reproducers never execute in the test process, where
pre-fix they overflowed the stack and aborted the whole runner.

That binary belongs to a sibling package, so `CARGO_BIN_EXE_bashkit` is
not available there. Two rules follow.

1. The test selection must include the `bashkit-cli` package. Its own
   integration tests do get `CARGO_BIN_EXE_bashkit`, which is what makes
   cargo export the binary at all; `--tests` across the workspace is
   enough, while `-p bashkit --test integration` on its own is not.
   `--bins` is not the mechanism: on cargo 1.101-nightly it builds the
   binary only as a test harness, inside the hashed build directory, and
   exports nothing runnable.
2. The harness must locate the binary by walking up from the test binary
   until it finds it, never at a fixed depth. Where cargo puts a test
   binary is not contractual: the pinned stable uses
   `<profile>/deps/<test>`, cargo 1.101-nightly's separate build
   directory uses `<profile>/build/<package>/<hash>/out/<test>`. Only the
   export into the profile directory is guaranteed, and that directory is
   an ancestor either way. Reading it as `exe.parent().parent()` passed on
   stable and landed in the hashed build directory under nightly, so the
   nightly ASAN job went red while required CI stayed green.

The walk stops at cargo's target directory, recognised by the
`CACHEDIR.TAG` cargo writes there, so an unrelated `bashkit` file above
the build tree can never be spawned in place of the binary under test —
that would be a false pass, worse than the failure. A depth bound applies
whether or not the marker is present.

`cli_lookup_resolves_known_cargo_layouts` covers both layouts over
synthetic trees (the live tree only ever has its own toolchain's layout),
plus the package-named `build/bashkit` *directory* not being mistaken for
the binary and a decoy above the target directory being refused.
`bashkit_cli_path()` fails closed naming every path it ruled out.

## Command-local input admission

`interpreter::redirection::input_admission_tests` uses two-byte ordinary and
lazy files to check admission before loading, including the empty open for
`<>`, exact byte/text storage boundaries, failed-admission refunds, surviving
stdin storage, and injected cancellation during a read. `stream::tests` covers
lossy decoding without changing bytes and capacity-owned copy boundaries.
The ordinary `input_redirection_*` integration cases cover binary stdin/fd
copies, empty read-write creation, existing-file preservation, here inputs,
and tiny process-substitution input (including mapfile). A two-byte buffered
pipe fixture checks admission and lease release while pipe stat stays zero
and the existing substitution owner remains open. The owner exposes buffered
length without copying; admission must precede its `read_file` copy.
The cases also cover nonfatal missing-file diagnostics and enabled/disabled
symlink-following for ordinary read/read-write input. These fixtures require no
host mount: a two-byte in-memory mounted file without synthetic parent entries
covers direct read, read-write/create and refusal before lazy loading. A missing
canonical parent is not proof the backend cannot stat/read the original path.
Two-byte overlay/read-only and rebased-namespace fixtures also check successful
read, rejected read-write open with backend diagnostics, and unchanged content.
These fixtures require no
subprocess, device read, large allocation, or original resource-exhaustion shape. Selecting
them is scoped proof, not a claim that the unchanged full/coverage suites ran.

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
  the test. It invokes concrete executables and passes parsed arguments as
  separate argv values; never concatenate them into a `sh -c` or `cmd /C`
  script. Constant shell builtins may be used only without caller arguments.
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

## Adding New Tests

1. Create or edit `.test.sh` file in appropriate category, standard format
2. Run `just check-bash-compat` to verify expected output matches real bash
3. Unimplemented feature → `### skip: reason`; intentional difference →
   `### bash_diff: reason`
4. Record the limitation in [Known Limitations](limitations.md) (skip reason = evidence)

## Resource-cap tests must not race the deadline

The opt-in input-redirect helper polls its one in-flight RSS sampling future by
mutable reference across `select!` rounds. A losing branch must leave that future
in its owned slot; only observing its result clears the slot. The ordinary
`ir_helper_concurrent_schedule` fixture uses the wrapper's same
`poll_owned_sample` helper with 64 bytes of in-memory pipe data and a release-gated
future. This verifies scheduling ownership, not subprocess cleanup, effective
resource caps, platform coverage or merged security proof.

The native input-redirect runner shares one owned lifecycle for the main child
and RSS sampler: wait, both capped captures and live first-failure notices are
polled together. Ordinary errors request kill before finite reap/capture completion;
completion waits for an owned pending sample and records incomplete cleanup as a
negative verdict. Reap timeout, I/O failure and success are separate outcomes.
The process feature is a native-only Tokio dev-dependency; WASM builds keep the
workspace feature set. Drop remains a best-effort backstop, not evidence of
reap or capture completion on panic or external cancellation. Tiny in-memory
fixtures verify notice delivery before EOF, nested reap failures and pending-sample
ownership. They do not establish subprocess, Linux or full security shipping proof.

The input helper follows the shared sanitizer-compatible launcher
policy: no virtual-AS cap; its receipt explicitly marks AS unsupported and
labels resident-memory observations as sampled RSS with an unchanged 1 GiB
cutoff, not hard OS enforcement or a between-sample peak guarantee. CPU 10s,
stack 4096 KiB, file backstop 128 MiB on measured Bash units, separate artifact
acceptance below 64 MiB, wall deadlines and capped drains remain unchanged.
Backend-admission futures are boxed at the recursive redirect boundary to
avoid embedding their state in each command frame; layout-only observations
do not establish a depth-execution result.

Controlled retained-pipe fixtures keep a caller-owned reader-handoff vector
outside their cleanup guard. A bounded `Drop` returns any unfinished ORIGINAL
handle to that owner rather than recording a count and detaching it. The owner
releases the controlled writer and joins the returned handle before asserting
completion; a timeout or failed drain remains a negative verdict.

A test whose expected outcome is a **memory or output cap** must pin its own
generous execution timeout instead of inheriting the `ExecutionLimits::default()`
30 s deadline. Otherwise the asserted diagnostic becomes a function of machine
speed: whichever limit trips first wins, and instrumented builds change the
answer.

The nightly AddressSanitizer job (`-Z sanitizer=address`, roughly 15x slower)
caught this on the jq cap tests, which reported `jq: execution timed out` where
a value-size cap was expected. `memory_growth_security_tests.rs` now routes
every cap assertion through a 600 s `SLOW_BUILD_TIMEOUT`, which covers ASAN and
Miri while still failing an unbounded or quadratic regression by orders of
magnitude.

Only a test that asserts a timeout sets a short deadline, and it sets it
explicitly. Before adding a cap test, confirm the cap is the sole limit that can
stop the script.

## Bounded-child launchers cap CPU and stack, never address space

A supervised-child harness bounds its child with `ulimit -t` (CPU) and
`ulimit -s` (stack), plus parent-side wall, output-drain and peak-RSS
watchdogs. It must **not** set an address-space cap (`ulimit -v`,
`RLIMIT_AS`).

AddressSanitizer reserves roughly 14 TiB of *virtual* address space for its
shadow map during startup, so any `RLIMIT_AS` makes an instrumented child die
in `ReserveShadowMemoryRange` before it executes a line of test code. The
child's failure is then the launcher's cap, not the bound under test, and the
test fails for a reason unrelated to what it asserts. This reddened the
nightly ASAN job on run 257 (`awk_field_depth_bounded`, the only harness that
still used `ulimit -v`) while required CI, which is not instrumented, stayed
green on the same commit -- the same divergence shape as the CLI-path bug in
run 252.

Cap resident memory instead: sample the live child's RSS from the parent
(`ps -o rss=`) each poll, keep the maximum, kill on breach and assert the
peak afterwards so growth fails closed even when the child exits between
polls. RSS is also the better proxy for the runaway *allocation* such a bound
exists to catch, and it costs nothing in virtual space, so it holds on
instrumented and native builds alike. Size the ceiling from a measured native
peak with room for instrumentation: `awk_field_depth_bounded` peaks near
29 MiB natively against a 2 GiB ceiling, which an ASAN build fits comfortably
while multi-gigabyte growth still trips it.

`BOUNDED_CLI_LIMITS` in `threat_model_tests.rs` and the probe harness in
`bashkit-cli/tests/cli_oneshot.rs` are the reference launchers.

The same file holds one test whose guard is **throughput**, not a cap
(`ordinary_workloads_still_fit`), and it has the opposite constraint: its
budget has to sit *between* the linear cost and the quadratic one on the
slowest build, so it must be loose enough for ASAN and tight enough to still
fail. That interval is real and it found a real bug -- a quadratic `join`
(68 s for 300k elements unoptimized) passed against 600 s natively and only
overran it under ASAN, which is how nightly #247 surfaced it. Two rules follow:

- Size a throughput guard from the quadratic penalty, not from convenience.
  Shrinking the workload to buy headroom shrinks the penalty with it, and a
  small enough array fits the budget whether the code is linear or not.
- Make the linear path fast rather than the budget generous. Raising the
  budget to fit a slow linear run also lets a quadratic run through.

## Spawned CLI children get their own coverage profile path

A test that spawns the instrumented `bashkit` binary must point the child's
`LLVM_PROFILE_FILE` somewhere `cargo tarpaulin` does not merge.

A child inherits the *parent's* profile-path template, so it writes a profraw
named after the parent test binary into `target/tarpaulin/profraws/` -- the
directory tarpaulin merges. Supervised children are killed by design, and a
SIGKILLed process never flushes its profraw, so the merge step panics inside
`llvm_profparser` (`unreachable!("llvm_profparsers works on complete data")`)
and the Coverage job fails. It is a race on whether the victim flushed, so it
is intermittent: main Coverage runs 4588 and 4600 failed two days apart on
unrelated commits, each after all 2,441 tests had passed, while required CI --
which is not instrumented -- stayed green on the same trees. The same
divergence shape as the ASAN address-space bug above.

`bashkit::testing::child_profile_path()` supplies the path: one directory per
process, a unique file per call, removal left to the OS. Cleanup is
deliberately not guarded, because a killed child's foreign descendant can
still be writing there and a per-call remove could fail for reasons unrelated
to the test. A test that instead *asserts* on the child's profraw, as
`run_input_shape_bounded` does for its bounded-artifact receipt, sets
`LLVM_PROFILE_FILE` to its own path; the rule is only that the child never
writes into the merge set.

Two layers enforce it. `bounded_child_launcher_scan_tests` scans every crate
and fails any file that reaches the binary (`bashkit_cli_path()`,
`CARGO_BIN_EXE_bashkit`) without setting a profile path, and it fails closed
when the scan finds no spawners at all. The scan cannot prove the variable
survives the `bash -c` launcher and its `exec`, so
`bounded_cli_child_profile_points_outside_the_merge_set` has a real bounded
child print its own `LLVM_PROFILE_FILE` back.

Redirecting also fixes attribution: without it the CLI's counters land in a
file tarpaulin credits to the parent test binary.

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

Three gates compare a spec category against the real host tool:
`bash_comparison_tests`, `sed_comparison_tests` and `grep_comparison_tests`.
All are `#[ignore]`d for local `cargo test` runs because they need the host
tool; CI runs them together with `spec_tests:: --ignored`, which selects
exactly the gates because they are the only ignored tests in the module.
`scripts/update-spec-expected.sh` (`just check-bash-compat`) runs the same
set. Tests marked `### bash_diff` are excluded from comparison; `### skip`
excludes from both spec tests and comparison.

**A spec category without a gate is unverified, not verified.** Its
expectations are only a record of what Bashkit printed when they were written.
Pointing the gate at `grep` for the first time turned up seven expectations
that had captured Bashkit's own bugs — the `(stdin)` label, `-z` output
terminators, `-a` deleting NUL bytes, the binary-match diagnostic on stdout,
`-L`'s inverted exit status, and `-s` reporting exit 1 where GNU grep exits 2.
Wiring only `bash_comparison_tests` into CI had the same effect on `sed`: the
gate added with the GNU-sed work existed but never ran there.

Categories still without a gate: `awk`, `jq`, `yq`, `python`, `typescript`.
`awk` is not a straightforward candidate — the usual host `awk` is mawk, while
Bashkit targets gawk, so a gate would need `### bash_diff` on every gawk
extension (`**`, `gensub`, 3-argument `match`, `\u` escapes) and on `for (k in
arr)`, whose iteration order awk leaves unspecified.

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

## Imported BashBox Corpus

`spec_cases/bash/bashbox-*.test.sh` holds bash-parity cases copied from the
test suite of [BashBox](https://github.com/shipfastlabs/bashbox) (MIT, PHP port
of just-bash; attribution in `NOTICE`). One file per BashBox test file.
Imported 2026-10-07: 1,753 (script, stdout) pairs extracted, kept only those
whose recorded stdout matched host bash 5.2 under `LANG=C.UTF-8`, and whose
expectation is representable in the spec format (1,044 cases).

- 624 pass in Bashkit and run in both the spec suite and the bash comparison gate.
- 420 are `### skip: TODO bashbox corpus gap` — Bashkit's stdout or exit status
  differs from real bash. These are bugs, not intentional divergences: fixing
  one means deleting its `skip` line (the comparison gate then guards it).
  Clusters: `declare`/`local`/attributes, `sort` and `xargs` flags, arithmetic on
  arrays, `declare -f`/`type` printing, named fds, ERR trap, `printf`, `tr`.

Do not re-import over existing files; fix cases in place. If an intentional
divergence is found, switch the case to `### bash_diff` with a canonical `L-*`
row in [Known Limitations](limitations.md).

## Debian-Oracle Scoreboard

`scripts/debian-oracle/run.sh` (`just debian-oracle`) runs the
[pseudo-linus](https://github.com/JoaoHenriqueBarbosa/pseudo-linus) test bench
(MIT) against Bashkit: about 9,600 command cases across 45 tools (awk, jq,
sed, grep, coreutils, ...) whose stdout, stderr, exit status and final file
tree were recorded on real Debian. The adapter in
`scripts/debian-oracle/adapter/` loads each fixture into the in-memory
filesystem and compares with the bench's own `harness::compare_outcome`.

- **Fetched, not vendored.** Parts of the bench corpus derive from GPL test
  suites (GNU grep/sed), so the script clones it at a pinned commit
  (`PSEUDO_LINUS_REV`) into `target/debian-oracle/` at run time. Bump the pin
  deliberately and refresh floors in the same change.
- **Floors.** `scripts/debian-oracle/floors.tsv` holds each tool's minimum
  lenient pass count. The run fails when a tool drops more than one case below
  its floor (one case of slack for wall-clock timeouts). After a fix raises a
  score, run `scripts/debian-oracle/run.sh --update-floors` and commit the
  floors so the gain is locked in.
- **CI.** `.github/workflows/debian-oracle.yml` runs on PRs touching
  builtins, the interpreter or the scoreboard, weekly, and on demand. The
  table goes to the job summary; failing cases are uploaded as an artifact.
- Goldens come from Debian's tool versions (jq 1.7.1, GNU grep 3.11, gawk);
  version-specific gaps (Bashkit targets jq 1.8) stay as failures rather than
  being special-cased.

## Bash-Oracle Scoreboard

`scripts/bash-oracle/run.py` (`just bash-oracle`) replays 106 whole-script
cases through the bashkit CLI and diffs each with output recorded from real
bash 5.2 in `scripts/bash-oracle/expected/`. Where the Debian-oracle
scoreboard scores individual coreutils tools, this one scores shell behavior:
redirection, traps, arithmetic, globbing, `set -euo pipefail`, xtrace,
process substitution, parameter expansion errors.

- **Recorded, not live.** The recordings are the oracle, so a run needs no bash
  on the machine and does not drift with the runner's bash version.
  `record.py` re-records; it keeps a case only when two runs in different empty
  directories agree and the output carries neither a host path nor the working
  directory, which keeps clock-, pid- and cwd-dependent cases out of the
  corpus. The cwd rule also excludes sandbox identity: bashkit's virtual
  filesystem starts in `/home/user` whatever the process cwd is, by design.
  For the same reason the recordings are made with bashkit's sandbox identity
  (`HOME=/home/sandbox`, `USER=sandbox`), so a gap is behavior, not identity.
- **Two scores.** `match` is status plus stdout, what a script observes, and the
  number the bash-parity work tracks (90/106 at the time of writing). `strict`
  also requires bash's stderr text (69/106); it trails behind because most
  builtin diagnostics still lack bash's `SCRIPT: line N:` prefix. `floor.txt` holds both as `<match> <strict>` and the
  run fails when either drops.
- **CI.** `.github/workflows/bash-oracle.yml` runs on PRs touching the
  interpreter, parser, builtins or the scoreboard, and on pushes to main.
- A case that newly matches raises the floor in the same change; a case
  representing a documented gap stays a gap rather than being deleted, so the
  corpus keeps measuring it (see [limitations](limitations.md)).

## Oils Spec Pass Rate

`scripts/oils-spec/run.py` (`just oils-spec`) runs the whole upstream Oils
spec suite (bash column, ~2,760 cases) through bashkit and real bash and
publishes one headline number, bashkit passes over real-bash passes. It is a
measurement, not a gate: no floor, no CI job. See
[Oils Spec Pass Rate](oils-spec.md).

## SQLite CLI Differential Tests

CSV comparisons explicitly set the host CLI row separator to LF. SQLite
shell releases differ in their default CSV row endings (LF versus CRLF);
field contents, including embedded newlines, remain compared byte-for-byte.

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
