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
| `parallel_scaling/*` | Scaling with session count |
| `single_*` | Individual operation overhead |

### Expected Results

- Light workload: ~2x parallel speedup
- Medium workload: ~4x parallel speedup
- Heavy workload: ~7x parallel speedup

Must not degrade. Compare before/after.
