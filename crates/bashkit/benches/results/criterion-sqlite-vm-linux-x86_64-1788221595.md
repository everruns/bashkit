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
- **Timestamp**: 1788221595

## CRUD (insert / update, Memory vs Vfs backend, n rows)

| Benchmark | Time |
|-----------|------|
| sqlite_crud/insert_mem/100 | 1.4289 ms |
| sqlite_crud/insert_vfs/100 | 2.7726 ms |
| sqlite_crud/update_mem/100 | 1.8710 ms |
| sqlite_crud/update_vfs/100 | 2.4754 ms |
| sqlite_crud/insert_mem/1000 | 4.8250 ms |
| sqlite_crud/insert_vfs/1000 | 6.1664 ms |
| sqlite_crud/update_mem/1000 | 5.5788 ms |
| sqlite_crud/update_vfs/1000 | 6.5787 ms |
| sqlite_crud/insert_mem/10000 | 37.890 ms |
| sqlite_crud/insert_vfs/10000 | 38.609 ms |
| sqlite_crud/update_mem/10000 | 40.244 ms |
| sqlite_crud/update_vfs/10000 | 40.999 ms |

## Indexing (create index, indexed lookup, full scan)

| Benchmark | Time |
|-----------|------|
| sqlite_index/create_index_mem/100 | 2.0668 ms |
| sqlite_index/indexed_lookup_mem/100 | 2.0851 ms |
| sqlite_index/full_scan_mem/100 | 1.6733 ms |
| sqlite_index/create_index_mem/1000 | 7.2668 ms |
| sqlite_index/indexed_lookup_mem/1000 | 8.3068 ms |
| sqlite_index/full_scan_mem/1000 | 5.1298 ms |
| sqlite_index/create_index_mem/10000 | 64.863 ms |
| sqlite_index/indexed_lookup_mem/10000 | 64.357 ms |
| sqlite_index/full_scan_mem/10000 | 40.238 ms |

## Query (GROUP BY aggregate)

| Benchmark | Time |
|-----------|------|
| sqlite_query/aggregate_mem/100 | 1.7619 ms |
| sqlite_query/aggregate_vfs/100 | 2.4821 ms |
| sqlite_query/aggregate_in_memory/100 | 1.5772 ms |
| sqlite_query/aggregate_mem/1000 | 6.1686 ms |
| sqlite_query/aggregate_vfs/1000 | 7.1580 ms |
| sqlite_query/aggregate_in_memory/1000 | 5.2710 ms |
| sqlite_query/aggregate_mem/10000 | 46.446 ms |
| sqlite_query/aggregate_vfs/10000 | 46.823 ms |
| sqlite_query/aggregate_in_memory/10000 | 36.241 ms |

## Output mode formatters (1k rows)

| Benchmark | Time |
|-----------|------|
| sqlite_output_mode/list | 6.0063 ms |
| sqlite_output_mode/csv | 6.2784 ms |
| sqlite_output_mode/json | 5.8009 ms |
| sqlite_output_mode/markdown | 7.0190 ms |
| sqlite_output_mode/box | 6.8177 ms |

## Persistence (cost per invocation)

| Benchmark | Time |
|-----------|------|
| sqlite_persistence/two_invocations_mem | 5.2902 ms |
| sqlite_persistence/two_invocations_vfs | 6.7096 ms |
| sqlite_persistence/memory_db_baseline | 5.1001 ms |

## Parallel sessions (N concurrent over shared VFS)

| Benchmark | Time |
|-----------|------|
| sqlite_parallel/mem/4 | 7.3641 ms |
| sqlite_parallel/vfs/4 | 8.4376 ms |
| sqlite_parallel/mem/16 | 22.687 ms |
| sqlite_parallel/vfs/16 | 26.859 ms |
| sqlite_parallel/mem/64 | 80.248 ms |
| sqlite_parallel/vfs/64 | 98.657 ms |
