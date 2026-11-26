# Criterion jq Paired Backend Benchmark

rustc 1.100.0 (0ef1bc386 2026-11-09); Linux x86_64; CPU affinity: 2.
3 alternating rounds, 20 samples, 0.3s warmup, 1.0s measurement target.
Bench profile overrides: `{}`.
Times are medians across round medians. Positive change means slower. Both backends assert identical output.

| Benchmark | Before | After | Change |
|---|---:|---:|---:|
| `jq/aggregate_4096_rows` | 11.064 ms | 11.474 ms | +3.7% |
| `jq/array_concat` | 1.169 ms | 1.311 ms | +12.1% |
| `jq/array_copy_on_write` | 3.000 ms | 2.999 ms | -0.0% |
| `jq/array_updates_4096` | 3.438 ms | 3.437 ms | -0.0% |
| `jq/distinct_filters` | 31.127 ms | 30.703 ms | -1.4% |
| `jq/hot_and_unique_filters` | 16.968 ms | 17.134 ms | +1.0% |
| `jq/interleaved_filters` | 3.353 ms | 3.411 ms | +1.7% |
| `jq/join_8192` | 60.806 ms | 60.556 ms | -0.4% |
| `jq/json_round_trip` | 2.572 ms | 2.579 ms | +0.3% |
| `jq/object_copy_on_write` | 8.027 ms | 7.822 ms | -2.6% |
| `jq/object_merge` | 2.740 ms | 2.752 ms | +0.4% |
| `jq/object_updates_4096` | 6.567 ms | 6.456 ms | -1.7% |
| `jq/repeated_filter` | 1.981 ms | 2.052 ms | +3.6% |
| `jq/single_lookup` | 0.501 ms | 0.496 ms | -1.1% |
| `jq/sort_unique` | 2.464 ms | 2.642 ms | +7.2% |

Before: `jq`; after: opt-in `experimental_jq`, both without default features.
All builds finish before measurement. The paired JSON retains every round's Criterion estimates.
Reproduce with `just bench-jq` (add `--quick` for thin-LTO development measurements).
