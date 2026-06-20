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

## API reference hosting

### Problem

Rust gets a hosted, versioned API reference for free via **docs.rs** on every
crates.io release. The PyPI package (`bashkit`) and the npm package
(`@everruns/bashkit`) have no equivalent — only a README and type definitions.

### Decision

Self-host per-language API references on **bashkit.sh** under `/api/`:

- `/api` — index linking all three languages (Rust → docs.rs externally;
  Python and TypeScript served locally).
- `/api/python`, `/api/typescript` — generated reference pages.

Generate **markdown** from each package's own source of truth and render it
through the existing Astro `DocsLayout`, so API pages inherit site branding
(colors, typography, Shiki theme) instead of shipping a foreign pdoc/TypeDoc
theme. This reuses the same single-source pipeline as the rustdoc guides.

Reference pages opt into `DocsLayout`'s `wide` mode (a 76rem column, a sticky
"On this page" rail built from the H2 headings — classes/types/modules; H3
method headings would run to ~150 entries — and word-wrapped code so long
signatures don't clip). Prose guides keep their 44rem reading width.

**Latest-only** (no per-version archive): the page reflects the newest
published release. **Generate-and-commit**: output lives in the `apidocs`
content collection (`site/src/content/apidocs/*.md`) and is committed, so the
node-only `site.yml` build just renders it. Regenerate on release (same pattern
as `site/src/data/performance-timeline.json`).

### Generators

| Language | Source of truth | Tool | Command |
|----------|-----------------|------|---------|
| Python | `crates/bashkit-python/bashkit/*.pyi` + integration modules | `griffe` (static, `allow_inspection=False`) | `just apidocs-python` |
| TypeScript | `crates/bashkit-js/*.ts` + napi-generated `index.d.ts` | `typedoc` (JSON) + custom renderer | `just apidocs-ts` |

Both languages are wired (`scripts/gen_python_apidocs.py`,
`scripts/gen_ts_apidocs.mjs`; `just apidocs` runs both). The Python generator
parses type stubs statically — no build needed. The TypeScript generator must
run `napi build` first (the `.ts` wrappers import types from the
build-generated, gitignored `index.d.ts`), so it cannot run in the node-only
`site.yml` — regenerate it in a Rust-capable release job, then commit the
output. A language's `/api/<slug>` route only appears once its markdown exists,
so the `/api` index degrades gracefully if a page is missing.

### Drift

`apidocs-drift.yml` regenerates each reference and fails if the committed
markdown is stale, same as `builtins-drift.yml`. It never auto-commits (a CI
bot commit would violate the AGENTS.md commit-attribution rule) — a human runs
`just apidocs` and commits. The Python check is cheap (static `griffe` parse,
pinned) so it runs on every PR touching the Python surface; the TypeScript
check needs a Rust `napi build`, so it runs only on the weekly schedule and on
demand. Regenerate-and-commit is a release step (see `specs/release-process.md`).

### Constraints

- Generated markdown must not contain local `*.md` links (`verify-public-links`
  fails on them) — use in-page anchors and absolute site/external links.
- New `apidocs` pages are registered in `site/src/pages/api/_meta.ts`, not the
  `/docs` `_meta.ts`, so `verify-doc-routes` does not require them.
