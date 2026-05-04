# Criterion SQLite Builtin Benchmark

Measures the `sqlite` builtin (Turso embedded engine) end-to-end through
the bashkit interpreter. Per-invocation overhead (interpreter setup, script
parse, engine open, VFS flush) is included in every number — these are
"what a script author observes", not isolated engine micro-benchmarks.

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1777865268

## CRUD (insert / update, Memory vs Vfs backend, n rows)

| Benchmark | Time |
|-----------|------|
| sqlite_crud/insert_mem/100 | 793.84 µs |
| sqlite_crud/insert_vfs/100 | 1.0603 ms |
| sqlite_crud/update_mem/100 | 783.89 µs |
| sqlite_crud/update_vfs/100 | 1.0599 ms |
| sqlite_crud/insert_mem/1000 | 787.70 µs |
| sqlite_crud/insert_vfs/1000 | 1.0040 ms |
| sqlite_crud/update_mem/1000 | 769.52 µs |
| sqlite_crud/update_vfs/1000 | 1.0788 ms |
| sqlite_crud/insert_mem/10000 | 783.19 µs |
| sqlite_crud/insert_vfs/10000 | 1.0513 ms |
| sqlite_crud/update_mem/10000 | 805.82 µs |
| sqlite_crud/update_vfs/10000 | 1.0885 ms |

## Indexing (create index, indexed lookup, full scan)

| Benchmark | Time |
|-----------|------|
| sqlite_index/create_index_mem/100 | 767.10 µs |
| sqlite_index/indexed_lookup_mem/100 | 797.03 µs |
| sqlite_index/full_scan_mem/100 | 779.05 µs |
| sqlite_index/create_index_mem/1000 | 764.17 µs |
| sqlite_index/indexed_lookup_mem/1000 | 803.58 µs |
| sqlite_index/full_scan_mem/1000 | 786.84 µs |
| sqlite_index/create_index_mem/10000 | 786.04 µs |
| sqlite_index/indexed_lookup_mem/10000 | 793.83 µs |
| sqlite_index/full_scan_mem/10000 | 790.69 µs |

## Query (GROUP BY aggregate)

| Benchmark | Time |
|-----------|------|
| sqlite_query/aggregate_mem/100 | 785.93 µs |
| sqlite_query/aggregate_vfs/100 | 1.0172 ms |
| sqlite_query/aggregate_in_memory/100 | 772.38 µs |
| sqlite_query/aggregate_mem/1000 | 759.07 µs |
| sqlite_query/aggregate_vfs/1000 | 1.0436 ms |
| sqlite_query/aggregate_in_memory/1000 | 739.46 µs |
| sqlite_query/aggregate_mem/10000 | 775.62 µs |
| sqlite_query/aggregate_vfs/10000 | 1.0358 ms |
| sqlite_query/aggregate_in_memory/10000 | 753.89 µs |

## Output mode formatters (1k rows)

| Benchmark | Time |
|-----------|------|
| sqlite_output_mode/list | 784.03 µs |
| sqlite_output_mode/csv | 801.51 µs |
| sqlite_output_mode/json | 801.95 µs |
| sqlite_output_mode/markdown | 787.84 µs |
| sqlite_output_mode/box | 806.72 µs |

## Persistence (cost per invocation)

| Benchmark | Time |
|-----------|------|
| sqlite_persistence/two_invocations_mem | 1.3172 ms |
| sqlite_persistence/two_invocations_vfs | 1.8539 ms |
| sqlite_persistence/memory_db_baseline | 766.32 µs |

## Parallel sessions (N concurrent over shared VFS)

| Benchmark | Time |
|-----------|------|
| sqlite_parallel/mem/4 | 1.5198 ms |
| sqlite_parallel/vfs/4 | 2.8110 ms |
| sqlite_parallel/mem/16 | 4.7613 ms |
| sqlite_parallel/vfs/16 | 8.1925 ms |
| sqlite_parallel/mem/64 | 19.606 ms |
| sqlite_parallel/vfs/64 | 31.420 ms |
