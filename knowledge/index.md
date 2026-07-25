---
title: Bashkit Knowledge
summary: Persistent, agent-maintained product and engineering knowledge for Bashkit.
tags:
  - bashkit
  - engineering
  - product
  - security
  - operations
---

# Bashkit Knowledge

This directory is Bashkit's canonical [Open Knowledge Format (OKF)](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md) bundle and persistent project memory.

## Contents

The documents record architecture, behavior, constraints, security decisions, testing strategy, release procedures, and intentional limitations. Generated factual inventories live under [`status/`](status/).

## Maintenance contract

- Treat this knowledge as part of the implementation, not as historical documentation.
- Before changing behavior, read the relevant knowledge documents and follow their decisions or update them in the same change.
- When code changes a documented behavior, design decision, invariant, limitation, threat, test strategy, operational process, or generated fact, update the affected knowledge in the same pull request.
- Record important decisions that are not recoverable from code. Prefer links to source and tests over duplicating volatile implementation details.
- Keep stable identifiers such as `TM-*` and `L-*`; never renumber them.
- Add new durable project knowledge here. User-facing guides remain in `docs/`; embedded Rust guides remain in `crates/bashkit/docs/`.
- Every knowledge subdirectory must contain an OKF `index.md` with `title` and `summary` frontmatter.
- Run the relevant drift checks and tests after updating generated or machine-validated knowledge.

## Knowledge map

| Area | Documents |
|---|---|
| Foundations | [Architecture](architecture.md), [parser](parser.md), [virtual filesystem](vfs.md), [builtins](builtins.md), [parallel execution](parallel-execution.md) |
| Security | [Threat model](threat-model.md), [security testing](security-testing.md), [credential injection](credential-injection.md), [request signing](request-signing.md), [HTTP transport](http-transport.md) |
| Runtimes and packages | [Python builtin](python-builtin.md), [TypeScript runtime](zapcode-runtime.md), [SQLite builtin](sqlite-builtin.md), [Python package](python-package.md), [Emscripten wheels](emscripten-wheels.md), [WebAssembly package](browser-package.md) |
| Integrations | [Tool contract](tool-contract.md), [scripted orchestration](scripted-tool-orchestration.md), [Git](git-support.md), [SSH](ssh-support.md) |
| Quality and operations | [Testing](testing.md), [limitations](limitations.md), [documentation](documentation.md), [maintenance](maintenance.md), [release process](release-process.md), [performance results](performance-results.md), [eval](eval.md) |
