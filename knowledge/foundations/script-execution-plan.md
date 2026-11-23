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

Experimental, isolated on `experiment/script-execution-plan`. The
`experimental_execution_plan` Cargo feature is off by default. This is not a
production compiler or a new public execution API.

## Decision

Compile a parsed script into an execution-owned plan before running its first
command. The ordinary script runner still handles alias/extglob reparsing,
source lines, output routing, hooks, trailing syntax errors, and shell recovery.
An allocation-free eligibility probe skips lowering for small one-shot regions;
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
and background-job handling around compiled regions. A compound containing an
unsupported body falls back as a whole. Functions, pipelines, word expansions
(including `$((...))`), and general tool-containing loop bodies are not lowered
in this first version.

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
change shell options or variable attributes safely. Failpoint builds use the
ordinary command executor.

## Resources and lifetime

Plans borrow their parsed script and are dropped on every exit path, including
future cancellation. They introduce no interpreter fields, process-global
cache, or pointer-keyed state. Optional storage reservations may decline and
fall back without exhausting the request. Compilation charges work before
tokenization/lowering; storage is conservatively admitted before allocations.
Compiler traversal, expression length, token count, and instruction-stack depth
are bounded.

One growable optional-storage lease covers the instruction arena. Capacity grows
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
