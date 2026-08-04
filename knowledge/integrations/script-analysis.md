---
type: Interface Contract
title: Script Analysis
description: Static pre-execution introspection of a script for host permission gating and audit.
tags:
  - bashkit
  - api
  - security
  - permissions
---

# Script Analysis

## Status
Implemented

## Summary

`analyze()` parses a script and reports what it *statically* refers to —
command names, arguments, redirect targets, function definitions, and whether
the script hides work behind expansions. Hosts use it to decide, **before
executing anything**, whether a script needs user approval.

Available as `Bash::analyze` (Rust), `bash.analyze()` (Node), and
`Bash.analyze()` (Python). The same walker backs all three.

## Motivation

Embedders that put bashkit behind an LLM need a permission prompt: "the agent
wants to run `rm -rf /data` — allow?". Answering that requires knowing which
commands a script invokes before it runs. Without a first-class API, hosts
either regex the script text (wrong on quoting, pipelines, substitutions) or
depend on parser internals.

Observed in the wild: `mayneyao/eidos` forked the npm package to expose a raw
AST and reimplemented a walker (`extractCommandNames`, `findFirstCommand`,
`wordText`) to derive permission keys like `eidos:record:update`. That walker
is the API this contract replaces.

Deliberately **not** solved here: exposing the AST itself. The AST is an
internal parser type that changes with parser work; `ScriptAnalysis` is a
narrow, stable projection of it.

## Security posture

**`analyze()` is advisory. It is not a sandbox boundary.**

A script's effective behavior is only knowable at runtime. Static analysis
cannot see through:

- dynamic dispatch — `$cmd foo`, `${arr[0]} foo`, `$(echo rm) -rf /`
- interpreter re-entry — `eval`, `source`, `.`, and any nested `bash`/`sh` (inline `-c` text, a script file, or stdin)
- shell functions and aliases that rebind a name
- wrapper commands that run other commands named in their arguments —
  `xargs`, `env`, `timeout`, `find -exec`, `awk 'system(…)'`. These are **not**
  flagged: they analyze as ordinary commands, so a host that allowlists one must
  treat its arguments as commands itself
- arguments built from variables — `rm "$target"`

The API reports these as *unknown*, never as *safe*: a command whose name is
not statically determined has `name == null`, and the script-level
`has_dynamic_commands` / `has_interpreter_reentry` flags are set. A host that treats "no
recognized dangerous command" as "safe" without checking those flags builds a
bypassable gate.

Enforcement primitives remain: the builtin registry (a command that does not
exist cannot run), `NetworkAllowlist`, the filesystem mount policy, and the
`before_tool` hook, which fires with the **resolved** command name at
execution time. Recommended pattern: `analyze()` for the pre-execution UX
decision, `before_tool` for the enforcement backstop.

Recorded as TM-ESC-032 in [Threat Model](../security/threat-model.md).

## Contract

### Entry points

| Language | Call | Returns |
|---|---|---|
| Rust | `bash.analyze(script)` / `analysis::analyze(script)` / `analysis::analyze_with_limits(script, depth, fuel)` | `Result<ScriptAnalysis>` |
| Node | `bash.analyze(script)` | `ScriptAnalysis` (throws on parse error) |
| Python | `bash.analyze(script)` | `ScriptAnalysis` (raises `BashError` on parse error) |

`Bash::analyze` uses the instance's configured parser limits; the free
functions use defaults. Analysis is pure — no VFS, network, environment, or
shell-state access, no mutation of the instance, and it never executes
anything.

### `ScriptAnalysis`

| Field | Type | Meaning |
|---|---|---|
| `commands` | `AnalyzedCommand[]` | Every simple command found, in source order |
| `redirects` | `AnalyzedRedirect[]` | File redirect targets, in source order |
| `functions` | `string[]` | Function names defined by the script |
| `has_dynamic_commands` | `bool` | Some command name is not statically known |
| `has_command_substitution` | `bool` | Script contains `$(…)`, backticks, or process substitution |
| `has_interpreter_reentry` | `bool` | Script hands a script back to the interpreter: `eval`, `source`, `.`, or a nested `bash`/`sh` |
| `truncated` | `bool` | Node budget hit; lists are incomplete |

### `AnalyzedCommand`

| Field | Type | Meaning |
|---|---|---|
| `name` | `string \| null` | Command name if fully literal, else `null` |
| `args` | `(string \| null)[]` | One entry per argument; `null` when not fully literal |
| `context` | `CommandContext` | Where the command sits |
| `assignments` | `string[]` | Names of prefix assignments (`FOO=1 cmd`) |

`CommandContext` is `Direct` (runs when the script runs), `Substitution`
(inside `$(…)`, backticks, or process substitution), or `FunctionBody` (inside
a function definition, so it runs only if the function is called).

### `AnalyzedRedirect`

| Field | Type | Meaning |
|---|---|---|
| `path` | `string \| null` | Target path if fully literal, else `null` |
| `mode` | `Read \| Write \| Append` | Access implied by the operator |

`>`, `>|`, and `&>` are `Write`; `>>` is `Append`; `<` is `Read`. Fd
duplications (`2>&1`), here-documents, and here-strings are not file targets and
are omitted. The parser does not support `<>`, so no read-write mode exists.

### Decisions

- **Literal-or-null.** A word yields `Some(text)` only when every part is a
  literal. Partial reconstruction (`"/tmp/$x"` → `"/tmp/"`) is deliberately not
  offered: it reads as a path but is not one, which is exactly the trap this
  API exists to remove.
- **Substitutions are walked.** Commands inside `$(…)` and `<(…)` appear in
  `commands` with `context = Substitution`. `echo $(rm -rf /)` must not look
  like a bare `echo`.
- **Function bodies are walked** and tagged, not skipped: a host that only
  wants what runs now filters on `context`, and a host that wants "everything
  this script could do" does not have to re-walk.
- **Flat list, source order.** No tree. Every consumer seen so far wants "the
  set of commands"; a tree would leak AST shape back into the public API.
- **Assignment-only commands** (`FOO=1`) report `name == null` with a
  non-empty `assignments` list and do **not** set `has_dynamic_commands` —
  nothing is hidden. `AnalyzedCommand::is_assignment_only()` (Rust) /
  `isAssignmentOnly` (Node) / `is_assignment_only` (Python) tells them apart
  from a genuinely unknown name.
- **Parse failure is an error, not an empty result.** An unparseable script
  must not analyze as "no commands"; hosts are expected to deny or prompt.
- **Node budget.** The walk stops after `MAX_ANALYSIS_NODES` (4096) recorded
  commands or redirects and sets `truncated`. Depth is already bounded by the
  parser (`HARD_MAX_AST_DEPTH` = 500) and total work by parser fuel.
  `truncated` means "incomplete" and hosts must treat it like a dynamic
  command.

## Verification

- `crates/bashkit/src/analysis.rs` — unit tests for word extraction, each AST
  node kind, contexts, redirect modes, and budget truncation.
- `crates/bashkit/tests/integration/script_analysis.rs` — end-to-end behavior,
  evasion cases (dynamic dispatch, eval, nested shells, nested substitution,
  function rebinding), malformed literal-boundary rejection, and the
  `before_tool` backstop pairing.
- `crates/bashkit/tests/proptest_security.rs` — invariants: never panics, plain
  command names come from a contiguous source span, respects the node budget,
  every dispatched command is reported for a transparent script, and
  runtime-resolved scripts are opaque. Quote removal and escapes can
  legitimately join multiple source spans into one literal command name.
- `crates/bashkit/fuzz/fuzz_targets/analyze_fuzz.rs` — libFuzzer target (in the
  `fuzz.yml` matrix) asserting the same plain-name and budget invariants;
  malformed normalization syntax has deterministic parser regressions.
- `crates/bashkit-js/__test__/analyze.spec.ts` and
  `__test__/runtime-compat/analyze.mjs` — Node/Bun/Deno surface.
- `crates/bashkit-python/tests/test_analyze.py` — Python surface.
- `crates/bashkit/docs/script-analysis.md` — rustdoc guide, doctested.

## See also

- [Tool Contract](tool-contract.md)
- [Threat Model](../security/threat-model.md)
- [Testing Strategy](../operations/testing.md)
