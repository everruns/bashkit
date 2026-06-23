# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1782168239

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 3.8932 ms |
| workload_types/light_parallel | 1.3279 ms |
| workload_types/medium_sequential | 18.656 ms |
| workload_types/medium_parallel | 5.1439 ms |
| workload_types/heavy_sequential | 56.175 ms |
| workload_types/heavy_parallel | 14.620 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 3.7044 ms |
| parallel_scaling/medium_par/10 | 1.2981 ms |
| parallel_scaling/shared_fs/10 | 807.42 µs |
| parallel_scaling/medium_seq/50 | 18.635 ms |
| parallel_scaling/medium_par/50 | 5.2968 ms |
| parallel_scaling/shared_fs/50 | 3.5919 ms |
| parallel_scaling/medium_seq/100 | 37.804 ms |
| parallel_scaling/medium_par/100 | 10.336 ms |
| parallel_scaling/shared_fs/100 | 6.8304 ms |
| parallel_scaling/medium_seq/200 | 74.215 ms |
| parallel_scaling/medium_par/200 | 20.338 ms |
| parallel_scaling/shared_fs/200 | 16.870 ms |
| parallel_scaling/medium_seq/500 | 182.29 ms |
| parallel_scaling/medium_par/500 | 50.491 ms |
| parallel_scaling/shared_fs/500 | 47.912 ms |
| parallel_scaling/medium_seq/1000 | 371.62 ms |
| parallel_scaling/medium_par/1000 | 97.672 ms |
| parallel_scaling/shared_fs/1000 | 140.56 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 39.904 µs |
| single_echo | 48.200 µs |
| single_file_write_read | 81.555 µs |
| single_grep | 76.044 µs |
| single_awk | 72.376 µs |
| single_sed | 194.96 µs |
| single_light_script | 75.035 µs |
| single_medium_script | 376.66 µs |
| single_heavy_script | 1.0537 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 3.893 ms | 1.328 ms | **2.93x** |
| medium | 18.656 ms | 5.144 ms | **3.63x** |
| heavy | 56.175 ms | 14.620 ms | **3.84x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 3.704 ms | 1.298 ms | 0.807 ms | **2.85x** |
| 50 | 18.635 ms | 5.297 ms | 3.592 ms | **3.52x** |
| 100 | 37.804 ms | 10.336 ms | 6.830 ms | **3.66x** |
| 200 | 74.215 ms | 20.338 ms | 16.870 ms | **3.65x** |
| 500 | 182.290 ms | 50.491 ms | 47.912 ms | **3.61x** |
| 1000 | 371.620 ms | 97.672 ms | 140.560 ms | **3.80x** |
