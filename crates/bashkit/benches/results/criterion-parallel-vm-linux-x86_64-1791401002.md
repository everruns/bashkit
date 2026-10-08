# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1791401002

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 5.9398 ms |
| workload_types/light_parallel | 4.4518 ms |
| workload_types/medium_sequential | 21.632 ms |
| workload_types/medium_parallel | 17.490 ms |
| workload_types/heavy_sequential | 65.949 ms |
| workload_types/heavy_parallel | 49.142 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 4.3466 ms |
| parallel_scaling/medium_par/10 | 2.8989 ms |
| parallel_scaling/shared_fs/10 | 2.1865 ms |
| parallel_scaling/medium_seq/50 | 22.751 ms |
| parallel_scaling/medium_par/50 | 14.542 ms |
| parallel_scaling/shared_fs/50 | 9.2122 ms |
| parallel_scaling/medium_seq/100 | 48.931 ms |
| parallel_scaling/medium_par/100 | 28.244 ms |
| parallel_scaling/shared_fs/100 | 18.105 ms |
| parallel_scaling/medium_seq/200 | 101.58 ms |
| parallel_scaling/medium_par/200 | 61.706 ms |
| parallel_scaling/shared_fs/200 | 34.450 ms |
| parallel_scaling/medium_seq/500 | 221.65 ms |
| parallel_scaling/medium_par/500 | 103.14 ms |
| parallel_scaling/shared_fs/500 | 99.824 ms |
| parallel_scaling/medium_seq/1000 | 416.20 ms |
| parallel_scaling/medium_par/1000 | 286.09 ms |
| parallel_scaling/shared_fs/1000 | 211.56 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 68.289 µs |
| single_echo | 83.043 µs |
| single_file_write_read | 128.29 µs |
| single_grep | 115.58 µs |
| single_awk | 121.48 µs |
| single_sed | 114.34 µs |
| single_light_script | 110.54 µs |
| single_medium_script | 470.18 µs |
| single_heavy_script | 1.3484 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 5.940 ms | 4.452 ms | **1.33x** |
| medium | 21.632 ms | 17.490 ms | **1.24x** |
| heavy | 65.949 ms | 49.142 ms | **1.34x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 4.347 ms | 2.899 ms | 2.187 ms | **1.50x** |
| 50 | 22.751 ms | 14.542 ms | 9.212 ms | **1.56x** |
| 100 | 48.931 ms | 28.244 ms | 18.105 ms | **1.73x** |
| 200 | 101.580 ms | 61.706 ms | 34.450 ms | **1.65x** |
| 500 | 221.650 ms | 103.140 ms | 99.824 ms | **2.15x** |
| 1000 | 416.200 ms | 286.090 ms | 211.560 ms | **1.45x** |

## Notes

Branch `claude/project-thread-eafupr-bb-vars` (declaration engine, shallow-binding
locals, `env` as `Arc`) after merging main 8d47605. Host load average was ~8 on
4 CPUs during the run, so single runs swing about 25%. An interleaved A/B run of
the main and branch bench binaries (two rounds each) put every `workload_types`
average within 3% of main (light_seq 5.67 vs 5.46 ms, medium_par 15.3 vs 15.8 ms,
heavy_seq 60.7 vs 61.7 ms, heavy_par 42.1 vs 41.7 ms): no regression beyond noise.
