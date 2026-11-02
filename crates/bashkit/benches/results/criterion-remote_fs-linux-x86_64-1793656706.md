# remote_fs bench, bashkit checkout (598ebb4 + this change)

Host: cloud container, linux x86_64. Tree: `git ls-files` of this repo, 3,720 files under `/workspace` (a `.git` dir present so `.gitignore` applies). Command: `cargo bench --bench remote_fs`.

Call counts are the signal: on a remote backend each call is a round trip ("at 10 ms" column).

## Before (origin/main, same CountingFs, no `walk` hook)

| command | backend | calls | read_file | read_dir | stat+lstat+exists | walk | search | at 10 ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `rg -l markkit crates` | index | 176281 | 7101 | 1981 | 167199 | 0 | 0 | 1762.8 s |
| `grep -rln markkit crates` | index | 23709 | 3140 | 1982 | 18587 | 0 | 0 | 237.1 s |
| `find /workspace -path '*markkit*'` | index | 7831 | 0 | 2055 | 5776 | 0 | 0 | 78.3 s |
| `rg -l SearchCapable crates` | index | 176281 | 7101 | 1981 | 167199 | 0 | 0 | 1762.8 s |
| `rg -l --no-ignore SearchCapable crates` | index | 5116 | 3133 | 1981 | 2 | 0 | 0 | 51.2 s |
| `grep -rl SearchCapable crates` | index | 23709 | 3140 | 1982 | 18587 | 0 | 0 | 237.1 s |
| `rg -c fn crates/bashkit/src/builtins` | index | 1255 | 264 | 17 | 974 | 0 | 0 | 12.6 s |
| `find crates -type f -name '*.rs'` | index | 7105 | 0 | 1982 | 5123 | 0 | 0 | 71.0 s |
| `find crates -ls` | index | 9087 | 0 | 3964 | 5123 | 0 | 0 | 90.9 s |
| `ls -R crates` | index | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `du -s crates` | index | 7105 | 0 | 1982 | 5123 | 0 | 0 | 71.0 s |
| `tree crates` | index | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | index | 463 | 212 | 17 | 233 | 0 | 0 | 4.6 s |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | index | 2785 | 0 | 17 | 2522 | 0 | 0 | 27.9 s |

## After

| command | backend | calls | read_file | read_dir | stat+lstat+exists | walk | search | at 10 ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `rg -l markkit crates` | plain | 5127 | 3135 | 1983 | 9 | 0 | 0 | 51.3 s |
| `grep -rln markkit crates` | plain | 5125 | 3140 | 1982 | 3 | 0 | 0 | 51.2 s |
| `find /workspace -path '*markkit*'` | plain | 2056 | 0 | 2055 | 1 | 0 | 0 | 20.6 s |
| `rg -l SearchCapable crates` | plain | 5127 | 3135 | 1983 | 9 | 0 | 0 | 51.3 s |
| `rg -l --no-ignore SearchCapable crates` | plain | 5116 | 3133 | 1981 | 2 | 0 | 0 | 51.2 s |
| `grep -rl SearchCapable crates` | plain | 5125 | 3140 | 1982 | 3 | 0 | 0 | 51.2 s |
| `rg -c fn crates/bashkit/src/builtins` | plain | 245 | 214 | 22 | 9 | 0 | 0 | 2.5 s |
| `find crates -type f -name '*.rs'` | plain | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `find crates -ls` | plain | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `ls -R crates` | plain | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `du -s crates` | plain | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `tree crates` | plain | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | plain | 233 | 212 | 17 | 3 | 0 | 0 | 2.3 s |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | plain | 780 | 0 | 17 | 517 | 0 | 0 | 7.8 s |
| `rg -l markkit crates` | index | 3 | 0 | 0 | 2 | 0 | 1 | 0.0 s |
| `grep -rln markkit crates` | index | 2 | 0 | 0 | 1 | 0 | 1 | 0.0 s |
| `find /workspace -path '*markkit*'` | index | 2056 | 0 | 2055 | 1 | 0 | 0 | 20.6 s |
| `rg -l SearchCapable crates` | index | 33 | 15 | 8 | 9 | 0 | 1 | 0.3 s |
| `rg -l --no-ignore SearchCapable crates` | index | 16 | 13 | 0 | 2 | 0 | 1 | 0.2 s |
| `grep -rl SearchCapable crates` | index | 28 | 13 | 0 | 14 | 0 | 1 | 0.3 s |
| `rg -c fn crates/bashkit/src/builtins` | index | 242 | 210 | 22 | 9 | 0 | 1 | 2.4 s |
| `find crates -type f -name '*.rs'` | index | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `find crates -ls` | index | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `ls -R crates` | index | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `du -s crates` | index | 1983 | 0 | 1982 | 1 | 0 | 0 | 19.8 s |
| `tree crates` | index | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | index | 233 | 212 | 17 | 3 | 0 | 0 | 2.3 s |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | index | 780 | 0 | 17 | 517 | 0 | 0 | 7.8 s |
| `rg -l markkit crates` | index+walk | 3 | 0 | 0 | 2 | 0 | 1 | 0.0 s |
| `grep -rln markkit crates` | index+walk | 2 | 0 | 0 | 1 | 0 | 1 | 0.0 s |
| `find /workspace -path '*markkit*'` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `rg -l SearchCapable crates` | index+walk | 33 | 15 | 8 | 9 | 0 | 1 | 0.3 s |
| `rg -l --no-ignore SearchCapable crates` | index+walk | 16 | 13 | 0 | 2 | 0 | 1 | 0.2 s |
| `grep -rl SearchCapable crates` | index+walk | 28 | 13 | 0 | 14 | 0 | 1 | 0.3 s |
| `rg -c fn crates/bashkit/src/builtins` | index+walk | 242 | 210 | 22 | 9 | 0 | 1 | 2.4 s |
| `find crates -type f -name '*.rs'` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `find crates -ls` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `ls -R crates` | index+walk | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `du -s crates` | index+walk | 2 | 0 | 0 | 1 | 1 | 0 | 0.0 s |
| `tree crates` | index+walk | 1982 | 0 | 1981 | 1 | 0 | 0 | 19.8 s |
| `tar -cf /tmp/src.tar crates/bashkit/src/builtins` | index+walk | 233 | 212 | 17 | 3 | 0 | 0 | 2.3 s |
| `cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | index+walk | 780 | 0 | 17 | 517 | 0 | 0 | 7.8 s |

## Criterion wall time, 100 us simulated latency per call

Run concurrently with a test build, so noisy. Walk-heavy rows are CPU-bound by `InMemoryFs::read_dir`, which scans every entry per call (O(entries x dirs)); that is the in-memory backend's cost, not round trips.

| bench | median |
|---|---:|
| `plain/rg -l markkit crates` | 3.1882 s |
| `plain/grep -rln markkit crates` | 2.9633 s |
| `plain/find /workspace -path '*markkit*'` | 2.5221 s |
| `plain/rg -l SearchCapable crates` | 3.2156 s |
| `plain/rg -l --no-ignore SearchCapable crates` | 3.0005 s |
| `plain/grep -rl SearchCapable crates` | 2.9670 s |
| `plain/rg -c fn crates/bashkit/src/builtins` | 67.001 ms |
| `plain/find crates -type f -name '*.rs'` | 2.3600 s |
| `plain/find crates -ls` | 2.3169 s |
| `plain/ls -R crates` | 2.4517 s |
| `plain/du -s crates` | 2.4584 s |
| `plain/tree crates` | 2.6678 s |
| `plain/tar -cf /tmp/src.tar crates/bashkit/src/builtins` | 48.890 ms |
| `plain/cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | 199.99 ms |
| `index/rg -l markkit crates` | 79.563 ms |
| `index/grep -rln markkit crates` | 80.336 ms |
| `index/find /workspace -path '*markkit*'` | 2.9004 s |
| `index/rg -l SearchCapable crates` | 94.123 ms |
| `index/rg -l --no-ignore SearchCapable crates` | 82.366 ms |
| `index/grep -rl SearchCapable crates` | 80.259 ms |
| `index/rg -c fn crates/bashkit/src/builtins` | 92.512 ms |
| `index/find crates -type f -name '*.rs'` | 2.8683 s |
| `index/find crates -ls` | 2.8529 s |
| `index/ls -R crates` | 2.9771 s |
| `index/du -s crates` | 3.0325 s |
| `index/tree crates` | 3.0087 s |
| `index/tar -cf /tmp/src.tar crates/bashkit/src/builtins` | 56.475 ms |
| `index/cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | 263.19 ms |
| `index+walk/rg -l markkit crates` | 80.651 ms |
| `index+walk/grep -rln markkit crates` | 78.429 ms |
| `index+walk/find /workspace -path '*markkit*'` | 2.6263 s |
| `index+walk/rg -l SearchCapable crates` | 96.695 ms |
| `index+walk/rg -l --no-ignore SearchCapable crates` | 81.962 ms |
| `index+walk/grep -rl SearchCapable crates` | 96.617 ms |
| `index+walk/rg -c fn crates/bashkit/src/builtins` | 96.170 ms |
| `index+walk/find crates -type f -name '*.rs'` | 2.5372 s |
| `index+walk/find crates -ls` | 2.5755 s |
| `index+walk/ls -R crates` | 2.8837 s |
| `index+walk/du -s crates` | 2.6339 s |
| `index+walk/tree crates` | 3.0670 s |
| `index+walk/tar -cf /tmp/src.tar crates/bashkit/src/builtins` | 51.691 ms |
| `index+walk/cp -r crates/bashkit/src/builtins /tmp/builtins-copy` | 249.46 ms |
