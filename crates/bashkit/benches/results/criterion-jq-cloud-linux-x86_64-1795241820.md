# Criterion jq Builtin Benchmark

The benchmark measures jq through `Bash::exec`. Each iteration starts a new
execution in a warmed process. The repeated and distinct cases each run 64
commands over the same small input. Only the distinct case varies the filter
source. The aggregate case processes 4,096 JSON objects in one command.

Run with `cargo bench --locked -p bashkit --features jq --bench jq`.

## Initial comparison

Baseline `290ebb01` plus the benchmark harness, compared with request-local
compiled-filter reuse and process-wide reuse of the parsed jaq library
prelude. Both builds used Rust 1.100.0, thin LTO, and eight codegen units.
The Linux x86-64 runner used one pinned Xeon Platinum 8370C CPU, a one-second
warmup, a three-second measurement target, and 30 Criterion samples per case.
Times are the Criterion point estimate per script.

| Benchmark | Before | After | Change |
|---|---:|---:|---:|
| `jq/single_lookup` | 1.061 ms | 0.802 ms | -24.5% |
| `jq/repeated_filter` | 60.564 ms | 2.801 ms | -95.4% |
| `jq/distinct_filters` | 62.478 ms | 48.727 ms | -22.0% |
| `jq/aggregate_4096_rows` | 13.435 ms | 12.964 ms | -3.5% |

Criterion detected a change in the first three workloads. The 4,096-row
aggregate was within measurement noise (`p = 0.30`).

## Inventory fan-out

The archived warehouse fan-out script ran unchanged with in-process fixture
tools and 100 warehouses. The same CPU ran both builds in alternating order
with 100 timed samples per build per round across three rounds. Every run
matched the expected stdout and returned zero with empty stderr.

The median fell from 107.88 ms to 9.48 ms
(11.4×). The 95th percentile fell from
123.13 ms to 11.12 ms. The baseline interpreter
binary was built from `c30c8ba4`, which has the same Bashkit crate and lockfile
as `290ebb01`. Both fan-out binaries used the bench runner's release profile
with 16 codegen units.
