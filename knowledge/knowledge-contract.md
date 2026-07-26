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

## Third-party OKF linters (evaluated 2026-07-26, not adopted)

Two off-the-shelf linters exist. Both were run against this bundle; neither
replaces `check_okf.py` as the gate.

[`okftool`](https://github.com/ryansann/okftool) (Rust, Apache-2.0) independently
confirms this bundle: `validate` reports `31 concepts · 0 diagnostics ·
CONFORMANT`, `lint` reports 0 errors. But it only enforces the spec's hard rule
(`type` present) and treats the rest as advisory, so it accepts both defects the
original migration shipped:

| Regression | `okftool validate` | `okftool lint` | `check_okf.py` |
|---|---|---|---|
| Concept missing `type` | fail | fail | fail |
| `summary` instead of `description` | **pass** | **pass** | fail |
| Frontmatter on `index.md` | **pass** | **pass** | fail |
| Concept absent from `index.md` | **pass** | **pass** | fail |
| `log.md` heading not `YYYY-MM-DD` | **pass** | **pass** | fail |

[`okflint`](https://github.com/mattdav/okflint) (Python, MIT) is manifest-driven:
it validates against a hand-authored `okf-base.yaml` rather than the spec, so
adopting it means writing and maintaining the rule set anyway. It also requires
Python 3.12+, above this repository's 3.9 floor.

Revisit when either publishes a 1.0 to crates.io/PyPI and enforces the reserved
`index.md`/`log.md` structures. Until then `check_okf.py` stays the gate, and
`okftool lint` is useful ad hoc for its advisory graph checks (it flags that most
concepts here have no cross-links to each other).
