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

## Built-in Commands (81)

| Category | Commands |
|----------|----------|
| Core | `echo`, `printf`, `cat`, `nl`, `read` |
| Navigation | `cd`, `pwd`, `ls`, `find` |
| Flow control | `true`, `false`, `exit`, `return`, `break`, `continue`, `test`, `[` |
| Variables | `export`, `set`, `unset`, `local`, `shift`, `source`, `.`, `eval`, `readonly`, `times` |
| Text processing | `grep`, `sed`, `awk`, `jq`, `head`, `tail`, `sort`, `uniq`, `cut`, `tr`, `wc`, `paste`, `column`, `diff`, `comm`, `strings` |
| File operations | `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `rmdir` |
| File inspection | `file`, `stat`, `less` |
| Archives | `tar`, `gzip`, `gunzip` |
| Byte tools | `od`, `xxd`, `hexdump` |
| Utilities | `sleep`, `date`, `basename`, `dirname`, `timeout`, `wait`, `watch` |
| Disk | `df`, `du` |
| Pipeline | `xargs`, `tee` |
| Shell | `bash`, `sh` (sandboxed re-invocation), `:` |
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

## LLM Eval Results

Bashkit includes an eval harness ([bashkit-eval](crates/bashkit-eval/)) that measures how well LLMs use bashkit as a bash tool in agentic workloads. 25 tasks across 10 categories test file operations, text processing, pipelines, scripting, data transformation, error recovery, and more.

### Latest Results (2026-02-08)

| Model | Score | Tasks Passed | Tool Call Success | Tokens (in/out) | Duration |
|-------|-------|-------------|-------------------|-----------------|----------|
| Claude Haiku 4.5 | **98%** | 23/25 | 87% (81/93) | 167K/19K | 2.9 min |
| Claude Opus 4.6 | 93% | 21/25 | 87% (125/143) | 242K/26K | 8.7 min |
| GPT-5.2 | 81% | 18/25 | 78% (80/103) | 84K/10K | 3.4 min |

### Category Breakdown

| Category | Opus 4.6 | Haiku 4.5 | GPT-5.2 |
|----------|----------|-----------|---------|
| archive_operations | 100% | 100% | 50% |
| complex_tasks | 69% | 100% | 88% |
| data_transformation | 94% | 100% | 62% |
| error_recovery | 100% | 100% | 86% |
| file_operations | 100% | 94% | 100% |
| jq_mastery | 100% | 100% | 100% |
| pipelines | 100% | 100% | 80% |
| scripting | 93% | 93% | 53% |
| system_info | 100% | 100% | 100% |
| text_processing | 100% | 100% | 100% |

### Impact of Interpreter Fixes

Tool call success (how often bashkit executes what models generate) improved significantly after recent fixes:

| Model | Before | After | Delta |
|-------|--------|-------|-------|
| Claude Opus 4.6 | 79% | 87% | **+8%** |
| Claude Haiku 4.5 | 77% | 87% | **+10%** |
| GPT-5.2 | 59% | 78% | **+19%** |

Key fixes: `date -d` compound expressions/quote stripping (eliminated 10k command limit exhaustion), awk field math.

### Remaining Bashkit Gaps

Failures that occur across all models (interpreter limitations, not model quality):

| Gap | Impact | Example |
|-----|--------|---------|
| Compound commands in pipelines | ~6 errors | `cmd \| while read line; do ... done` |
| Awk associative arrays | ~9 errors | `arr[$key]=$val` |
| Heredoc-to-file redirect | ~10 errors | `cat > file <<'EOF'` writes to stdout instead |
| `source`/`.` function loading | ~5 errors | Functions from sourced files not in caller scope |
| `chmod` symbolic modes | ~6 errors | `chmod +x file` → "invalid mode" |
| Parser fuel / `[[ ]]` | ~25 errors | Complex conditionals exhaust parser budget |

### Model Behavior

- **Claude models** adapt when bashkit rejects a command — retry with simpler constructs (e.g., `[[ ]]` → `[ ]`, pipelines → temp files)
- **GPT-5.2** tends to repeat failing patterns, leading to lower tool success despite fewer total calls
- **Haiku 4.5** best score/cost ratio — fewer tokens, faster, highest pass rate

Full results with per-task traces in [eval-results/](eval-results/). See [bashkit-eval](crates/bashkit-eval/) for usage and options.

```bash
just eval                    # Run eval with default model
just eval-save               # Run and save results
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
