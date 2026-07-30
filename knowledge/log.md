# Bashkit Knowledge Update Log

## 2026-07-29

* **Enforcement**: Added `scripts/check_doc_links.py` (`just check-doc-links`, wired into `just check` and CI) to reject dangling relative markdown links in `docs/`, `crates/bashkit/docs/`, and root markdown — the site's basename-matching link rewriter hid 14 of them from every existing verifier. Rule and rationale recorded in [Documentation](operations/documentation.md) and [Maintenance](operations/maintenance.md).
* **Decision**: Page titles/descriptions stay in `DOC_META`; markdown frontmatter is rejected for both doc trees so rustdoc `include_str!` embedding stays clean.
* **Creation**: [Script Analysis](integrations/script-analysis.md) — `analyze()` in Rust, Node, and Python reports the commands, arguments, redirect targets, and function definitions a script statically refers to, so hosts can gate execution on a permission prompt without depending on parser internals. Contract, decisions (literal-or-null, substitutions walked, flat source order, node budget), and verification pointers live in the concept.
* **Threat**: Added TM-ESC-032 (host permission gate bypass via static analysis) to [Threat Model](security/threat-model.md) and the public `crates/bashkit/docs/threat-model.md`. `analyze()` is advisory; `is_opaque()` and the `before_tool` hook are the documented mitigations. Backed by proptest invariants in `tests/proptest_security.rs` and the `analyze_fuzz` libFuzzer target.
* **Limitation**: Recorded `<>` (read-write redirect) as unimplemented in [Limitations](operations/limitations.md) — the parser rejects the operator, which is why `ScriptAnalysis` has no read-write redirect mode. Evidence: skipped spec test `readwrite_redirect_opens_file`.

## 2026-07-26

* **Restructure**: Grouped the 29 domain concepts into `foundations/`, `security/`, `runtimes/`, `integrations/`, and `operations/`, each with its own `index.md`; the root index now enumerates root concepts and subdirectories only. Frontmatter `type` values are unchanged — directory conveys domain, `type` conveys kind.
* **Enforcement**: `scripts/check_okf.py` now rejects dangling bundle-relative links, ignoring code spans and fenced blocks.

* **Decision**: Evaluated four OKF implementations and adopted [`okf-lint`](https://github.com/rpmoore/okf-lint) as the upstream spec gate alongside `scripts/check_okf.py`, which covers the bundle-local conventions it does not enforce. Rationale and coverage tables in [Knowledge Maintenance Contract](knowledge-contract.md).

## 2026-07-25

* **Conformance**: Made the bundle conform to OKF v0.2 — added the required `type` to every concept, renamed `summary` to `description`, stripped frontmatter from reserved `index.md` files, declared `okf_version: "0.2"` at the bundle root, and added this log.
* **Creation**: Moved the maintenance rules out of `index.md` into [Knowledge Maintenance Contract](knowledge-contract.md), and described the generated builtin inventory in [Builtin Inventory](status/builtin-inventory.md).
* **Migration**: Moved `specs/` to `knowledge/` as the canonical knowledge bundle.
