# Browser package compatibility and cold start — 2026-10-15

Bashkit's published/source compatibility gap is version drift. The npm 0.18.2
artifact identifies release commit `567511386572d4c4b9f649ab1da97aad53b9f341`
(September 22), while the independent study's source is `491c3281`, 269 commits
newer. The source and published distributions use the same build path. The
package version remains 0.18.2 on unreleased main; it is not an artifact identity.

A dedicated `wasm-release` profile (`opt-level = "s"`, fat LTO, Binaryen 125
`-Oz`) reduces current-package Brotli transfer by **13.9%** without dropping
`jq`, `Terminal`, commands, or resource limits. Native release settings stay
unchanged. A more aggressive `"z"` profile saved 25.0% but slowed the arithmetic
sample about 50%, so it was rejected.

## Independent evidence and validation scope

- [brush-web compatibility study](https://github.com/ShaulLavo/brush-web/pull/7):
  Oils / Brush / Bashkit corpora reported source 95.4 / 84.1 / 96.2% versus
  published 55.1 / 55.8 / 70.8% for builtins-only cases. Its own report attributes
  the old package's failures to later compatibility work.
- [mobile cold-start study](https://github.com/ShaulLavo/brush-web/pull/9):
  transfer dominated startup; source grew across those 269 commits; removing
  Terminal saved only about 50 KB Brotli; `wasm-opt -Oz` alone did not help.
- [initial API comparison](https://github.com/ShaulLavo/brush-web/pull/5):
  the published package lacked the source tree's newer Terminal API.

This investigation independently checks npm `gitHead` against the release tag,
compares 24 bounded GNU Bash cases, rebuilds the exact release source, and measures
real Chromium startup of the full current package. The 24 cases are a regression
sample, **not a rerun or independent confirmation of the three large-corpus
percentages above**. They isolate representative release gaps without turning a
large, evolving third-party corpus into a fragile release gate.

## Compatibility

GNU Bash 5.3.20 is the oracle. Fresh shell per case; exact stdout bytes and exit
status; stderr not scored. Results are in the matching JSON report.

| Artifact | Cases passed |
|---|---:|
| npm 0.18.2 | 19/24 |
| Rebuilt v0.18.2 source tag | 19/24, identical five failures |
| Current source, original release profile | 24/24 |
| Current source, selected shipping profile | 24/24 |

Published failures: sparse array indices, negative array slices, associative
array literals, arithmetic array updates, zero-padded brace sequences. The
existing source fixes remain intact under the new profile. All 70 browser
package integration/packaging tests pass, including async host filesystem,
timers, cancellation/resource limits, jq, binary output, and interactive terminal.

## Artifact attribution

Same source base `8a817bf0`, Rust 1.95.0, wasm-bindgen 0.2.128, single-threaded
`wasm32-unknown-unknown`, `terminal` enabled, and pinned Binaryen 125 `-Oz` for
both baseline and selected builds. Byte counts include `index.js`, generated JS
glue, and the WASM; compression is per file, as served by a CDN.

| Profile | Raw bytes | gzip-9 bytes | Brotli-11 bytes |
|---|---:|---:|---:|
| Original release: opt 3, thin LTO | 9,519,539 | 3,570,277 | 2,468,453 |
| Selected: opt s, fat LTO | 7,668,904 | 2,973,501 | 2,125,682 |
| Rejected: opt z, fat LTO | 6,420,068 | 2,506,391 | 1,851,368 |

The selected build saves 342,771 Brotli bytes (13.9%), 596,776 gzip bytes (16.7%),
and 1,850,635 raw bytes (19.4%). Its WASM is 99.6% of the Brotli load graph;
wrapper + glue total about 8 KB. The WASM code section falls from 8,279,718 to
6,433,520 bytes; data stays roughly constant (1,149,749 to 1,143,328 bytes).
The improvement removes generated code overhead rather than command capability
or symbol names: the baseline has no large debug/name custom section to strip.

The old npm artifact is 1,826,367 Brotli bytes for the same three-file graph.
Current source carries substantially more behavior. Even after this change,
that old artifact remains smaller; publishing the repaired source does not mean
restoring the old feature set or promising just-bash's much smaller payload.

## Cold startup

Five trials per artifact/condition, fresh browser context (cold HTTP cache),
rotated order, Chrome 155.0.8059.79, Node 26.5.0, macOS Apple Silicon. Brotli-11
loopback server and CDP network throttling, page CPU 4x. Both artifacts use the
real loader and must run `echo hi`, drive Terminal, and paint the result. Samples
and resource timings are retained. Full method: [experiment guide](../README.md).

| Condition | Baseline median | Selected median | Saved |
|---|---:|---:|---:|
| Loopback + page CPU 4x | 196.6 ms | 168.0 ms | 28.6 ms |
| Fast 4G (1,012,500 B/s, 165 ms) | 3,285.1 ms | 2,919.5 ms | 365.6 ms |
| Slow 4G (180,000 B/s, 562.5 ms) | 16,177.1 ms | 14,250.7 ms | 1,926.4 ms |

On Slow 4G, the last resource arrives at median 16,043.4 / 14,135.4 ms. The
remaining loader work is 42.7 / 38.9 ms; the first `Bash.execute('echo hi')`
is 56.9 / 46.5 ms. Resource transfer dominates both artifacts. These are
component medians; do not add independently computed medians as an exact sum.

These numbers are package-only, not brush-web's navigation path: no Ghostty,
framework, worker, or other shared assets. CPU throttling covers this page's
interpreter, but does not model a physical phone. Contexts have cold HTTP caches;
the browser process can share compiled-code caches. No warm-visit claim is made.
The development host also ran repository checks: timings are exploratory, while
byte counts and semantic gates are deterministic. The selected bundle crossed
below 15 seconds here, but an adopter must retain an appropriate startup budget
for their larger application and slower networks.

## Runtime tradeoff

One warmup, seven measured fresh-instance executions; serial exploratory samples
on the same development host, Node 26.5.0. Not a clean-host performance baseline.

| Workload | Original median | Selected median | Rejected z median |
|---|---:|---:|---:|
| 20,000 arithmetic iterations | 44.24 ms | 49.49 ms | 66.18 ms |
| 1,000 command substitutions | 6.28 ms | 6.15 ms | 7.81 ms |
| jq map/sum over 1,000 values | 2.46 ms | 2.28 ms | 3.07 ms |
| sed/sort/wc over 10,000 lines | 9.83 ms | 10.26 ms | 11.97 ms |

The selected profile trades about 12% arithmetic throughput in this sample for
14% less download. No claim of universal throughput improvement. The gain is a
real first-visit improvement with all package behavior retained.

## Gates and publication boundary

CI and npm publication run identical checks: all browser package tests, 24-case
GNU Bash parity, and raw/gzip/Brotli budgets (8,000,000 / 3,100,000 / 2,220,000
bytes, about 4-5% measured headroom). There is no noisy wall-clock CI threshold.
The original artifact fails these budgets; the shipping artifact passes.
`build-info.json` and npm-archive tests make exact source/profile/hash verifiable.

No npm release was performed by this benchmark change. On October 15,
`just check-nightly` failed because one of the last three nightly runs had a red
Advisory Audit ([run 38679252914](https://github.com/everruns/bashkit/actions/runs/38679252914));
all three fuzz runs were green. The release contract prohibits publication until
that gate is green. A merged code change is distinct from a repaired npm package.
