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
  flagged opaque: they analyze as ordinary commands, so a host that allowlists
  one must treat its arguments as commands itself. `ScriptAnalysis::command_wrappers()`
  reports which prefix-style wrappers a script uses, and `analysis::COMMAND_WRAPPERS` /
  `is_command_wrapper()` publish the list, so hosts do not hardcode a private
  copy that drifts from ours. `find -exec` and language payloads
  (`awk 'system(…)'`) are outside that list — they need per-tool argument
  knowledge. `time` is absent by design: it is a keyword, so the timed command
  is already the reported command name
- arguments built from variables — `rm "$target"`

The API reports these as *unknown*, never as *safe*: a command whose name is
not statically determined has `name == null`, and the script-level
`has_dynamic_commands` / `has_interpreter_reentry` flags are set. A host that treats "no
recognized dangerous command" as "safe" without checking those flags builds a
bypassable gate.

Enforcement primitives remain: the builtin registry (a command that does not
exist cannot run — unless a `CommandResolver` is installed, see below),
`NetworkAllowlist`, the filesystem mount policy, and the `before_tool` hook,
which fires with the **resolved** command name at execution time. Recommended
pattern: `analyze()` for the pre-execution UX decision, `before_tool` for the
enforcement backstop.

### `CommandResolver` and the enumerable-name assumption

`BashBuilder::command_resolver` installs a last-chance resolver, consulted only
after every other dispatch route misses and immediately before the 127 path. It
grants no new capability — a resolver is embedder-supplied host code exactly
like a builtin passed to `BashBuilder::builtin`, which could already spawn
processes. What it changes is narrower and worth stating plainly:

- **The registry stops bounding the reachable name set.** "A command that does
  not exist cannot run" holds only while every name that reaches host code is
  registered. `Bash::builtin_names()` does not enumerate resolver-provided
  names, so a gate built on that list is incomplete once a resolver exists.
- **`before_tool` is unaffected.** Resolved commands dispatch through the same
  builtin path as every other builtin, so the hook fires with the resolved name
  and can cancel the call. The recommended pattern above holds unchanged.
  `command_resolver_tests::before_tool_veto_blocks_a_resolved_command` pins this.

A resolver that gates internally (returning `None` for names it will not run)
restores a closed set; that is the recommended shape when the embedder has a
policy to enforce.

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
| `has_command_substitution` | `bool` | Script contains `$(…)`, including inside arithmetic expansion, backticks, or process substitution |
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
  like a bare `echo`. The parser stores arithmetic expansion as an expression
  string, so an embedded `$()` or backtick substitution cannot be walked from the AST; analysis instead
  sets both `has_command_substitution` and `has_dynamic_commands`, making the
  script opaque before runtime reparses and executes that substitution.
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
  function rebinding), reserved-control-byte and malformed literal-boundary
  analysis rejection, and the
  `before_tool` backstop pairing.
- `crates/bashkit/tests/proptest_security.rs` — invariants: never panics,
  command-name characters come from the source in order, respects the node
  budget, every dispatched command is reported for a transparent script, and
  runtime-resolved scripts are opaque. Quote removal, backtick syntax, and
  escapes can legitimately join multiple source spans; ANSI-C `$'...'` can
  decode characters absent from the source and is explicitly exempt.
- `crates/bashkit/fuzz/fuzz_targets/analyze_fuzz.rs` — libFuzzer target (in the
  `fuzz.yml` matrix) asserting the same ordered-name and budget invariants;
  malformed normalization syntax has deterministic parser regressions.
- `crates/bashkit-js/__test__/analyze.spec.ts` and
  `__test__/runtime-compat/analyze.mjs` — Node/Bun/Deno surface.
- `crates/bashkit-python/tests/test_analyze.py` — Python surface.
- `crates/bashkit/docs/script-analysis.md` — rustdoc guide, doctested.

## See also

- [Tool Contract](tool-contract.md)
- [Threat Model](../security/threat-model.md)
- [Testing Strategy](../operations/testing.md)
