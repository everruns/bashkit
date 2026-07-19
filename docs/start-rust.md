# Get started in Rust

Embed the Bashkit sandbox as a library in your Rust application or agent
runtime. Every command is reimplemented in Rust and executes in-process against
a virtual filesystem — no `fork`, no `exec`, no host shell.

## Install

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

`http_client` enables `curl`/`wget` and the network allowlist. Embedded Python
(Monty) is a git-only dependency, so there is no `python` feature from the
crates.io release — to run Python inside the shell see the [Python
builtin](python.md) guide, and to embed Bashkit *in* a Python app see [Get
started in Python](start-python.md).

## First script

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

## Persistent state

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

## Configure the sandbox

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

See [Sandbox configuration & limits](configuration.md) for the full set of
options, including the network allowlist.

## Examples

Runnable Rust examples in the repo:

- [`basic.rs`](https://github.com/everruns/bashkit/blob/main/crates/bashkit/examples/basic.rs) — minimal execution
- [`resource_limits.rs`](https://github.com/everruns/bashkit/blob/main/crates/bashkit/examples/resource_limits.rs) — enforcing limits
- [`custom_builtins.rs`](https://github.com/everruns/bashkit/blob/main/crates/bashkit/examples/custom_builtins.rs) — adding your own commands
- [`agent_tool.rs`](https://github.com/everruns/bashkit/blob/main/crates/bashkit/examples/agent_tool.rs) — exposing Bashkit as an LLM tool

## Next steps

- [Sandbox configuration & limits](configuration.md) — resource limits, filesystem, allowlist.
- [Custom builtins](custom_builtins.md) — add your own Rust commands to the shell.
- [Snapshotting](snapshotting.md) — serialize and restore interpreter state.
- [Security](security.md) — sandbox boundaries and what scripts cannot do.
- Full API reference: [docs.rs/bashkit](https://docs.rs/bashkit/latest/bashkit/).
