# Bashkit

[![CI](https://github.com/everruns/bashkit/actions/workflows/ci.yml/badge.svg)](https://github.com/everruns/bashkit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/bashkit.svg)](https://crates.io/crates/bashkit)
[![docs.rs](https://img.shields.io/docsrs/bashkit)](https://docs.rs/bashkit)
[![Repo: Agent Friendly](https://img.shields.io/badge/Repo-Agent%20Friendly-blue)](AGENTS.md)

Sandboxed bash interpreter for multi-tenant environments. Written in Rust.

## Features

- **POSIX compliant** - Substantial IEEE 1003.1-2024 Shell Command Language compliance
- **Sandboxed execution** - No real filesystem access by default
- **Virtual filesystem** - InMemoryFs, OverlayFs, MountableFs
- **Resource limits** - Command count, loop iterations, function depth
- **Network allowlist** - Control HTTP access per-domain
- **Async-first** - Built on tokio
- **Experimental: Git support** - Sandboxed git operations on the virtual filesystem (`git` feature)
- **Experimental: Python support** - Embedded Python interpreter via [Monty](https://github.com/pydantic/monty) (`python` feature)

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

## Built-in Commands (68+)

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
| Shell | `bash`, `sh` (sandboxed re-invocation), `eval`, `:`  |
| System info | `whoami`, `hostname`, `uname`, `id`, `env`, `printenv`, `history` |
| Network | `curl`, `wget` (requires allowlist) |
| Experimental | `python`, `python3` (requires `python` feature), `git` (requires `git` feature) |

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

## Experimental: Git Support

Enable the `git` feature for sandboxed git operations on the virtual filesystem.
All git data lives in the VFS — no host filesystem access.

```toml
[dependencies]
bashkit = { version = "0.1", features = ["git"] }
```

```rust
use bashkit::{Bash, GitConfig};

let mut bash = Bash::builder()
    .git(GitConfig::new()
        .author("Deploy Bot", "deploy@example.com"))
    .build();

// Local operations: init, add, commit, status, log
// Branch operations: branch, checkout, diff, reset
// Remote operations: remote add/remove, clone/push/pull/fetch (sandbox mode)
```

See [specs/010-git-support.md](specs/010-git-support.md) for the full specification.

## Experimental: Python Support

Enable the `python` feature to embed the [Monty](https://github.com/pydantic/monty) Python interpreter (pure Rust, Python 3.12).
Python code runs in-memory with configurable resource limits and VFS bridging — files created
by bash are readable from Python and vice versa.

```toml
[dependencies]
bashkit = { version = "0.1", features = ["python"] }
```

```rust
use bashkit::Bash;

let mut bash = Bash::builder().python().build();

// Inline code
bash.exec("python3 -c \"print(2 ** 10)\"").await?;

// Script files from VFS
bash.exec("python3 /tmp/script.py").await?;

// VFS bridging: pathlib.Path operations work with the virtual filesystem
bash.exec(r#"python3 -c "
from pathlib import Path
Path('/tmp/data.txt').write_text('hello from python')
""#).await?;
bash.exec("cat /tmp/data.txt").await?; // "hello from python"
```

Limitations: no `open()` (use `pathlib.Path`), no network, no classes, no third-party imports.
See [crates/bashkit/docs/python.md](crates/bashkit/docs/python.md) for the full guide.

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

Bashkit includes a benchmark tool to compare performance against bash and just-bash.

```bash
just bench              # Quick benchmark run
just bench --save       # Save results with system identifier
just bench-verbose      # Detailed output
just bench-list         # List all benchmarks
```

See [crates/bashkit-bench/README.md](crates/bashkit-bench/README.md) for methodology and assumptions.

## Python Bindings

Python bindings with LangChain integration are available in [crates/bashkit-python](crates/bashkit-python/README.md).

```python
from bashkit import BashTool

tool = BashTool()
result = await tool.execute("echo 'Hello, World!'")
print(result.stdout)
```

## Security

Bashkit is designed as a sandboxed interpreter for untrusted scripts. See the [security policy](SECURITY.md) for reporting vulnerabilities and the [threat model](specs/006-threat-model.md) for detailed analysis of 60+ identified threats.

## Acknowledgments

This project was inspired by [just-bash](https://github.com/vercel-labs/just-bash) from Vercel Labs. Huge kudos to the Vercel team for pioneering the idea of a sandboxed bash interpreter for AI-powered environments. Their work laid the conceptual foundation that made Bashkit possible.

## Ecosystem

Bashkit is part of the [Everruns](https://everruns.com) ecosystem.

## License

MIT
