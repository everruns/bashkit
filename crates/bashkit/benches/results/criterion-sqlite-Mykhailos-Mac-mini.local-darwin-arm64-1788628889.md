# Criterion SQLite Builtin Benchmark

Measures the `sqlite` builtin (Turso embedded engine) end-to-end through
the bashkit interpreter. Per-invocation overhead (interpreter setup, script
parse, engine open, VFS flush) is included in every number — these are
"what a script author observes", not isolated engine micro-benchmarks.

## System Information

- **Moniker**: `Mykhailos-Mac-mini.local-darwin-arm64`
- **Hostname**: Mykhailos-Mac-mini.local
- **OS**: darwin
- **Architecture**: arm64
- **CPUs**: ?
- **Timestamp**: 1788628889

## CRUD (insert / update, Memory vs Vfs backend, n rows)

| Benchmark | Time |
|-----------|------|
| sqlite_crud/insert_mem/100 | 376.52 µs |
| sqlite_crud/insert_vfs/100 | 454.23 µs |
| sqlite_crud/update_mem/100 | 393.30 µs |
| sqlite_crud/update_vfs/100 | 469.39 µs |
| sqlite_crud/insert_mem/1000 | 1.2050 ms |
| sqlite_crud/insert_vfs/1000 | 1.2952 ms |
| sqlite_crud/update_mem/1000 | 1.3168 ms |
| sqlite_crud/update_vfs/1000 | 1.4104 ms |
| sqlite_crud/insert_mem/10000 | 9.5156 ms |
| sqlite_crud/insert_vfs/10000 | 9.6216 ms |
| sqlite_crud/update_mem/10000 | 10.540 ms |
| sqlite_crud/update_vfs/10000 | 10.528 ms |

## Indexing (create index, indexed lookup, full scan)

| Benchmark | Time |
|-----------|------|
| sqlite_index/create_index_mem/100 | 461.62 µs |
| sqlite_index/indexed_lookup_mem/100 | 551.89 µs |
| sqlite_index/full_scan_mem/100 | 383.43 µs |
| sqlite_index/create_index_mem/1000 | 1.8613 ms |
| sqlite_index/indexed_lookup_mem/1000 | 1.9269 ms |
| sqlite_index/full_scan_mem/1000 | 1.2768 ms |
| sqlite_index/create_index_mem/10000 | 17.195 ms |
| sqlite_index/indexed_lookup_mem/10000 | 17.333 ms |
| sqlite_index/full_scan_mem/10000 | 10.251 ms |

## Query (GROUP BY aggregate)

| Benchmark | Time |
|-----------|------|
| sqlite_query/aggregate_mem/100 | 413.84 µs |
| sqlite_query/aggregate_vfs/100 | 512.10 µs |
| sqlite_query/aggregate_in_memory/100 | 370.97 µs |
| sqlite_query/aggregate_mem/1000 | 1.4284 ms |
| sqlite_query/aggregate_vfs/1000 | 1.9106 ms |
| sqlite_query/aggregate_in_memory/1000 | 1.9607 ms |
| sqlite_query/aggregate_mem/10000 | 12.167 ms |
| sqlite_query/aggregate_vfs/10000 | 11.430 ms |
| sqlite_query/aggregate_in_memory/10000 | 9.4998 ms |

## Output mode formatters (1k rows)

| Benchmark | Time |
|-----------|------|
| sqlite_output_mode/list | 1.5921 ms |
| sqlite_output_mode/csv | 1.5973 ms |
| sqlite_output_mode/json | 1.5124 ms |
| sqlite_output_mode/markdown | 1.8245 ms |
| sqlite_output_mode/box | 1.8314 ms |

## Persistence (cost per invocation)

| Benchmark | Time |
|-----------|------|
| sqlite_persistence/two_invocations_mem | 1.2210 ms |
| sqlite_persistence/two_invocations_vfs | 1.4615 ms |
| sqlite_persistence/memory_db_baseline | 1.1896 ms |

## Parallel sessions (N concurrent over shared VFS)

| Benchmark | Time |
|-----------|------|
| sqlite_parallel/mem/4 | 1.1094 ms |
| sqlite_parallel/vfs/4 | 1.3285 ms |
| sqlite_parallel/mem/16 | 2.8895 ms |
| sqlite_parallel/vfs/16 | 3.2371 ms |
| sqlite_parallel/mem/64 | 10.171 ms |
| sqlite_parallel/vfs/64 | 11.525 ms |
