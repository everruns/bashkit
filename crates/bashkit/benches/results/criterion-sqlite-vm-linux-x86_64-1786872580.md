# Criterion SQLite Builtin Benchmark

Measures the `sqlite` builtin (Turso embedded engine) end-to-end through
the bashkit interpreter. Per-invocation overhead (interpreter setup, script
parse, engine open, VFS flush) is included in every number, these are
"what a script author observes", not isolated engine micro-benchmarks.

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1786872580

## CRUD (insert / update, Memory vs Vfs backend, n rows)

| Benchmark | Time |
|-----------|------|
| sqlite_crud/insert_mem/100 | 1.5815 ms |
| sqlite_crud/insert_vfs/100 | 2.1505 ms |
| sqlite_crud/update_mem/100 | 1.5770 ms |
| sqlite_crud/update_vfs/100 | 2.1971 ms |
| sqlite_crud/insert_mem/1000 | 4.4522 ms |
| sqlite_crud/insert_vfs/1000 | 5.0772 ms |
| sqlite_crud/update_mem/1000 | 4.7909 ms |
| sqlite_crud/update_vfs/1000 | 5.2744 ms |
| sqlite_crud/insert_mem/10000 | 33.482 ms |
| sqlite_crud/insert_vfs/10000 | 36.968 ms |
| sqlite_crud/update_mem/10000 | 40.663 ms |
| sqlite_crud/update_vfs/10000 | 40.667 ms |

## Indexing (create index, indexed lookup, full scan)

| Benchmark | Time |
|-----------|------|
| sqlite_index/create_index_mem/100 | 1.8766 ms |
| sqlite_index/indexed_lookup_mem/100 | 1.9933 ms |
| sqlite_index/full_scan_mem/100 | 1.5917 ms |
| sqlite_index/create_index_mem/1000 | 6.6359 ms |
| sqlite_index/indexed_lookup_mem/1000 | 6.6437 ms |
| sqlite_index/full_scan_mem/1000 | 5.4651 ms |
| sqlite_index/create_index_mem/10000 | 61.268 ms |
| sqlite_index/indexed_lookup_mem/10000 | 59.069 ms |
| sqlite_index/full_scan_mem/10000 | 36.494 ms |

## Query (GROUP BY aggregate)

| Benchmark | Time |
|-----------|------|
| sqlite_query/aggregate_mem/100 | 1.6842 ms |
| sqlite_query/aggregate_vfs/100 | 2.3234 ms |
| sqlite_query/aggregate_in_memory/100 | 1.4605 ms |
| sqlite_query/aggregate_mem/1000 | 5.1008 ms |
| sqlite_query/aggregate_vfs/1000 | 5.6511 ms |
| sqlite_query/aggregate_in_memory/1000 | 4.2516 ms |
| sqlite_query/aggregate_mem/10000 | 41.015 ms |
| sqlite_query/aggregate_vfs/10000 | 42.004 ms |
| sqlite_query/aggregate_in_memory/10000 | 33.062 ms |

## Output mode formatters (1k rows)

| Benchmark | Time |
|-----------|------|
| sqlite_output_mode/list | 5.0912 ms |
| sqlite_output_mode/csv | 5.8390 ms |
| sqlite_output_mode/json | 5.1766 ms |
| sqlite_output_mode/markdown | 5.9508 ms |
| sqlite_output_mode/box | 5.8332 ms |

## Persistence (cost per invocation)

| Benchmark | Time |
|-----------|------|
| sqlite_persistence/two_invocations_mem | 4.7396 ms |
| sqlite_persistence/two_invocations_vfs | 6.1163 ms |
| sqlite_persistence/memory_db_baseline | 4.5421 ms |

## Parallel sessions (N concurrent over shared VFS)

| Benchmark | Time |
|-----------|------|
| sqlite_parallel/mem/4 | 5.7387 ms |
| sqlite_parallel/vfs/4 | 7.0490 ms |
| sqlite_parallel/mem/16 | 20.482 ms |
| sqlite_parallel/vfs/16 | 25.568 ms |
| sqlite_parallel/mem/64 | 67.982 ms |
| sqlite_parallel/vfs/64 | 83.281 ms |
