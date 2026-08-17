---
type: Subsystem Design
title: Snapshot History and Deltas
description: Content-addressed snapshot objects, commit DAG with forks, chunked binary content, and the version and capability compatibility rules for restore.
tags:
  - bashkit
  - snapshot
  - persistence
  - history
  - versioning
---

# Snapshot History and Deltas

## Status

Implemented. `crates/bashkit/src/snapshot/` holds the object graph, container,
chunker, capability fingerprint, and graph operations; Python and JS bindings
expose `commit`/`checkout` and the graph queries.

**Released over two versions, reader first.** The reason is in
[Decision: version policy that survives upgrades](#decision-version-policy-that-survives-upgrades):
`min_reader` makes every future format change safe but cannot make the first
one safe, because the readers that predate it reject a v2 container with a JSON
parse error naming neither the version nor the format.

| Release | Reads v2 | Writes v2 | Public API |
|---|---|---|---|
| 0.14.5 (patch) | yes | no | unchanged, object-graph types `pub(crate)` |
| 0.15.0 (minor) | yes | yes | `commit`/`checkout`, `SnapshotGraph`, `CheckoutPolicy`, bindings |

0.14.5 exists so a deployment can become able to *read* v2 before anything
writes it, making it a safe rollback target for 0.15.0. It could ship as a patch
only because it added no public API, hence `pub(crate)` on the new types and
`Error::Internal` in place of the typed variants, since `Error` is not
`#[non_exhaustive]`. 0.15.0 lifts both, along with the `allow(dead_code)` that
covered the then-unreachable encoder.

Rolling back past 0.14.5 strands any v2 snapshot, and no code change can
retrofit that, the failing readers already shipped.

Driver: [#2221](https://github.com/everruns/bashkit/issues/2221), which needs one
snapshot per conversation message with session truncation and branching.

Snapshots remain a between-executions boundary. Process-local host calls may
park a live execution through `ExecutionHandle`, but that handle contains a
pinned Rust future plus its owned `Bash` session and is deliberately excluded
from packed snapshots and commits. Dropping it drops the session; migration of
a pending call would require a separate explicit-continuation interpreter
design. See [Builtin Commands](builtins.md).

## Problem

The v1 format was a single `serde_json` blob: `[32-byte digest][JSON]`. Four
properties made per-message snapshots impractical.

1. **Whole-tree re-encode.** A turn writes one or two files; the snapshot
   re-encoded every file in the VFS.
2. **Binary-hostile encoding.** `VfsEntryKind::File { content: Vec<u8> }` went
   through `serde_json` with no `serde_bytes`, so every byte became a decimal
   integer in a JSON array.
3. **No compression, no dedup.** Identical files across snapshots and across
   branches were stored in full, every time.
4. **Non-deterministic bytes.** `ShellState` uses `HashMap`, and
   `VfsSnapshot.entries` came from `HashMap` iteration, so identical state
   serialized differently run to run. External delta compression could not
   help either.

Restore was also replace-only, and `from_bytes` rejected any `version != 1` in
both directions, so bumping the format would have invalidated stored snapshots.

## Decision: content-addressed object graph

| Object | Contains | Encoding |
|---|---|---|
| `Chunk` | One content-defined slice of file content | raw bytes |
| `File` | Inline content, or size plus an ordered chunk list | binary |
| `Tree` | Every VFS entry sorted by path: path, kind, mode, content ref | binary |
| `Shell` | `ShellState` | canonical JSON |
| `Caps` | Capability fingerprint of the producing instance | canonical JSON |
| `Commit` | Parents, tree, shell, caps, counters, host metadata | canonical JSON |

Object ID is `SHA-256(kind_byte || payload)`. The kind byte is domain
separation: without it a chunk and a tree with identical bytes would collide.

**Why a graph, not a patch chain.** The consumer read pattern is "materialize an
arbitrary past point", and branching makes history a tree. A linear patch chain
optimizes writes and penalizes exactly those reads, and every fork forces a
replay or a new baseline. Here, unchanged content costs one hash reference, any
commit materializes directly, and forks share storage automatically.

A fork is a commit whose parent is not the branch tip, there is no fork
operation. Truncation is pointing at an older `CommitId`.

**Structural objects are binary, descriptive objects are canonical JSON.** File
and tree objects carry raw bytes and a fixed shape. Commit, caps, and shell
objects need field-level forward compatibility: an unknown field must be
ignorable, which serde gives and a positional binary format does not.

### Canonical JSON

`canonical_json` re-emits through `serde_json::Value` with every object key
sorted. Snapshot identity is a content hash, so unstable key order would mean
identical state producing different commit IDs. Tree entries are sorted by path
for the same reason.

### Hashing

SHA-256. Already a direct dependency, wasm-safe, and, the deciding argument,
content addressing puts the hash in the host's database, where SHA-256 is
stdlib in Python and built into Postgres while BLAKE3 is a third-party
dependency in both. Measured throughput on an AVX2 host without SHA-NI was
~215 MB/s against ~2500 MB/s for `blake3` with `pure`; the gap is real but is
largely absorbed by only hashing on commit. The container header carries a hash
algorithm ID so this can change without a framing break, but note the data-side
asymmetry: one store holding two algorithms keeps identical content under two
IDs, so a switch means rehashing history or losing dedup across the boundary.

## Decision: two-phase pull API, no store trait

Bashkit never calls into host storage. `commit` returns objects; `checkout`
takes them.

```rust
let packed = bash.commit(CommitOptions::new().parent(prev).have(store.keys()))?;
store.extend(packed.into_objects());

bash.checkout(commit_id, &store, CheckoutPolicy::default())?;
```

A store trait would have to cross PyO3 and wasm-bindgen as an async callback,
the hardest part of the design to build, bind, and test, in exchange for
nothing the pull model cannot express. Tests use a `HashMap` as the store.

`CommitOptions::have` is what makes commits incremental; without it every
commit is self-contained. `PackedCommit::to_bytes` refuses to pack an
incremental commit rather than emitting bytes that cannot be restored.

`SnapshotGraph::plan_checkout` returns objects that are missing *and currently
reachable*. Chunk IDs live inside file manifests, so a cold caller cannot learn
them in one round; callers loop until the plan is empty. The graph is four
levels deep, so it converges in at most four rounds.

`snapshot()` and `from_snapshot()` remain, redefined as a **packed commit**:
the commit plus every object it reaches, sealed with the existing digest. The
public byte API is unchanged.

## Decision: chunked, binary-safe content

- Chunks are raw bytes. File content is never text-encoded.
- Files at or below `INLINE_MAX` (4 KiB) are inlined in their manifest; larger
  files are split by content-defined chunking (2 KiB min, 16 KiB average,
  64 KiB max) and the manifest holds the chunk list.
- Each object is deflate-compressed when that helps, and stored raw when it
  does not.

Content-defined chunking delivers the per-file differential: an edit re-chunks
only around the edit, so untouched chunks are already in the store. It behaves
identically for text and binary, needs no per-path base tracking, needs no diff
algorithm, and dedups across paths and forks as a side effect. Byte-delta
encoding (bsdiff, xdelta, `zstd --patch-from`) was rejected: it needs a base per
path, degrades on forks, and the C-backed implementations complicate wasm.

**Chunking is in-tree, not `fastcdc`.** The proposal named an in-tree gear-hash
CDC as the fallback if a crate could not clear `cargo deny`; that fallback was
taken up front, because the parameters have to be pinned in the format spec
regardless and the algorithm is ~100 lines. The gear table is generated by a
fixed xorshift sequence in a `const fn`, so it is identical on every platform.
`MIN_CHUNK`, `AVG_BITS`, and `MAX_CHUNK` are format constants: changing one
changes every object ID.

**Object identity covers decoded content, not compressed framing.** A host may
recompress its store freely. The consequence, verified by test: flipping unused
padding bits in a deflate stream's final byte produces byte-different storage
that decodes identically, and is correctly not treated as tampering. Bytes
appended *after* the stream are rejected, because a decoder would otherwise
ignore them and let a store carry payload the content address does not cover.

## Decision: version policy that survives upgrades

Three independent numbers in the container header.

| Field | Meaning | Bump when |
|---|---|---|
| `container_version` | Envelope framing | The byte layout changes |
| `schema_version` | Metadata field set | Fields are added or removed |
| `min_reader` | Oldest reader that can read this correctly | Semantics change incompatibly |

- A reader accepts any snapshot with `min_reader <= READER_VERSION`, however
  much newer the other two are.
- Unknown metadata fields are ignored. Metadata structs use `#[serde(default)]`
  and must not use `deny_unknown_fields`.
- `min_reader > READER_VERSION` yields `Error::SnapshotTooNew { required,
  supported }`, a typed error, never a panic, never a partial restore.
- v1 JSON payloads stay readable. `decode_sealed` dispatches on the body
  prefix: `BKSNAP` magic means v2, anything else is parsed as v1 JSON.

**Golden corpus.** `crates/bashkit/tests/fixtures/snapshots/` holds one snapshot
per released format version, restored by every CI run
(`snapshot_fixture_tests`). Fixtures are never regenerated, that would defeat
their purpose. A new format version adds a file via
`cargo run -p bashkit --example generate_snapshot_fixtures`, and a test asserts
the corpus listing matches the tests, so a fixture cannot be added without one.

Corpus restores use `CheckoutPolicy::Force` on purpose: a fixture's fingerprint
names the builtin set of the build that wrote it, so any later release would
fail the capability gate. The corpus tests format compatibility; capability
policy is tested separately.

**The one break `min_reader` cannot prevent is its own introduction.** Readers
older than 0.14.5 parse the sealed body as JSON and fail at column 1 on a v2
container. That is why the reader shipped a release ahead of the writer, see
Status.

## Decision: capability fingerprint

Each commit references a caps object recording builtin names (via
`Bash::builtin_names()`, which already covers baked-in, host-registry, and
scripted tools), semantics-changing cargo features, the filesystem backend kind
(`FileSystemExt::backend_kind`), and the bashkit version (informational, not
compared).

The fingerprint is its own object rather than being inlined in the commit,
because a session's environment is identical across every commit it produces,
so content addressing stores the ~2 KB builtin list once per session instead of
once per message. It records names, not a hash of them, because `Superset` has
to compute which names are missing in order to report them.

| Policy | Rule |
|---|---|
| `Superset` (default) | Live capabilities must contain the snapshot's; additions are fine |
| `Strict` | Exact match in both directions |
| `Force` | Restore regardless, caller accepts the consequences |

**The default is `Superset`, changed from `Strict` during implementation.** The
dangerous direction is asymmetric: restoring into an environment lacking a tool
the session used can produce a session that cannot run, while extra tools
cannot. And because the fingerprint records the whole builtin set, `Strict` as
the default would mean every stored snapshot stops restoring the moment bashkit
ships a new builtin, pushing long-lived callers straight to `Force` and losing
the check entirely.

Two limits, stated because they are easy to over-read:

- The fingerprint proves the environments match. It cannot prove a restored
  session will behave.
- A snapshot taken before a tool was registered carries no record of it, so
  `Strict` rejects an otherwise fine restore into a richer instance.

Where captured state provably requires a feature, the state is checked instead
of the fingerprint: a VFS holding SQLite databases (detected by file magic) is
refused on a build without the `sqlite` feature under **every** policy,
including `Force`.

v1 snapshots carry no fingerprint, so v1 restores skip the gate entirely. That
is a deliberate consequence of keeping old snapshots readable.

## History and diff surface

```rust
SnapshotGraph::read_commit(id, &store)   // CommitObject
SnapshotGraph::parents(id, &store)
SnapshotGraph::meta(id, &store)          // opaque host metadata
SnapshotGraph::capabilities(id, &store)
SnapshotGraph::ancestry(id, &store, limit)
SnapshotGraph::plan_checkout(id, &store)
SnapshotGraph::reachable(id, &store)     // for host-side GC
SnapshotGraph::diff(a, b, &store)        // SnapshotDiff
```

`diff` compares content addresses, so detecting a change never reads file
content. `ancestry` walks only what the supplied store contains and stops
cleanly at a pruned ancestor rather than failing; it tracks visited commits so a
hostile store serving a cycle cannot make it loop.

Bashkit does not own the DAG index. Hosts store the `CommitId` per message,
for the driving consumer, a foreign key from their message table.

`reachable` deliberately does **not** traverse parents: checking out a commit
never needs its ancestors, so including them would make GC retain history a host
had already decided to prune.

## Bindings

Python and JS mirror the Rust API with one deliberate difference: the object
store is a plain `dict[str, bytes]` / `Record<string, Buffer>` keyed by **hex**
object id, not an opaque handle. Hosts persist these objects in their own
database, so the binding hands back what a driver, JSON column, or blob store
already accepts. Conversion copies on every call, which is the cost of not
owning storage; a host that wants to avoid loading a whole session uses
`plan_checkout` to fetch wave by wave.

`CheckoutPolicy` crosses as a case-insensitive string (`"strict"`,
`"superset"`, `"force"`) rather than an enum class, so no import is needed for
the common path and an unknown value fails with a named error.

`PackedCommit.packed` (JS) / `PackedCommit.to_bytes()` (Python) is `null` /
raises for an incremental commit, packing one would produce bytes that cannot
be restored, so the binding refuses rather than returning a broken blob.

Python: `crates/bashkit-python/tests/test_snapshot_history.py`, runnable
example in `crates/bashkit-python/examples/session_history.py`.
JS: `crates/bashkit-js/__test__/snapshot-history.spec.ts` (AVA, Node) **and**
`__test__/runtime-compat/snapshot-history.test.mjs`. The second is required, not
duplicated effort: AVA runs only under Node, while the object store crosses as
`Record<string, Buffer>`, and Buffer semantics differ between Node, Bun, and
Deno. The runtime-compat file covers the cases where that difference would show
, binary round trips, byte-level mutation, and hex id stability.

## Security

The graph is a Merkle tree. Every object is verified against its content hash on
load, so sealing or HMAC-ing the root commit authenticates everything beneath
it. The unkeyed digest remains forgeable exactly as TM-SNAP-001 describes.

Validate-before-mutate is preserved without exception: v2 checkout reconstructs
a `Snapshot` and funnels into the same `restore_snapshot_inner` as v1, so limit
validation, atomic VFS replacement, builtin cache invalidation, and monotonic
counter merging are identical across formats by construction rather than by
parallel implementations that could drift. Capability and state-evidence checks
run before that call.

Registered in [Threat Model](../security/threat-model.md): TM-SNAP-002 (object
store poisoning), TM-SNAP-003 (hash agility), TM-SNAP-004 (chunk/decompression
bomb), TM-SNAP-005 (malformed graph), TM-SNAP-006 (capability mismatch).

Decoder hardening: declared counts are bounded before allocation, per-object
decompression is capped at 256 MiB, object kinds are checked against the kind
expected from context, and commit parent lists are capped at 64, on **encode**
as well as decode, so `commit` cannot hand back a commit that no checkout would
accept.

Materialization is separately bounded, because per-object caps do not bound the
*assembled* result. A file manifest is small even when it names millions of
chunks, so repeating one chunk id would otherwise grow a file without limit
before `FsLimits`, which only runs on a finished `VfsSnapshot`, could reject
it. Three checks, all in `resolve_file`:

- the declared file size is charged to a per-checkout budget **before**
  allocating, so an absurd declaration is refused rather than attempted;
- assembly stops the moment accumulated chunks exceed the declared size;
- a chunk larger than `MAX_CHUNK` is rejected, since the chunker never emits one.

The budget (`MAX_CHECKOUT_BYTES`, 4 GiB) is shared across every file in a tree,
so many individually-legal files cannot add up past it. It is a backstop against
absurd input, not a replacement for `FsLimits`.

Object ids parse by decoding byte pairs, never by slicing the `str`. Ids arrive
as strings from three languages and `str::len` counts bytes, so a 64-byte string
containing any multi-byte character would otherwise be sliced mid-character and
panic where it must return an error.

**A forged tree names paths directly, bypassing every bash command that would
normally create them.** Probed rather than assumed: traversal (`/../../etc/passwd`),
non-normalized (`/a/../b.txt`), relative, and empty paths are all *accepted into
the map but unreachable*, path resolution never produces those keys, so they are
inert rather than an escape. A NUL byte in a component is rejected outright by
`FsLimits::validate_path`, atomically. Duplicate paths are last-wins, which is
deterministic because tree entries are sorted.

The invariant worth protecting is "unreachable", not "rejected": if restore ever
gained path normalization, `/a/../b.txt` would resolve to `/b.txt` and a forged
snapshot could shadow a real file. `forged_paths_are_inert_rather_than_a_sandbox_escape`
is what would catch that.

`ancestry` distinguishes an absent ancestor from a corrupt one: absence ends the
walk (a normal retention outcome), while an object that is present but fails
verification or decoding propagates the error. Collapsing the two would report
data loss as ordinary pruned history.

## Tests

| Direction | Where |
|---|---|
| Round-trip: shell, files, symlinks, directories, modes | `snapshot_history_tests` |
| Binary fidelity: NUL, 0x7f/0x80/0xff, multi-chunk files | `snapshot_history_tests` |
| Forks: divergence, isolation, storage sharing | `snapshot_history_tests` |
| Mid-history: every ancestor checks out to its own state | `snapshot_history_tests` |
| Differential: checkout equals packed restore at every step | `snapshot_history_tests` |
| Ancestry, diff, metadata, reachability | `snapshot_history_tests` |
| Incremental storage and chunk reuse on a large-file edit | `snapshot_history_tests` |
| Determinism: identical state, identical commit ID | `snapshot_history_tests` |
| Capability policy: all three, plus untouched-on-failure | `snapshot_history_tests` |
| Corruption: content tamper, truncation, appended bytes, missing object, unknown root, type confusion, cycles | `snapshot_history_tests` |
| Keyed snapshots and wrong-key rejection | `snapshot_history_tests` |
| Version: too-new rejected typed, newer non-breaking accepted | `snapshot_history_tests` |
| Golden corpus per format version | `snapshot_fixture_tests` |
| Limits: over-limit checkout refused atomically | `snapshot_history_tests` |
| Encoding units: chunker, objects, container, capabilities, graph budgets | `src/snapshot/*` unit tests |
| Materialization bounds: repeated chunks, absurd declared size, oversized chunk, cumulative tree budget | `src/snapshot/graph.rs` unit tests |
| Object-id parsing: non-ASCII, wrong length, both hex cases | `src/snapshot/objects.rs` unit tests |
| JS runtime parity across Node, Bun, and Deno | `crates/bashkit-js/__test__/runtime-compat/snapshot-history.test.mjs` |
| Hostile VFS entries: traversal, non-normalized, relative, empty, NUL, duplicate paths, escaping and self-looping symlinks | `snapshot_history_tests` |
| Arbitrary/truncated bytes never panic; instance survives every rejection | `snapshot_history_tests`, `tests/proptest_security.rs` |
| Decoder fuzzing: container, objects, ids, graph walkers | `fuzz/fuzz_targets/snapshot_fuzz.rs` |

Note on the corruption tests: they assert that changes to *content* are always
caught, not that every possible bit flip is. See the framing invariant above.

Benchmarks: `cargo bench -p bashkit --bench snapshot_history`, which also prints
a v1-vs-v2 size table and the marginal cost of an incremental commit. Results go
in `crates/bashkit/benches/results/`.

## Known gaps

- Reading *through* a symlink does not dereference in the VFS. Pre-existing and
  unrelated to snapshots, links round-trip correctly as links.
- No per-path content-hash cache in the VFS, so every commit rehashes the whole
  tree. Steady-state commit latency, not correctness; an mtime-based shortcut
  is not a safe substitute because restore stamps `modified = now` on every
  entry.
- **Large binary files: chunking works, memory and manifest size do not
  scale as well.** Measured with `cargo run --release -p bashkit --example
  large_binary_probe` on incompressible content: a 16-byte edit in the middle of
  a 64 MB file costs 152 KB rather than 64 MB, and content round-trips exactly.
  But peak RSS runs ~5x file size during commit, because `VfsSnapshot` clones
  every file's content before the object encoder ever sees it; and the file
  manifest (32 bytes per chunk, ~2 KB per MB of file) is re-stored whole on
  every edit, which is what dominates that 152 KB. Fixing the first needs a
  streaming `vfs_snapshot`; the second needs the manifest itself chunked, the
  same shape as the flat-tree problem below.
- **The tree is one flat object listing every path**, so each commit re-stores
  the whole tree even when one file changed. Measured: an incremental commit
  costs 1.1 KB at 10 files but 19 KB at 500, where the tree dominates. Splitting
  trees per directory (git-style) would make it O(changed paths + depth); the
  format permits it, since a tree entry already carries a typed content ref.
  Numbers in `crates/bashkit/benches/results/`.
- Merge commits are representable (multiple parents) but nothing creates them,
  and there is no content merge.

## See also

- [Virtual Filesystem](vfs.md), `VfsSnapshot`, limits, restore semantics
- [Threat Model](../security/threat-model.md), TM-SNAP-001 through TM-SNAP-006
- [Testing](../operations/testing.md), test layout and binary-splitting criteria
- [Limitations](../operations/limitations.md), negative spec
