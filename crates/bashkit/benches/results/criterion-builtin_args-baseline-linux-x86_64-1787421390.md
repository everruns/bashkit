# builtin_args — first run (baseline)

`cargo bench --bench builtin_args -- --warm-up-time 1 --measurement-time 3`

Host: 4× Intel Xeon @ 2.10 GHz, linux-x86_64, cloud runner (noisy; treat
absolutes as indicative, deltas as the signal).
Bashkit: v0.17.0, `4b634fd`.

Question this run answers: ported coreutils builtins rebuild their whole
`clap::Command` on every invocation. Is that visible?

## Raw

500 invocations per iteration; per-invocation column is `time ÷ 500`.

| bench | time / 500 inv | per invocation | vs `:` floor |
|---|---|---|---|
| `clap_vs_handrolled/colon_noop` | 1.658 ms | 3.32 µs | — |
| `clap_vs_handrolled/echo_handrolled` | 2.002 ms | 4.00 µs | +0.69 µs |
| `clap_vs_handrolled/printf_handrolled` | 2.049 ms | 4.10 µs | +0.78 µs |
| `clap_vs_handrolled/cat_clap` | 7.006 ms | 14.01 µs | +10.70 µs |
| `clap_vs_handrolled/ls_clap` | 30.408 ms | 60.82 µs | +57.50 µs |
| `arg_surface_size/cat_12_args` | 7.017 ms | 14.03 µs | +10.71 µs |
| `arg_surface_size/ls_60_args` | 29.497 ms | 58.99 µs | +55.68 µs |
| `flag_count/cat_0_flags` | 7.263 ms | 14.53 µs | — |
| `flag_count/cat_1_flag` | 7.484 ms | 14.97 µs | +0.44 µs / flag |
| `flag_count/cat_3_flags` | 7.952 ms | 15.90 µs | +0.46 µs / flag |

## Reading

1. **clap is the dominant per-invocation cost.** `cat` on a 4-byte file costs
   14.0 µs against 4.0 µs for `echo` — a builtin whose body does comparable
   work minus the file read. Roughly 10 µs per invocation is arg handling. For
   `ls` the clap share is ~73% of a 60.8 µs invocation.
2. **Cost scales with arg-surface size.** `ls` (~60 args) costs 59–61 µs
   against `cat`'s 14 µs. `ls`'s body does more than `cat`'s (readdir, stat,
   column layout), so this row is not a clean isolation, but it is the largest
   number on the board.
3. **The `flag_count` rows do NOT isolate construction cost.** Adding flags to
   the argv costs ~0.45 µs each, which initially read as "construction
   dominates, matching is cheap". A follow-up standalone clap harness showed
   that inference is wrong — see below.

## Correction: where the time actually goes

Measured directly against `cat_command()` outside bashkit (200k iterations):

| step | `cat` |
|---|---|
| construct `Command` only | 1.09 µs |
| `Command::clone()` only | 0.83 µs |
| construct + `try_get_matches_from` | 7.69 µs |
| parse alone (clone + parse − clone) | 6.27 µs |

**Parsing dominates, not construction** — 6.3 µs of the 7.7 µs round trip.
Caching to skip construction alone is worth ~1 µs of `cat`'s 14 µs, and
`Command::clone()` is not even cheaper than rebuilding (0.83 vs 1.09 µs, inside
noise end-to-end; a naive `LazyLock` + clone prototype measured as a 2.7%
*regression*).

The cacheable part is inside the parse: clap finalizes a `Command`
(`_build_self` — resolves groups, propagates settings, indexes args) on first
parse and skips it when the `Built` flag is set. Calling `Command::build()`
once and cloning the finalized value carries that flag:

| | `cat` (12 args) | `ls` (~60 args) |
|---|---|---|
| fresh build + parse | 7.34 µs | 44.65 µs |
| cached *unbuilt* + parse | 7.02 µs (−4.3%) | 37.80 µs (−15.3%) |
| cached *built* + parse | 6.50 µs (−11.4%) | 35.53 µs (−20.4%) |

## Outcome

Implemented as `builtins::clap_cache::cached_command!` across all 11 ported
builtins. End-to-end re-run on this host (`--save-baseline cached`):

| bench | before | after | change |
|---|---|---|---|
| `clap_vs_handrolled/cat_clap` | 7.006 ms | 6.862 ms | −2.1% |
| `clap_vs_handrolled/ls_clap` | 30.408 ms | 25.852 ms | **−15.0%** |
| `arg_surface_size/cat_12_args` | 7.017 ms | 6.717 ms | −4.3% |
| `arg_surface_size/ls_60_args` | 29.497 ms | 26.728 ms | −9.4% |
| `flag_count/cat_0_flags` | 7.263 ms | 6.553 ms | −9.8% |

`ls` drops from 60.8 to 51.7 µs per invocation. The win tracks arg-surface
size, so the large ports gain most and `cat` gains little.

## Context

Run in response to the usage-rs question (jdx/usage). Conclusion on adoption was
no — our clap definitions are codegen output from uutils' `uu_app()`, not
hand-maintained CLI boilerplate — but the perf claim behind it pointed at a real
cost, which this bench now quantifies and guards.
