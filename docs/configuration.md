# Sandbox configuration & limits

Every Bashkit binding runs scripts inside the same sandbox: an in-memory virtual
filesystem, no `fork`/`exec`, no host access, and hard resource ceilings. This
page covers the knobs that shape that sandbox — resource limits, the filesystem,
identity, and the network allowlist. The Rust builder is the reference API; the
Python and JavaScript bindings expose the same options through constructor
arguments (see the notes at the end).

## Resource limits

Limits are enforced while the script runs — a script that exceeds one is
terminated, not allowed to exhaust the host. Set them with `ExecutionLimits`:

```rust
use bashkit::{Bash, ExecutionLimits};

let limits = ExecutionLimits::new()
    .max_commands(1000)
    .max_loop_iterations(10000)
    .max_function_depth(100);

let mut bash = Bash::builder().limits(limits).build();
```

## The filesystem

Scripts see a virtual filesystem, never the host disk. Pick a backend and pass
it to the builder:

```rust
use bashkit::{Bash, InMemoryFs};
use std::sync::Arc;

let mut bash = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))
    .build();
```

See the [Virtual filesystem](filesystem.md) guide for the layering stack
(`OverlayFs`, `MountableFs`) and the opt-in `realfs` host-mount backend.

## Identity & working directory

```rust
use bashkit::Bash;

let mut bash = Bash::builder()
    .env("HOME", "/home/agent")
    .cwd("/home/agent")
    .username("agent")
    .hostname("sandbox")
    .build();
```

## Network allowlist

HTTP for `curl`/`wget` requires the `http_client` feature and an explicit
allowlist — outbound requests are denied by default:

```rust
use bashkit::{Bash, NetworkAllowlist};

let mut bash = Bash::builder()
    .network(NetworkAllowlist::new().allow("https://api.github.com"))
    .build();
```

See [Networking](networking.md) for per-domain control, and
[Security](security.md) for the full list of sandbox boundaries.

## Other bindings

The Python and JavaScript bindings take the same options as constructor
arguments rather than a builder. For example, in JavaScript:

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({
  cwd: "/home/agent",
  env: { HOME: "/home/agent" },
  maxCommands: 1000,
  maxLoopIterations: 10000,
  maxMemory: 64 * 1024 * 1024,
});
```

The [Python](start-python.md) and [Node](start-node.md) quickstarts show the
per-language constructor options.

## See also

- [Get started](start.md) — pick your target and run a first script.
- [Virtual filesystem](filesystem.md) — the VFS backends and layering.
- [Networking](networking.md) — the HTTP allowlist in depth.
- [Security](security.md) — sandbox boundaries and threat model.
