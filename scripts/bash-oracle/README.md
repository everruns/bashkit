# Bash-oracle scoreboard

Every case in `cases/` is a bash script whose status, stdout and stderr were
recorded from real bash (5.2) into `expected/`. `run.py` replays each one
through the bashkit CLI and diffs it with the recording, so the scoreboard needs
no bash on the runner and does not drift with the runner's bash version.

```bash
just bash-oracle                      # score every case
just bash-oracle --case nounset       # diff one case, section by section
python3 scripts/bash-oracle/run.py --bin path/to/bashkit
```

## Two scores

- **match** — status plus stdout, what a script actually observes. This is the
  number the bash-parity work tracks.
- **strict** — also requires bash's stderr text. It trails behind, because
  some wording (parser errors, xtrace details) still differs from bash.

`floor.txt` holds both floors as `<match> <strict>`. `run.py` exits 1 when
either score falls below its floor, and prints the new number to put in
`floor.txt` when a fix raises one. Raise a floor in the same change that earns
it; never lower one without saying why in the PR.

## Recording

`record.py` re-records from real bash. It runs each case twice in two different
empty temp directories and keeps it only when both runs agree and the output
carries neither a host path nor the working directory, so a case that depends
on the clock, on `$RANDOM`, on a pid or on where it was run never becomes a
flaky recording.

The working-directory rule also keeps sandbox identity out of the corpus:
bashkit's virtual filesystem starts in `/home/user` whatever the process cwd
is, by design, so a case printing the real cwd would be a permanent gap that
measures nothing. The same holds for host environment variables such as
`LC_ALL` or `TZ`: the bashkit CLI does not import them, so cases do not print them.

```bash
python3 scripts/bash-oracle/record.py          # all cases
python3 scripts/bash-oracle/record.py nounset  # one case
```

Cases run with `LC_ALL=C.UTF-8`, `HOME=/home/sandbox`, `USER=sandbox`,
`TERM=dumb` and the host `PATH`. The home and user values are bashkit's own
sandbox identity, so a gap is a behavioral difference rather than a difference
in who the shell thinks it is. The script's own path is normalized to `SCRIPT` and the working
directory to `CWD`, so the recordings are machine-independent.

## Adding a case

Write `cases/<name>.sh`, record it, and check the recording in alongside it. A
case should exercise one area of bash behavior, print deterministic output, and
not need network, a real filesystem outside its temp directory, or a tool
outside coreutils.

See also `knowledge/operations/testing.md` and `scripts/debian-oracle/` (the
other scoreboard, which scores individual coreutils tools against Debian's).
