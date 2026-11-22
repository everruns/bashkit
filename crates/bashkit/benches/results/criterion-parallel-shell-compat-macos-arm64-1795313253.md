# Host shell compatibility: filesystem overhead

Local Criterion comparison on macOS arm64, Rust 1.100.0. Baseline is
`290ebb01` (main); candidate is the ownership/bracket-function/WASM-result
change. Both runs use the same target directory, default features, 10 samples,
1-second warmup and 1-second measurement. This is a narrow local comparison,
not a full parallel-scaling run; other validation builds were active.

```sh
cargo bench -p bashkit --bench parallel_execution -- single_file_write_read \
  --sample-size 10 --warm-up-time 1 --measurement-time 1 --save-baseline compat-before
cargo bench -p bashkit --bench parallel_execution -- single_file_write_read \
  --sample-size 10 --warm-up-time 1 --measurement-time 1 --baseline compat-before
```

| Benchmark | Before | After | Change |
| --- | --- | --- | --- |
| single_file_write_read | 41.961 µs | 42.349 µs | +0.6296% |

Candidate 95% interval: 42.268–42.441 µs. Relative-change 95% interval:
−0.2007% to +1.4212%.

Criterion reports no statistically significant performance change
(`p = 0.16`). This covers the metadata-layout change's basic VFS path; semantic
regressions are covered by the native specs, RealFs suite and WASM integration tests.
