# Embedding Bashkit

Run Bashkit as a library inside your own application or agent runtime. Every
command is reimplemented in Rust and executes in-process against a virtual
filesystem — no `fork`, no `exec`, no host shell. This is the starting point
for building a shell, a sandboxed code-runner, or an agent tool on top of
Bashkit.

Bashkit ships three bindings with the same core semantics:

- **Rust** — the core crate (`cargo add bashkit`)
- **Python** — a PyO3 wheel (`pip install bashkit`)
- **TypeScript / JavaScript** — a NAPI runtime for Node, Bun, and Deno
  (`npm i @everruns/bashkit`)

## Rust

```bash
cargo add bashkit
```

Opt-in features pull in heavier capabilities only when you need them:

```bash
cargo add bashkit --features http_client
cargo add bashkit --features git
cargo add bashkit --features typescript
cargo add bashkit --features sqlite
cargo add bashkit --features realfs
cargo add bashkit --features scripted_tool
```

`http_client` enables `curl`/`wget` and the network allowlist shown below.
Embedded Python (Monty) is a git-only dependency, so there is no `python`
feature from the crates.io release — to run Python inside the shell see the
[Python builtin](python.md) guide. The `pip install bashkit` wheel below is a
separate, standalone binding.

### Minimal execution

```rust
use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::new();
    let result = bash.exec("echo hello world").await?;
    print!("{}", result.stdout);
    Ok(())
}
```

### Persistent state

A `Bash` instance keeps its environment and virtual filesystem across calls, so
each `exec` sees what the previous one did:

```rust
use bashkit::Bash;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let mut bash = Bash::new();
bash.exec("export APP_ENV=dev").await?;
bash.exec("printf 'data\n' > /tmp/file.txt").await?;

let env = bash.exec("echo $APP_ENV").await?;
let file = bash.exec("cat /tmp/file.txt").await?;
assert_eq!(env.stdout, "dev\n");
assert_eq!(file.stdout, "data\n");
# Ok(())
# }
```

### Configure the sandbox

Use the builder to set resource limits, the filesystem, identity, and the
working directory:

```rust
use bashkit::{Bash, ExecutionLimits, InMemoryFs};
use std::sync::Arc;

let limits = ExecutionLimits::new()
    .max_commands(1000)
    .max_loop_iterations(10000)
    .max_function_depth(100);

let mut bash = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))
    .env("HOME", "/home/agent")
    .cwd("/home/agent")
    .username("agent")
    .hostname("sandbox")
    .limits(limits)
    .build();
```

### Network allowlist

HTTP for `curl`/`wget` requires the `http_client` feature and an explicit
allowlist — outbound requests are denied by default:

```rust
use bashkit::{Bash, NetworkAllowlist};

let mut bash = Bash::builder()
    .network(NetworkAllowlist::new().allow("https://api.github.com"))
    .build();
```

## Python

```bash
pip install bashkit
```

```python
from bashkit import Bash

bash = Bash()
result = bash.execute_sync("echo 'Hello, World!'")
print(result.stdout)

bash.execute_sync("export APP_ENV=dev")
print(bash.execute_sync("echo $APP_ENV").stdout)
```

## TypeScript / JavaScript

```bash
npm i @everruns/bashkit
```

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();
const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout);

bash.executeSync("X=42");
console.log(bash.executeSync("echo $X").stdout);
```

## Next steps

- [Custom builtins](custom_builtins.md) — add your own Rust commands to the shell.
- [Snapshotting](snapshotting.md) — serialize and restore interpreter state for
  checkpoint/resume flows.
- [Security](security.md) — sandbox boundaries and what scripts cannot do.
- Full API reference: [docs.rs/bashkit](https://docs.rs/bashkit/latest/bashkit/).
