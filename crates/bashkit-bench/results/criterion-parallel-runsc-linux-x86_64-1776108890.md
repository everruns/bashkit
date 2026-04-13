# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `runsc-linux-x86_64`
- **Hostname**: runsc
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 16
- **Timestamp**: 1776108890

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 10.742 ms |
| workload_types/light_parallel | 2.2647 ms |
| workload_types/medium_sequential | 26.687 ms |
| workload_types/medium_parallel | 3.2065 ms |
| workload_types/heavy_sequential | 52.536 ms |
| workload_types/heavy_parallel | 6.1678 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 3.8348 ms |
| parallel_scaling/medium_par/10 | 1.0511 ms |
| parallel_scaling/shared_fs/10 | 779.39 µs |
| parallel_scaling/medium_seq/50 | 20.790 ms |
| parallel_scaling/medium_par/50 | 2.9669 ms |
| parallel_scaling/shared_fs/50 | 2.4452 ms |
| parallel_scaling/medium_seq/100 | 39.368 ms |
| parallel_scaling/medium_par/100 | 5.5791 ms |
| parallel_scaling/shared_fs/100 | 5.0337 ms |
| parallel_scaling/medium_seq/200 | 94.801 ms |
| parallel_scaling/medium_par/200 | 15.513 ms |
| parallel_scaling/shared_fs/200 | 15.301 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 23.633 µs |
| single_echo | 91.590 µs |
| single_file_write_read | 100.15 µs |
| single_grep | 109.95 µs |
| single_awk | 112.47 µs |
| single_sed | 239.76 µs |
| single_light_script | 111.92 µs |
| single_medium_script | 414.01 µs |
| single_heavy_script | 1.0392 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 10.742 ms | 2.265 ms | **4.74x** |
| medium | 26.687 ms | 3.207 ms | **8.32x** |
| heavy | 52.536 ms | 6.168 ms | **8.52x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 3.835 ms | 1.051 ms | 0.779 ms | **3.65x** |
| 50 | 20.790 ms | 2.967 ms | 2.445 ms | **7.01x** |
| 100 | 39.368 ms | 5.579 ms | 5.034 ms | **7.06x** |
| 200 | 94.801 ms | 15.513 ms | 15.301 ms | **6.11x** |
