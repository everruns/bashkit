# Bashkit Knowledge Update Log

## 2026-07-26

* **Restructure**: Grouped the 29 domain concepts into `foundations/`, `security/`, `runtimes/`, `integrations/`, and `operations/`, each with its own `index.md`; the root index now enumerates root concepts and subdirectories only. Frontmatter `type` values are unchanged — directory conveys domain, `type` conveys kind.
* **Enforcement**: `scripts/check_okf.py` now rejects dangling bundle-relative links, ignoring code spans and fenced blocks.

* **Decision**: Evaluated four OKF implementations and adopted [`okf-lint`](https://github.com/rpmoore/okf-lint) as the upstream spec gate alongside `scripts/check_okf.py`, which covers the bundle-local conventions it does not enforce. Rationale and coverage tables in [Knowledge Maintenance Contract](knowledge-contract.md).

## 2026-07-25

* **Conformance**: Made the bundle conform to OKF v0.2 — added the required `type` to every concept, renamed `summary` to `description`, stripped frontmatter from reserved `index.md` files, declared `okf_version: "0.2"` at the bundle root, and added this log.
* **Creation**: Moved the maintenance rules out of `index.md` into [Knowledge Maintenance Contract](knowledge-contract.md), and described the generated builtin inventory in [Builtin Inventory](status/builtin-inventory.md).
* **Migration**: Moved `specs/` to `knowledge/` as the canonical knowledge bundle.
