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
- **Timestamp**: 1791345434

## Raw start (fresh OS process, 10 runs)

First `python3 -c 'print(1)'` in a new process, timed from `main`
(includes building `Bash` and any one-time runtime load).

| Runtime | Measure | Min | Median | Max |
|---------|---------|-----|--------|-----|
| monty | first call (fresh process) | 0.403 ms | 0.575 ms | 0.758 ms |
| monty | second call | 0.035 ms | 0.049 ms | 0.077 ms |
| cpython | first call (fresh process) | 18.490 ms | 18.850 ms | 21.080 ms |
| cpython | second call | 4.264 ms | 4.412 ms | 6.798 ms |

## Per call (warm session)

| Benchmark | Time |
|-----------|------|
| python_call/monty/pass | 16.352 µs |
| python_call/monty/print | 17.752 µs |
| python_call/monty/json_roundtrip | 29.666 µs |
| python_call/monty/file_write_read | 50.227 µs |
| python_call/cpython/pass | 3.7618 ms |
| python_call/cpython/print | 4.4237 ms |
| python_call/cpython/json_roundtrip | 6.2783 ms |
| python_call/cpython/file_write_read | 10.624 ms |

## Per session (new Bash per call)

| Benchmark | Time |
|-----------|------|
| python_session/monty/new_bash_print | 91.235 µs |
| python_session/cpython/new_bash_print | 4.6739 ms |

## Compute (CPU-bound Python)

| Benchmark | Time |
|-----------|------|
| python_compute/monty/loop_sum_1e5 | 57.251 ms |
| python_compute/monty/fib_20 | 17.381 ms |
| python_compute/monty/string_build_1e4 | 7.3383 ms |
| python_compute/cpython/loop_sum_1e5 | 1.4959 s |
| python_compute/cpython/fib_20 | 68.709 ms |
| python_compute/cpython/string_build_1e4 | 211.72 ms |

## Parallel sessions (N concurrent, one call each)

| Benchmark | Time |
|-----------|------|
| python_parallel/monty/1 | 171.00 µs |
| python_parallel/monty/16 | 543.77 µs |
| python_parallel/monty/64 | 1.8129 ms |
| python_parallel/cpython/1 | 5.0482 ms |
| python_parallel/cpython/16 | 42.664 ms |
| python_parallel/cpython/64 | 156.64 ms |

## Import outside the snapshot (CPython, bytecode stdlib)

| Benchmark | Time |
|-----------|------|
| python_import/cpython/email_message | 265.54 ms |
| python_import/cpython/http_client | 461.12 ms |

## Load (CPython, concurrent tenants, 4 calls each, one process)

| Tenants | Calls | Wall (s) | Calls/s | p50 (ms) | p99 (ms) | max (ms) | Failures | Peak RSS (MB) |
|---------|-------|----------|---------|----------|----------|----------|----------|---------------|
| 1 | 4 | 0.07 | 55 | 15.35 | 28.60 | 28.60 | 0 | 48 |
| 16 | 64 | 0.40 | 160 | 66.73 | 165.81 | 166.11 | 0 | 134 |
| 64 | 256 | 1.57 | 163 | 297.42 | 614.05 | 617.84 | 0 | 437 |
| 256 | 1024 | 6.32 | 162 | 1126.59 | 2598.56 | 2601.63 | 0 | 1590 |
| 1024 | 4096 | 24.65 | 166 | 5924.72 | 6653.26 | 6722.88 | 0 | 3039 |
