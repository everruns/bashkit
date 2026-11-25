# Validation record

## Rebase and review fixes (2026-11-24)

Runtime source: `cc622ef229eecd469e1068bbb87ae065e1bb8b81`, based on
`c7098c393a6957dbb8c9888b9fb9de9f6ffd42ff`. The later `8e739c58` test-only
change updates the exact work-counter expectation after opaque loops stopped
paying for rejected lowering. The compiler remains disabled by default.

- Revision `8e739c58` passed all 102 GitHub checks (101 successful, one expected
  skip), including feature-on/off workspace, security/failpoint, optional-runtime
  and discovered fuzz-target suites. [CI run](https://github.com/everruns/bashkit/actions/runs/45541437938).
- Local focused validation: 20 execution-plan unit tests, the serialized
  command-boundary failpoint parity regression, and two mixed-function
  integration tests pass. GNU Bash cases cover DEBUG/RETURN/ERR, suppressed and
  unsuppressed errexit, variable attributes, loop control and incoming status.
- Review-driven regressions first reproduced the wasted opaque-loop lowering
  and buffered final DEBUG-output ordering. Both fixes are verified; the latter
  fixes the shared ordinary/prepared executor and preserves budgeted accumulation.
- Local all-target/all-feature Clippy passes. Ordinary `just run-script` and the
  compiler-enabled CLI produce the expected `45` / `47` output for a function
  using substitutions, JSON regex captures and dynamically bound jq.
- Final clean browser runtime: 86 Node tests, 24/24 GNU Bash comparisons and
  all profile/budget checks pass. [Exact size/build identity](../../crates/bashkit-wasm/bench/results/wasm-mixed-plan-rebase-2026-11-24.json).
- Earlier local ASan checks on the initial rebase completed 192 script-plan
  differential inputs and 1,024 conditional-regex inputs without failures;
  final-source CI rebuilds and smokes all discovered targets in both modes.
- Fresh single-threaded measurements retain the unchanged original tool and
  fan-out scripts, identical lockfiles/profiles, four alternating rounds and
  CPU 2 affinity. Raw samples and identities are in [results/](results/).
- The saved broad suite completed all 96 cases with zero errors and 100% Bash
  output agreement. The full compiler-enabled Criterion parallel suite also
  completed through 1,000 sessions. Both reports and generated site data are
  retained in the standard result directories.

The local sandbox exposes synthetic ancestor `.git` directories under the
workspace and `/tmp`, affecting the real-ripgrep oracle. `/dev/shm` additionally
matches the host-mount policy's sensitive-prefix rule. Full-suite local reruns
use an explicitly writable `/var/tmp/bashkit-2983` fixture directory instead;
no test or security policy is relaxed.

## Original implementation (2026-11-23)

Implementation tested: `9cab5a45e22f6cf9dc8f6322078ae6087cd935a1`
(Rust sources; subsequent experiment documentation/results are separate).
Compiler remains disabled by default.

- `just pre-pr`: passed, including 7,503 workspace/unit/integration/doc tests,
  277 Python script tests, formatting, normal Clippy, documentation/capability/
  workflow/knowledge checks, fork consistency and cargo-vet.
- `just test`: passed the same 7,503 tests plus all 19 serialized security
  failpoint tests. The main suites include 4,000 core unit tests and 2,707
  integration tests. Expected platform/doc ignores remain unchanged.
- `cargo test -p bashkit --features failpoints,jq,experimental_plan_benchmark_control
  --lib --test security_failpoint_tests -- --test-threads=1`: passed all 3,549
  compiler-enabled unit tests and 19 security failpoint tests.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Final local browser package: 86/86 Node tests and 24/24 GNU Bash comparison
  cases. The normal 3.1 MB gzip cap is unchanged; the compiler profile has a
  separate 3.12 MB cap, with unchanged raw/Brotli caps. Both cap selection and
  enforcement are tested. The extra Binaryen pass was rejected after runtime
  probes; [measurements](../../crates/bashkit-wasm/bench/results/wasm-mixed-plan-2026-11-23.md)
  record its size/speed tradeoff.
- The earlier CI browser job [138051986432](https://github.com/everruns/bashkit/actions/runs/45277210089/job/138051986432)
  also passes streaming/main-thread/worker checks and optional embedded runtimes.
  That revision used the subsequently rejected extra optimization pass; final
  packaging keeps the ordinary optimization pipeline.
- A local ASan differential campaign at `b77b5203` completed 256 runs with seed
  `20261123`, eight structured seeds, maximum input length 512 and 30-second
  wall budget. It compares native and mixed-function execution with AST
  execution, including subsequent session use. The later Rust change normalizes
  the cold arithmetic compiler's source argument and outlines WASM visitors;
  final PR CI rebuilds and smokes all discovered fuzz targets in both modes.

Final-source CI fuzz build/smoke jobs also passed for both
[ordinary execution](https://github.com/everruns/bashkit/actions/runs/45277210089/job/138051986934)
and [execution plans](https://github.com/everruns/bashkit/actions/runs/45277210089/job/138051986327),
covering every manifest-discovered target.

Resource coverage includes compilation admission/work limits, command and loop
counts, live plan storage during callbacks, release after errors and cancellation,
function scopes, output/status, dynamically changed jq arguments/filter files,
unreachable filters and function shadowing. Tests also compare ordinary and
prepared loops/functions directly with GNU Bash.

Dependency policy: full local `cargo deny check` reports the baseline
unmaintained chrono/chrono-tz advisories (RUSTSEC-2026-0543 and
RUSTSEC-2026-0555), with no safe upgrade available in the existing graph.
Bans/licenses/sources and CI's existing Audit policy pass. No root dependency,
audit-policy exception or test limit was changed.

The 9cab5a45 PR revision finished all 104 checks: 103 successful and one
expected skip. Both native CI modes, all discovered fuzz targets, optional
runtimes, host-tool oracles, serialized failpoints and security properties pass.

Exploratory broad validation ran all 96 cases against Bash with zero errors
and 100% output match for both compiler revisions. The parallel-execution
Criterion quick comparison also completed. These non-publishing checks do not
update the site or establish tight performance confidence bounds. Repeated
large-case diagnostics used 40 samples, five warmups and four alternating
process rounds with CPU 2 affinity after unpinned runs showed host timing
clusters. The remaining short/fallback costs remain a default-enablement gate.
