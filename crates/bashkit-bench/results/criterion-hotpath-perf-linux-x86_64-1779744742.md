# Hot-path Performance: Before / After

Criterion micro-benchmarks (`cargo bench --bench hotpath`).
Baseline saved BEFORE the perf changes, comparison shows AFTER.

| Bench | Before (µs) | After (µs) | Change |
|---|---:|---:|---:|
| cmdsubst/subst_nested_3 | 131.64 | 52.05 | **-60.5%** |
| cmdsubst/subst_simple_100 | 794.74 | 571.86 | **-28.0%** |
| cmdsubst/subst_with_many_vars | 586.23 | 426.15 | **-27.3%** |
| cmdsubst/subst_with_vars_50 | 588.00 | 399.92 | **-32.0%** |
| functions/call_500 | 3293.78 | 2686.13 | **-18.4%** |
| functions/recursive_fib_10 | 3002.32 | 2570.39 | **-14.4%** |
| loops/for_list_100 | 462.18 | 258.23 | **-44.1%** |
| loops/for_range_1k_arith | 4560.79 | 3040.68 | **-33.3%** |
| loops/nested_for_50x50 | 9740.39 | 6362.39 | **-34.7%** |
| loops/while_inc_1k | 3062.82 | 2111.25 | **-31.1%** |
| param_exp/default_op_500 | 1338.87 | 616.85 | **-53.9%** |
| param_exp/substring_500 | 1384.09 | 768.55 | **-44.5%** |
| param_exp/uppercase_300 | 837.59 | 395.38 | **-52.8%** |
| pipelines/seq_grep_wc | 177.51 | 91.61 | **-48.4%** |
| pipelines/seq_sort_uniq | 183.09 | 116.04 | **-36.6%** |
| shopt/plain_1k_loop | 4706.60 | 2959.84 | **-37.1%** |
| shopt/strict_mode_1k_loop | 4589.75 | 2956.80 | **-35.6%** |
| startup/assign_echo | 108.57 | 38.76 | **-64.3%** |
| startup/echo_hi | 99.81 | 36.47 | **-63.5%** |
| startup/empty | 100.86 | 35.40 | **-64.9%** |
| variables/assign_200 | 532.08 | 307.56 | **-42.2%** |
| variables/local_in_function | 1522.69 | 1225.45 | **-19.5%** |
| variables/read_200 | 1312.51 | 974.66 | **-25.7%** |

## Summary

- 23 cases benchmarked
- median change: **-36.6%**
- mean change:   **-39.7%**
- best:          **-64.9%** (startup/empty)
- worst:         **-14.4%** (functions/recursive_fib_10)
