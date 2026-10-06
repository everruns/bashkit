# Python Runtimes Benchmark: CPython (wasm) vs Monty

`python3` through the bashkit interpreter, end to end. CPython 3.14 runs as a
WebAssembly guest on wasmtime's Pulley interpreter, from a pre-initialized
snapshot; Monty runs natively in-process. See
knowledge/runtimes/cpython-wasm.md.

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1791319327

## Raw start (fresh OS process, 10 runs)

First `python3 -c 'print(1)'` in a new process, timed from `main`
(includes building `Bash` and any one-time runtime load).

| Runtime | Measure | Min | Median | Max |
|---------|---------|-----|--------|-----|
| monty | first call (fresh process) | 0.330 ms | 0.369 ms | 0.595 ms |
| monty | second call | 0.035 ms | 0.038 ms | 0.069 ms |
| cpython | first call (fresh process) | 20.947 ms | 21.896 ms | 27.749 ms |
| cpython | second call | 5.682 ms | 6.239 ms | 7.849 ms |

## Per call (warm session)

| Benchmark | Time |
|-----------|------|
| python_call/monty/pass | 16.872 µs |
| python_call/monty/print | 15.414 µs |
| python_call/monty/json_roundtrip | 26.897 µs |
| python_call/monty/file_write_read | 41.716 µs |
| python_call/cpython/pass | 5.4560 ms |
| python_call/cpython/print | 6.3116 ms |
| python_call/cpython/json_roundtrip | 8.9680 ms |
| python_call/cpython/file_write_read | 13.095 ms |

## Per session (new Bash per call)

| Benchmark | Time |
|-----------|------|
| python_session/monty/new_bash_print | 68.699 µs |
| python_session/cpython/new_bash_print | 6.4656 ms |

## Compute (CPU-bound Python)

| Benchmark | Time |
|-----------|------|
| python_compute/monty/loop_sum_1e5 | 57.185 ms |
| python_compute/monty/fib_20 | 17.725 ms |
| python_compute/monty/string_build_1e4 | 7.6934 ms |
| python_compute/cpython/loop_sum_1e5 | 1.5575 s |
| python_compute/cpython/fib_20 | 76.010 ms |
| python_compute/cpython/string_build_1e4 | 230.49 ms |

## Parallel sessions (N concurrent, one call each)

| Benchmark | Time |
|-----------|------|
| python_parallel/monty/1 | 149.51 µs |
| python_parallel/monty/16 | 457.04 µs |
| python_parallel/monty/64 | 1.3613 ms |
| python_parallel/cpython/1 | 7.3690 ms |
| python_parallel/cpython/16 | 72.834 ms |
| python_parallel/cpython/64 | 290.82 ms |

## Load (CPython, concurrent tenants, 4 calls each, one process)

| Tenants | Calls | Wall (s) | Calls/s | p50 (ms) | p99 (ms) | max (ms) | Failures | Peak RSS (MB) |
|---------|-------|----------|---------|----------|----------|----------|----------|---------------|
| 1 | 4 | 0.09 | 42 | 19.66 | 36.68 | 36.68 | 0 | 50 |
| 16 | 64 | 0.74 | 87 | 163.71 | 281.40 | 286.94 | 0 | 182 |
| 64 | 256 | 2.76 | 93 | 548.42 | 935.22 | 937.04 | 0 | 619 |
| 256 | 1024 | 11.52 | 89 | 2517.48 | 4021.28 | 4022.99 | 0 | 2431 |
| 1024 | 4096 | 44.60 | 92 | 10100.95 | 14775.18 | 14933.32 | 0 | 4688 |
