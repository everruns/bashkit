# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `16abbd7c05cb-linux-x86_64`
- **Hostname**: 16abbd7c05cb
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 5
- **Timestamp**: 1795497002

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 5.4747 ms |
| workload_types/light_parallel | 2.0802 ms |
| workload_types/medium_sequential | 23.334 ms |
| workload_types/medium_parallel | 8.2699 ms |
| workload_types/heavy_sequential | 68.004 ms |
| workload_types/heavy_parallel | 22.373 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 4.4802 ms |
| parallel_scaling/medium_par/10 | 1.8191 ms |
| parallel_scaling/shared_fs/10 | 1.2834 ms |
| parallel_scaling/medium_seq/50 | 23.229 ms |
| parallel_scaling/medium_par/50 | 7.7675 ms |
| parallel_scaling/shared_fs/50 | 6.6056 ms |
| parallel_scaling/medium_seq/100 | 44.021 ms |
| parallel_scaling/medium_par/100 | 19.132 ms |
| parallel_scaling/shared_fs/100 | 11.040 ms |
| parallel_scaling/medium_seq/200 | 91.672 ms |
| parallel_scaling/medium_par/200 | 30.459 ms |
| parallel_scaling/shared_fs/200 | 26.247 ms |
| parallel_scaling/medium_seq/500 | 238.56 ms |
| parallel_scaling/medium_par/500 | 75.654 ms |
| parallel_scaling/shared_fs/500 | 62.310 ms |
| parallel_scaling/medium_seq/1000 | 463.17 ms |
| parallel_scaling/medium_par/1000 | 151.04 ms |
| parallel_scaling/shared_fs/1000 | 150.82 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 59.246 µs |
| single_echo | 75.146 µs |
| single_file_write_read | 123.49 µs |
| single_grep | 130.36 µs |
| single_awk | 132.98 µs |
| single_sed | 116.88 µs |
| single_light_script | 143.40 µs |
| single_medium_script | 479.71 µs |
| single_heavy_script | 1.4346 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 5.475 ms | 2.080 ms | **2.63x** |
| medium | 23.334 ms | 8.270 ms | **2.82x** |
| heavy | 68.004 ms | 22.373 ms | **3.04x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 4.480 ms | 1.819 ms | 1.283 ms | **2.46x** |
| 50 | 23.229 ms | 7.768 ms | 6.606 ms | **2.99x** |
| 100 | 44.021 ms | 19.132 ms | 11.040 ms | **2.30x** |
| 200 | 91.672 ms | 30.459 ms | 26.247 ms | **3.01x** |
| 500 | 238.560 ms | 75.654 ms | 62.310 ms | **3.15x** |
| 1000 | 463.170 ms | 151.040 ms | 150.820 ms | **3.07x** |
