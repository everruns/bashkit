# Parallel Execution

## Threading Model

- Single `Bash` instance: sequential (`&mut self`)
- Multiple `Bash` instances: parallel via `tokio::spawn`
- Filesystem: thread-safe via `Arc<dyn FileSystem>` + `RwLock`
- `Arc::clone(&fs)` shares one filesystem across instances; instances run in parallel sharing it

## Benchmark

Run `cargo bench --bench parallel_execution` when changes touch:
- `Arc`, `RwLock`, shared state
- `Interpreter`, `Bash`, `FileSystem`
- Async paths (`tokio::spawn`, `.await`)
- Builtins (grep, awk, sed, etc.)

### Key Metrics

| Benchmark | What it measures |
|-----------|------------------|
| `workload_types/*` | Parallel vs sequential speedup |
| `parallel_scaling/*` | Scaling with session count (10–1000 sessions) |
| `single_*` | Individual operation overhead |

### Correctness at Scale

Throughput numbers are meaningless if sessions silently error out. The
`parallel_sessions_tests` integration suite asserts that a 1000-session
fan-out (each its own `Bash`, sharing one `Arc<dyn FileSystem>`) actually
produces correct per-session output, and that concurrent sessions sharing a
filesystem don't cross-contaminate. Run via `just test` (no extra features).


### Expected Results

- Light workload: ~2x parallel speedup
- Medium workload: ~4x parallel speedup
- Heavy workload: ~7x parallel speedup

Must not degrade. Compare before/after.
