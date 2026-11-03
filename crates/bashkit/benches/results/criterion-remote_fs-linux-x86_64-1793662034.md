# remote_fs bench, bashkit checkout (bounded `walk` batches)

Host: cloud container, linux x86_64. Tree: `git ls-files` of this repo, 3,725 files under `/workspace` (`.git` present so `.gitignore` applies). Command: `cargo bench --bench remote_fs`.

Follows `criterion-remote_fs-linux-x86_64-1793656706.md` (the before/after for the round-trip fix, with origin/main numbers). This run adds bounded `walk` answers (`WalkOptions::max_entries`, 10k entries) and per-file rg input charging. Call counts are unchanged at this tree size: every walked subtree fits in one batch, so `find`/`du` on a bulk backend still take one `walk` call. Trees above 10k entries take one `walk` per batch, holding at most one batch in memory.

Call counts are the signal: on a remote backend each call is a round trip ("at 10 ms" column).

## Round trips per command

| command | backend | calls | read_file | read_dir | stat+lstat+exists | walk | search | at 10 ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `rg -l markkit crates` | plain | 5133 | 3140 | 1984 | 9 | 0 | 0 | 51.3 s |
| `grep -rln markkit crates` | plain | 5131 | 3145 | 1983 | 3 | 0 | 0 | 51.3 s |
| `find /workspace -path '*markkit*'` | plain | 2057 | 0 | 2056 | 1 | 0 | 0 | 20.6 s |
| `rg -l SearchCapable crates` | plain | 5133 | 3140 | 1984 | 9 | 0 | 0 | 51.3 s |
| `rg -l --no-ignore SearchCapable crates` | plain | 5122 | 3138 | 1982 | 2 | 0 | 0 | 51.2 s |
| `grep -rl SearchCapable crates` | plain | 5131 | 3145 | 1983 | 3 | 0 | 0 | 51.3 s |
| `rg -c fn crates/bashkit/src/builtins` | plain | 246 | 215 | 22 | 9 | 0 | 0 | 2.5 s |
| `find crates -type f -name '*.rs'` | plain | 1984 | 0 | 1983 | 1 | 0 | 0 | 19.8 s |
| `find crates -ls` | plain | 1984 | 0 | 1983 | 1 | 0 | 0 | 19.8 s |
| `ls -R crates` | plain | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `du -s crates` | plain | 1984 | 0 | 1983 | 1 | 0 | 0 | 19.8 s |
| `tree crates` | plain | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | plain | 234 | 213 | 17 | 3 | 0 | 0 | 2.3 s |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | plain | 783 | 0 | 17 | 519 | 0 | 0 | 7.8 s |
| `rg -l markkit crates` | index | 20 | 4 | 6 | 9 | 0 | 1 | 0.2 s |
| `grep -rln markkit crates` | index | 6 | 2 | 0 | 3 | 0 | 1 | 0.1 s |
| `find /workspace -path '*markkit*'` | index | 2057 | 0 | 2056 | 1 | 0 | 0 | 20.6 s |
| `rg -l SearchCapable crates` | index | 39 | 18 | 11 | 9 | 0 | 1 | 0.4 s |
| `rg -l --no-ignore SearchCapable crates` | index | 19 | 16 | 0 | 2 | 0 | 1 | 0.2 s |
| `grep -rl SearchCapable crates` | index | 34 | 16 | 0 | 17 | 0 | 1 | 0.3 s |
| `rg -c fn crates/bashkit/src/builtins` | index | 243 | 211 | 22 | 9 | 0 | 1 | 2.4 s |
| `find crates -type f -name '*.rs'` | index | 1984 | 0 | 1983 | 1 | 0 | 0 | 19.8 s |
| `find crates -ls` | index | 1984 | 0 | 1983 | 1 | 0 | 0 | 19.8 s |
| `ls -R crates` | index | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `du -s crates` | index | 1984 | 0 | 1983 | 1 | 0 | 0 | 19.8 s |
| `tree crates` | index | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | index | 234 | 213 | 17 | 3 | 0 | 0 | 2.3 s |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | index | 783 | 0 | 17 | 519 | 0 | 0 | 7.8 s |
| `rg -l markkit crates` | index+walk | 20 | 4 | 6 | 9 | 0 | 1 | 0.2 s |
| `grep -rln markkit crates` | index+walk | 6 | 2 | 0 | 3 | 0 | 1 | 0.1 s |
| `find /workspace -path '*markkit*'` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `rg -l SearchCapable crates` | index+walk | 39 | 18 | 11 | 9 | 0 | 1 | 0.4 s |
| `rg -l --no-ignore SearchCapable crates` | index+walk | 19 | 16 | 0 | 2 | 0 | 1 | 0.2 s |
| `grep -rl SearchCapable crates` | index+walk | 34 | 16 | 0 | 17 | 0 | 1 | 0.3 s |
| `rg -c fn crates/bashkit/src/builtins` | index+walk | 243 | 211 | 22 | 9 | 0 | 1 | 2.4 s |
| `find crates -type f -name '*.rs'` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `find crates -ls` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `ls -R crates` | index+walk | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `du -s crates` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `tree crates` | index+walk | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | index+walk | 234 | 213 | 17 | 3 | 0 | 0 | 2.3 s |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | index+walk | 783 | 0 | 17 | 519 | 0 | 0 | 7.8 s |

## Criterion wall time, 100 us simulated latency per call

Median of 10 samples. Walk-heavy rows are CPU-bound by `InMemoryFs::read_dir` (a scan over all entries per call), not by round trips.

| command | backend | median |
|---|---|---:|
| `rg -l markkit crates` | plain | 3.0597 s |
| `grep -rln markkit crates` | plain | 2.8932 s |
| `find /workspace -path '*markkit*'` | plain | 2.4749 s |
| `rg -l SearchCapable crates` | plain | 3.0207 s |
| `rg -l --no-ignore SearchCapable crates` | plain | 2.8959 s |
| `grep -rl SearchCapable crates` | plain | 2.9299 s |
| `rg -c fn crates/bashkit/src/builtins` | plain | 67.735 ms |
| `find crates -type f -name '*.rs'` | plain | 2.3173 s |
| `find crates -ls` | plain | 2.2931 s |
| `ls -R crates` | plain | 2.2760 s |
| `du -s crates` | plain | 2.2095 s |
| `tree crates` | plain | 2.3023 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | plain | 47.803 ms |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | plain | 189.74 ms |
| `rg -l markkit crates` | index | 87.746 ms |
| `grep -rln markkit crates` | index | 76.834 ms |
| `find /workspace -path '*markkit*'` | index | 2.7527 s |
| `rg -l SearchCapable crates` | index | 102.13 ms |
| `rg -l --no-ignore SearchCapable crates` | index | 80.975 ms |
| `grep -rl SearchCapable crates` | index | 79.148 ms |
| `rg -c fn crates/bashkit/src/builtins` | index | 85.890 ms |
| `find crates -type f -name '*.rs'` | index | 2.6288 s |
| `find crates -ls` | index | 2.7281 s |
| `ls -R crates` | index | 2.7668 s |
| `du -s crates` | index | 2.7436 s |
| `tree crates` | index | 2.7659 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | index | 59.947 ms |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | index | 217.61 ms |
| `rg -l markkit crates` | index+walk | 86.722 ms |
| `grep -rln markkit crates` | index+walk | 76.385 ms |
| `find /workspace -path '*markkit*'` | index+walk | 2.4043 s |
| `rg -l SearchCapable crates` | index+walk | 94.729 ms |
| `rg -l --no-ignore SearchCapable crates` | index+walk | 76.746 ms |
| `grep -rl SearchCapable crates` | index+walk | 77.005 ms |
| `rg -c fn crates/bashkit/src/builtins` | index+walk | 87.444 ms |
| `find crates -type f -name '*.rs'` | index+walk | 2.4254 s |
| `find crates -ls` | index+walk | 2.4682 s |
| `ls -R crates` | index+walk | 2.6807 s |
| `du -s crates` | index+walk | 2.4679 s |
| `tree crates` | index+walk | 2.8006 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | index+walk | 58.151 ms |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | index+walk | 226.55 ms |
