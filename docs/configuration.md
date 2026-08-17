# Sandbox configuration & limits

Every Bashkit binding runs scripts inside the same sandbox: an in-memory virtual
filesystem, no `fork`/`exec`, no host access, and hard resource ceilings. This
page covers the knobs that shape that sandbox, resource limits, the filesystem,
identity, and the network allowlist. The Rust builder is the reference API; the
Python and JavaScript bindings expose the same options through constructor
arguments (see the notes at the end).

## Resource limits

Use a named profile when you want one coherent baseline across execution,
session memory, the managed VFS, network, and compiled embedded runtimes:

```rust
use bashkit::{Bash, ExecutionLimits, ExecutionProfile, ExecutionProfileName};

let profile = ExecutionProfile::builder(ExecutionProfileName::Hardened)
    .execution_limits(
        ExecutionLimits::new()
            .max_commands(5_000) // explicit per-field override
            .max_stdout_bytes(512 * 1024),
    )
    .build()?;

let mut bash = Bash::builder().profile(profile).build();
# Ok::<(), bashkit::ExecutionProfileError>(())
```

The closed names are:

- `Standard`, current secure library defaults; the default profile.
- `Hardened`, tighter limits across every resource family. The isolated VFS
  stays writable under tighter quotas.
- `Interactive`, current REPL intent: relaxed execution/session counters,
  with secure memory, VFS, network, and runtime defaults unchanged.

Profiles never enable network access. Apply a network allowlist explicitly.
Call `profile(...)` before fine-grained builder methods; later calls are
intentional overrides. A custom `FileSystem` owns its own quota contract and
replaces the profile's managed-VFS limits.

Limits are enforced while the script runs, a script that exceeds one is
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
allowlist, outbound requests are denied by default:

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
import { Bash, ExecutionProfile } from "@everruns/bashkit";

const bash = new Bash({
  profile: ExecutionProfile.Hardened,
  cwd: "/home/agent",
  env: { HOME: "/home/agent" },
  maxCommands: 1000,
  maxLoopIterations: 10000,
  maxMemory: 64 * 1024 * 1024,
});
```

Python exposes the same selector as an enum:

```python
from bashkit import Bash, ExecutionProfile

bash = Bash(profile=ExecutionProfile.Hardened, max_commands=5_000)
```

The native Node binding and browser-WASM package expose a closed
`ExecutionProfileName` union. C ABI v1 accepts `"profile": "hardened" |
"standard" | "interactive"` in its versioned JSON object and rejects unknown
values. Browser WASM has no network or embedded-runtime surface; C ABI v1 has
no network/runtime callbacks; profiles only cover capabilities each documented
binding supports. Scripted tools always remain logic-only even when a profile
is selected.

The [Python](start-python.md) and [Node](start-node.md) quickstarts show the
per-language constructor options.

## See also

- [Get started](start.md), pick your target and run a first script.
- [Virtual filesystem](filesystem.md), the VFS backends and layering.
- [Networking](networking.md), the HTTP allowlist in depth.
- [Security](security.md), sandbox boundaries and threat model.
