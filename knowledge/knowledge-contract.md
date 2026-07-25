---
type: Playbook
title: Knowledge Maintenance Contract
description: Rules for maintaining the Bashkit knowledge bundle and its OKF conformance.
tags:
  - bashkit
  - knowledge
  - okf
  - process
---

# Knowledge Maintenance Contract

`knowledge/` is Bashkit's canonical [Open Knowledge Format (OKF) v0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
bundle and persistent project memory.

## Maintenance rules

- Treat this knowledge as part of the implementation, not as historical documentation.
- Before changing behavior, read the relevant knowledge documents and follow their decisions or update them in the same change.
- When code changes a documented behavior, design decision, invariant, limitation, threat, test strategy, operational process, or generated fact, update the affected knowledge in the same pull request.
- Record important decisions that are not recoverable from code. Prefer links to source and tests over duplicating volatile implementation details.
- Keep stable identifiers such as `TM-*` and `L-*`; never renumber them.
- Add new durable project knowledge here. User-facing guides remain in `docs/`; embedded Rust guides remain in `crates/bashkit/docs/`.
- Run the relevant drift checks and tests after updating generated or machine-validated knowledge.

## OKF conformance rules

The bundle targets OKF v0.2, declared as `okf_version: "0.2"` in the bundle-root [`index.md`](index.md).

- Every `.md` file except the reserved `index.md` and `log.md` is a **concept document** and MUST start with a YAML frontmatter block containing a non-empty `type`.
- `title`, `description`, and `tags` are recommended and used throughout this bundle; `description` is a single sentence that index entries reuse verbatim.
- `index.md` files carry **no** frontmatter, except the bundle-root `index.md`, which may carry only `okf_version`.
- `index.md` bodies are link lists grouped under headings: `* [Title](path) - description`.
- `log.md` bodies are date-grouped entries (`## YYYY-MM-DD`), newest first.
- Prose that is not a directory listing belongs in a concept document, not in an `index.md`.

Type values are producer-defined. This bundle uses: `Architecture`, `Subsystem Design`,
`Interface Contract`, `Package Design`, `Threat Model`, `Test Strategy`, `Limitations`,
`Playbook`, `Generated Inventory`.

## Enforcement

`scripts/check_okf.py knowledge` validates the rules above. It runs in the CI lint job,
via `just check-okf`, and from `scripts/tests/test_check_okf.py` (covered by `just check`).

```console
$ python3 scripts/check_okf.py knowledge
knowledge: OKF v0.2 conformant (31 concepts, 2 index files, 1 log file)
```
