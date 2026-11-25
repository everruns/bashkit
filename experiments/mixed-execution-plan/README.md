# Mixed execution-plan experiment

The disabled-by-default `experimental_execution_plan` feature prepares eligible
function bodies and control flow around interpreter-only operations. Scripts use
ordinary Bash syntax. Tool calls, pipelines, command substitutions, regexes and
array access retain the ordinary executor; supported arithmetic regions and
`for ((...))` clauses can compile independently.

The `jq` builtin already compiles filters with jaq and maintains a bounded,
execution-local LRU. Mixed plans use that same backend. Compilation remains lazy:
unreachable bad filters do not fail, and current arguments, files, environment
and function shadowing still apply. This change adds no persistent compiler cache
or new jq syntax.

## Reproduce

Use Rust 1.100.0 and an existing checkout of
[rivet-dev/code-mode-language-benchmark](https://github.com/rivet-dev/code-mode-language-benchmark).
The harness reads its fan-out script and fixtures from that checkout; it does not
modify them. These are exploratory comparisons and do not update `/benches`.

Same-binary interpreter/automatic-plan comparison:

```sh
CODE_MODE_BENCHMARK=/path/to/code-mode-language-benchmark \
  cargo run --locked --release \
  --manifest-path experiments/mixed-execution-plan/Cargo.toml > /tmp/mixed-loops.json
```

Both modes attach the same execution-local diagnostic extension. Each case uses
a fresh shell, excludes shell construction, and includes parsing, lowering and
execution. Four rounds alternate mode order; each round discards five warmups
and keeps 30 samples. Exact stdout, stderr and exit status are checked.
The six cases cover arithmetic inside a function, substitutions with regex/array
access, jq with changing bindings, the original fan-out script with 100 warehouses,
a short function and short arithmetic.

To compare the original event-JSON workload against the previous compiler,
create a separate baseline checkout at `c7098c393a6957dbb8c9888b9fb9de9f6ffd42ff`
and run:

```sh
python3 experiments/mixed-execution-plan/reproduce-original.py \
  --baseline /path/to/baseline --candidate "$PWD" \
  --benchmark /path/to/code-mode-language-benchmark \
  --output /tmp/mixed-original-new
```

The output directory must not exist. The helper builds both direct-Bash adapters
with the same adapter source, preserved dependency lock, Rust version and fat-LTO
profile. Both enable the experimental compiler. Four rounds alternate process
order; every run checks at least 100,000 measured calls, 1,000 memory sessions and
2,000 startup samples. The guest retains `$(call_tool ...)`, JSON text and
`BASH_REMATCH`; it does not use `ScriptedTool` or a typed return-value shortcut.

Run timings without competing builds. Same-binary diagnostic measurements and
separate production-baseline builds answer different questions and should both
be retained. The compiler remains experimental; gains on arithmetic loops alone
do not establish an end-to-end speedup for agent workloads.

## Rebase measurements (2026-11-24)

Runtime source `cc622ef2`, based on `c7098c39` (including #2984, #2996 and
#3002). Rust 1.100.0, Linux x86-64, four-CPU quota; single-threaded experiments
pinned to CPU 2. The same scripts, profiles, four alternating rounds and
fresh-shell methodology above were retained. Timings ran after builds and before
the final local test rerun, without competing builds or tests.

| Same-binary case | AST (ms) | Auto (ms) | Time change |
|---|---:|---:|---:|
| arithmetic-function | 19.3058 | 8.7628 | -54.6% |
| substitution-function | 17.7876 | 17.7411 | -0.3% |
| jq-function | 2.2571 | 2.1939 | -2.8% |
| fanout-100 | 7.9948 | 7.8886 | -1.3% |
| short-function | 0.0314 | 0.0317 | +0.8% |
| short-arithmetic | 0.0296 | 0.0299 | +1.1% |

The original event-JSON workload measured **20.3159 µs/call on the baseline
versus 20.4339 µs/call with mixed plans (+0.6%)**. Both production adapters use
the same frozen lockfile, fat-LTO profile and compiler feature. There is no clear
end-to-end tool-call gain. The arithmetic gain is substantial; the smaller
workload differences do not justify enabling the compiler by default. jq still
uses the same existing compiler and execution-local filter cache.

The rebase also preserves current output-memory accounting, skips lowering
wholly opaque loops, and fixes buffered final DEBUG-trap output order in the
shared arithmetic-loop executor. Raw samples and build identities are retained
in [results/](results/); earlier results below remain historical comparisons.

## Measured results (2026-11-23)

Linux x86-64, Xeon Platinum 8370C, Rust 1.100.0, four-CPU cgroup quota.
The table below pins the single-threaded process to CPU 2. Times are medians
of four round medians; negative time change means less elapsed time. Each
sample includes parsing, compilation and execution on a fresh shell.

| Same-binary case | AST (ms) | Auto (ms) | Time change |
|---|---:|---:|---:|
| arithmetic-function | 19.3967 | 9.0729 | -53.2% |
| substitution-function | 24.7907 | 23.0915 | -6.9% |
| jq-function | 2.3604 | 2.2936 | -2.8% |
| fanout-100 | 8.0037 | 8.3125 | +3.9% |
| short-function | 0.0309 | 0.0313 | +1.2% |
| short-arithmetic | 0.0294 | 0.0298 | +1.3% |

Arithmetic inside a function improves substantially. Tool/substitution and jq
results are smaller and noisy; jq filter compilation and JSON processing are
unchanged. Fan-out and short scripts still have overhead, so this is not a
case for enabling the compiler by default.

The separate production-baseline comparison uses the unchanged original
event-JSON adapter with the compiler enabled in both builds:

| Original tool workload | Previous compiler (µs/call) | Mixed compiler (µs/call) | Time change |
|---|---:|---:|---:|
| CPU 2 | 30.615 | 31.523 | +3.0% |
| Unpinned | 31.745 | 30.772 | -3.1% |

The sign reverses between affinity settings and individual rounds vary.
There is no clear end-to-end tool-call improvement. The original JSON text,
`$(...)` and regex/array validation remain intact.

Raw samples, per-run original outputs, build profiles, dependency-lock hashes
and binary identities are in [results/](results/). Both pinned and unpinned
runs are retained. All runs were serial, after task builds/tests had finished.
An initial unpinned broad-suite pass showed roughly 2× timing clusters in
both baseline and candidate; repeated fixed-CPU diagnostics did not reproduce
that large regression. Treat small differences as exploratory host measurements.
On Linux, prefix the reproduction commands with `taskset -c 2` for the pinned
configuration, choosing a permitted CPU on another host.

## Validation

The PR runs existing feature-on/off CI, including optional runtime bindings,
security/failpoints and discovered fuzz targets. Local validation includes
real-Bash differential scripts, plan storage and cancellation checks, an ASan
script-plan differential campaign, and browser package and compatibility tests.
The [validation record](validation.md) lists exact commands and results.
See the [design and limits](../../knowledge/foundations/script-execution-plan.md)
and [public activation guide](../../crates/bashkit/docs/compatibility.md#experimental-execution-plans).
