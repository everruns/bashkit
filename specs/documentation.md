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

## Agent-facing site surfaces

The `bashkit.sh` site (`site/`) exposes machine-readable surfaces for coding
agents. All are **generated**, not hand-maintained, so they cannot drift from
the canonical docs/content:

- **Markdown routes** — every page has a `text/markdown` representation via
  content negotiation (the Worker) or a direct `.md` URL: `/index.md`,
  `/docs.md`, `/docs/<slug>.md`. The per-doc route serves the raw canonical
  source plus a generated navigation footer.
- **`/llms.txt` + `/llms-full.txt`** ([llmstxt.org](https://llmstxt.org)) — the
  curated agent entry point and the full-text inline of every guide. Both are
  generated from `DOC_META` (`site/src/pages/docs/_meta.ts`) and the content
  tables in `site/src/content/home.ts`.
- **Agent Skills discovery index** — `/.well-known/agent-skills/index.json`
  plus the `bashkit.tar.gz` skill archive. Unlike the above, this is a separate
  digest-locked artifact built from `skills/bashkit/` via `rosie-skills`, **not**
  regenerated at site build; refresh it when `skills/bashkit/` changes.

### Contract

- A new public guide needs a `DOC_META` entry (slug, title, summary, SEO
  fields, section). `verify-doc-routes.mjs` rejects any `docs/*.md` without a
  route; `verify-llms.mjs` rejects any `DOC_META` doc missing from `/llms.txt`
  or `/llms-full.txt`.
- The Markdown surfaces (`/index.md`, `/docs.md`, every `/docs/<slug>.md`) must
  link back to `/llms.txt` — enforced by `verify-llms.mjs`.
- `/llms.txt` is advertised via the homepage HTTP `Link` header
  (`site/public/_headers`), `robots.txt`, and a footer link.
- All site verify scripts run in the `postbuild` chain (CI **Site Build** job),
  so these guarantees hold on every build.
