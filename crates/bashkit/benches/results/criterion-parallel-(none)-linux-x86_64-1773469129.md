# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `(none)-linux-x86_64`
- **Hostname**: (none)
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1773469129

## Workload Comparison (50 sessions)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| workload_types/light_sequential | 9.0104 ms |
| workload_types/light_parallel | 2.1407 ms |
| workload_types/medium_sequential | 27.940 ms |
| workload_types/medium_parallel | 6.4281 ms |
| workload_types/heavy_sequential | 74.842 ms |
| workload_types/heavy_parallel | 17.002 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| parallel_scaling/medium_seq/10 | 6.0494 ms |
| parallel_scaling/medium_par/10 | 1.7021 ms |
| parallel_scaling/shared_fs/10 | 1.2076 ms |
| parallel_scaling/medium_seq/50 | 26.697 ms |
| parallel_scaling/medium_par/50 | 7.2423 ms |
| parallel_scaling/shared_fs/50 | 5.2092 ms |
| parallel_scaling/medium_seq/100 | 64.667 ms |
| parallel_scaling/medium_par/100 | 14.184 ms |
| parallel_scaling/shared_fs/100 | 10.863 ms |
| parallel_scaling/medium_seq/200 | 127.68 ms |
| parallel_scaling/medium_par/200 | 31.426 ms |
| parallel_scaling/shared_fs/200 | 18.504 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 23.768 µs |
| single_echo | 93.811 µs |
| single_file_write_read | 160.05 µs |
| single_grep | 148.92 µs |
| single_awk | 140.22 µs |
| single_sed | 275.38 µs |
| single_light_script | 163.55 µs |
| single_medium_script | 580.79 µs |
| single_heavy_script | 1.4832 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 9.010 ms | 2.141 ms | **4.21x** |
| medium | 27.940 ms | 6.428 ms | **4.35x** |
| heavy | 74.842 ms | 17.002 ms | **4.40x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 6.049 ms | 1.702 ms | 1.208 ms | **3.55x** |
| 50 | 26.697 ms | 7.242 ms | 5.209 ms | **3.69x** |
| 100 | 64.667 ms | 14.184 ms | 10.863 ms | **4.56x** |
| 200 | 127.680 ms | 31.426 ms | 18.504 ms | **4.06x** |
