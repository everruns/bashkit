---
type: Experimental Design
title: Script Execution Plan Experiment
description: Execution-owned compilation of supported Bash command trees with interpreter fallback.
tags:
  - bashkit
  - interpreter
  - performance
  - experiment
---

# Script Execution Plan Experiment

## Status

Experimental opt-in compiler, shipped behind the disabled-by-default
`experimental_execution_plan` Cargo feature. All embedding crates forward the
flag; the existing public execution APIs and Bash syntax stay the same.

## Decision

Compile a parsed script into an execution-owned plan before running its first
command. The ordinary script runner still handles alias/extglob reparsing,
source lines, output routing, hooks, trailing syntax errors, and shell recovery.
An inexpensive eligibility probe skips lowering for small one-shot regions;
supported loops and larger straight-line regions still compile. This is a
conservative local cost policy, not profiling or persistent hotness state.
Reparsed script views use ordinary execution. Nested `eval`/`source` scripts
build their own local plans through the same entry point.

Supported scalar command trees lower to a flat instruction vector with explicit
branches, jumps, arithmetic operations, and loop boundaries. Numeric literal
assignments use the existing assignment executor. The instruction evaluator
uses the existing arithmetic variable resolver and ordered write overlay;
variable values are read during execution.

All native regions share one instruction arena; each stores a range into it.
Arithmetic tokens borrow source text where possible and transfer owned names
instead of cloning them. The boxed plan keeps its storage out of the ordinary
script future. Ordinary and compiled command/list dispatch use separate futures,
with shared macros preserving identical command boundaries and orchestration.
A fallback command immediately enters the ordinary dispatcher and does not
carry a prepared-program selector through its nested execution.

Mixed command lists retain the existing list orchestration with prepared child
commands. This preserves conditional execution, builtin/tool calls, redirects,
and background-job handling around compiled regions. Shell control nodes carry
prepared child sequences, including `if`, word and arithmetic `for`, `while`,
`until`, and braces. Their output routing, condition
suppression, traps, loop control and error cleanup use the same orchestration
as ordinary execution. Individual unsupported leaves (tools, pipelines,
substitutions, regexes and arrays) retain their existing executor. Safe arithmetic
`for` clauses are compiled independently and recheck variable attributes, values
and dynamic shell state before each evaluation.
Controls with no compiled child or clause retain ordinary execution, preserving
synchronous limit reporting and avoiding an empty prepared dispatcher.

Profitable function bodies receive a fresh invocation-owned plan after entering
the normal call frame. Small functions retain the ordinary boxed command future.
Plans borrow the definition through the invocation; redefinition and recursion
cannot reuse stale code. Compilation errors flow through normal frame cleanup,
and abandoning the future releases its plan storage. No plan is stored in the
function table or interpreter. Redirected compound boundaries still fall back.

The `jq` builtin already compiles filters using jaq and reuses its bounded,
execution-local LRU. Planned functions and loops call that same backend. Input,
`--arg` bindings, environment and filter files are resolved at each actual call;
filter compilation stays lazy, preserving branch behavior and diagnostic order.

## Eligibility and fallback

Native regions support arithmetic commands, numeric scalar assignments, brace
groups, `if`/`elif`/`else`, arithmetic `for`, `while`, and `until`. Arithmetic
supports safe scalar operators, assignment, increments, and comma sequences.
Unsupported operators and grammar use the ordinary evaluator.

Runtime admission declines native regions with tracing, traps, `errexit`,
`nounset`, no-exec, active output pipes, pending substitution errors, variable
attributes beyond export, arrays, namerefs, or expression-valued variables.
These are conservative backend limits, not changes to accepted Bash syntax.
Admission runs when the region executes, so a preceding fallback command can
change shell options or variable attributes safely. Resource failpoints remain
active in native execution. When the
`interp::execute_command` injector is configured, admission declines so its
whole-command error and early-status actions retain their AST boundary.

## Resources and lifetime

Plans borrow their parsed script or active function definition and are dropped
on every exit path, including future cancellation. They introduce no interpreter
fields, process-global cache, or pointer-keyed state. Optional storage reservations may decline and
fall back without exhausting the request. Compilation charges work before
tokenization/lowering; storage is conservatively admitted before allocations.
Compiler traversal, expression length, token count, and instruction-stack depth
are bounded.

One growable optional-storage lease covers the instruction arena. Token and
expression-op admission includes geometric vector growth and source text, using
their actual Rust type layouts. Control-node boxes and exact child/block vectors
are admitted before allocation. Command/child vectors allocate their admitted
capacity explicitly; a 1 KiB tokenizer-boundary regression checks transient
storage even when compilation ultimately declines. Capacity grows
geometrically, and rejected lowering rolls back instructions while retaining
the reservation for allocated capacity. Buffers drop before the lease releases
their accounting, including failed compilation; a drop-order regression test
checks this invariant.

The VM charges every executable command node and loop iteration, checks shared
cancellation/deadline/memory state between instructions, yields periodically,
and restores active loop counters when execution returns an error. Arithmetic
uses the normal recursion/fuel budget and variable-write checks. Compilation
has real work/live-memory costs, so tight budgets can stop execution earlier
than the ordinary interpreter.

## Validation and performance gates

PR CI runs the full workspace and optional-runtime test slices with the feature
both disabled and enabled, including CLI/C ABI, binding suites, browser tests,
realfs, strict host-tool oracles, failpoints and security properties. Nightly
ASan, Miri admission/lifetime checks, tenant soaks and Windows suites also cover
both modes. Manifest-discovered fuzz targets run bounded PR smoke cases in both
modes and longer scheduled campaigns. The dedicated `script_plan_fuzz` target
uses finite supported loops/branches plus mixed functions with substitutions,
regex captures and locals. An execution-local interpreter selector compares
output, status, writes and subsequent session use. Structured proptest scripts
and mixed-function fixtures also compare with real Bash. Native-specific unit tests prove
backend eligibility, resource fault injection, compilation admission refunds,
and abandoned-future cleanup instead of accepting silent AST fallback.

Unit tests require native regions to be selected and compare results, variable
writes, PIPESTATUS, command counts, and scope cleanup with ordinary AST
execution. Public-API differential probes cover mixed scripts and fallback
contexts. Baseline/candidate binaries must use identical dependency lockfiles
and compiler/profile settings.

The additional `experimental_plan_benchmark_control` feature is diagnostic only
and is also off by default. A typed execution extension selects automatic plan
admission or interpreter-only execution in one binary. Both benchmark modes
attach the same extension type; no shared switch or cache is introduced. Normal
experimental builds contain no selector lookup. Same-binary controls complement,
and do not replace, comparisons with a production baseline build.

Measure parsing, compilation, and execution together on fresh Bash instances.
Report short/one-shot scripts and fallback workloads alongside supported hot
regions. A supported-loop speedup alone is insufficient to enable the feature
by default. Before production, broaden differential/resource coverage, reduce
compilation overhead, and audit native command-boundary equivalence.

## See also

- [Architecture](architecture.md)
- [Parser](parser.md)
- [Security Testing](../security/security-testing.md)
- [Performance Results](../operations/performance-results.md)
- [Experiment results and remaining gates](../../experiments/script-execution-plan/README.md)
