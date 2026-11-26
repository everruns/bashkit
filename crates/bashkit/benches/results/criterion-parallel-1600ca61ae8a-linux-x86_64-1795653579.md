# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `1600ca61ae8a-linux-x86_64`
- **Hostname**: 1600ca61ae8a
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 5
- **Timestamp**: 1795653579

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 5.7097 ms |
| workload_types/light_parallel | 2.1760 ms |
| workload_types/medium_sequential | 23.884 ms |
| workload_types/medium_parallel | 8.0467 ms |
| workload_types/heavy_sequential | 66.525 ms |
| workload_types/heavy_parallel | 22.401 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 4.5028 ms |
| parallel_scaling/medium_par/10 | 1.6563 ms |
| parallel_scaling/shared_fs/10 | 1.1640 ms |
| parallel_scaling/medium_seq/50 | 22.868 ms |
| parallel_scaling/medium_par/50 | 7.6553 ms |
| parallel_scaling/shared_fs/50 | 5.3160 ms |
| parallel_scaling/medium_seq/100 | 45.850 ms |
| parallel_scaling/medium_par/100 | 16.148 ms |
| parallel_scaling/shared_fs/100 | 10.472 ms |
| parallel_scaling/medium_seq/200 | 91.352 ms |
| parallel_scaling/medium_par/200 | 30.679 ms |
| parallel_scaling/shared_fs/200 | 28.013 ms |
| parallel_scaling/medium_seq/500 | 235.53 ms |
| parallel_scaling/medium_par/500 | 94.576 ms |
| parallel_scaling/shared_fs/500 | 60.677 ms |
| parallel_scaling/medium_seq/1000 | 451.09 ms |
| parallel_scaling/medium_par/1000 | 149.05 ms |
| parallel_scaling/shared_fs/1000 | 143.27 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 59.559 µs |
| single_echo | 77.715 µs |
| single_file_write_read | 117.22 µs |
| single_grep | 127.94 µs |
| single_awk | 132.17 µs |
| single_sed | 115.50 µs |
| single_light_script | 110.49 µs |
| single_medium_script | 455.89 µs |
| single_heavy_script | 1.3301 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 5.710 ms | 2.176 ms | **2.62x** |
| medium | 23.884 ms | 8.047 ms | **2.97x** |
| heavy | 66.525 ms | 22.401 ms | **2.97x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 4.503 ms | 1.656 ms | 1.164 ms | **2.72x** |
| 50 | 22.868 ms | 7.655 ms | 5.316 ms | **2.99x** |
| 100 | 45.850 ms | 16.148 ms | 10.472 ms | **2.84x** |
| 200 | 91.352 ms | 30.679 ms | 28.013 ms | **2.98x** |
| 500 | 235.530 ms | 94.576 ms | 60.677 ms | **2.49x** |
| 1000 | 451.090 ms | 149.050 ms | 143.270 ms | **3.03x** |
