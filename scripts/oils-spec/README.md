# Oils spec pass rate

Runs the upstream [Oils](https://github.com/oils-for-unix/oils) spec suite
(Apache-2.0), bash column, through bashkit and through real bash, and reports
one headline number:

    pass rate = cases bashkit passes / cases real bash passes

```bash
just oils-spec                         # build CLI (realfs), run all, save results
python3 scripts/oils-spec/run.py arith var-sub --fails /tmp/fails.txt
python3 scripts/oils-spec/run.py --no-bash   # bashkit over all cases, no bash
```

`run.py` fetches Oils at the pinned `OILS_REV` into `target/oils-spec/oils`
(override with `--cache` or `OILS_SPEC_CACHE`); the test files are never
vendored. The bashkit CLI must be built with the `realfs` feature
(`cargo build -p bashkit-cli --features realfs`), because the Oils checkout and
`bin/` are mounted read-only into the sandbox.

## How a case is scored

The parser follows Oils' `test/sh_spec.py`: `#### name` starts a case,
`## stdout:` / `## STDOUT: ... ## END` / `## stdout-json:` / `## status:` /
`## stderr:` assert, and `## OK bash`, `## N-I bash`, `## BUG bash` overrides
win over the generic values. Status defaults to 0; stderr is checked only when
asserted. Files without `bash` in `## compare_shells`, and `## suite: disabled`
files, are skipped.

- Real bash runs the Oils way: code on stdin, `SH=bash`, `TMP`, `REPO_ROOT`,
  `LC_ALL=C.UTF-8`, a fresh temp cwd. Cases it fails (for example ones that
  call `python2`) leave the denominator.
- bashkit runs `bashkit -c CODE bash` with stdin closed. The same variables are
  exported by a prefix glued onto the first code line, so `$LINENO` matches.
- `bin/` holds bash stand-ins for Oils' Python 2 helpers (`argv.py`,
  `printenv.py`, `stdout_stderr.py`, `read_from_fd.py`, `foo=bar`), with the
  same output. Both shells use them.
- Each bashkit miss is classed `signals`, `processes`, `interactive`, `host` or
  `behavior` by keyword, so by-design gaps stand apart. All still count.

## Results

`--save` (what `just oils-spec` passes) writes
`results/oils-spec-<platform>-<UTC stamp>.json` and `.md`: totals, failure
classes, and per spec file the counts plus every case bashkit misses, so two
runs diff case by case. A subset run is never saved. The `/benches` site page
shows the latest headline and history.

See `knowledge/operations/oils-spec.md` for the decisions and the current
number.
