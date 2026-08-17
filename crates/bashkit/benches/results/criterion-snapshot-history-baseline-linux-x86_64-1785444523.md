# criterion, snapshot_history, baseline

First run of `cargo bench -p bashkit --bench snapshot_history`, establishing the
baseline for the content-addressed snapshot object graph.

- **Host**: Intel Xeon @ 2.80 GHz, 4 vCPU, Linux x86_64 (no SHA-NI)
- **Command**: `cargo bench -p bashkit --bench snapshot_history -- --warm-up-time 1 --measurement-time 2`
- **Workload**: `N` small text files under `/w/src`, plus one 20 000-line file
  under `/w/docs` so both the inline and chunked paths are exercised
- **Moniker**: `baseline`, first measurement of this bench, nothing to diff against

## Size, the number that motivated the work

Bytes for the same state, and the marginal cost of one more commit after
appending to a single file.

| files | v1 JSON | v2 packed | ratio | incremental commit |
|------:|--------:|----------:|------:|-------------------:|
| 10 | 330 594 | 43 816 | 7.55x | 1 151 |
| 100 | 354 896 | 56 600 | 6.27x | 4 419 |
| 500 | 464 496 | 113 756 | 4.08x | 18 929 |

The packed format alone is 4-7.5x smaller. The number that decides whether
per-message history is affordable is the last column: storing another commit
after a one-file edit costs **1.1 KB / 4.4 KB / 19 KB**, against a 330-464 KB
full v1 snapshot, 287x, 80x, and 24x cheaper respectively.

The ratio narrows as file count grows because the tree is a **single flat
object** listing every path, so each commit re-stores the whole tree. At 500
files that tree dominates the incremental cost. Splitting trees per directory
(git-style) would make it O(changed paths + depth) instead of O(total files);
see the known gaps in `knowledge/foundations/snapshot-history.md`.

## Capture

| workload | v1 JSON | v2 packed | v2 incremental |
|---|---:|---:|---:|
| 10 files | 2.02 ms | 4.41 ms | **715 µs** |
| 100 files | 2.21 ms | 5.81 ms | **853 µs** |
| 500 files | 2.88 ms | 11.25 ms | **1.46 ms** |

Packed capture is 2-4x *slower* than v1: it hashes, chunks, and deflates
everything, where v1 only serialized. That is the price of content addressing,
paid on the checkpoint/resume path.

The steady-state path is the incremental column, a warm store, so unchanged
objects are hashed but neither compressed nor emitted. It is 1.4-2.8x faster
than a v1 full snapshot *and* orders of magnitude smaller, which is the
combination session history needs.

## Restore

| workload | v1 JSON | v2 packed | v2 checkout |
|---|---:|---:|---:|
| 10 files | 2.78 ms | 1.38 ms | **1.16 ms** |
| 100 files | 3.00 ms | 1.74 ms | **1.43 ms** |
| 500 files | 4.04 ms | 3.70 ms | **3.05 ms** |

v2 restores faster than v1 at every size, decoding a binary container beats
parsing a JSON integer array per byte. `checkout` from a store beats unpacking
a container because it skips container framing entirely.

## Graph operations

100-file workspace, two commits.

| operation | time |
|---|---:|
| `diff` | 149 µs |
| `plan_checkout` (warm store) | 1.26 ms |
| `reachable` | 1.27 ms |

`diff` compares content addresses rather than content, so it never reads file
data, which is why it is an order of magnitude cheaper than the walks.

## Reading these numbers

Capture time is dominated by SHA-256, and this host has no SHA-NI (measured
~215 MB/s, against ~1.5-2 GB/s expected on hardware that has it). Expect the
packed-capture regression to shrink materially on production hardware. Nothing
here is hash-bound on the restore side.


## Large binary files

Separate probe (`cargo run --release -p bashkit --example large_binary_probe`),
incompressible pseudo-random content, single file, same host. Not a criterion
bench, one-shot timings, so treat them as magnitudes.

| size | objects | stored | commit | checkout | 16-byte edit | peak RSS |
|-----:|--------:|-------:|-------:|---------:|-------------:|---------:|
| 1 MB | 51 | 1.00x | 22 ms | 10 ms | 8.8 KB | 13 MB |
| 8 MB | 436 | 1.00x | 123 ms | 61 ms | 77 KB | 62 MB |
| 32 MB | 1 776 | 1.00x | 489 ms | 257 ms | 80 KB | 230 MB |
| 64 MB | 3 575 | 1.00x | 1 003 ms | 556 ms | 152 KB | 455 MB |

What this shows:

- **Chunking earns its keep on binary.** A 16-byte edit in the middle of a
  64 MB file costs 152 KB, not 64 MB, roughly 440x better than re-storing.
  Content round-trips byte for byte.
- **Incompressible data is stored raw** (1.00x), as intended: deflate is not
  allowed to inflate an object.
- **Commit throughput is ~64 MB/s** after the compressibility probe landed.
  Before it, deflating incompressible chunks and discarding every result cost
  2 439 ms at 64 MB, the probe cut that to 1 003 ms with identical output.
- **Peak RSS runs ~5x file size** during commit (the figures above include the
  probe's own two copies of the payload). `VfsSnapshot` clones every file's
  content before the object encoder sees it, so a streaming `vfs_snapshot` is
  what would fix this.
- **The per-edit floor is the file manifest**, at 32 bytes per chunk, about
  2 KB per MB of file, re-stored whole on every edit. That is what dominates
  the 152 KB figure at 64 MB, not the changed chunk. Chunking the manifest
  itself would remove it, the same shape as the flat-tree limit above.

Practical read: files up to a few MB are comfortable. Tens of MB work but cost
seconds of commit time and hundreds of MB of transient memory, so a workspace
built around large binaries wants the streaming snapshot first.
