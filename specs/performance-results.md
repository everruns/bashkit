# Performance Results and Site Aggregation

Benchmark, Criterion, and LLM evaluation runs are historical artifacts. The
static site exposes the latest snapshot at `/benches` by aggregating those
artifacts during site build.

## Result Locations

Saved runs MUST write machine-readable data and Markdown reports to these
directories:

| Harness | Result directory | Site input |
|---------|------------------|------------|
| `bashkit-bench` | `crates/bashkit-bench/results/` | `bench-*.json` plus matching `bench-*.md` |
| Criterion benches | `crates/bashkit/benches/results/` | `criterion-*.md` |
| `bashkit-eval` | `crates/bashkit-eval/results/` | `eval-*.json`, `scripting-eval-*.json`, plus matching `.md` reports |

Markdown files are the user-facing reports linked from `/benches`; JSON files
are the aggregation input for benchmark and eval summaries.

## Run Commands

Default recipes that represent a real benchmark/eval run MUST save artifacts in
the directories above: `just bench`, `just eval`, `just eval-scripting`,
`just bench-parallel`, `just bench-sqlite`.

Non-saving exploratory commands may exist, but their names or comments must make
clear that they do not update the site.

After a successful saved run, the recipe MUST refresh generated site data:
`pnpm --dir site run data:performance` (updates local `/benches` without a full
site build).

## Site Data Build

`site/scripts/build-performance-data.mjs` is the only supported transformer for
the `/benches` page. It reads the result directories above and writes
`site/src/data/performance-timeline.json`.

`site/package.json` MUST run that transformer in `prebuild`, so every
`pnpm run build` refreshes `/benches` from the latest committed result artifacts.

When changing result schemas, update the transformer and this spec in the same
PR. Do not hand-edit `performance-timeline.json` except by running the script.
