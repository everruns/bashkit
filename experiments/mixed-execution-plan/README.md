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
create a separate baseline checkout at `abc42a7cd4b5ef0af7f60002fda698517a9db417`
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
