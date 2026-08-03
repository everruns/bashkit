# Snapshotting in Bashkit

Bashkit can serialize an interpreter into opaque bytes and restore it later.
Use snapshots for checkpoint/resume flows, warm sandbox caching, or rolling back
to a known-good virtual workspace.

## What a snapshot captures

- Shell state: variables, exported env, arrays, aliases, and current working directory
- Virtual filesystem contents by default
- Session counters used by interpreter limits

Pass snapshot options to skip VFS capture when you only want shell state.

`restore_snapshot()` preserves the current instance configuration such as limits,
builtins, and filesystem backend, then replaces shell state and VFS contents
with the snapshot. `from_snapshot()` creates a fresh instance from bytes.

In the Rust core, `Bash::from_snapshot()` returns a default-configured
interpreter. If you need custom limits, builtins, or filesystem wiring, build
that instance first and call `restore_snapshot()` on it.

## Rust

```rust
use bashkit::{Bash, ExecutionLimits, SnapshotOptions};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::new();
bash.exec("export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt")
    .await?;

let snapshot = bash.snapshot()?;
let shell_only = bash.snapshot_with_options(SnapshotOptions {
    exclude_filesystem: true,
    exclude_functions: false,
})?;

let mut restored = Bash::from_snapshot(&snapshot)?;
assert_eq!(restored.exec("echo $BUILD_ID").await?.stdout.trim(), "42");
assert_eq!(
    restored.exec("cat /workspace/state.txt").await?.stdout.trim(),
    "ready"
);

// Reuse an explicitly configured instance and preserve its limits.
let limits = ExecutionLimits::new().max_commands(100);
let mut configured = Bash::builder().limits(limits).build();
configured.restore_snapshot(&snapshot)?;
configured.restore_snapshot(&shell_only)?;
# Ok(())
# }
```

## Python

Python exposes snapshotting on both `Bash` and `BashTool`:

```python
from bashkit import Bash

bash = Bash(username="agent", max_commands=100)
bash.execute_sync(
    "export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt"
)

snapshot = bash.snapshot()
shell_only = bash.snapshot(exclude_filesystem=True)
prompt_only = bash.snapshot(exclude_filesystem=True, exclude_functions=True)

restored = Bash.from_snapshot(snapshot, username="agent", max_commands=100)
assert restored.execute_sync("echo $BUILD_ID").stdout.strip() == "42"
assert restored.execute_sync("cat /workspace/state.txt").stdout.strip() == "ready"

restored.reset()
restored.restore_snapshot(snapshot)
assert restored.execute_sync("pwd").stdout.strip() == "/workspace"
restored.restore_snapshot(shell_only)
```

## Node.js / TypeScript

Node exposes snapshotting on `Bash` and `BashTool`:

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({ username: "agent", maxCommands: 100 });
bash.executeSync(
  "export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt",
);

const snapshot = bash.snapshot();
const shellOnly = bash.snapshot({ excludeFilesystem: true });
const promptOnly = bash.snapshot({
  excludeFilesystem: true,
  excludeFunctions: true,
});

const restored = Bash.fromSnapshot(snapshot, {
  username: "agent",
  maxCommands: 100,
});
if (restored.executeSync("echo $BUILD_ID").stdout.trim() !== "42") {
  throw new Error("snapshot restore failed");
}

restored.reset();
restored.restoreSnapshot(snapshot);
restored.restoreSnapshot(shellOnly);
```

## Session history: commits, forks, and rewinds

A packed snapshot is one self-contained blob, which is the right shape for
checkpoint/resume but the wrong shape for keeping a snapshot per conversation
turn: each blob re-encodes the whole workspace, so unchanged files are stored
again and again, and a branch cannot share anything with its parent.

For that, use commits. A commit captures the same state as a content-addressed
object graph — file chunks, a tree, shell state — and hands you the objects to
persist plus a `CommitId` to remember. Unchanged content is a 32-byte hash
reference, not a copy.

```rust
use bashkit::{Bash, CheckoutPolicy, CommitOptions, ObjectId, SnapshotGraph};
use std::collections::HashMap;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
// Any key-value store works. Bashkit never touches it directly.
let mut store: HashMap<ObjectId, Vec<u8>> = HashMap::new();
let mut bash = Bash::new();

bash.exec("echo one > /log.txt").await?;
let first = bash.commit(CommitOptions::new())?;
let first_id = first.id();
store.extend(first.into_objects());

bash.exec("echo two >> /log.txt").await?;
// `have` makes the commit incremental: objects the store already holds are
// not emitted again.
let second = bash.commit(
    CommitOptions::new()
        .parent(first_id)
        .have(store.keys())
        .meta("message_id", "msg-2"),
)?;
let second_id = second.id();
store.extend(second.into_objects());

// Rewind, or fork: check out any commit, tip or not. No copy, no replay.
let mut branch = Bash::new();
branch.checkout(first_id, &store, CheckoutPolicy::default())?;
assert_eq!(branch.exec("cat /log.txt").await?.stdout, "one\n");

// What changed during that turn?
let diff = SnapshotGraph::diff(first_id, second_id, &store)?;
assert_eq!(diff.files_modified, vec!["/log.txt".to_string()]);

// Ancestry walks the parent pointers your store holds.
let history = SnapshotGraph::ancestry(second_id, &store, 100)?;
assert_eq!(history, vec![second_id, first_id]);
# Ok(())
# }
```

A fork is simply a commit whose parent is not the branch tip — there is no
separate fork operation. Truncating a session is pointing at an older
`CommitId`. Objects no commit reaches are yours to collect;
`SnapshotGraph::reachable` lists what a commit needs.

### Python

```python
import sqlite3
from bashkit import Bash, SnapshotGraph

db = sqlite3.connect("sessions.db")
db.execute("CREATE TABLE IF NOT EXISTS objects (id TEXT PRIMARY KEY, blob BLOB)")

def store_objects(objects: dict[str, bytes]) -> None:
    # Objects are immutable and keyed by their own hash, so INSERT OR IGNORE is
    # the whole write path — concurrent sessions converge instead of conflicting.
    db.executemany("INSERT OR IGNORE INTO objects VALUES (?, ?)", objects.items())
    db.commit()

def known_ids() -> list[str]:
    return [row[0] for row in db.execute("SELECT id FROM objects")]

def load_all() -> dict[str, bytes]:
    return {row[0]: row[1] for row in db.execute("SELECT id, blob FROM objects")}

bash = Bash()
bash.execute_sync("echo one > /log.txt")
first = bash.commit(meta={"message_id": "msg-1"})
store_objects(first.objects)

bash.execute_sync("echo two >> /log.txt")
second = bash.commit(parents=[first.id], have=known_ids(), meta={"message_id": "msg-2"})
store_objects(second.objects)

# Rewind, or fork: check out any commit, tip or not.
branch = Bash()
branch.checkout(first.id, load_all())
assert branch.execute_sync("cat /log.txt").stdout == "one\n"

diff = SnapshotGraph.diff(first.id, second.id, load_all())
assert diff.files_modified == ["/log.txt"]
```

Object ids are hex strings and stores are plain `dict[str, bytes]`, so both drop
straight into a database column with no marshalling. A full runnable example —
including lazy fetching and garbage collection — is in
`crates/bashkit-python/examples/session_history.py`.

### Node.js / TypeScript

```typescript
import { Bash, snapshotDiff, snapshotAncestry } from "@everruns/bashkit";

const store: Record<string, Buffer> = {};

const bash = new Bash();
bash.executeSync("echo one > /log.txt");
const first = bash.commit({ meta: { messageId: "msg-1" } });
Object.assign(store, first.objects);

bash.executeSync("echo two >> /log.txt");
const second = bash.commit({
  parents: [first.id],
  have: Object.keys(store),
  meta: { messageId: "msg-2" },
});
Object.assign(store, second.objects);

const branch = new Bash();
branch.checkout(first.id, store);

const diff = snapshotDiff(first.id, second.id, store);
const history = snapshotAncestry(second.id, store);
```

`commit()` returns `packed` — self-contained bytes equivalent to `snapshot()` —
or `null` when `have` made the commit incremental, since packing one would
produce bytes that cannot be restored.

### Fetching objects on demand

`checkout` takes the objects it needs rather than calling back into your
storage, which keeps database access on your side of the boundary. When you do
not want to load a whole session's objects up front, ask what is missing,
fetch that, and repeat until the plan comes back empty:

```rust,ignore
loop {
    let need = SnapshotGraph::plan_checkout(commit_id, &local)?;
    if need.is_empty() {
        break;
    }
    for id in need {
        local.insert(id, my_database.load(id).await?);
    }
}
bash.checkout(commit_id, &local, CheckoutPolicy::default())?;
```

Chunk IDs live inside file manifests, so a cold start takes a few rounds; the
graph is four levels deep, so it always converges quickly.

## Capability checks on restore

Every commit records the environment that produced it: builtin names, the
compile-time features that change interpreter semantics, and the filesystem
backend. On restore that fingerprint is compared against the live instance.

| Policy | Rule |
|---|---|
| `Superset` (default) | The live instance must have everything the snapshot's had. Extra builtins are fine. |
| `Strict` | Environments must match exactly. Expect this to reject snapshots from any other bashkit version. |
| `Force` | Restore regardless. |

The default is `Superset` because the dangerous direction is asymmetric:
restoring into an environment missing a tool the session used can produce a
broken session, while extra tools cannot. It also means snapshots keep
restoring after a bashkit upgrade adds builtins — under `Strict` they would
not.

The fingerprint proves the environments match; it cannot prove a restored
session will behave. Where state provably requires a feature, the state itself
is checked: a workspace holding SQLite databases is refused on a build without
the `sqlite` feature under every policy.

`SnapshotGraph::capabilities(id, &store)` reads the fingerprint without
restoring, and `CapabilityFingerprint::capture(&bash)` fingerprints a live
instance, so you can test compatibility before committing to a restore.

## Format versions and upgrades

Snapshot bytes carry three version numbers: the container framing, the metadata
schema, and a `min_reader` floor. A build reads anything whose `min_reader` it
satisfies, however much newer the other two are, and unknown metadata fields
are ignored rather than rejected. A snapshot that genuinely needs a newer
bashkit fails with `Error::SnapshotTooNew` naming both versions, so you can
prompt an upgrade instead of discarding stored state.

Snapshots written by older bashkit versions, including the original v1 JSON
format, keep restoring. A checked-in corpus of snapshots from every released
format version is replayed on every CI run to keep that true.

## Security note

The default snapshot format includes integrity checks for accidental corruption,
but it does not authenticate untrusted bytes. If snapshots cross trust
boundaries such as shared storage or network transfer, use Rust's keyed APIs
(`snapshot_to_bytes_keyed`, `from_snapshot_keyed`, `restore_snapshot_keyed`) or
treat the snapshot bytes as trusted-only input.

Within a commit graph, every object is named by the hash of its content and
re-verified when loaded, so authenticating the root commit authenticates
everything beneath it. Objects loaded from a store you do not control are still
checked — a substituted or truncated object fails before any state is applied.

## See also

- [Security](./security.md)
- [Embedded Python guide](../crates/bashkit/docs/python.md)
- [Embedded TypeScript guide](../crates/bashkit/docs/typescript.md)
