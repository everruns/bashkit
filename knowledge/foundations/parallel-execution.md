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

## Pipelines

- Every stage of a multi-command pipeline runs in a subshell: variables,
  cwd, options and fds changed in a stage are rolled back after it, and
  `exit`/`return` end only that stage. `shopt -s lastpipe` keeps the last
  stage in the current shell (bash does this when job control is off, which
  is always the case here). So `echo x | read v` leaves `v` unset, as in bash.
- Leading stages that are one builtin call (`echo`, `grep`, `seq` ...) run
  first, one after another, on the cheap subshell snapshot
  (`enter_pipeline_stage`/`exit_pipeline_stage`): a builtin produces its
  output as one value anyway, and this keeps `echo x | grep x` fork-free.
- The generators `yes` and `seq` (unredirected, not shadowed by a
  function) also start the concurrent part: as a producer stage they write
  4 KiB chunks straight into the pipe through `Context::stdout_stream`
  (the interpreter hands the pipe only to the stage's own simple command,
  matched by address, never to a command run from its arguments), so
  `seq 1000000 | head -1` stops after one chunk with `PIPESTATUS` `141 0`.
  Their output caps still apply when the reader takes everything.
- Plain `cat` (no display flags) streams the same way and is also a
  streaming filter: the interpreter hands it its input pipe through
  `Context::stdin_stream` instead of collecting stdin up front, so
  `while :; do echo; done | cat | head -1` stops with SIGPIPE. `cat` with
  flags drains the input pipe and runs buffered.
- From the first stage that runs shell code (loop, group, function, `eval`,
  nested shell), the rest run concurrently (`execute_streaming_stages`, on
  with `concurrent_jobs`). Each non-last stage is a forked interpreter like a
  background job; the last stays in this shell so its output streams to the
  caller. All stage futures are polled on the caller's task, readers first.
- Stages talk through `interpreter/pipe.rs`: a 4 KiB shared buffer. A writer
  appends at command boundaries (through the streaming output hook, so
  command substitutions and redirects stay out of the pipe) and waits at its
  next command while the buffer is full. Before a command in a reading stage
  takes stdin, `fill_stdin_from_pipe` pulls what it needs: one line for
  `read`, N lines/bytes for `head` (then closes the read end), nothing for
  commands that never read stdin, all of it otherwise.
- When the reader is gone (stage finished, or `head` done), the writer's next
  append fails and its next command ends the stage with 141, like SIGPIPE:
  `while :; do echo x; done | head -n 2` prints two lines and `PIPESTATUS`
  is `141 0`.
- Concurrent nesting costs native stack (each level polls the next from its
  own poll), so a pipeline nested more than 4 subshells deep runs its stages
  in sequence (TM-DOS-124). `concurrent_jobs(false)` runs every stage in
  sequence. Gaps: L-PIPE-001.

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
