# BashKit

[![CI](https://github.com/everruns/bashkit/actions/workflows/ci.yml/badge.svg)](https://github.com/everruns/bashkit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Sandboxed bash interpreter for multi-tenant environments. Written in Rust.

## Features

- **POSIX compliant** - Substantial IEEE 1003.1-2024 Shell Command Language compliance
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

## Built-in Commands (60+)

| Category | Commands |
|----------|----------|
| Core | `echo`, `printf`, `cat`, `read` |
| Navigation | `cd`, `pwd`, `ls`, `find` |
| Flow control | `true`, `false`, `exit`, `return`, `break`, `continue`, `test`, `[` |
| Variables | `export`, `set`, `unset`, `local`, `shift`, `source` |
| Text processing | `grep`, `sed`, `awk`, `jq`, `head`, `tail`, `sort`, `uniq`, `cut`, `tr`, `wc` |
| File operations | `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `rmdir` |
| File inspection | `file`, `stat`, `less` |
| Archives | `tar`, `gzip`, `gunzip` |
| Utilities | `sleep`, `date`, `basename`, `dirname`, `timeout`, `wait` |
| Pipeline | `xargs`, `tee` |
| System info | `whoami`, `hostname`, `uname`, `id`, `env`, `printenv` |
| Network | `curl`, `wget` (stub, requires allowlist) |

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

### Sandbox Identity

Configure the sandbox username and hostname for `whoami`, `hostname`, `id`, and `uname`:

```rust
let mut bash = Bash::builder()
    .username("deploy")      // Sets whoami, id, and $USER env var
    .hostname("my-server")   // Sets hostname, uname -n
    .build();

// whoami → "deploy"
// hostname → "my-server"
// id → "uid=1000(deploy) gid=1000(deploy)..."
// echo $USER → "deploy"
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

## Benchmarks

BashKit includes a benchmark tool to compare performance against bash and just-bash.

```bash
just bench              # Quick benchmark run
just bench --save       # Save results with system identifier
just bench-verbose      # Detailed output
just bench-list         # List all benchmarks
```

Key findings:
- **~2000x faster startup** - No subprocess overhead (0.004ms vs 9ms)
- **~200-1000x faster for tools** - grep/sed/awk run in-process
- **~550x faster recursive functions** - Fibonacci(10): 1ms vs 586ms

See [crates/bashkit-bench/README.md](crates/bashkit-bench/README.md) for methodology and assumptions.

## Acknowledgments

This project was inspired by [just-bash](https://github.com/vercel-labs/just-bash) from Vercel Labs. Huge kudos to the Vercel team for pioneering the idea of a sandboxed bash interpreter for AI-powered environments. Their work laid the conceptual foundation that made BashKit possible.

## License

MIT
