# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `79f1c12bcc44-linux-x86_64`
- **Hostname**: 79f1c12bcc44
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 5
- **Timestamp**: 1792174213

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 4.7671 ms |
| workload_types/light_parallel | 3.8270 ms |
| workload_types/medium_sequential | 19.462 ms |
| workload_types/medium_parallel | 11.116 ms |
| workload_types/heavy_sequential | 67.110 ms |
| workload_types/heavy_parallel | 40.803 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 5.7822 ms |
| parallel_scaling/medium_par/10 | 2.8052 ms |
| parallel_scaling/shared_fs/10 | 2.2985 ms |
| parallel_scaling/medium_seq/50 | 21.728 ms |
| parallel_scaling/medium_par/50 | 18.059 ms |
| parallel_scaling/shared_fs/50 | 12.548 ms |
| parallel_scaling/medium_seq/100 | 80.187 ms |
| parallel_scaling/medium_par/100 | 31.149 ms |
| parallel_scaling/shared_fs/100 | 17.277 ms |
| parallel_scaling/medium_seq/200 | 130.53 ms |
| parallel_scaling/medium_par/200 | 57.381 ms |
| parallel_scaling/shared_fs/200 | 46.624 ms |
| parallel_scaling/medium_seq/500 | 429.79 ms |
| parallel_scaling/medium_par/500 | 95.375 ms |
| parallel_scaling/shared_fs/500 | 74.238 ms |
| parallel_scaling/medium_seq/1000 | 485.25 ms |
| parallel_scaling/medium_par/1000 | 186.85 ms |
| parallel_scaling/shared_fs/1000 | 153.30 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 51.480 µs |
| single_echo | 63.227 µs |
| single_file_write_read | 109.18 µs |
| single_grep | 136.37 µs |
| single_awk | 157.81 µs |
| single_sed | 110.85 µs |
| single_light_script | 94.890 µs |
| single_medium_script | 393.06 µs |
| single_heavy_script | 1.1201 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 4.767 ms | 3.827 ms | **1.25x** |
| medium | 19.462 ms | 11.116 ms | **1.75x** |
| heavy | 67.110 ms | 40.803 ms | **1.64x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 5.782 ms | 2.805 ms | 2.299 ms | **2.06x** |
| 50 | 21.728 ms | 18.059 ms | 12.548 ms | **1.20x** |
| 100 | 80.187 ms | 31.149 ms | 17.277 ms | **2.57x** |
| 200 | 130.530 ms | 57.381 ms | 46.624 ms | **2.27x** |
| 500 | 429.790 ms | 95.375 ms | 74.238 ms | **4.51x** |
| 1000 | 485.250 ms | 186.850 ms | 153.300 ms | **2.60x** |
