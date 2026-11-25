# Criterion jq Experimental Owned Containers

Compares the established `jq` configuration with opt-in `experimental_jq`.
Both use jaq for parsing and evaluation. Scripts run through `Bash::exec`
and assert their outputs. The new update cases each build 4,096 entries.

Rust 1.100.0, Linux x86-64, repository bench profile (fat LTO, one codegen
unit), no default features. Each final run used 20 samples, a 0.3-second
warmup and a one-second measurement target. Builds and test runs completed
before the final paired measurements. This is an exploratory single pair;
it does not establish a performance improvement. Positive change is slower.

| Benchmark | Before | After | Change |
|---|---:|---:|---:|
| `jq/aggregate_4096_rows` | 13.240 ms | 13.351 ms | +0.8% |
| `jq/array_updates_4096` | 3.895 ms | 4.248 ms | +9.1% |
| `jq/distinct_filters` | 38.181 ms | 37.719 ms | -1.2% |
| `jq/hot_and_unique_filters` | 20.573 ms | 20.377 ms | -1.0% |
| `jq/interleaved_filters` | 4.161 ms | 3.877 ms | -6.8% |
| `jq/object_updates_4096` | 7.123 ms | 8.061 ms | +13.2% |
| `jq/repeated_filter` | 2.314 ms | 2.329 ms | +0.6% |
| `jq/single_lookup` | 0.580 ms | 0.577 ms | -0.6% |

The owned path adds lifecycle interruption, incremental container accounting,
pre-copy admission and retained iterator charges. Resource admission can
also reject earlier near the configured byte limit. Keep the experiment
disabled by default while reviewing compatibility, safety and performance.

Reproduce the two builds with `cargo bench -p bashkit --no-default-features
--features jq --bench jq` and the same command using `--features experimental_jq`.
The paired JSON includes the raw Criterion estimates for both configurations.
