---
type: Subsystem Design
title: Parallel Execution
description: Threading model, shared ownership, and concurrency safety requirements.
tags:
  - bashkit
  - concurrency
  - performance
---

# Parallel Execution

## Status
Implemented

## Threading Model

- Single `Bash` instance: sequential (`&mut self`)
- Multiple `Bash` instances: parallel via `tokio::spawn`
- Filesystem: thread-safe via `Arc<dyn FileSystem>` + `RwLock`
- `Arc::clone(&fs)` shares one filesystem across instances; instances run in parallel sharing it

## Background Jobs

`cmd &` runs concurrently inside one `exec()`, on by default
(`BashBuilder::concurrent_jobs(false)` restores sequential execution).

- A job is a forked interpreter (`Interpreter::fork_for_job`): copied
  variables, functions, cwd and options; shared filesystem, execution budget,
  cancel token, hooks and extensions. Changes inside the job never reach the
  parent, like a forked subshell.
- Jobs are futures in a `FuturesUnordered` owned by `JobControl`
  (`interpreter/jobs.rs`), polled by `with_jobs` alongside the foreground
  future on the same task. No `tokio::spawn`: works on wasm and inside the
  in-process `Terminal`, and needs no `Send + 'static` runtime handle.
- At spawn the job is polled once. A job that never blocks finishes right
  there, so `echo a & echo b` keeps bash's usual order.
- Output of a job that finishes later is captured and delivered at `wait`, at
  each top-level command boundary, and at the end of `exec()`. Streams are
  not interleaved byte by byte.
- `exec()` waits for every job (`finish_all`) before returning; nothing
  outlives the call. Within an interactive session this means a job started
  at one prompt finishes before the next prompt.
- Jobs get virtual PIDs from 1001. `$$` is 1. `ps`, `pgrep`, `pkill`, `kill`
  and `jobs` read the same table. `kill` aborts the job's future; its status
  becomes 128+signal.
- Limit: `max_background_jobs` (TM-DOS-122).

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

## See also

- [Bashkit Architecture](architecture.md), shared-ownership decisions this builds on
- [Builtin Commands](builtins.md), builtin execution under concurrency
- [Performance Results](../operations/performance-results.md), where benchmark results are kept
- [Testing Strategy](../operations/testing.md), concurrency test expectations
