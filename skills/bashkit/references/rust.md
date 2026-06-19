# Rust API

Use Rust examples when the user wants to embed Bashkit in an application or agent runtime.

## Install

```bash
cargo add bashkit
```

Optional features:

```bash
cargo add bashkit --features http_client
cargo add bashkit --features git
cargo add bashkit --features ssh
cargo add bashkit --features jq
cargo add bashkit --features bot-auth
cargo add bashkit --features python
cargo add bashkit --features typescript
cargo add bashkit --features sqlite
cargo add bashkit --features realfs
cargo add bashkit --features scripted_tool
```

## Minimal Execution

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

## Persistent State

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

## Configure Sandbox

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

## Network Allowlist

HTTP access for `curl`/`wget` requires the `http_client` feature and an allowlist.

```rust
use bashkit::{Bash, NetworkAllowlist};

let mut bash = Bash::builder()
    .network(NetworkAllowlist::new().allow("https://api.github.com"))
    .build();
```

## Reference

- Rust API docs: https://docs.rs/bashkit/latest/bashkit/
- Rust examples: https://github.com/everruns/bashkit/tree/main/crates/bashkit/examples
