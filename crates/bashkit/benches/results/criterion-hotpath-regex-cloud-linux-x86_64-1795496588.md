# Conditional regex search reuse

Baseline: main `abc42a7c`. Candidate: bounded execution-local regex search
scratch, allocated on first use. Both use the default interpreter; the
experimental execution plan is disabled. Runtime source and binary hashes,
complete native rows and Criterion estimates are in [criterion-hotpath-regex-cloud-linux-x86_64-1795496588.json](criterion-hotpath-regex-cloud-linux-x86_64-1795496588.json).

## Existing code-mode tool adapter

Four alternating-order paired rounds, pinned to CPU 2, with no concurrent
builds or profilers. Each run measures at least 100,000 calls, 2,000 fresh
starts and 1,000 live sessions. Values below are medians of the four run
medians (memory uses the four session-growth results). Memory measures fresh
sessions after `echo $((1 + 1))`, before any regex is used.

The unchanged Bash adapter defines `run()`, captures each host JSON reply
with `$(call_tool ...)`, matches its ID using `[[ =~ ]]`, and checks
`BASH_REMATCH` through arithmetic. The ExecutionHandle host bridge, argument
and reply formats, function, loop, warmup and resource settings are unchanged.
This is direct Bash, with a registered host builtin. Build: Rust 1.100.0,
opt-level 3, fat LTO, one codegen unit; the same frozen external consumer lock
on both sides. Absolute numbers are specific to this Linux host, and are not
hardware-normalized comparisons with other engines' README results.

| Benchmark | Before | After | Change |
|---|---:|---:|---:|
| Existing adapter/tool call | 32.3439 µs | 21.0450 µs | -34.93% |
| Existing adapter/start | 58.4780 µs | 58.8225 µs | +0.59% |

Tool-call throughput is **1.537×** baseline.
Fresh session resident growth: 36651 →
36573 bytes/session. The startup difference here
is below 1%; it is not evidence of a startup speedup.

## Criterion baseline and comparison

The identical benchmark-only changes were copied into a detached `abc42a7c`
worktree for the before run. Both use the root lock and `[profile.bench]`
(fat LTO, one codegen unit), with default features and no execution plan.
All cases construct a fresh Bash and include parsing/execution. The two regex
cases also assert exact output and status. First saved baseline for the new
`conditional_regex` group. 100 samples, 3-second warmup and 5-second measurement
per case, before then after, pinned CPU 2. Table values are actual sample
median estimates, converted from nanoseconds.

| Benchmark | Before | After | Change |
|---|---:|---:|---:|
| `startup/empty` | 82.712 µs | 70.609 µs | -14.63% |
| `startup/echo_hi` | 79.049 µs | 73.553 µs | -6.95% |
| `startup/assign_echo` | 84.898 µs | 79.906 µs | -5.88% |
| `conditional_regex/capture_1k` | 17993.618 µs | 18356.118 µs | +2.01% |
| `conditional_regex/nonmatch_1k` | 4700.684 µs | 4461.569 µs | -5.09% |

The capture-loop mean estimates are 19.220 ms before and 19.075 ms after,
with overlapping 95% confidence intervals (18.671–19.846 ms and
18.472–19.885 ms). This does not establish a capture-loop improvement;
its sample median is 2.01% higher. Nonmatches improve about 5%. These
workspace microbenchmarks and the external consumer use different dependency
locks and workloads, so the 34.93% tool-loop result must not be generalized
to every regex script.

## Reproduction and scope

Run the original adapter with `taskset -c 2 bench-bashkit event-json` in four
alternating before/after pairs, using its unchanged common harness and lock.
The JSON's `original_adapter.reproducer_files` contains the full adapter,
common harness, manifests and frozen consumer lock. Extract those relative
paths into a fresh directory, replace `PATH_TO_BASHKIT` with the absolute
core-crate path for each revision, then build `candidate/Cargo.toml` with
`cargo build --release --locked`.
For Criterion, run each saved binary with
`--bench 'startup|conditional_regex' --noplot --save-baseline LABEL` and a
shared `CRITERION_HOME`. The adapter's lock differs from the workspace lock;
comparison pairs always share a lock and feature set.

Broader coverage uses `just bench`. Parallel coverage uses the built
`cargo bench -p bashkit --bench parallel_execution --no-run` binary with
`--bench`, then `scripts/bench-parallel.sh --dry` to save its cached output.
That run uses default core features, without unifying optional binding runtimes.
Their separate saved reports do not establish before/after speedups.

The fix retains immutable compiled programs under the existing entry/text
bounds and reuses one bounded search scratch buffer. The cache is local to an
execution, cleared before compilation misses and execution reset, and replaced
on pattern changes. Engine-reported retained scratch heap is capped at 64 KiB,
plus fixed metadata. No new dependency, global cache, Bash syntax, host
protocol or shared session state is introduced. Larger searches release
scratch after use; compilation and transient engine limits are unchanged.
