# Snapshotting in Bashkit

Bashkit can serialize an interpreter into opaque bytes and restore it later.
Use snapshots for checkpoint/resume flows, warm sandbox caching, or rolling back
to a known-good virtual workspace.

## What a snapshot captures

- Shell state: variables, exported env, arrays, aliases, and current working directory
- Virtual filesystem contents
- Session counters used by interpreter limits

`restore_snapshot()` preserves the current instance configuration such as limits,
builtins, and filesystem backend, then replaces shell state and VFS contents
with the snapshot. `from_snapshot()` creates a fresh instance from bytes.

In the Rust core, `Bash::from_snapshot()` returns a default-configured
interpreter. If you need custom limits, builtins, or filesystem wiring, build
that instance first and call `restore_snapshot()` on it.

## Rust

```rust
use bashkit::{Bash, ExecutionLimits};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::new();
bash.exec("export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt")
    .await?;

let snapshot = bash.snapshot()?;

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

restored = Bash.from_snapshot(snapshot, username="agent", max_commands=100)
assert restored.execute_sync("echo $BUILD_ID").stdout.strip() == "42"
assert restored.execute_sync("cat /workspace/state.txt").stdout.strip() == "ready"

restored.reset()
restored.restore_snapshot(snapshot)
assert restored.execute_sync("pwd").stdout.strip() == "/workspace"
```

## Node.js / TypeScript

Node exposes snapshotting on `Bash`:

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({ username: "agent", maxCommands: 100 });
bash.executeSync(
  "export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt",
);

const snapshot = bash.snapshot();

const restored = Bash.fromSnapshot(snapshot, {
  username: "agent",
  maxCommands: 100,
});
if (restored.executeSync("echo $BUILD_ID").stdout.trim() !== "42") {
  throw new Error("snapshot restore failed");
}

restored.reset();
restored.restoreSnapshot(snapshot);
```

`BashTool` snapshot parity for Node is tracked in [issue #1301](https://github.com/everruns/bashkit/issues/1301).

## Security note

The default snapshot format includes integrity checks for accidental corruption,
but it does not authenticate untrusted bytes. If snapshots cross trust
boundaries such as shared storage or network transfer, use Rust's keyed APIs
(`snapshot_to_bytes_keyed`, `from_snapshot_keyed`, `restore_snapshot_keyed`) or
treat the snapshot bytes as trusted-only input.

## See also

- [Security](./security.md)
- [Embedded Python guide](../crates/bashkit/docs/python.md)
- [Embedded TypeScript guide](../crates/bashkit/docs/typescript.md)
