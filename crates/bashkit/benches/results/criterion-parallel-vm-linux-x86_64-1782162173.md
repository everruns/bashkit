# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1782162173

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 2.9735 ms |
| workload_types/light_parallel | 982.07 µs |
| workload_types/medium_sequential | 14.484 ms |
| workload_types/medium_parallel | 4.1479 ms |
| workload_types/heavy_sequential | 47.101 ms |
| workload_types/heavy_parallel | 12.260 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 2.8015 ms |
| parallel_scaling/medium_par/10 | 1.0280 ms |
| parallel_scaling/shared_fs/10 | 661.04 µs |
| parallel_scaling/medium_seq/50 | 14.232 ms |
| parallel_scaling/medium_par/50 | 4.0529 ms |
| parallel_scaling/shared_fs/50 | 2.6310 ms |
| parallel_scaling/medium_seq/100 | 27.965 ms |
| parallel_scaling/medium_par/100 | 7.9751 ms |
| parallel_scaling/shared_fs/100 | 5.5717 ms |
| parallel_scaling/medium_seq/200 | 57.317 ms |
| parallel_scaling/medium_par/200 | 15.607 ms |
| parallel_scaling/shared_fs/200 | 14.397 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 31.669 µs |
| single_echo | 38.924 µs |
| single_file_write_read | 60.190 µs |
| single_grep | 58.610 µs |
| single_awk | 62.195 µs |
| single_sed | 153.72 µs |
| single_light_script | 65.103 µs |
| single_medium_script | 297.61 µs |
| single_heavy_script | 943.32 µs |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 2.974 ms | 0.982 ms | **3.03x** |
| medium | 14.484 ms | 4.148 ms | **3.49x** |
| heavy | 47.101 ms | 12.260 ms | **3.84x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 2.801 ms | 1.028 ms | 0.661 ms | **2.73x** |
| 50 | 14.232 ms | 4.053 ms | 2.631 ms | **3.51x** |
| 100 | 27.965 ms | 7.975 ms | 5.572 ms | **3.51x** |
| 200 | 57.317 ms | 15.607 ms | 14.397 ms | **3.67x** |
