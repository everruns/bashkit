# Hot-path Bench: Attributes + SHOPT Extensions

Criterion micro-benchmarks (`cargo bench --bench hotpath`) after adding the
`bench_attributes` group and extending `bench_shopt` to cover the parts of
the CoW/bitset perf rework (`10cd01d`) that had no direct coverage:
variable-attribute bitset (`declare -i/-u/-l/-r`, namerefs) and the
remaining `BashFlags` bits (`-x`, pipefail, `-a`, `expand_aliases`).

100 samples per case, default Criterion config. `[profile.bench]` =
release + lto=fat + codegen-units=1.

## New: `attributes/*`

| Case | Time (median) |
|---|---:|
| `integer_assign_1k` | 2.39 ms |
| `uppercase_attr_500` | 631 µs |
| `lowercase_attr_500` | 624 µs |
| `nameref_rw_500` | 996 µs |
| `readonly_reassign_attempts_500` | 484 µs |

## New: extra `shopt/*` cases

| Case | Time (median) | vs `plain_1k_loop` |
|---|---:|---|
| `plain_1k_loop` (existing) | 2.65 ms |, |
| `strict_mode_1k_loop` (existing) | 2.63 ms | flat |
| `xtrace_1k_loop` | 2.72 ms | +2.7% (bit-test on hot path is cheap) |
| `pipefail_pipeline_200` | 474 µs | n/a (different shape) |
| `allexport_assign_200` | 376 µs | n/a |
| `expand_aliases_500` | 2.64 ms | flat vs plain alias-less |

## Full hotpath table (all 32 cases for the record)

| Group / case | Time (median) |
|---|---:|
| `startup/empty` | 34.4 µs |
| `startup/echo_hi` | 35.6 µs |
| `startup/assign_echo` | 37.6 µs |
| `loops/for_range_1k_arith` | 2.64 ms |
| `loops/while_inc_1k` | 1.83 ms |
| `loops/for_list_100` | 229 µs |
| `loops/nested_for_50x50` | 5.64 ms |
| `variables/assign_200` | 293 µs |
| `variables/read_200` | 894 µs |
| `variables/local_in_function` | 1.17 ms |
| `attributes/integer_assign_1k` | 2.39 ms |
| `attributes/uppercase_attr_500` | 631 µs |
| `attributes/lowercase_attr_500` | 624 µs |
| `attributes/nameref_rw_500` | 996 µs |
| `attributes/readonly_reassign_attempts_500` | 484 µs |
| `cmdsubst/subst_simple_100` | 558 µs |
| `cmdsubst/subst_nested_3` | 50.2 µs |
| `cmdsubst/subst_with_vars_50` | 385 µs |
| `cmdsubst/subst_with_many_vars` | 414 µs |
| `shopt/strict_mode_1k_loop` | 2.63 ms |
| `shopt/plain_1k_loop` | 2.65 ms |
| `shopt/xtrace_1k_loop` | 2.72 ms |
| `shopt/pipefail_pipeline_200` | 474 µs |
| `shopt/allexport_assign_200` | 376 µs |
| `shopt/expand_aliases_500` | 2.64 ms |
| `pipelines/seq_grep_wc` | 87.3 µs |
| `pipelines/seq_sort_uniq` | 109 µs |
| `functions/call_500` | 2.58 ms |
| `functions/recursive_fib_10` | 2.38 ms |
| `param_exp/default_op_500` | 504 µs |
| `param_exp/substring_500` | 602 µs |
| `param_exp/uppercase_300` | 334 µs |
