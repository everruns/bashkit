# Parallel Execution

## Threading Model

- Single `Bash` instance: sequential (`&mut self`)
- Multiple `Bash` instances: parallel via `tokio::spawn`
- Filesystem: thread-safe via `Arc<dyn FileSystem>` + `RwLock`

## Arc Usage

`Arc::new()` enables shared ownership of filesystem across instances without cloning.

```rust
let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
let bash1 = Bash::builder().fs(Arc::clone(&fs)).build();
let bash2 = Bash::builder().fs(Arc::clone(&fs)).build();
// bash1 and bash2 can run in parallel, sharing fs
```

## Benchmark

Run when changes touch:
- `Arc`, `RwLock`, shared state
- `Interpreter`, `Bash`, `FileSystem`
- Async paths (`tokio::spawn`, `.await`)
- Builtins (grep, awk, sed, etc.)

```bash
cargo bench --bench parallel_execution
```

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
