# Documentation Approach

## Decision

Embed external markdown files into rustdoc via `#[doc = include_str!(...)]`
on empty doc modules in `lib.rs`.

Rationale:
1. **Single source of truth**: markdown in `crates/bashkit/docs/` is canonical
2. **Dual visibility**: same content on GitHub and docs.rs
3. **No duplication** across platforms
4. **Cross-linking**: rustdoc links connect guides to API types

Docs must live inside `crates/bashkit/docs/` (not repo-root `docs/`) so they
ship in the published crate and `include_str!` works when built from crates.io.
Repo-root `docs/` is for user-facing site articles.

## Requirements

- Each guide markdown starts with a "See also" section linking related guides/API docs
- Doc module gets a `///` summary above the `#[doc = include_str!]` for rustdoc cross-links; reference types with `` [`TypeName`] `` syntax
- New guides: add file in `crates/bashkit/docs/`, add doc module in `lib.rs`, link it from the crate docs `# Guides` section, preview with `cargo doc --open`

## Code Examples

Rust examples in guides are compiled/tested by `cargo test --doc`. Hide
boilerplate from rendered docs with `# ` line prefixes.

| Fence | When to use |
|-------|-------------|
| `` ```rust `` | Complete examples using only bashkit types — tested |
| `` ```rust,no_run `` | Compiles but shouldn't execute |
| `` ```rust,ignore `` | Uses external crates or feature-gated APIs in non-gated modules |

Doc modules behind `#[cfg(feature = "...")]` (e.g., `python_guide`) may use
feature-gated APIs freely. Non-gated modules (e.g., `threat_model`,
`compatibility_scorecard`) must NOT — use `rust,ignore` there.
