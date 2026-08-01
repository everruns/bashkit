# Bashkit Knowledge Update Log

## 2026-08-01

* **Creation**: Added [Snapshot History and Deltas](foundations/snapshot-history.md) in response to [#2221](https://github.com/everruns/bashkit/issues/2221) — a content-addressed object graph with a commit DAG for forks, content-defined chunking for binary-safe per-file differentials, a two-phase pull API instead of a host store trait, three-number version rules backed by a golden fixture corpus, and a capability fingerprint gating restore.
* **Decision**: Split the rollout across two releases — reader first as a patch, writer and public API next as a minor. `min_reader` makes every *future* format change safe, but cannot make the first one safe: readers that predate it fail on a v2 container with a JSON parse error naming neither version nor format. A release that reads v2 without writing it gives deployments a rollback target before anything can produce v2. Recorded in the concept's Status.
* **Decision**: The reader release adds no public API, so it can ship as a patch — new types are `pub(crate)`, and the version refusal reuses `Error::Internal` because `Error` is not `#[non_exhaustive]` and a new variant would be breaking. The v2 encoder ships alongside it reachable only from tests: a decoder cannot be tested without something that produces valid input for it.
* **Decision**: Content-defined chunking is in-tree (gear hash, pinned constants) rather than the `fastcdc` crate — the proposal's fallback, taken up front since the parameters are format constants regardless.
* **Decision**: Recorded the rejected alternative — byte-delta patch chains (bsdiff/xdelta/`zstd --patch-from`) — and why the graph model wins for the branch-and-truncate read pattern.
* **Enforcement**: Object identity covers decoded content, not compressed framing, so hosts may recompress freely; bytes appended after a deflate stream are rejected rather than silently ignored.
* **Threats**: Registered TM-SNAP-002 through TM-SNAP-005 in [Threat Model](security/threat-model.md). TM-SNAP-006 (capability mismatch) lands with the policy it describes.

## 2026-07-31

* **Repair**: Converted 56 in-bundle references from the code-span repository-path form (`` `knowledge/<doc>.md` ``) into resolving relative links. All but three carried pre-restructure flat paths that had been dangling since 2026-07-26 — invisible to both linters because a code span is text, not a link.
* **Cross-links**: Added `## See also` sections to the nine concepts that linked to no other concept, and normalised the heading to `## See also` bundle-wide. Every concept is now reachable from another concept, not just from its `index.md`.
* **Enforcement**: `scripts/check_okf.py` now rejects a concept that links to no other concept, a bundle document referenced as a repository path, and a `Generated Inventory` missing `generated.by` or `resource`; it also validates OKF actor syntax and `status`/`stale_after` values where present. `okf-lint 0.1.1` passes all three regressions — coverage table updated in [Knowledge Maintenance Contract](knowledge-contract.md).
* **Decision**: Of OKF's optional families, only the trust family is adopted, and only for generated concepts (`generated.by` + `resource`). `generated.at`, `status`, `stale_after`, `verified`, `sources`, and `usage_window` stay unused — the same-PR update rule and the drift workflows defend staleness better than a metadata date. Rationale in [Knowledge Maintenance Contract](knowledge-contract.md).

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
