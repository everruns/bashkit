# BashKit

Sandboxed bash interpreter for multi-tenant environments. Written in Rust.

## Features

- **Sandboxed execution** - No real filesystem access by default
- **Virtual filesystem** - InMemoryFs, OverlayFs, MountableFs
- **Resource limits** - Command count, loop iterations, function depth
- **Network allowlist** - Control HTTP access per-domain
- **Async-first** - Built on tokio

## Quick Start

```rust
use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::new();
    let result = bash.exec("echo hello world").await?;
    println!("{}", result.stdout); // "hello world\n"
    Ok(())
}
```

## Built-in Commands

| Category | Commands |
|----------|----------|
| Core | `echo`, `printf`, `cat`, `read` |
| Navigation | `cd`, `pwd` |
| Flow control | `true`, `false`, `exit`, `test`, `[` |
| Variables | `export`, `set`, `unset`, `local`, `source` |
| Text processing | `grep`, `sed`, `awk`, `jq` |

## Shell Features

- Variables and parameter expansion (`$VAR`, `${VAR:-default}`, `${#VAR}`)
- Command substitution (`$(cmd)`)
- Arithmetic expansion (`$((1 + 2))`)
- Pipelines and redirections (`|`, `>`, `>>`, `<`, `<<<`)
- Control flow (`if`/`elif`/`else`, `for`, `while`, `case`)
- Functions (POSIX and bash-style)
- Arrays (`arr=(a b c)`, `${arr[@]}`, `${#arr[@]}`)
- Glob expansion (`*`, `?`)
- Here documents (`<<EOF`)

## Configuration

```rust
use bashkit::{Bash, ExecutionLimits, InMemoryFs};
use std::sync::Arc;

let limits = ExecutionLimits::new()
    .max_commands(1000)
    .max_loop_iterations(10000)
    .max_function_depth(100);

let mut bash = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))
    .env("HOME", "/home/user")
    .cwd("/home/user")
    .limits(limits)
    .build();
```

## Virtual Filesystem

```rust
use bashkit::{InMemoryFs, OverlayFs, MountableFs, FileSystem};
use std::sync::Arc;

// Layer filesystems
let base = Arc::new(InMemoryFs::new());
let overlay = Arc::new(OverlayFs::new(base));

// Mount points
let mut mountable = MountableFs::new(Arc::new(InMemoryFs::new()));
mountable.mount("/data", Arc::new(InMemoryFs::new()));
```

## CLI Usage

```bash
# Run a script
bashkit-cli run script.sh

# Interactive REPL
bashkit-cli repl
```

## Development

```bash
just build        # Build project
just test         # Run tests
just check        # fmt + clippy + test
just pre-pr       # Pre-PR checks
```

## Acknowledgments

This project was inspired by [just-bash](https://github.com/vercel-labs/just-bash) from Vercel Labs. Huge kudos to the Vercel team for pioneering the idea of a sandboxed bash interpreter for AI-powered environments. Their work laid the conceptual foundation that made BashKit possible.

## License

MIT
