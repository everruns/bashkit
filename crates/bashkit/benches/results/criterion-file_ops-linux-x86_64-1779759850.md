# VFS / File-Ops Bench: Initial Baseline

Criterion micro-benchmarks (`cargo bench --bench file_ops`) — first
coverage for the VFS read path, tree traversal, glob expansion, `rg`,
and `grep -r` vs `rg` parity.

100 samples per case, default Criterion config. `[profile.bench]` =
release + lto=fat + codegen-units=1. Shared seeded `InMemoryFs`
(`FsLimits::unlimited`) per group.

## Read throughput

`Throughput::Bytes` reported, so the rows include MiB/s.

| File size | `cat` | `grep <literal>` | `grep -c` |
|---|---:|---:|---:|
| 1 KB | 57 µs | 53 µs | 46 µs |
| 1 MB | 2.97 ms (~329 MiB/s) | 4.50 ms (~217 MiB/s) | 3.70 ms (~264 MiB/s) |
| 50 MB | 223 ms (~225 MiB/s) | 341 ms (~147 MiB/s) | 275 ms (~182 MiB/s) |

## Traversal (1000-file tree across 10 subdirs)

| Case | Time (median) |
|---|---:|
| `ls -R /work` | 1.25 ms |
| `find /work -name 'f001.txt'` | 1.78 ms |
| `find /work` (no filter) | 1.77 ms |
| `for f in /work/d00/*` (shallow glob) | 267 µs |
| `for f in /work/**/*` (globstar) | 3.35 ms |
| `echo /work/**/*` (globstar → argv) | 2.00 ms |

## `rg` workloads (same tree)

| Case | Time (median) |
|---|---:|
| `rg needle /work` | 3.21 ms |
| `rg '\balpha \d+ \d+\b' /work` | 3.58 ms |
| `rg --no-ignore needle /work` | 3.17 ms |
| `rg --multiline 'alpha.*\n.*beta' /work` | 3.60 ms |
| `rg needle /work/d00/f000.txt` (single file) | 540 µs |

## `grep -r` vs `rg` parity

| Query | `grep -r` | `rg` | rg vs grep |
|---|---:|---:|---|
| literal `needle` | **2.06 ms** | 3.26 ms | rg **+58%** slower |
| regex `alpha [0-9]+ [0-9]+` | **2.17 ms** | 3.38 ms | rg **+56%** slower |

`rg` is consistently slower than `grep -r` on this 1000-file × ~6-line
tree. Likely per-invocation init/threading overhead dominating on tiny
files; `rg`'s strengths show on much larger inputs that aren't in this
bench yet. The parity table makes the gap visible going forward.
