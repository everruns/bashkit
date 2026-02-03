# Parallel Execution Benchmark Results

**Date:** 2026-02-03
**System:** runsc-linux-x86_64 (16 CPUs)
**Benchmark:** `cargo bench --bench parallel_execution`

## Workload Types (50 iterations)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| light_sequential | 3.21 ms | 15.58 Kelem/s |
| light_parallel | 637 µs | 78.45 Kelem/s |
| medium_sequential | 8.70 ms | 5.75 Kelem/s |
| medium_parallel | 1.20 ms | 41.77 Kelem/s |
| heavy_sequential | 27.88 ms | 1.79 Kelem/s |
| heavy_parallel | 2.88 ms | 17.39 Kelem/s |

**Parallel speedup:**
- Light workloads: ~5x faster
- Medium workloads: ~7.3x faster
- Heavy workloads: ~9.7x faster

## Parallel Scaling (10-200 tasks)

| Tasks | Sequential | Parallel | Shared FS | Speedup |
|-------|-----------|----------|-----------|---------|
| 10 | 1.75 ms | 514 µs | 418 µs | 3.4x |
| 50 | 8.80 ms | 1.16 ms | 1.00 ms | 7.6x |
| 100 | 18.25 ms | 2.13 ms | 1.70 ms | 8.6x |
| 200 | 36.31 ms | 4.17 ms | 3.25 ms | 8.7x |

## Single Operations (100 samples)

| Operation | Time |
|-----------|------|
| bash_new (interpreter init) | 10.49 µs |
| echo | 46.75 µs |
| file_write_read | 59.89 µs |
| grep | 74.29 µs |
| awk | 74.34 µs |
| sed | 185.73 µs |
| light_script | 67.58 µs |
| medium_script | 171.76 µs |
| heavy_script | 505.15 µs |
