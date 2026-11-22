# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `a9d59d0fdfe3-linux-x86_64`
- **Hostname**: a9d59d0fdfe3
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 5
- **Timestamp**: 1795314525

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 6.1603 ms |
| workload_types/light_parallel | 2.3878 ms |
| workload_types/medium_sequential | 26.670 ms |
| workload_types/medium_parallel | 8.2585 ms |
| workload_types/heavy_sequential | 68.155 ms |
| workload_types/heavy_parallel | 26.163 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 4.7173 ms |
| parallel_scaling/medium_par/10 | 1.8414 ms |
| parallel_scaling/shared_fs/10 | 1.3012 ms |
| parallel_scaling/medium_seq/50 | 23.725 ms |
| parallel_scaling/medium_par/50 | 8.2302 ms |
| parallel_scaling/shared_fs/50 | 7.2815 ms |
| parallel_scaling/medium_seq/100 | 46.450 ms |
| parallel_scaling/medium_par/100 | 17.274 ms |
| parallel_scaling/shared_fs/100 | 11.391 ms |
| parallel_scaling/medium_seq/200 | 115.18 ms |
| parallel_scaling/medium_par/200 | 34.851 ms |
| parallel_scaling/shared_fs/200 | 23.454 ms |
| parallel_scaling/medium_seq/500 | 232.87 ms |
| parallel_scaling/medium_par/500 | 78.240 ms |
| parallel_scaling/shared_fs/500 | 64.056 ms |
| parallel_scaling/medium_seq/1000 | 472.17 ms |
| parallel_scaling/medium_par/1000 | 202.24 ms |
| parallel_scaling/shared_fs/1000 | 164.85 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 61.917 µs |
| single_echo | 77.953 µs |
| single_file_write_read | 124.68 µs |
| single_grep | 129.28 µs |
| single_awk | 134.93 µs |
| single_sed | 124.53 µs |
| single_light_script | 117.01 µs |
| single_medium_script | 583.28 µs |
| single_heavy_script | 1.4084 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 6.160 ms | 2.388 ms | **2.58x** |
| medium | 26.670 ms | 8.258 ms | **3.23x** |
| heavy | 68.155 ms | 26.163 ms | **2.61x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 4.717 ms | 1.841 ms | 1.301 ms | **2.56x** |
| 50 | 23.725 ms | 8.230 ms | 7.282 ms | **2.88x** |
| 100 | 46.450 ms | 17.274 ms | 11.391 ms | **2.69x** |
| 200 | 115.180 ms | 34.851 ms | 23.454 ms | **3.30x** |
| 500 | 232.870 ms | 78.240 ms | 64.056 ms | **2.98x** |
| 1000 | 472.170 ms | 202.240 ms | 164.850 ms | **2.33x** |
