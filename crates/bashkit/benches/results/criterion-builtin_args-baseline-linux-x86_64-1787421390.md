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

1. **clap is the dominant per-invocation cost for `cat`.** `cat` on a 4-byte
   file costs 14.0 µs against 4.0 µs for `echo` — a builtin whose body does
   comparable work minus the file read. Roughly 10 µs per invocation is arg
   handling.
2. **Matching flags is cheap; building the `Command` is not.** Adding flags to
   the same `cat` command line costs ~0.45 µs each, so parse/match is a small
   fraction of the 10 µs. The remainder is construction of the arg tree, paid
   once per invocation and thrown away.
3. **Cost scales with arg-surface size.** `ls` (~60 args) costs 59–61 µs
   against `cat`'s 14 µs. `ls`'s body does more than `cat`'s (readdir, stat,
   column layout), so this row is not a clean isolation — but the direction
   matches (1) and (2), and it is the largest number on the board.

## Implication

A script doing `for f in *; do cat "$f"; done` over 1000 files pays ~10 ms in
`clap::Command` construction alone, and an `ls`-heavy loop several times that.
Caching the built `Command` (e.g. `LazyLock` + clone, or bypassing clap for the
common no-flag path) targets the construction half, which is where nearly all
of the cost sits.

Not measured here: whether `Command::clone()` is meaningfully cheaper than
`<util>_command()`. That is the next experiment before committing to a fix.

## Context

Run in response to the usage-rs question (jdx/usage). Conclusion on adoption was
no — our clap definitions are codegen output from uutils' `uu_app()`, not
hand-maintained CLI boilerplate — but the perf claim behind it pointed at a real
cost, which this bench now quantifies and guards.
