# Wasm Coreutils Benchmark: uutils (wasm) vs native builtins

Commands through the bashkit interpreter, end to end. `wasm` runs uutils
0.12 compiled to wasm32-wasip1 as a WebAssembly guest on wasmtime's Pulley
interpreter (`coreutils <util>`); `native` is bashkit's own builtin. See
knowledge/runtimes/wasm-coreutils.md.

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1791511758

## Raw start (fresh OS process, 10 runs)

First `coreutils echo 1` in a new process, timed from `main` (includes
building `Bash` and the one-time engine and module load).

| Measure | Min | Median | Max |
|---------|-----|--------|-----|
| wasm, first call (fresh process) | 6.319 ms | 6.712 ms | 7.152 ms |
| wasm, second call | 2.313 ms | 2.340 ms | 2.444 ms |
| native builtin | 0.037 ms | 0.041 ms | 0.048 ms |

## Per call (warm session, small input)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| coreutils_call/native/cat_small | 25.364 µs |  |
| coreutils_call/native/echo | 10.484 µs |  |
| coreutils_call/native/pipeline | 105.04 µs |  |
| coreutils_call/native/sha256sum_small | 19.760 µs |  |
| coreutils_call/native/sort_small | 31.924 µs |  |
| coreutils_call/native/wc_small | 22.247 µs |  |
| coreutils_call/wasm/cat_small | 3.5380 ms |  |
| coreutils_call/wasm/echo | 2.3213 ms |  |
| coreutils_call/wasm/pipeline | 14.907 ms |  |
| coreutils_call/wasm/sha256sum_small | 3.9130 ms |  |
| coreutils_call/wasm/sort_small | 7.8351 ms |  |
| coreutils_call/wasm/wc_small | 3.8011 ms |  |

## Throughput (1 MiB input)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| coreutils_throughput/native/cat_1m | 1.9280 ms | 518.67 MiB/s |
| coreutils_throughput/native/sha256sum_1m | 5.3417 ms | 187.21 MiB/s |
| coreutils_throughput/native/sort_1m | 17.952 ms | 55.703 MiB/s |
| coreutils_throughput/native/wc_1m | 5.3562 ms | 186.70 MiB/s |
| coreutils_throughput/wasm/cat_1m | 6.0768 ms | 164.56 MiB/s |
| coreutils_throughput/wasm/sha256sum_1m | 194.14 ms | 5.1508 MiB/s |
| coreutils_throughput/wasm/sort_1m | 734.26 ms | 1.3619 MiB/s |
| coreutils_throughput/wasm/wc_1m | 224.14 ms | 4.4615 MiB/s |

## Per session (new Bash per call)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| coreutils_session/native/new_bash_sort | 90.495 µs |  |
| coreutils_session/wasm/new_bash_sort | 6.8056 ms |  |

## Parallel sessions (N concurrent, one call each)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| coreutils_parallel/native/1 | 176.52 µs | 5.6650 Kelem/s |
| coreutils_parallel/native/16 | 606.57 µs | 26.378 Kelem/s |
| coreutils_parallel/native/64 | 2.0958 ms | 30.537 Kelem/s |
| coreutils_parallel/wasm/1 | 11.058 ms | 90.431 elem/s |
| coreutils_parallel/wasm/16 | 49.841 ms | 321.02 elem/s |
| coreutils_parallel/wasm/64 | 198.96 ms | 321.67 elem/s |
