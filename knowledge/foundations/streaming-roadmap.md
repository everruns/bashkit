---
type: Subsystem Design
title: Streaming Handler Roadmap
description: Live command I/O implementation, whole-buffer boundary audit, and remaining runtime acceptance gates.
tags:
  - bashkit
  - streaming
  - handlers
  - roadmap
---

# Streaming Handler Roadmap

## Status and scope

Implemented the public live command boundary and explicit execution sink/capture
policy. This is an incremental engine change; process substitutions, buffered
utilities and legacy tool/terminal queues remain separate work. Input process
substitutions suppress outer output sinks while collecting their producer bytes,
including incremental builtins and live host commands; this preserves the
existing buffered `/dev/fd` behavior without leaking intermediate output.
Their input captures retain budget-owned bytes with the descriptor instead of
decoding through command-substitution text; NUL and invalid UTF-8 survive.
Native process
spawning, a browser process host, and jobs surviving `exec()` require separate
designs; the default remains an in-process sandbox.

Design direction: prefer one coherent streaming-first API over preserving old
signatures. Breaking API changes are allowed. Measure and document migration
cost; do not retain parallel legacy APIs solely for compatibility. Buffered
capture remains a useful operation in the final API, not a legacy exception.
Request isolation, resource limits and shell semantics remain requirements.

[brush-web PR #12](https://github.com/ShaulLavo/brush-web/pull/12) merged an
owner-approved WASM-first shell plan on 2026-10-16. It retains Bashkit's parser
and ports its bash rules into a new interpreter/engine. This is evidence of a
consumer requirement, not evidence that the replacement engine already works.
The plan identifies four whole-buffer boundaries and gates compatibility work
on browser streaming acceptance tests. Its local spike report is not present in
this repository; the audit below independently checks the named boundaries.

Reviewed Bashkit: `76834b9e09e254cc857410f3ec94b58a9b0b1cee`.
brush-web's inspected Bashkit revision is
`491c3281f668b76d64b15210b8f5d7bf97244be0`; do not assume its findings describe
every later revision unchanged.

## Implemented command boundary

`Command::execute(CommandContext) -> Result<CommandStatus>` exposes shared-cursor
byte input and awaited byte output after descriptor routing. `Bash::execute`
selects async sinks, bounded capture, or discard. The existing `Builtin` value
contract remains for buffered utilities and synchronous hosts; it does not claim
live I/O. Root analysis `CommandContext` is renamed `CommandPlacement`.

The adapter preserves duplicate-fd write ordering, opens redirects before host
invocation, and streams file writes. Pipes strictly bound live writes at 4 KiB;
reader close wakes writers. The request budget and completion guard remain
active through final sink flush; retained handles are revoked on every exit.
Sink errors drop pending engine futures. Sandbox byte caps still bound delivery
independently of a smaller explicit capture cap (TM-INT-009). Deadlines/cancellation cover sinks on
native and JS/WASM executors. Live output cannot safely apply whole-output
transforming after hooks, so those combinations reject before running.

Pipeline futures have a separate heap boundary: their cancellation-guard state
must not enlarge every recursive simple-command poll frame. Both buffered and
live execution must preserve depth-32 substitution tests, including coverage
builds (TM-DOS-089); stack limits and substitution limits are not raised.

List/loop/pipeline accumulation drops bytes already delivered by explicit live
execution. Final streaming filters and registered live commands receive stdin
pipes independently of output destination. A hidden command-substitution scope
must not inherit the surrounding producer's stdout pipe. Accepted output drains
before ordinary engine failure; a failed sink drops the engine immediately. Pipeline scope guards restore child
state on dropped futures. Async output emission stays boxed to respect the
existing native debug-build stack budget. Live execution yields every 64
commands for fairness; buffered synchronous APIs retain their single-poll model.

Background output remains buffered until completion. Jobs finishing after the
last top-level command drain into live stdout/stderr before execution writers
close, including scripts with no explicit `wait`. Job cleanup runs even if that
delivery fails; explicit live execution does not copy delivered job bytes into
its discarded final engine result.

Streaming utilities and host commands share `CommandIo` output destinations.
Their redirects open once; incremental writes, buffered option results and
diagnostics use those same destinations. Saved original stdout/stderr copies
(`exec 3>&1 >log; echo hi >&3`) reach the awaited output pumps, including caps
and sink failure. Top-level list elements flush saved-descriptor output before
the next element can emit, preserving per-destination order.

Internal streaming readers charge each pipe read to aggregate input/work.
`InputChunks` uses capacity-owning byte buffers for records and returned pieces;
`cat` and `tr` whole-input paths keep capture leases while consuming data. A
long delimiter-free record can fail admission even with sink-only execution.
`streaming_audit_tests` pins input/live-byte rejection, session recovery, byte
substitution, saved-fd delivery and gated redirected filters. This does not
establish memory bounds for every buffered utility or tool/terminal queue.

Native Python and NAPI JS expose live commands and async byte sinks on Bash and
BashTool; JS/WASM uses the same core on its cooperative executor. C ABI exposes
output-only synchronous callbacks.

NAPI command I/O uses type-checked opaque `External` handles, matching the
filesystem boundary. Read/write entry points clone Rust capabilities on the JS
thread before spawning futures; pending I/O does not borrow generated JS class
instances. Forged and mismatched handles reject before execution. This removes
the generated class pointer-conversion paths flagged by CodeQL.

CLI oneshot/interactive output uses the new builder. The
[public support matrix](../../docs/streaming.md) distinguishes verified native
and JS/WASM APIs from the remaining tool/Terminal, Pyodide, WASI and Hyperlight
limitations and host-specific edge verification. Process
substitutions are still buffered; the audit below records the original baseline
and remaining roadmap. New tests live in core `streaming_command_tests`, Python
`test_streaming_commands.py`, NAPI `streaming-commands.spec.ts`, WASM
`streaming.test.mjs`, and C ABI/compiled consumer suites.

## Baseline contract before this change

- `Builtin::execute(Context) -> Result<ExecResult>` returns output values.
  `Context::stdin` is borrowed `StreamData`; `CommandResolver::resolve` returns
  an `Arc<dyn Builtin>`, not a process handle.
- `Context::stdin_stream` and `stdout_stream` are crate-private and reached
  through `ShellRef`. A public custom builtin cannot use them. Internal
  `yes`, `seq`, plain `cat`, selected `grep` and `tr` paths already stream over
  bounded pipes. Compound stages can emit at command boundaries.
- `OutputCallback` is synchronous and returns `()`. `exec_streaming` still
  accumulates its final `ExecResult`; observing chunks does not provide an
  awaitable, backpressured sink or eliminate capture.

Sources: [builtin context and traits](../../crates/bashkit/src/builtins/mod.rs),
[dispatch and pipeline routing](../../crates/bashkit/src/interpreter/mod.rs),
[execution options](../../crates/bashkit/src/lib.rs), and
[result types](../../crates/bashkit/src/interpreter/state.rs).

## Four whole-buffer boundaries

| Boundary | Baseline behavior at the inspected revision | Roadmap target |
| --- | --- | --- |
| Final pipeline stage | `execute_streaming_stages` marks only producer commands with `stream_stdout_command`. `execute_dispatched_command_inner` supplies streaming stdin only when a builtin stdout pipe is present. A final simple filter therefore generally reaches `fill_stdin_from_pipe` and waits for EOF; `absorb_stage` keeps its returned stdout. A final compound loop can already stream through the observer. | Route every stage through descriptor-backed readers/writers, including the last stage's output destination. Add a test that observes a final filter's first output while its producer is still running. |
| Generic stdin collection | `fill_stdin_from_pipe` drains `StdinDemand::All` before dispatch. Internal streaming filters bypass it selectively; `Context::stdin_to_end` explicitly collects for buffered options. Unknown/custom commands have no public incremental reader. | Give handlers a reader at dispatch and let the implementation consume on demand. Whole-input utilities explicitly request bounded capture or spill; replacing name-based exceptions must not make `read` overconsume input. |
| Process substitution | `expand_process_substitution` runs `<(...)` to completion into a `String`, then registers `ProcSubData::Input`. `>(...)` stores bytes in a shared `Vec` and queues `deferred_proc_subs` for later execution. Neither is a live pipe. | Use concurrently scheduled substitution tasks and shared descriptor endpoints in both directions. Preserve subshell isolation, fd ownership, EOF, cancellation and byte fidelity. Keep `$(...)` as a separate, explicitly budgeted capture boundary. |
| Accumulated result output | `set_output_callback` explicitly retains output in `ExecResult`; pipeline/list/loop accumulators and finished-job output can also retain data. Output caps limit retained output but do not turn the observer into flow control. | Separate delivery from capture at the execution API and audit every accumulator. Offer a sink-only mode with status/side effects and explicit capture metadata; retain bounded capture where the caller needs a final value. |

All interpreter symbols above are in
[interpreter/mod.rs](../../crates/bashkit/src/interpreter/mod.rs).
Supporting sources: [pipe.rs](../../crates/bashkit/src/interpreter/pipe.rs),
[jobs.rs](../../crates/bashkit/src/interpreter/jobs.rs),
[builtin I/O helpers](../../crates/bashkit/src/builtins/mod.rs), and
[process-substitution VFS](../../crates/bashkit/src/fs/proc_sub_fds.rs).
These paths interact: exposing a streaming handler alone does not remove
final-stage dispatch, substitution, or result-capture buffering.

## Proposed handler contract

Here, handler means a registered/resolved custom command, not `OutputCallback`,
a trap, or an embedded-language callback. Exact Rust types remain open.

1. **Invocation.** Expanded command name/argv, effective exported environment
   including command-prefix assignments, cwd, and the request-scoped VFS view.
   Preserve current resolution precedence. Stream capability belongs to the
   resolved implementation, not a command-name allowlist. Do not expose mutable
   interpreter internals to external handlers; shell-state changes keep their
   existing controlled channels.
2. **I/O.** Async byte reader and separate stdout/stderr writers, backed by
   the same routing abstraction as builtins, redirects and pipes. Specify
   partial reads/writes, explicit EOF and close, and shared endpoint ownership:
   EOF follows the last writer close; broken pipe follows the last reader
   close. A full sink suspends the producer without blocking the executor.
   Redirection and descriptor duplication happen before handler execution;
   nonstandard descriptors must be bound explicitly if supported, otherwise
   rejected explicitly. Preserve byte order per stream; `2>&1` shares one
   ordered destination. No implicit UTF-8 decoding.
3. **Completion.** Streaming handlers return exit status and permitted side
   effects, without returning the emitted bytes a second time. Ordinary command
   failures remain statuses; request cancellation, deadline and budget failures
   remain execution errors. Broken-pipe policy must distinguish a producer
   terminated with 141 from a handler that handles the write error and returns
   its own status. Preserve `PIPESTATUS`, `pipefail`, `!` and `lastpipe` behavior.
4. **Lifecycle.** Reads, writes and host awaits share the originating request's
   cancellation/deadline and revocable capability lease. Closing a request
   wakes blocked operations, drops work and closes endpoints; late writes and
   retained capabilities cannot reach a later request. CPU-only handler work
   must cooperatively yield/check budgets. External host resources need an
   explicit cleanup acknowledgment before request teardown completes.
5. **Memory.** Admit buffer growth before allocation. Define limits for pipe
   storage, pending writes, handler workspaces, capture, and host/worker output
   queues. Bounded pipes alone are insufficient. A slow sink backpressures;
   whole-input operations spill through a quota-controlled facility or fail
   explicitly. Record output truncation separately from successful delivery;
   never silently drop a pipe write to satisfy a capture cap.
6. **Execution and capture.** Use one command I/O contract and a separate
   completion value. Make bounded capture and caller-supplied output sinks
   explicit execution policies; avoid separate legacy and streaming handler
   families. Sink-only execution must not retain a hidden full-output copy.
   Whole-input utilities use an explicit budgeted collection helper. Capturing
   execution can tee delivered bytes into bounded `StreamData` and return
   truncation/status metadata. Migrate `exec_streaming`, bindings and tools to
   the final model together with their published contracts; a synchronous
   observer is not a backpressure adapter. Tool JSON text encoding and
   byte-oriented engine I/O remain distinct boundaries.

The [Request Execution Lifecycle](../security/request-lifecycle.md) and
[Tool Contract](../integrations/tool-contract.md) remain authoritative for
implemented behavior. An optional real-process adapter would additionally need
spawn/wait/kill/readiness, descriptor bindings, and verified teardown; none is
implicitly granted by adding streaming handler I/O.

## Breaking-change policy and public contract

The [public streaming guide](../../docs/streaming.md) is the canonical
consumer-facing API guide, migration reference and runtime support matrix.
The runtime verification matrix below retains test evidence and outstanding checks.
Rust root analysis imports migrate to `CommandPlacement`; live handler
registration is available alongside the buffered utility operation. The command
API and the remaining roadmap have separate support boundaries; sharing the
core does not establish runtime parity.

The largest source changes are custom handlers moving from input/output values
to readers/writers, and binding/host implementations gaining async I/O and
cleanup. Callers that only execute scripts and collect output should have a
small, mechanical migration to explicit capture. Existing script text normally
needs no rewrite; timing, early close and substitution semantics need
differential tests. C ABI changes obey its versioning rules even when Rust and
language APIs break deliberately.

The public guide records per-language signatures, byte types, async restrictions,
capture defaults/caps, completion/errors and cancellation/reentry. Extending a
remaining surface requires its own runnable proof and updated matrix entry.
Do not infer runtime support from compilation.

For release, every published surface must have runtime evidence for its stated
contract. Any unavailable capability must be explicit in the matrix and public
docs; reduced/experimental targets keep their existing qualification. An
unimplemented row cannot silently count as supported. No promise of universal
API parity or new process-host functionality follows from sharing the core.

## Incremental roadmap and acceptance

Current progress: the command boundary, explicit sinks/capture and live final
filter/handler routing are implemented. Whole-input collection helpers, general
fd lifecycle, process substitutions and the remaining tool/job/terminal queues
are still roadmap work. The matrix reports current support independently of
these future acceptance targets.

0. **Contract and surface matrix.** Complete the per-language contract above,
   record before/after migration examples and test ownership for every public
   matrix row, and establish runnable proofs for constrained hosts. Public docs
   and migration guidance are release deliverables, updated with each step.
1. **Public command I/O and capture policy.** Settle reader/writer, completion,
   close, cancellation and capture semantics. Prove a custom handler works as
   producer, intermediate filter and final filter with deterministic readiness
   gates. Provide budgeted whole-input helpers for commands needing them.
2. **General stage routing.** Remove the final-stage and generic-stdin barriers
   together; route sinks, redirects and explicit descriptor inheritance through
   the same I/O contract. Test shell-state isolation and `lastpipe` separately.
3. **Live process substitutions.** Replace both capture/deferred paths with
   scheduled tasks. Test descriptor duplication/last close, nested substitutions,
   early consumer exit, arbitrary bytes and cancellation cleanup.
4. **End-to-end memory proof.** Audit list/loop/job accumulators and binding/tool
   queues in sink-only mode. Demonstrate a retained-memory plateau with growing
   output; report retained capture and peak intermediate memory separately.

Every implementation step needs tests appropriate to its boundary:

- A gated long-lived ticker through a final filter delivers a matching line
  before producer completion. Assert the producing task is still active at
  observation; checking concatenated chunks after completion proves no latency
  property.
- More than 1 MiB, including NUL and invalid UTF-8, crosses builtin/handler,
  handler/builtin, handler/handler and builtin/builtin pipelines unchanged.
  Separate full-capture assertions from sink-only delivery assertions.
- Slow readers and output sinks bound memory and suspend producers; an
  always-ready background task cannot starve foreground progress. Account for
  oversized writes/records and utilities that intrinsically accumulate data.
- Early reader exit wakes blocked writers, releases endpoints/job slots/budget
  leases, and gives correct statuses, including a handler that handles EPIPE.
- Cancellation and deadline expiry during read, write, capture and host awaits
  leave no live work or late output; a subsequent request remains usable.
- Redirects, `2>&1`, substitutions, pipeline state isolation and `lastpipe`
  preserve GNU bash semantics. Compare stdout bytes, stderr where applicable,
  status and `PIPESTATUS` under identical oracle inputs.

Extend the existing [pipeline integration tests](../../crates/bashkit/tests/integration/pipeline_tests.rs)
and [byte-stream tests](../../crates/bashkit/tests/integration/byte_stream_tests.rs);
neither alone establishes all of these gates. New command tests add gated
native and single-threaded WASM proofs, including a real browser main thread and
worker. Process-host tests remain separate and must assert actual children,
interrupt delivery and bounded worker/terminal queues. brush-web's sixteen tests across
seven scenarios are a useful consumer target, not a Bashkit test result.

## Runtime verification matrix

This table records engineering evidence and outstanding runtime checks. The
[public guide](../../docs/streaming.md#runtime-support) describes available APIs
and user-visible limitations without local-run or release-process language.

| Surface | Implemented contract | Evidence / remaining gate |
| --- | --- | --- |
| Rust async library | Live handlers, byte sinks, explicit bounded capture | `streaming_command_tests`: gated final filter, binary >1 MB, routing, broken pipe, deadline, cancellation, revoked handles and recovery. |
| Native Python | Async byte commands and sinks on `Bash`/`BashTool` | `test_streaming_commands.py`; existing output-observer regression tests. Native Linux tested; other wheel platforms use CI. |
| Native Node | Same live command contract on `Bash`/`BashTool` | `streaming-commands.spec.ts`; output observer regressions. Linux addon tested. |
| Bun / Deno native addon | Same NAPI command/sink contract | `runtime-compat/streaming-commands.test.mjs` passes on Bun 1.4.3 and Deno 2.9.6; CI runs the runtime-specific suites. |
| JS/WASM | Async commands/sinks, no threads or COOP/COEP | `streaming.test.mjs`; browser main-thread/worker smoke. Sync remains restricted. |
| NAPI package threaded WASI/browser fallback | Existing generated addon API | New public command/sink wrapper helpers are not exposed through its browser entry. Use `@everruns/bashkit-wasm`; parity needs a separate runtime gate. |
| Other JS / edge hosts | WASM package where required JS facilities exist | Host-specific runtime verification required. |
| CLI / interactive shell | New async byte output driver, without retaining a final output copy | CLI regression suites; stdin is still read within its existing host-input cap. Interactive interrupt and slow-terminal checks remain runtime gates. |
| In-process Terminal | Existing polling and terminal output contract | Terminal regression tests; new `commands` registration and async sink options are not a Terminal API. |
| Rust tools / scripted tools | Existing informational output stream and final tool result | Tool streams still use their existing queue/encoding contract; do not claim end-to-end bounded backpressure. Python/JS `BashTool.execute` byte sinks are supported separately. |
| Native C ABI | `bashkit_execute_streaming`, synchronous byte callbacks, optional capture | ABI tests cover bytes, capture, callback failure and reentry. C consumer/header checks. No custom-command registration. |
| Pyodide / JupyterLite | Existing reduced synchronous API | New async commands/sinks unavailable; actual wheel remains a separate runtime gate. |
| WASI library targets | Shared core source remains portable | Compilation prerequisite only; no published WASI streaming entry point or tested embedding. |
| Non-JS component / Hyperlight | Existing experimental contract | No new component streaming boundary; host and micro-VM tests required before support claims. |

## Local verification (2026-10-16)

- Rust: 3,727 unit tests and 2,351 integration tests pass (four existing ignores).
  Fifteen new streaming command cases cover live delivery, binary routing,
  caps, substitutions, cancellation, sink failure, deadlines and cleanup.
- CLI with realfs: 89 unit and 68 oneshot tests pass, including four 2 MiB
  stack checks and gated partial-line delivery before script completion.
- Native Python 3.12: 851 pass, six optional-dependency skips.
- Native Node: 605 AVA tests and 167 runtime compatibility tests pass. The
  compatibility suite also passes on Bun 1.4.3 (167 tests) and Deno 2.9.6
  (27 tests, 163 nested steps). Deno CI uses `deno test`, not `deno run`.
- JS/WASM: 75 package tests pass. Real Chromium main-thread and module-worker
  tests prove gated first delivery, async sink progress, bytes, abort and reuse.
  Optimized package remains within existing raw/gzip/Brotli transfer budgets.
- C ABI: 20 contract tests pass; C11 and C++17 header consumers compile and the
  new streaming C program runs. Header symbols match the Windows export list.
- Core and binding Clippy checks pass with warnings denied; formatting, Python
  lint, TypeScript wrapper/test type checks, docs links, OKF and workflow parity
  pass. `wasm32-wasip2` core check passes; this is not a runtime support claim.
- Criterion parallel execution and the 96-case Bash comparison harness ran with
  saved reports in their established results directories. All 96 cases match
  GNU Bash output with zero errors. Timings share this development host with
  other checks and are exploratory; no before/after performance claim.
- Rustdoc guide examples pass with their required features (159 tests, 56
  existing ignores). Public site build and API-reference regeneration pass.

## See also

- [Parallel Execution](parallel-execution.md), current scheduling and pipe semantics
- [Builtin Commands](builtins.md), command registration and side effects
- [Known Limitations](../operations/limitations.md), L-PIPE-001, L-PROC-002/003/005 and L-STREAM-001
- [Browser Package](../runtimes/browser-package.md), single-threaded WASM constraints
- [Testing Strategy](../operations/testing.md), differential and integration checks
- [Streaming design and migration](../../docs/streaming.md), public API direction and surface/test matrix
