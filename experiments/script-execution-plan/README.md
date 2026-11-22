# Whole-script execution-plan experiment

Isolated branch: `experiment/script-execution-plan`. Implementation commit:
`7b7225e7`. Baseline: `782a4c55` (the shipped stream/dispatch optimization).
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
| short_tool | 0.0163 | 0.0163 | -0.1% | +0.5% |
| short_arithmetic | 0.0431 | 0.0467 | -8.4% | -9.2% |
| straight_arithmetic | 0.2996 | 0.2883 | +3.8% | +2.2% |
| while_1000 | 3.8733 | 1.4595 | +62.3% | +61.3% |
| branch_loop | 6.6303 | 2.6471 | +60.1% | +59.5% |
| assignment_loop | 3.0171 | 2.2410 | +25.7% | +25.4% |
| pipeline_fallback | 2.5541 | 2.6231 | -2.7% | -2.2% |
| substitution_fallback | 1.6041 | 1.6443 | -2.5% | -2.0% |
| mixed_script | 3.9509 | 1.7694 | +55.2% | +54.3% |
| arithmetic_for_100 | 0.4205 | 0.2034 | +51.6% | +45.9% |
| arithmetic_for_1000 | 3.9119 | 1.7453 | +55.4% | +54.5% |
| arithmetic_for_5000 | 19.7788 | 9.1169 | +53.9% | +53.4% |
| existing_fanout | 8.1085 | 7.9425 | +2.0% | +2.0% |

Arithmetic-heavy loops improve substantially (roughly 2.1–2.7x faster). The
mixed script combines fallback assignment/output commands with a compiled
arithmetic region. Straight-line arithmetic gains are much smaller.
Short arithmetic and fallback pipeline/substitution cases show overhead. The
small fan-out difference is not a demonstrated general tool-orchestration win.
This is promising evidence for the architecture, not a reason to enable it by
default. Next gates are reducing one-shot compilation/fallback overhead and
expanding backend coverage to normal tool-containing control flow and word
expansions without weakening shell boundaries.

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
  --output /tmp/bashkit-script-plan-results
```

The output directory must be empty. The script creates isolated consumer
projects, builds both binaries with one shared lockfile/profile, checks their
behavior, and collects timings. The fixture checkout is linked into this
private directory; no third-party fixture source is vendored here. The timing
orchestrator uses Linux `taskset`; the Rust runner and differential cases can
also be run directly on another host. Reports from different hosts or compiler
profiles should not be compared as absolute latency measurements.
