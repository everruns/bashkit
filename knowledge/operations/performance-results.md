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
| `bashkit-replay` gap telemetry | `crates/bashkit-eval/results/gaps/` | `gaps-*.json` plus matching `gaps-*.md` |
| Oils spec pass rate | `scripts/oils-spec/results/` | `oils-spec-*.json` plus matching `oils-spec-*.md` |

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
directories above: `just bench`, `just bench-parallel`, `just bench-sqlite`, `just bench-python`, `just bench-wasm-coreutils`,
`just gaps`, `just oils-spec`.

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

The transformer also emits `gapTelemetry`: `runs` (one point per
`gaps-*.json`: calls, gap calls and failed calls, recorded vs now) and
`latest` (that report's top 20 gaps and first 10 fixed gaps). `/benches`
renders both next to the Python startup panel.

The transformer also emits `pythonStartup`, one point per
`criterion-python-*.md` report: median first call and next call from the raw
start table, warm `python_call/<runtime>/print`, and CPython
`python_import/cpython/http_client` when present. `/benches` shows the latest
point (CPython vs Monty) and the CPython history. `just bench-python` produces
these reports. `criterion-wasm-coreutils-*.md` reports (`just
bench-wasm-coreutils`) are saved alongside but not yet charted on `/benches`.

The transformer also emits `oilsSpec`: `runs` (one point per
`oils-spec-*.json`: pass rate, bashkit passes among bash passes, Oils and
bashkit revisions) and `latest` (that report plus its 12 spec files with the
most misses). `/benches` renders both after the gap telemetry panels. See
[Oils Spec Pass Rate](oils-spec.md).

When changing result schemas, update the transformer and this spec in the same
PR. Do not hand-edit `performance-timeline.json` except by running the script.
