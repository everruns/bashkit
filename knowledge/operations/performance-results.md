---
type: Playbook
title: Performance Results
description: Benchmark harnesses, result locations, naming, and publication contract.
tags:
  - bashkit
  - benchmarks
  - performance
---

# Performance Results and Site Aggregation

## Status
Implemented

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
| `bashkit-eval` (archived) | `crates/bashkit-eval/results/` | `eval-*.json`, `scripting-eval-*.json`, plus matching `.md` reports |

Markdown files are the user-facing reports linked from `/benches`; JSON files
are the aggregation input for benchmark and eval summaries.

> `bashkit-eval` was reimplemented as a [mira](https://github.com/everruns/mira)
> study (see [Evaluation Framework](eval.md)); mira now owns eval run output (written under
> `./results/<run_id>/` in mira's own format). The `crates/bashkit-eval/results/`
> directory is retained as an **archive** of pre-mira runs and remains the
> `/benches` eval input until the site is re-wired to mira's output format
> (follow-up).

## Run Commands

Default benchmark recipes that represent a real run MUST save artifacts in the
directories above: `just bench`, `just bench-parallel`, `just bench-sqlite`.

The comparison harness resolves `bash` from `PATH` and requires Bash 4 or newer
(case conversion and associative arrays are benchmarked). A missing or older
requested Bash oracle aborts the run instead of publishing misleading output
mismatches; macOS `/bin/bash` 3.2 is unsupported, so put an installed modern Bash
first on `PATH`. JSON reports include optional `runner_versions` metadata and
Markdown records the selected Bash path and version. The site transformer ignores
this additive field; historical reports remain readable. Regression tests in
`crates/bashkit-bench/src/runners.rs` cover PATH precedence and missing/old Bash.

`bashkit-eval` runs through the `mira` host (`just eval`, `just eval-scripting`);
mira writes its own run folder under `./results/<run_id>/` and is not part of the
benchmark save contract above.

Non-saving exploratory commands may exist, but their names or comments must make
clear that they do not update the site.

After a successful saved run, the recipe MUST refresh generated site data:
`pnpm --dir site run data:performance` (updates local `/benches` without a full
site build). The equivalent dependency-free command is
`node site/scripts/build-performance-data.mjs`.

## Site Data Build

`site/scripts/build-performance-data.mjs` is the only supported transformer for
the `/benches` page. It reads the result directories above and writes
`site/src/data/performance-timeline.json`.

`site/package.json` MUST run that transformer in `prebuild`, so every
`pnpm run build` refreshes `/benches` from the latest committed result artifacts.

When changing result schemas, update the transformer and this spec in the same
PR. Do not hand-edit `performance-timeline.json` except by running the script.
