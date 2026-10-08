---
type: Test Strategy
title: Oils Spec Pass Rate
description: Upstream Oils spec suite (bash column) run through bashkit and real bash, one headline pass rate per run.
tags:
  - bashkit
  - testing
  - compatibility
  - oils
---

# Oils Spec Pass Rate

## Status

Implemented. Harness: `scripts/oils-spec/run.py` (`just oils-spec`).

## Headline

**77.3%**: bashkit passes 2080 of the 2692 Oils spec cases real bash 5.2.21
passes (2759 cases in 132 spec files; bashkit passes 2086 of all cases,
75.6%). Oils `57d3f0d088c3`, 2026-10-08. Report:
`scripts/oils-spec/results/oils-spec-linux-x86_64-20261008T115238Z.md`.
The first run (74.2%, `...T103235Z.md`) missed 16 cases each in
`globignore` and `word-split`, 14 in `builtin-cd`, 13 in `alias` and
`dbracket`; those files now miss 1, 0, 5, 7 and 1.

Spec files with the most misses at the latest run: `builtin-completion`
(34), `array` (25), `prompt` (25), `builtin-trap-bash` (19), `var-ref` (15).
24 of the 132 files pass completely.

## Why

The in-repo spec cases (`crates/bashkit/tests/spec_cases/`) are written for
Bashkit in an Oils-like format, so their pass rate measures what was chosen to
be tested. The whole upstream Oils suite was written to pin bash, dash, mksh,
zsh and OSH behavior, not bashkit's, so its pass rate is an outside measure of
"closer to real bash" that moves with each PR (roadmap step 5, "Measure real
workloads").

## Decisions

- **Fetched, not vendored.** Oils is Apache-2.0 but the suite is large and
  upstream-owned. `run.py` pins `OILS_REV` (Oils pushes no release tags to
  GitHub, so a master commit) and shallow-fetches it into
  `target/oils-spec/oils` (`--cache` / `OILS_SPEC_CACHE` override). Bump the pin
  deliberately and save a new result in the same change.
- **Bash column only.** Files whose `## compare_shells` lists `bash` (or
  `bash-4.4`) run; `## suite: disabled` and OSH/YSH-only files are skipped. The
  parser ports Oils' `test/sh_spec.py` tokenizer (comment lines are dropped even
  inside code and `STDOUT` blocks, as Oils runs them) and builds assertions as
  it does for a `bash` label: `## OK|N-I|BUG bash` values win over the generic
  `stdout`/`stderr`/`status`; unasserted status means 0; stderr is compared only
  when asserted.
- **Denominator is real bash.** Real bash runs each case the Oils way (code on
  stdin, `SH=bash`, `TMP`, `REPO_ROOT`, `LC_ALL=C.UTF-8`, fresh cwd).
  Cases real bash itself fails (no `python2` on the runner, root making `-r`/`-w`
  true, bash version drift past 5.2, missing locales: 66 at the first run) are
  out of the headline, so the number is about bashkit, not the runner.
  `--no-bash` skips bash and reports bashkit over all cases.
- **bashkit invocation.** `bashkit -c CODE bash` (so `$0` is `bash`, as with
  stdin), stdin closed, `--timeout`. The CLI imports no host env, so
  `export SH=bash TMP=/tmp REPO_ROOT=/oils ...;` is glued onto the first code
  line, keeping `$LINENO` aligned. The Oils checkout and the helper shims are
  mounted read-only, so the CLI must be built with `realfs`
  (`cargo build -p bashkit-cli --features realfs`); the harness refuses a
  binary without `--mount-ro`.
- **Helper shims.** Oils' `spec/bin/argv.py`, `printenv.py`,
  `stdout_stderr.py`, `read_from_fd.py` are Python 2. `scripts/oils-spec/bin/`
  re-implements them in bash with identical output (argv.py renders a Python 2
  list repr from `od` bytes) and both shells use them; bashkit runs no host
  Python. A shim that bashkit mis-executes counts against bashkit, which is
  fair: it is ordinary bash.
- **Failure classes.** Each bashkit miss gets a keyword class: `signals`,
  `processes`, `interactive` (completion, history, prompts, bind), `host`, or
  `behavior`. The heuristic separates gaps bashkit keeps by design (see
  [Known Limitations](limitations.md)) from plain behavior differences; all of
  them still count in the rate. First run: behavior 530, interactive 110,
  host 27, processes 19, signals 10.
- **No floor, no CI gate yet.** Unlike the bash-oracle and Debian-oracle
  scoreboards ([Testing Strategy](testing.md)), this is a headline metric:
  bash-version drift on the runner moves the denominator. Saved results carry
  the per-file missed-case list, so two runs diff case by case.

## Results

`just oils-spec` builds the CLI with `realfs`, runs the whole suite and saves
`scripts/oils-spec/results/oils-spec-<platform>-<UTC stamp>.{json,md}`
(see [Performance Results](performance-results.md)). `run.py --fails FILE`
writes code, expected and actual output for every miss; positional spec names
(`run.py arith var-sub`) run a subset, which is never saved. The `/benches`
page shows the latest rate, the 12 spec files with the most misses and the
run history. `scripts/tests/test_oils_spec.py` covers the parser rules, the
scoring and the `argv.py` shim.

## See also

- [Testing Strategy](testing.md) - bash-oracle and Debian-oracle scoreboards
- [Performance Results](performance-results.md) - result locations
- [Known Limitations](limitations.md) - by-design gaps behind the failure classes
