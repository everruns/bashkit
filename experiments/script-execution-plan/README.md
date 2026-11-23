# Whole-script execution-plan experiment

Isolated branch: `experiment/script-execution-plan`. Initial implementation: `7b7225e7`; dispatch/allocation/policy refinements:
`15cd14b4`, `c1963b9c`, `36220a35`; diagnostic control: `d3280018`. Baseline: `782a4c55` (the shipped stream/dispatch optimization).
The `experimental_execution_plan` feature is off by default. This experiment
has not been merged into production.

The public `Bash::exec` entry point compiles an execution-owned plan. Supported
regions execute flat instructions for scalar arithmetic, numeric assignments,
branches and loops. Normal shell orchestration and fallback commands handle
other syntax, tool calls, redirects, traps and dynamic contexts. The source
needs no annotations or extra Bash syntax. Programs borrow one script and do
not survive its execution; there is no shared compilation cache.

Example supported region in an ordinary mixed script:

```bash
label=total
total=0
for ((i=0;i<1000;i++)); do
    if ((i&1)); then
        ((total+=2))
    else
        ((total+=i))
    fi
done
printf '%s %s\n' "$label" "$total"
```

See the [design and eligibility limits](../../knowledge/foundations/script-execution-plan.md)
and [implementation](../../crates/bashkit/src/interpreter/script_plan.rs).

## Results

Five alternating rounds per workload/build. Each process warms up 10 fresh
Bash instances and measures 60 more. Values are medians of the five per-round
medians. All 7,800 measured executions assert exact stdout, stderr and status.
Runs were pinned to one Linux CPU after local builds finished. Timings use the
system allocator without allocation-count instrumentation.

Both binaries use Rust 1.100.0, identical dependency lockfiles, default library
features plus jq, and a standalone release profile with 16 codegen units and
LTO disabled. The candidate additionally enables the experimental feature.
This measures the complete `Bash::exec` call: parsing, compilation, execution
and output collection. The last column also includes construction of a fresh
Bash instance and custom fixture builtin registration where applicable.
Positive percentages mean less time; negative percentages mean more time.

| Workload | Baseline exec (ms) | Plan exec (ms) | Exec reduction | Construction + exec reduction |
|---|---:|---:|---:|---:|
| short_tool | 0.0161 | 0.0160 | +0.1% | -1.0% |
| short_arithmetic | 0.0283 | 0.0282 | +0.4% | -1.3% |
| straight_arithmetic | 0.3064 | 0.2698 | +12.0% | +7.7% |
| while_1000 | 3.9890 | 1.4848 | +62.8% | +61.7% |
| branch_loop | 6.9396 | 2.6939 | +61.2% | +60.6% |
| assignment_loop | 3.1261 | 2.3165 | +25.9% | +25.3% |
| pipeline_fallback | 2.6579 | 2.6646 | -0.3% | -0.2% |
| substitution_fallback | 1.6670 | 1.7337 | -4.0% | -4.2% |
| mixed_script | 4.0972 | 1.8244 | +55.5% | +54.6% |
| arithmetic_for_100 | 0.4347 | 0.2061 | +52.6% | +45.5% |
| arithmetic_for_1000 | 4.1152 | 1.8058 | +56.1% | +55.2% |
| arithmetic_for_5000 | 20.4192 | 8.8743 | +56.5% | +56.3% |
| existing_fanout | 8.2532 | 8.1651 | +1.1% | +1.1% |

The previous short-arithmetic penalty is gone in this comparison: small cold
scripts skip lowering. Straight-line arithmetic now takes about 12% less time;
supported arithmetic/branch loops take 26–63% less. Pipelines are approximately
flat. Substitutions remain slower in separate builds: 4.0% in this matrix and
3.7% in a prior 20-round focused repeat. This remaining regression is a production
gate, even with unchanged fallback allocation counts. The small fan-out gain
is not a demonstrated general tool-orchestration win. The feature stays off
by default.

The original experiment's samples are preserved in [results/v1](results/v1/).
Compare builds within each matched run; absolute latency across these separate
sessions is not a measure of the refinements alone.

## What changed

Ordinary commands and lists now keep their own async futures. Compiled dispatch
uses separate futures, sharing the same command-boundary/list-orchestration
macros. This removes the prepared-program payload from every fallback command.
The script keeps a boxed optional plan.

Native regions now use ranges in one instruction arena and one growable resource
lease. Arithmetic parsing borrows source text and moves owned token names.
Small cold scripts skip lowering after a cheap AST probe; loops and larger
straight-line blocks still compile. All state belongs to one execution. There
is no global cache or persistent hotness tracker, and no Bash syntax change.

A buffer-drop regression test exposed an early lease release on failed lowering.
The builder now drops its buffers before releasing their memory reservation.

## Allocation profile

Allocation counts are collected in separate binaries with a benchmark-only
counting allocator. They count allocation/reallocation calls and total requested
bytes during `Bash::exec`, excluding Bash construction; bytes are not peak/live
memory. Timing results above use uninstrumented binaries.

| Workload | Baseline calls | Plan calls | Baseline requested bytes | Plan requested bytes |
|---|---:|---:|---:|---:|
| short_tool | 85 | 85 | 23,432 | 23,440 |
| short_arithmetic | 168 | 168 | 44,964 | 44,972 |
| straight_arithmetic | 3,344 | 2,554 | 909,298 | 533,118 |
| pipeline_fallback | 33,230 | 33,230 | 5,890,231 | 5,890,239 |
| substitution_fallback | 18,583 | 18,583 | 4,646,593 | 4,646,601 |
| arithmetic_for_1000 | 56,202 | 25,207 | 8,917,649 | 1,333,565 |

Fallback dispatch no longer enlarges every command allocation. Only eight
extra requested bytes remain per execution. Sharing the arena also removes hundreds
of compiler allocations in a script with many separate arithmetic statements.

## Same-binary diagnostic

Positive changes below mean slower automatic admission; negative changes mean
faster. These timings include the matched diagnostic extension in both modes.

| Workload | Interpreter (µs) | Auto (µs) | Auto time change | Per-process change range |
|---|---:|---:|---:|---:|
| short_tool | 16.596 | 16.591 | -0.03% | -0.27% to +0.28% |
| short_arithmetic | 29.331 | 29.562 | +0.79% | +0.39% to +0.94% |
| straight_arithmetic | 313.188 | 274.923 | -12.22% | -13.68% to -9.77% |
| pipeline_fallback | 2850.398 | 2867.340 | +0.59% | -2.23% to +0.61% |
| substitution_fallback | 1752.316 | 1747.843 | -0.26% | -1.54% to +1.38% |
| colon_fallback | 4537.475 | 4505.358 | -0.71% | -1.08% to +1.15% |
| substitution_unrolled | 1991.370 | 1975.273 | -0.81% | -1.32% to +0.98% |

All 30,000 measured executions assert exact output/status. Cold arithmetic has
less than 1% admission-probe overhead in this control, compared with the original
8% separate-build penalty. The substitution penalty is not reproduced by merely
enabling plan construction inside one binary. This narrows the investigation;
it does not establish why production and experimental builds differ or clear
the remaining 4% regression.

TODO(experiment): isolate the ordinary script runner's async frame and examine
the compiled fallback path before enabling this feature by default. The current
script future still carries an optional plan even when admission declines;
command and list futures have already been separated. Same-binary controls
cannot measure overhead caused by enabling the feature in the build itself.

The extra `same_binary` harness feature enables a separate diagnostic library
feature. Both modes attach the same typed per-execution extension and alternate
order inside one process: interpreter-only versus automatic plan admission.
Each of five processes measures 200 pairs after ten warmup pairs per workload.
Normal experimental builds contain no control lookup. The control uses no
shared flag or compiler state. It helps isolate runtime plan costs from other
build differences; it does not replace the production-baseline comparison.

This exploratory harness does not update the performance site. Raw samples
and binary/lockfile hashes are in [results](results/).
The existing fan-out script and inventory fixture come from
[code-mode-language-benchmark](https://github.com/rivet-dev/code-mode-language-benchmark)
commit `258b364ed8c3281667149f7beed69b3d28c1e1b3`. The fixture is expanded to
100 warehouses; list/get calls are in-process Rust builtins returning JSON text.
The experiment does not choose or invoke an LLM.

## Correctness

The public-API differential runner matches all 199 cases against the current
interpreter. It includes changing bounds, nested loops/branches, output/status,
variable attributes, arrays, namerefs, aliases, traps, shell options, eval/source,
functions, background jobs, pipes/substitutions, resource rejection and session
reuse. Narrow resource probes require rejection in both builds while permitting
diagnostic quantities to differ: compilation consumes additional budget.

A GNU Bash oracle comparison matched 162 of 163 successful, untraced cases.
The remaining case is an existing baseline gap, reproduced identically by both
binaries: `x=010; ((x+=1)); echo "$x"` prints `11` in Bashkit and `9` in Bash.
It is recorded in `results/bash-oracle-results.json`; this experiment does not
claim to fix that pre-existing octal-variable behavior.

Native-specific unit tests require the VM to be selected and compare variables,
ordered arithmetic writes, exit status, PIPESTATUS and command charges with AST
execution. Error tests check loop-scope restoration. Full integration/resource
validation and Clippy results are recorded in `results/validation.txt`.

## Reproduce

Use two checkouts: baseline commit `782a4c55` and the experiment branch. Also
check out the external benchmark repository at the revision above. With Rust
available, run:

```bash
python3 experiments/script-execution-plan/reproduce.py \
  --baseline-repo /path/to/baseline \
  --candidate-repo /path/to/experiment \
  --benchmark-repo /path/to/code-mode-language-benchmark \
  --output /tmp/bashkit-script-plan-results \
  --allocation-profile --same-binary
```

The output directory must be empty. The script creates isolated consumer
projects, builds both binaries with one shared lockfile/profile, checks their
behavior, and collects timings. Optional flags also build separate allocation-profile
binaries and run the same-binary diagnostic. The fixture checkout is linked into this
private directory; no third-party fixture source is vendored here. The timing
orchestrator uses Linux `taskset`; the Rust runner and differential cases can
also be run directly on another host. Reports from different hosts or compiler
profiles should not be compared as absolute latency measurements.
