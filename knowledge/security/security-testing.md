---
type: Test Strategy
title: Security Testing
description: Fail-point injection and layered security regression testing strategy.
tags:
  - bashkit
  - testing
  - security
---

# Security Testing with Fail Points

## Status
Implemented

## Overview

Bashkit uses [fail-rs](https://github.com/tikv/fail-rs) for fault injection
security testing of error handling paths and resource limit enforcement.
Fail points are disabled by default, zero runtime overhead when disabled.

```bash
cargo test --features failpoints security_ -- --test-threads=1
```

## Execution-plan coverage

Security suites run with the experimental compiler enabled as well as disabled.
Resource failpoints are honored inside native regions. Only an active
`interp::execute_command` injector forces AST admission, preserving its
whole-command early-error semantics. Native unit tests assert admission before
injecting a loop-counter reset and verify that the command budget still stops
the loop. Separate tests cover dropped futures, reuse, optional-memory decline,
work-budget failure and reservation refunds. `script_plan_fuzz` compares bounded
native arithmetic/branch loops and mixed functions containing substitutions,
regex captures and local variables with the AST executor. Structured proptest
cases and mixed-function fixtures also compare with real Bash. Mixed-control
regressions require prepared backend selection, exhaust the live-byte budget
with loop/condition output, and verify storage refunds and loop-frame cleanup
in both dispatch modes. Function capture-limit tests preserve complete streams
for substitutions and redirects while truncating captured UTF-8 bytes. See
[Execution Plans](../foundations/script-execution-plan.md).

## Conditional regex search reuse

`builtins::search_common::tests` compares cached search captures with the string
`regex` API, including UTF-8 boundaries, optional groups, invalid patterns,
pattern switches and eviction/recompilation. A bounded nullable pattern checks
that search scratch over 64 KiB is released without discarding its compiled
program. `conditional-regex-reuse.test.sh` compares ordinary captures, nonmatches,
UTF-8, `nocasematch` and invalid statuses with real Bash. The discovered
`conditional_regex_fuzz` target compares cold, repeated and switched searches
through `Bash::exec`, with at most 512 bytes of quoted pattern/haystack data;
nightly fuzzing covers both interpreter and execution-plan configurations.
See [Architecture](../foundations/architecture.md#conditional-regex-searches)
and TM-DOS-023 in the [Threat Model](threat-model.md).

## jq generated structure depth

The opt-in `experimental_jq` owned-container runtime runs the same jq/yq
compatibility and resource regression suites. Its additional API tests cover
pre-copy COW admission, refused child attachment, exact depth invalidation,
incremental charging and drop refunds. Its mutation differential corpus uses
real jq as the oracle, with memory/depth limits as deliberate exceptions.
See [Vendored jaq-json](../runtimes/jaq-json-vendor.md#experimental-owned-boundary).

`jq_lifecycle_fuzz` supplies valid construction, mutation, COW, merge,
comparison, formatting and cleanup cases to both storage backends and both
shell execution modes. Every input ends with a recovery probe on the same
interpreter. Native model properties compare checked mutations with Rust
containers and require full ledger refunds. Nightly Miri checks admission,
unwind/refund and owning-iterator lifetimes; ASan executes the jq/yq suites.

Both backends must interrupt exponential traversal of depth-bounded shared
trees. Small deterministic polling tests cover equality, ordering, hashing,
containment, byte conversion and formatting. A supervised 60-level DAG test
also verifies the real deadline and subsequent recovery. The required native
matrix runs these tests on Linux, macOS and Windows with pinned jq 1.8.1 as
the compatibility oracle. See [Native jq backend coverage](../operations/testing.md#native-jq-backend-coverage).

`memory_growth_security_tests::jq` checks depth 100/101 independently of heap,
output and evaluator recursion limits. It covers new containers, index updates,
slice updates, shared subtrees, removal/COW cache invalidation, `fromjson`,
sticky failure and session recovery. Python's `test_jq_depth.py` runs #3013's
100,000-step filter and variants in timed child processes, so a native stack
overflow fails the test without killing pytest. Keep pre-fix probes bounded;
deep crash regressions belong in supervised children. `jq_fuzz` synthesizes
short reductions around the depth boundary, since random source mutation
rarely generates valid filters that amplify runtime structure. The Rust suite
replays the same full-scale filters in
`jq_value_depth_tests::generated_depth_bounded` (supervised child, 2 MiB
stack), and `container_mutation_sites_are_reviewed` fails when a new
container construction or mutation site appears under `builtins/jq`. Review each
recursive lifecycle phase, including destruction while unwinding; panic
catching alone is not a stack-overflow boundary. See
[Vendored jaq-json](../runtimes/jaq-json-vendor.md#structural-depth-3013).

## Available Fail Points

### Resource Limits (`limits.rs`)

| Fail Point | Actions | Security Test Purpose |
|------------|---------|----------------------|
| `limits::tick_command` | `skip_increment`, `force_overflow`, `corrupt_high` | Test command limit bypass resistance |
| `limits::tick_loop` | `skip_check`, `reset_counter` | Test loop limit bypass resistance |
| `limits::push_function` | `skip_check`, `corrupt_depth` | Test recursion limit bypass resistance |

### Filesystem (`fs/memory.rs`)

| Fail Point | Actions | Security Test Purpose |
|------------|---------|----------------------|
| `fs::read_file` | `io_error`, `permission_denied`, `corrupt_data` | Test read failure handling |
| `fs::write_file` | `io_error`, `disk_full`, `permission_denied`, `partial_write` | Test write failure handling |

### Interpreter (`interpreter/mod.rs`)

| Fail Point | Actions | Security Test Purpose |
|------------|---------|----------------------|
| `interp::execute_command` | `panic`, `error`, `exit_nonzero` | Test command execution failure handling |

### yq atomic replacement (`builtins/yq.rs`)

| Fail Point | Actions | Security Test Purpose |
|------------|---------|----------------------|
| `yq::temp_allocate` | `exhausted` | Source preservation when no sibling temporary can be allocated |
| `yq::temp_chmod` | `error` | Temporary cleanup and source preservation on mode-copy failure |
| `yq::temp_rename` | `error` | Temporary cleanup and source preservation before atomic replacement |
| `sed::temp_allocate` | `exhausted` | `sed -i` source preservation when no sibling temporary can be allocated |
| `sed::temp_chmod` | `error` | `sed -i` temporary cleanup and source preservation on mode-copy failure |
| `sed::temp_rename` | `error` | `sed -i` temporary cleanup and source preservation before atomic replacement |

The existing `fs::write_file` actions cover yq temporary-write failures,
including the partial-write class. `security_yq_inplace_*` asserts original
bytes and absence of `.bashkit-yq-*` files after every injected stage.

## Usage

In tests: `fail::cfg("limits::tick_command", "return(skip_increment)")` before,
`fail::cfg("name", "off")` after. Action syntax (return/panic/sleep/pause,
probability `10%`, count `5*`): see [fail crate docs](https://docs.rs/fail).

## Security Test Categories

1. **Resource limit bypass**: counter corruption, check skipping, overflow/underflow
2. **Filesystem failure**: I/O errors, permission denied, disk full, partial writes, data corruption
3. **Interpreter failure**: execution errors, panic recovery, unexpected exit codes

## dd GNU Regression Oracle

`dd_tests` compares only tiny ordinary copies against a GNU dd binary. Discovery
requires a successful `--version` and recognizes the `dd (coreutils)` package
banner as well as `dd (GNU coreutils)`. GNU dd's
[version-option code](https://github.com/coreutils/coreutils/blob/v9.4/src/dd.c#L2288)
uses the package name; requiring the longer phrase anywhere in the output rejects
GNU versions without a matching footer. The inert banner fixture also rejects
uutils, BSD, and unrelated programs. Linux CI must find GNU dd; all byte, stderr,
and exit-status assertions remain mandatory. See [Builtins](../foundations/builtins.md).

## od Width Regression Oracle

`od_resource_tests` compares valid-width layouts with GNU `od` when available
(required in Linux CI). Zero and type-misaligned widths are asserted directly
against Bashkit's existing type-size fallback. Some GNU versions abort on
`-w0`, so an invalid-width GNU subprocess cannot serve as a compatibility
oracle; see the [GNU report](https://lists.gnu.org/archive/html/bug-coreutils/2025-06/msg00014.html).
The enormous-width tests invoke only the fixed builtin, after safe bounded
regressions established the allocation path.

## Function Definition Metadata

`function_metadata_security_tests` uses small function budgets and at most a few
KiB of padded source operands. It covers per-function and aggregate filename
charges, exact byte boundaries, failed redefinition atomicity, replacement and
unset refunds, filename rollback in subshells/substitutions/pipelines/background
jobs/executable VFS scripts, and removal of old metadata on shell-state restore. The interpreter unit
test `function_metadata_shares_storage_and_drops_with_functions` proves definitions
from one source share their filename allocation and that rollback/unset leave no
orphan entries or retained filename allocation. No host-exhaustion probe is needed.
The real-Bash comparison sources both the original and post-unset replacement;
Bash versions differ in the filename they assign to a definition inside `-c`.

## Arithmetic Associative-Key Budgets

`arithmetic_resource_tests` exercises the public `Bash::exec` path with keys
bounded to a few KiB and small live-memory/work budgets. It covers repeated
`$name` and `${name}` through `((...))`, quoted `let`, and `$((...))`, reads,
assignment/compound assignment/increment/decrement, recursive key scanning,
namerefs, 64 KiB key boundaries, and session recovery after rejection. Finite
valid-key cases compare stdout and status with Bash >= 4 (associative arrays
are unavailable in macOS's stock Bash 3.2; Linux CI supplies Bash 5.x).
The interpreter unit test checks the exact UTF-8 append boundary and a
cancellation checkpoint without growing a buffer after rejection. Resource
probes run in externally supervised subprocesses; full memory-exhaustion
payloads are unnecessary.

## Pipeline admission fixtures

`pipeline_admission_*` unit fixtures use ordinary two-stage no-output commands,
one occupied job slot, zero storage admission, and an inert environment value.
They assert typed errors, cancellation before construction, atomic shared job
admission, and lease/slot refunds. `pipeline_admission_tests` compares a tiny
export-isolation pipeline with real Bash and covers sequential zero-slot and
lastpipe behavior. These fixtures need no large exported values or fan-out.

## Make Substitution Admission

`builtins::make::expand::tests::subst_*` covers checked result-size arithmetic,
remaining allowance, non-overlapping Unicode/empty-pattern semantics, and tiny
shared live-memory/work/cancellation budgets. Near-cap tests modify only a
numeric counter; the rejected result would contain four bytes. Overflow tests
operate on lengths without allocating them. Parser ownership tests retain two
four-byte results and verify shared clones and final lease release.
`make_substitution_tests` exercises ordinary immediate/recursive/append
assignments and recipes through `Bash::exec`. No allocation or recursive
amplification reproducer is needed for these admission contracts.
See [Builtins](../foundations/builtins.md) and TM-DOS-126 in the
[Threat Model](threat-model.md).

## Standard-stream capture admission

`fs::std_streams::tests` uses at most a few bytes per buffer. It checks
shared capacity admission before growth, alias/write/append atomicity,
empty-write work and cancellation, retained collection byte/text leases,
lossy UTF-8 boundaries, stdin copy/container ownership, and temporary-copy
admission with ordinary filesystem pass-through. Zero device-copy admission
checks only a numeric read ceiling and never loads device bytes. Its
`windows_containment_` name also includes it in the existing Windows job; the
assertion requires the pre-read admission error for direct and absolute-symlink
sources. The numeric
`budgeted_vec_length_cap_also_bounds_geometric_capacity` fixture checks a
five-byte ceiling without a large allocation. `dev_fd_path_tests` adds
zero/three-byte admission configurations, session recovery, discarded
redirects, and a two-byte alias/append comparison with real Bash.
No amplified-output or host-exhaustion workload is required.

## Streaming boundary regressions

`streaming_audit_tests` uses a producer emitting 1 KiB stack chunks, a 16 KiB
aggregate input budget, and a 64 KiB live-memory budget. It checks builtin
stream readers, delimiter-free records and whole-input collectors without host
exhaustion workloads, plus recovery after refusal. Gated producers prove
redirected filters deliver before producer completion. Saved-fd tests cover
stdout/stderr, ordering, output caps and failed sinks; byte substitution and
redirect permutations compare against real Bash. These complement the existing
`streaming_command_tests` cancellation/revocation and pipe backpressure tests.

## Adding New Fail Points

Add `fail_point!("module::function", |action| ...)` under
`#[cfg(feature = "failpoints")]` at the critical location, document it in this
spec, and add tests in `tests/security_failpoint_tests.rs`.

## Best Practices

1. **Always clean up**: Call `fail::cfg("name", "off")` after tests.
2. **Single-threaded tests**: Use `--test-threads=1` for fail point tests due to global state.
3. **Document actions**: List all supported actions in code comments and this spec.
4. **Test both paths**: Test that fail points affect behavior AND that normal operation works without them.

## Builtin diagnostics and fuzzing

For TM-INF-022, use `Display` or a domain formatter such as
`format_compile_errors` in `builtins/jq/errors.rs`. Cap diagnostics at 1 KB.
The builtin source scan rejects Debug formats. Test-only Debug formatting
requires a per-line `// debug-ok: <reason>`.

Add a `no_leak_*` regression test for each new library-wrapping builtin,
using `bashkit::testing::assert_no_leak` with malformed input. Run fuzz
cases through `bashkit::testing::fuzz_exec` so they check output invariants.
See the [threat model](threat-model.md) and the `bashkit::testing` rustdoc.

A banned shape the executed script itself spells is an echo, not a leak, and
`assert_fuzz_invariants` skips it for that script only (`script_explains`).
A diagnostic that quotes its operand therefore needs no new strip template.
The echoed operand is a substring of the script and the diagnostic frames it
with its own punctuation, so attribution decomposes the *pattern*: it drops a
leading fragment the template's `: ` lead could supply and a trailing fragment
its `: ` trail could supply, then asks whether the remaining core is script
text. That is what excuses ` { code:` in `awk: cmd. line:1: { code:…` and in
`printf: { code:: invalid number`. Only the shapes starting with a space or
ending in a colon have a fragment to drop; the rest stay absolute, and an
empty core is never accepted, so a builtin format string that spells a shape
itself still fails.
Attribution compares against the raw script, so a lexer that rewrites word
text breaks it: dropped `\x1e`/`\x1f` bytes once turned
`tomlq 'target/debug/\x1f\x1f\x1fdeps/'` into a diagnostic quoting the banned
host path `target/debug/deps/`. See the
[parser design](../foundations/parser.md) on marker bytes in script text.
`assert_no_leak` has no script to attribute against and stays absolute, so
hand-written `no_leak_*` cases must not put a banned shape in their input.
Replays of the inputs that turned nightly fuzz runs red live in
`crates/bashkit/tests/fuzz_crash_regressions.rs`.

Attribution also looks through the characters a parser on the way *deletes*
from the data it carries (`DROPPED_BY_PARSERS`): NUL, which shell word
expansion drops, and `"`/`'`/`\`, which shell quote removal and the `csv`
field parser drop. A needle spliced with one of those still reaches the output
intact, so the script has to be matched the same way -- run 45932089002 flagged
the host canary from a `csv` field spelled `..._D"O_NOT_LEAK`.

Keep the three stderr budgets distinct, and do not let one constant serve two
of them. "Cap diagnostics at 1 KB" above is the bound on a *single* diagnostic
(L-ARITH-002). `MAX_STDERR_BYTES` (1 KiB) is the per-call budget for the strict
path, `assert_no_leak`, whose cases choose their own input. The fuzz path uses
`MAX_FUZZ_STDERR_BYTES` (4 KiB), the largest
`ExecutionLimits::max_stderr_bytes` any target configures, because a fuzz
target inlines bytes it did not choose and the engine emits up to the limit
that target declared — so a tighter harness figure there fails a legitimately
chatty script rather than catching anything. Run 45932089002 hit this on
`printf_fuzz`, where one `invalid number` diagnostic per operand came to 1,283
bytes.

Raising a single shared constant is the trap: it moves the strict cap too,
which is why these are now separate and
`loosening_the_fuzz_cap_did_not_loosen_the_strict_path` guards the split. The
eight targets that configure no limit take the 1 MiB default, so for them the
4 KiB harness figure is the operative bound rather than a satisfiable one — if
one of them ever emits legitimately more, give it an explicit
`max_stderr_bytes` rather than raising the harness cap.

## Filesystem Certification

The backend-neutral helper in
`crates/bashkit/tests/support/filesystem_security_conformance.rs` certifies
binary/null content, unsafe pathname rejection, root-clamped canonical identity,
and normalized public error kinds. The integration and RealFs feature suites run
the same helper against every production storage adapter. Wrapper cases cover
symlink identity, read-only policy, mount boundaries, transactional failures,
quota retention, and tar preflight. `security_fs_failed_writes_are_atomic` runs
all `fs::write_file` failpoint actions and proves prior bytes and usage survive.

## JavaScript Security Tests

The JavaScript/TypeScript bindings have a dedicated security test suite at
`crates/bashkit-js/__test__/security.spec.ts` (run: `cd crates/bashkit-js &&
pnpm test`). White-box and black-box scenarios across 18 categories:

1. Resource limit enforcement (TM-DOS)
2. Output truncation (TM-DOS)
3. Sandbox escape prevention (TM-ESC)
4. VFS security, path traversal, file count, nesting, filename limits (TM-DOS, TM-INJ)
5. Instance isolation (TM-ISO)
6. Error message safety (TM-INT)
7. TypeScript wrapper injection prevention (TM-INJ)
8. Adversarial script inputs, null bytes, deep nesting, expansion bombs
9. Unicode & encoding attacks (TM-UNI)
10. Injection via constructor options (TM-INJ)
11. Concurrency & cancellation (TM-DOS)
12. Async API security
13. BashTool metadata safety
14. Bash feature abuse, traps, special variables, /dev/tcp
15. Mounted files security
16. Rapid instance creation/destruction
17. Edge case inputs
18. Async factory security

## Related Files

- `crates/bashkit/tests/security_failpoint_tests.rs` - Fail-point security tests
- `crates/bashkit/tests/threat_model_tests.rs` - Threat model tests (51 tests)
- `crates/bashkit/tests/builtin_error_security_tests.rs` - Builtin error security tests (39 tests, includes TM-INT-003)
- `crates/bashkit-js/__test__/security.spec.ts` - JavaScript security tests (90+ tests, 18 categories)
- `crates/bashkit/src/limits.rs` - Resource limit fail points
- `crates/bashkit/src/fs/memory.rs` - Filesystem fail points
- `crates/bashkit/src/interpreter/mod.rs` - Interpreter fail points, panic catching
- `crates/bashkit/src/builtins/system.rs` - Hardcoded system builtins
- [Threat Model](threat-model.md) - Threat model specification
