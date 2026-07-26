# Bashkit Knowledge Update Log

## 2026-07-26

* **Decision**: Evaluated four OKF implementations and adopted [`okf-lint`](https://github.com/rpmoore/okf-lint) as the upstream spec gate alongside `scripts/check_okf.py`, which covers the bundle-local conventions it does not enforce. Rationale and coverage tables in [Knowledge Maintenance Contract](knowledge-contract.md).

## 2026-07-25

* **Conformance**: Made the bundle conform to OKF v0.2 — added the required `type` to every concept, renamed `summary` to `description`, stripped frontmatter from reserved `index.md` files, declared `okf_version: "0.2"` at the bundle root, and added this log.
* **Creation**: Moved the maintenance rules out of `index.md` into [Knowledge Maintenance Contract](knowledge-contract.md), and described the generated builtin inventory in [Builtin Inventory](status/builtin-inventory.md).
* **Migration**: Moved `specs/` to `knowledge/` as the canonical knowledge bundle.
