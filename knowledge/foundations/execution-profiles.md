---
type: Feature
title: Execution Profiles
description: Typed policy bundles spanning Bashkit execution, memory, filesystem, network, and embedded-runtime limits.
tags:
  - bashkit
  - policy
  - security
  - limits
---

# Execution Profiles

## Decision

`ExecutionProfile` is the single typed aggregate for policy defaults. A closed
`ExecutionProfileName` selects `Hardened`, `Standard`, or `Interactive`; a
validated `ExecutionProfileBuilder` replaces individual policy families with
the existing typed limit objects. This avoids a second per-field configuration
language and keeps each limit type as its canonical source of defaults.

Profiles establish a baseline. Existing builder methods are explicit,
last-write overrides. Call `.profile(...)` before `.limits(...)`,
`.memory_limits(...)`, `.filesystem_limits(...)`, `.network(...)`, or embedded
runtime registration when mixing the surfaces.

## Named semantics

- `Standard` exactly matches current secure library defaults: bounded
  execution/session/memory/VFS, writable isolated VFS, network disabled, and
  default limits for compiled embedded runtimes. It is the default.
- `Hardened` tightens every resource family and runtime budget. It keeps the
  isolated VFS writable: VFS mutation is a safe and useful capability under
  byte/count quotas, so a resource profile must not silently remove it.
- `Interactive` exactly preserves CLI/REPL intent: relaxed execution counters
  and unlimited session counters, while memory, VFS, network, and runtime
  defaults remain secure. One-shot CLI mode uses `Standard`.

Named profiles never enable network access. Hosts may explicitly override the
destination family with a typed `ProfileNetworkPolicy::Allowlist` (Rust) or the
existing binding-specific network configuration. When `http_client` is
compiled, profiles also select typed HTTP timeout/response-size limits;
`Hardened` tightens both without granting a destination capability.

## Filesystem boundary

Profile filesystem quotas apply to the builder-managed `InMemoryFs`. A custom
`FileSystem` passed through `BashBuilder::fs` owns its quota contract and
replaces this family; Bashkit cannot truthfully impose in-memory accounting on
an arbitrary backend. Real read-write mounts retain their documented host-disk
responsibility.

`ScriptedTool` and `ScriptingToolSet` accept profiles for execution, session,
and memory policy, but always retain logic-only mode: filesystem, network, and
embedded-runtime capabilities stay unavailable.

## Validation and bindings

`ExecutionProfileBuilder::build()` rejects contradictory VFS bounds, AST depth
above the parser hard cap, zero time/runtime budgets, and network configuration
when `http_client` is unavailable (TM-DOS-097). Named profiles are valid by
construction.

Supported surfaces: Rust `BashBuilder`, `BashToolBuilder`,
`ScriptedToolBuilder`, and `ScriptingToolSetBuilder`; CLI; Python; native Node
/ Bun / Deno; browser WASM; and C ABI v1 JSON configuration. Binding selectors
are closed enums/unions and their existing per-field options override the
profile. Browser WASM has no network or embedded runtimes by platform contract;
C ABI v1 has no network/runtime callback surface; scripted tools remain
logic-only as described above.

## Verification

`execution_profile_tests` covers Standard/Interactive parity, Hardened VFS
quota behavior, aggregate-budget parity, explicit override ordering, and
TM-DOS-097 validation.

## See also

- [Architecture](architecture.md)
- [Virtual Filesystem](vfs.md)
- [Threat Model](../security/threat-model.md)
- [Interactive Shell](../integrations/interactive-shell.md)
