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
- Every `index.md` enumerates **its own directory**: the concepts beside it and the subdirectories under it, nothing deeper.
- `log.md` bodies are date-grouped entries (`## YYYY-MM-DD`), newest first.
- Prose that is not a directory listing belongs in a concept document, not in an `index.md`.
- Links between concepts are relative and must resolve; links inside code spans and fenced blocks are text, not links.
- Every concept links to at least one other concept, a `## See also` section at the end is the conventional place. The bundle is a graph an agent traverses, not a pile of files.
- Bundle documents are referenced as relative markdown links, never as repository paths. A `knowledge/<doc>.md` inside a code span is not a link, so no checker sees it rot; the 2026-07-26 restructure left ~30 of those dangling for five days.

## Trust and lifecycle metadata

OKF's provenance, trust, and lifecycle families are optional, and this bundle
uses one of them deliberately.

- A concept whose content is produced by a tool carries `type: Generated Inventory`, `resource` (the artifact it describes, bundle-relative), and `generated.by` (the producing actor, OKF actor syntax: `<producer>/<version>`, `human:<id>`, or `process:<id>`). That triple is what lets a reader decide whether to trust the document or go read the source, the distinction hand-written concepts do not need to make.
- `generated.at` is deliberately unused: it would go stale on the first regeneration that forgets to bump it, and the drift workflows already prove freshness against the source of truth.
- `status`, `stale_after`, `verified`, `sources`, and `usage_window` are unused. Staleness here is defended by the rule that a behavior change updates its knowledge in the same pull request, plus the drift workflows, a calendar date would train people to bump the date rather than re-read the doc. If any of these are adopted later, `check_okf.py` already rejects malformed `status` and `stale_after` values.

## Layout

Concepts are grouped into domain subdirectories, each with its own `index.md`:

| Directory | Holds |
|---|---|
| [`foundations/`](foundations/) | Interpreter architecture, parser, VFS, builtins, concurrency |
| [`security/`](security/) | Threat model, security testing, credential/signing/HTTP boundaries |
| [`runtimes/`](runtimes/) | Embedded language runtimes and the published language packages |
| [`integrations/`](integrations/) | Tool contract, Git, SSH, interactive shell |
| [`operations/`](operations/) | Testing, limitations, docs, maintenance, release, benchmarks, eval |
| [`status/`](status/) | Machine-generated inventories |

Only this contract and `log.md` sit at the bundle root. A doc's directory conveys
its domain; its `type` conveys its kind, the two are independent, so a
`Subsystem Design` in `security/` and one in `runtimes/` share a type and differ
in placement. When adding a concept, put it in the matching domain, add it to that
directory's `index.md`, and prefer moving over duplicating if the domain changes.

Type values are producer-defined. This bundle uses: `Architecture`, `Subsystem Design`,
`Interface Contract`, `Package Design`, `Threat Model`, `Test Strategy`, `Limitations`,
`Playbook`, `Generated Inventory`.

## Enforcement

`scripts/check_okf.py knowledge` validates the rules above. It runs in the CI lint job,
via `just check-okf`, and from `scripts/tests/test_check_okf.py` (covered by `just check`).

```console
$ python3 scripts/check_okf.py knowledge
knowledge: OKF v0.2 conformant (32 concepts, 7 index files, 1 log file)
```

## Third-party OKF linters (evaluated 2026-07-26)

Four off-the-shelf implementations were built and run against this bundle and
against a fixture per regression class. All four agree the bundle is conformant;
they differ in how much they enforce.

| Tool | Language / license | Distribution | Spec | Verdict |
|---|---|---|---|---|
| [`okf-lint`](https://github.com/rpmoore/okf-lint) | Rust, Apache-2.0 | crates.io `0.1.1` | v0.2 | **adopted** |
| [`okftool`](https://github.com/ryansann/okftool) | Rust, Apache-2.0 | source / release binaries | v0.2 | not adopted |
| [`okf`](https://crates.io/crates/okf) | Rust, Apache-2.0 | crates.io `0.1.0-alpha.1` | **v0.1** | not adopted |
| [`okflint`](https://github.com/mattdav/okflint) | Python 3.12+, MIT | PyPI `0.3.1` | manifest-driven | not adopted |

Regression coverage (fail = caught, pass = slipped through):

| Regression | `okf` | `okftool` | `okf-lint` | `check_okf.py` |
|---|---|---|---|---|
| Concept missing `type` | fail | fail | fail | fail |
| `summary` instead of `description` | pass | pass | pass | fail |
| Frontmatter on `index.md` | pass | pass | fail | fail |
| Concept absent from `index.md` | pass | pass | pass | fail |
| `log.md` heading not `YYYY-MM-DD` | pass | pass | fail | fail |
| Bundle doc referenced as `knowledge/<doc>.md` |, |, | pass | fail |
| Concept links to no other concept |, |, | pass | fail |
| `Generated Inventory` without `generated.by` / `resource` |, |, | pass | fail |

The last three rows were added 2026-07-31 and measured against `okf-lint` only;
`okf` and `okftool` were not re-run, hence `—`.

`okf-lint` is the strongest of the four: it is the only one that enforces the
reserved `index.md`/`log.md` structures, it cites the spec section in every
diagnostic, and it installs from crates.io with a pinned version. It runs in CI
and in `just check-okf`, with `--max-line-length 10000` because knowledge docs
wrap prose at author discretion.

It does not subsume `check_okf.py`. The rows it lets through are bundle-local
conventions: `summary` instead of `description` (exactly the defect the original
migration shipped), a concept missing from its `index.md`, a bundle document
referenced by repository path, a concept that links to nothing, and a
`Generated Inventory` that does not say who produced it. Both checks therefore
run together.

The others were rejected on substance, not maturity: `okftool` treats everything
but `type` as advisory (0 errors on all five regressions); `okf` implements OKF
**v0.1**, so it validates this v0.2 bundle against the wrong revision; `okflint`
validates against a hand-authored `okf-base.yaml` rather than the spec, so
adopting it means maintaining the rule set anyway, and it requires Python 3.12+
against this repository's 3.9 floor.

`okftool lint` and `okf graph` remain useful ad hoc for advisory graph checks.
Their 2026-07-26 finding, that most concepts had no cross-links to each other,
was acted on 2026-07-31: every concept now links to at least one other, and
`check_okf.py` keeps it that way.

## See also

- [Builtin Inventory](status/builtin-inventory.md), the bundle's Generated Inventory concept
- [Maintenance](operations/maintenance.md), release-time checks that keep knowledge in sync
- [Documentation Architecture](operations/documentation.md), the doc trees this bundle is not
