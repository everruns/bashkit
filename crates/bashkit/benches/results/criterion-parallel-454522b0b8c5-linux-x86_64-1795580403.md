# Criterion Parallel Execution Benchmark

## System Information

- **Moniker**: `454522b0b8c5-linux-x86_64`
- **Hostname**: 454522b0b8c5
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 5
- **Timestamp**: 1795580403

## Workload Comparison (50 sessions)

| Benchmark | Time |
|-----------|------|
| workload_types/light_sequential | 5.3688 ms |
| workload_types/light_parallel | 2.0121 ms |
| workload_types/medium_sequential | 23.303 ms |
| workload_types/medium_parallel | 7.7065 ms |
| workload_types/heavy_sequential | 68.496 ms |
| workload_types/heavy_parallel | 29.278 ms |

## Parallel Scaling (medium workload)

| Benchmark | Time |
|-----------|------|
| parallel_scaling/medium_seq/10 | 4.6059 ms |
| parallel_scaling/medium_par/10 | 1.6856 ms |
| parallel_scaling/shared_fs/10 | 1.5123 ms |
| parallel_scaling/medium_seq/50 | 22.826 ms |
| parallel_scaling/medium_par/50 | 9.8753 ms |
| parallel_scaling/shared_fs/50 | 6.1589 ms |
| parallel_scaling/medium_seq/100 | 45.919 ms |
| parallel_scaling/medium_par/100 | 15.074 ms |
| parallel_scaling/shared_fs/100 | 10.664 ms |
| parallel_scaling/medium_seq/200 | 90.751 ms |
| parallel_scaling/medium_par/200 | 29.905 ms |
| parallel_scaling/shared_fs/200 | 21.678 ms |
| parallel_scaling/medium_seq/500 | 230.57 ms |
| parallel_scaling/medium_par/500 | 75.175 ms |
| parallel_scaling/shared_fs/500 | 60.623 ms |
| parallel_scaling/medium_seq/1000 | 456.72 ms |
| parallel_scaling/medium_par/1000 | 149.00 ms |
| parallel_scaling/shared_fs/1000 | 145.71 ms |

## Single Operations

| Benchmark | Time |
|-----------|------|
| single_bash_new | 58.101 µs |
| single_echo | 73.625 µs |
| single_file_write_read | 115.91 µs |
| single_grep | 127.53 µs |
| single_awk | 130.66 µs |
| single_sed | 115.22 µs |
| single_light_script | 109.24 µs |
| single_medium_script | 455.30 µs |
| single_heavy_script | 1.3358 ms |

## Speedup Summary

| Workload | Sequential | Parallel | Speedup |
|----------|-----------|----------|---------|
| light | 5.369 ms | 2.012 ms | **2.67x** |
| medium | 23.303 ms | 7.707 ms | **3.02x** |
| heavy | 68.496 ms | 29.278 ms | **2.34x** |

| Sessions | Sequential | Parallel | Shared FS | Par Speedup |
|----------|-----------|----------|-----------|-------------|
| 10 | 4.606 ms | 1.686 ms | 1.512 ms | **2.73x** |
| 50 | 22.826 ms | 9.875 ms | 6.159 ms | **2.31x** |
| 100 | 45.919 ms | 15.074 ms | 10.664 ms | **3.05x** |
| 200 | 90.751 ms | 29.905 ms | 21.678 ms | **3.03x** |
| 500 | 230.570 ms | 75.175 ms | 60.623 ms | **3.07x** |
| 1000 | 456.720 ms | 149.000 ms | 145.710 ms | **3.07x** |

## Build identity

Runtime `cc622ef2`, Rust 1.100.0, `experimental_execution_plan` enabled.
Full Criterion run, unpinned, four-CPU cgroup quota. These measurements
validate this build; there is no paired baseline speedup claim.
Exact binary hashes are in `experiments/mixed-execution-plan/results/2026-11-24-binaries.json`.
