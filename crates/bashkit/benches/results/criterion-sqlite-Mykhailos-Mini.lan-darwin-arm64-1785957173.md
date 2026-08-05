# Criterion SQLite Builtin Benchmark

Measures the `sqlite` builtin (Turso embedded engine) end-to-end through
the bashkit interpreter. Per-invocation overhead (interpreter setup, script
parse, engine open, VFS flush) is included in every number — these are
"what a script author observes", not isolated engine micro-benchmarks.

## System Information

- **Moniker**: `Mykhailos-Mini.lan-darwin-arm64`
- **Hostname**: Mykhailos-Mini.lan
- **OS**: darwin
- **Architecture**: arm64
- **CPUs**: 12
- **Timestamp**: 1785957173

## CRUD (insert / update, Memory vs Vfs backend, n rows)

| Benchmark | Time |
|-----------|------|
| sqlite_crud/insert_mem/100 | 172.03 µs |
| sqlite_crud/insert_vfs/100 | 215.20 µs |
| sqlite_crud/update_mem/100 | 174.84 µs |
| sqlite_crud/update_vfs/100 | 216.36 µs |
| sqlite_crud/insert_mem/1000 | 172.79 µs |
| sqlite_crud/insert_vfs/1000 | 216.32 µs |
| sqlite_crud/update_mem/1000 | 174.14 µs |
| sqlite_crud/update_vfs/1000 | 216.38 µs |
| sqlite_crud/insert_mem/10000 | 169.97 µs |
| sqlite_crud/insert_vfs/10000 | 211.62 µs |
| sqlite_crud/update_mem/10000 | 170.51 µs |
| sqlite_crud/update_vfs/10000 | 212.72 µs |

## Indexing (create index, indexed lookup, full scan)

| Benchmark | Time |
|-----------|------|
| sqlite_index/create_index_mem/100 | 170.70 µs |
| sqlite_index/indexed_lookup_mem/100 | 169.90 µs |
| sqlite_index/full_scan_mem/100 | 169.51 µs |
| sqlite_index/create_index_mem/1000 | 170.21 µs |
| sqlite_index/indexed_lookup_mem/1000 | 171.17 µs |
| sqlite_index/full_scan_mem/1000 | 170.12 µs |
| sqlite_index/create_index_mem/10000 | 170.25 µs |
| sqlite_index/indexed_lookup_mem/10000 | 174.37 µs |
| sqlite_index/full_scan_mem/10000 | 169.42 µs |

## Query (GROUP BY aggregate)

| Benchmark | Time |
|-----------|------|
| sqlite_query/aggregate_mem/100 | 171.25 µs |
| sqlite_query/aggregate_vfs/100 | 213.76 µs |
| sqlite_query/aggregate_in_memory/100 | 162.06 µs |
| sqlite_query/aggregate_mem/1000 | 170.31 µs |
| sqlite_query/aggregate_vfs/1000 | 211.66 µs |
| sqlite_query/aggregate_in_memory/1000 | 162.62 µs |
| sqlite_query/aggregate_mem/10000 | 170.30 µs |
| sqlite_query/aggregate_vfs/10000 | 211.82 µs |
| sqlite_query/aggregate_in_memory/10000 | 161.48 µs |

## Output mode formatters (1k rows)

| Benchmark | Time |
|-----------|------|
| sqlite_output_mode/list | 169.83 µs |
| sqlite_output_mode/csv | 176.13 µs |
| sqlite_output_mode/json | 169.86 µs |
| sqlite_output_mode/markdown | 169.71 µs |
| sqlite_output_mode/box | 170.12 µs |

## Persistence (cost per invocation)

| Benchmark | Time |
|-----------|------|
| sqlite_persistence/two_invocations_mem | 185.15 µs |
| sqlite_persistence/two_invocations_vfs | 364.51 µs |
| sqlite_persistence/memory_db_baseline | 163.62 µs |

## Parallel sessions (N concurrent over shared VFS)

| Benchmark | Time |
|-----------|------|
| sqlite_parallel/mem/4 | 278.07 µs |
| sqlite_parallel/vfs/4 | 423.87 µs |
| sqlite_parallel/mem/16 | 761.56 µs |
| sqlite_parallel/vfs/16 | 995.30 µs |
| sqlite_parallel/mem/64 | 2.6410 ms |
| sqlite_parallel/vfs/64 | 3.6123 ms |
