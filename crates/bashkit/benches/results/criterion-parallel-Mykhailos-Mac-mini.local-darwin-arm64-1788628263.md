# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `Mykhailos-Mac-mini.local-darwin-arm64`
- **Hostname**: Mykhailos-Mac-mini.local
- **OS**: darwin
- **Architecture**: arm64
- **CPUs**: ?
- **Timestamp**: 1788628263

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 1.5995 ms |
| workload_types/light_parallel | 1.6871 ms |
| workload_types/medium_sequential | 7.1423 ms |
| workload_types/medium_parallel | 3.0198 ms |
| workload_types/heavy_sequential | 25.355 ms |
| workload_types/heavy_parallel | 7.1101 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 1.7688 ms |
| parallel_scaling/medium_par/10 | 490.81 µs |
| parallel_scaling/shared_fs/10 | 298.25 µs |
| parallel_scaling/medium_seq/50 | 6.7227 ms |
| parallel_scaling/medium_par/50 | 1.8625 ms |
| parallel_scaling/shared_fs/50 | 1.0918 ms |
| parallel_scaling/medium_seq/100 | 13.412 ms |
| parallel_scaling/medium_par/100 | 3.5558 ms |
| parallel_scaling/shared_fs/100 | 2.0785 ms |
| parallel_scaling/medium_seq/200 | 26.646 ms |
| parallel_scaling/medium_par/200 | 6.9718 ms |
| parallel_scaling/shared_fs/200 | 5.1099 ms |
| parallel_scaling/medium_seq/500 | 73.207 ms |
| parallel_scaling/medium_par/500 | 18.433 ms |
| parallel_scaling/shared_fs/500 | 13.094 ms |
| parallel_scaling/medium_seq/1000 | 140.23 ms |
| parallel_scaling/medium_par/1000 | 39.202 ms |
| parallel_scaling/shared_fs/1000 | 30.817 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 12.553 µs |
| single_echo | 16.711 µs |
| single_file_write_read | 25.404 µs |
| single_grep | 24.181 µs |
| single_awk | 24.413 µs |
| single_sed | 58.494 µs |
| single_light_script | 28.181 µs |
| single_medium_script | 136.97 µs |
| single_heavy_script | 465.35 µs |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 1.599 ms | 1.687 ms | **0.95x** |
| medium | 7.142 ms | 3.020 ms | **2.37x** |
| heavy | 25.355 ms | 7.110 ms | **3.57x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 1.769 ms | 0.491 ms | 0.298 ms | **3.60x** |
| 50 | 6.723 ms | 1.863 ms | 1.092 ms | **3.61x** |
| 100 | 13.412 ms | 3.556 ms | 2.079 ms | **3.77x** |
| 200 | 26.646 ms | 6.972 ms | 5.110 ms | **3.82x** |
| 500 | 73.207 ms | 18.433 ms | 13.094 ms | **3.97x** |
| 1000 | 140.230 ms | 39.202 ms | 30.817 ms | **3.58x** |
