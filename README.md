# Bashkit

[![CI](https://github.com/everruns/bashkit/actions/workflows/ci.yml/badge.svg)](https://github.com/everruns/bashkit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/bashkit.svg)](https://crates.io/crates/bashkit)
[![docs.rs](https://img.shields.io/docsrs/bashkit)](https://docs.rs/bashkit)
[![Repo: Agent Friendly](https://img.shields.io/badge/Repo-Agent%20Friendly-blue)](AGENTS.md)

Virtual sandbox. Written in Rust.

## Features

- **Secure by default** - No process spawning, no filesystem access, no network access unless explicitly enabled. [60+ threats](specs/006-threat-model.md) analyzed and mitigated
- **POSIX compliant** - Substantial IEEE 1003.1-2024 Shell Command Language compliance
- **Sandboxed, in-process execution** - All 150 commands reimplemented in Rust, no `fork`/`exec`
- **Virtual filesystem** - InMemoryFs, OverlayFs, MountableFs with optional RealFs backend (`realfs` feature)
- **Resource limits** - Command count, loop iterations, function depth, output size, filesystem size, parser fuel
- **Network allowlist** - HTTP access denied by default, per-domain control
- **Multi-tenant isolation** - Each interpreter instance is fully independent
- **Custom builtins** - Extend with domain-specific commands
- **LLM tool contract** - `BashTool` with discovery metadata, streaming output, and system prompts
- **Scripted tool orchestration** - Compose ToolDef+callback pairs into multi-tool bash scripts (`scripted_tool` feature)
- **MCP server** - Model Context Protocol endpoint via `bashkit mcp`
- **Async-first** - Built on tokio
- **Language bindings** - Python (PyO3) and JavaScript/TypeScript (NAPI-RS) for Node.js, Bun, and Deno
- **Experimental: Git support** - Virtual git operations on the virtual filesystem (`git` feature)
- **Experimental: Python support** - Embedded Python interpreter via [Monty](https://github.com/pydantic/monty) (`python` feature)
- **Experimental: TypeScript support** - Embedded TypeScript interpreter via [ZapCode](https://github.com/TheUncharted/zapcode) (`typescript` feature)

## Install

```bash
cargo add bashkit
```

Or add to `Cargo.toml`:

```toml
[dependencies]
bashkit = "0.1"
```

Optional features:

```bash
cargo add bashkit --features git              # Virtual git operations
cargo add bashkit --features python           # Embedded Python interpreter
cargo add bashkit --features typescript       # Embedded TypeScript interpreter
cargo add bashkit --features realfs           # Real filesystem backend
cargo add bashkit --features scripted_tool    # Tool orchestration framework
```

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

## LLM Tool Contract

`BashTool` follows the toolkit-library contract: builder for reusable config,
immutable tool metadata for discovery, and single-use executions for each call.

```rust
use bashkit::{BashTool, Tool};
use futures::StreamExt;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let tool = BashTool::builder()
    .username("agent")
    .hostname("sandbox")
    .build();

println!("{}", tool.description());
println!("{}", tool.system_prompt());

let execution = tool.execution(serde_json::json!({
    "commands": "printf 'hello\nworld\n'"
}))?;
let mut stream = execution.output_stream().expect("stream available");

let handle = tokio::spawn(async move { execution.execute().await });
while let Some(chunk) = stream.next().await {
    println!("{}: {}", chunk.kind, chunk.data);
}

let output = handle.await??;
assert_eq!(output.result["stdout"], "hello\nworld\n");
# Ok(())
# }
```

## Overview

<div align="center">
  <a href="https://www.youtube.com/watch?v=0rIGX7mSlMg">
    <img src="assets/overview-thumb.jpg" alt="Watch the overview video" width="600">
    <br>
    <strong>▶ Watch the 10-minute overview</strong>
  </a>
</div>

## Built-in Commands (150)

| Category | Commands |
|----------|----------|
| Core | `echo`, `printf`, `cat`, `nl`, `read`, `mapfile`, `readarray` |
| Navigation | `cd`, `pwd`, `ls`, `tree`, `find`, `pushd`, `popd`, `dirs` |
| Flow control | `true`, `false`, `exit`, `return`, `break`, `continue`, `test`, `[` |
| Variables | `export`, `set`, `unset`, `local`, `shift`, `source`, `.`, `eval`, `readonly`, `times`, `declare`, `typeset`, `let`, `alias`, `unalias` |
| Shell | `bash`, `sh` (virtual re-invocation), `:`, `trap`, `caller`, `getopts`, `shopt`, `command`, `type`, `which`, `hash`, `compgen`, `fc`, `help` |
| Text processing | `grep`, `rg`, `sed`, `awk`, `jq`, `head`, `tail`, `sort`, `uniq`, `cut`, `tr`, `wc`, `paste`, `column`, `diff`, `comm`, `strings`, `tac`, `rev`, `seq`, `expr`, `fold`, `expand`, `unexpand`, `join`, `iconv` |
| File operations | `mkdir`, `mktemp`, `mkfifo`, `rm`, `cp`, `mv`, `touch`, `chmod`, `chown`, `ln`, `rmdir`, `realpath`, `readlink`, `split` |
| File inspection | `file`, `stat`, `less` |
| Archives | `tar`, `gzip`, `gunzip`, `zip`, `unzip` |
| Byte tools | `od`, `xxd`, `hexdump`, `base64` |
| Checksums | `md5sum`, `sha1sum`, `sha256sum` |
| Utilities | `sleep`, `date`, `basename`, `dirname`, `timeout`, `wait`, `watch`, `yes`, `kill`, `bc`, `clear` |
| Disk | `df`, `du` |
| Pipeline | `xargs`, `tee` |
| System info | `whoami`, `hostname`, `uname`, `id`, `env`, `printenv`, `history` |
| Data formats | `csv`, `json`, `yaml`, `tomlq`, `template`, `envsubst` |
| Network | `curl`, `wget` (requires allowlist), `http` |
| DevOps | `assert`, `dotenv`, `glob`, `log`, `retry`, `semver`, `verify`, `parallel`, `patch` |
| Experimental | `python`, `python3` (requires `python` feature), `ts`, `typescript`, `node`, `deno`, `bun` (requires `typescript` feature), `git` (requires `git` feature) |

## Shell Features

- Variables and parameter expansion (`$VAR`, `${VAR:-default}`, `${#VAR}`, `${var@Q}`, case conversion `${var^^}`)
- Command substitution (`$(cmd)`, `` `cmd` ``)
- Arithmetic expansion (`$((1 + 2))`, `declare -i`, `let`)
- Pipelines and redirections (`|`, `>`, `>>`, `<`, `<<<`, `2>&1`, `&>`)
- Control flow (`if`/`elif`/`else`, `for`, `while`, `until`, `case` with `;;`/`;&`/`;;&`, `select`)
- Functions (POSIX and bash-style) with dynamic scoping, FUNCNAME stack, `caller`
- Indexed arrays (`arr=(a b c)`, `${arr[@]}`, `${#arr[@]}`, slicing, `+=`)
- Associative arrays (`declare -A map=([key]=val)`)
- Nameref variables (`declare -n`)
- Brace expansion (`{a,b,c}`, `{1..10}`, `{01..05}`)
- Glob expansion (`*`, `?`) and extended globs (`@()`, `?()`, `*()`, `+()`, `!()`)
- Glob options (`dotglob`, `nullglob`, `failglob`, `nocaseglob`, `globstar`)
- Here documents (`<<EOF`, `<<-EOF` with tab stripping, `<<<` here-strings)
- Process substitution (`<(cmd)`, `>(cmd)`)
- Coprocesses (`coproc`)
- Background execution (`&`) with `wait`
- Shell options (`set -euxo pipefail`, `shopt`)
- Alias expansion
- Trap handling (`trap cmd EXIT`, `trap cmd ERR`)
- `[[ ]]` conditionals with regex matching (`=~`, BASH_REMATCH)

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

### Virtual Identity

Configure the virtual username and hostname for `whoami`, `hostname`, `id`, and `uname`:

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

Enable the `git` feature for virtual git operations on the virtual filesystem.
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
// Remote operations: remote add/remove, clone/push/pull/fetch (virtual mode)
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

Stdlib modules: `math`, `re`, `pathlib`, `os` (getenv/environ), `sys`, `typing`.
Limitations: no `open()` (use `pathlib.Path`), no network, no classes, no third-party imports.
See [crates/bashkit/docs/python.md](crates/bashkit/docs/python.md) for the full guide.

## Experimental: TypeScript Support

Enable the `typescript` feature to embed the [ZapCode](https://github.com/TheUncharted/zapcode) TypeScript interpreter (pure Rust, no V8).
TypeScript code runs in-memory with configurable resource limits and VFS bridging via external function suspend/resume.

```toml
[dependencies]
bashkit = { version = "0.1", features = ["typescript"] }
```

```rust
use bashkit::Bash;

let mut bash = Bash::builder().typescript().build();

// Inline code (ts, node, deno, bun aliases all work)
bash.exec("ts -c \"console.log(2 ** 10)\"").await?;
bash.exec("node -e \"console.log('hello')\"").await?;

// Script files from VFS
bash.exec("ts /tmp/script.ts").await?;

// VFS bridging: readFile/writeFile async functions
bash.exec(r#"ts -c "await writeFile('/tmp/data.txt', 'hello from ts')"#).await?;
bash.exec("cat /tmp/data.txt").await?; // "hello from ts"
```

Compat aliases (`node`, `deno`, `bun`) and unsupported-mode hints are configurable:

```rust
use bashkit::{Bash, TypeScriptConfig};

// Only ts/typescript, no compat aliases
let bash = Bash::builder()
    .typescript_with_config(TypeScriptConfig::default().compat_aliases(false))
    .build();
```

Limitations: no `import`/`require`, no `eval()`, no network, no `process`/`Deno`/`Bun` globals.
See [crates/bashkit/docs/typescript.md](crates/bashkit/docs/typescript.md) for the full guide.

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
bashkit run script.sh

# Interactive REPL
bashkit repl

# MCP server (Model Context Protocol)
bashkit mcp

# Mount real filesystem (read-only or read-write)
bashkit run script.sh --mount-ro /data
bashkit run script.sh --mount-rw /workspace
```

## Development

```bash
just build        # Build project
just test         # Run tests
just check        # fmt + clippy + test
just pre-pr       # Pre-PR checks
```

## LLM Eval Results

Bashkit includes an [eval harness](crates/bashkit-eval/) that measures how well LLMs use bashkit as a bash tool in agentic workloads — 58 tasks across 15 categories.

| Model | Score | Tasks Passed | Tool Call Success | Duration |
|-------|-------|-------------|-------------------|----------|
| Claude Haiku 4.5 | **97%** | **54/58** | 88% | 8.6 min |
| Claude Sonnet 4.6 | 93% | 48/58 | 85% | 20.5 min |
| Claude Opus 4.6 | 91% | 50/58 | 88% | 20.1 min |
| GPT-5.3-Codex | 91% | 51/58 | 83% | 19.6 min |
| GPT-5.2 | 77% | 41/58 | 67% | 7.0 min |

**Delta from v0.1.7** (on shared 37 tasks): Haiku 98%→100%, Opus 93%→96%, GPT-5.2 86%→86% (3 more tasks). Interpreter fixes unblocked `json_to_csv_export` and `script_function_lib` across models. See the [detailed analysis](crates/bashkit-eval/README.md#results).

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

## Language Bindings

### Python

Python bindings with LangChain integration are available in [crates/bashkit-python](crates/bashkit-python/README.md).

```python
from bashkit import BashTool

tool = BashTool()
print(tool.description())
print(tool.help())
result = await tool.execute("echo 'Hello, World!'")
print(result.stdout)
```

### JavaScript / TypeScript

NAPI-RS bindings for Node.js, Bun, and Deno. Available as `@everruns/bashkit` on npm.

```typescript
import { BashTool } from '@everruns/bashkit';

const tool = new BashTool({ username: 'agent', hostname: 'sandbox' });
const result = await tool.execute("echo 'Hello, World!'");
console.log(result.stdout);

// Direct VFS access
await tool.writeFile('/tmp/data.txt', 'hello');
const content = await tool.readFile('/tmp/data.txt');
```

Platform matrix: macOS (x86_64, aarch64), Linux (x86_64, aarch64), Windows (x86_64), WASM.
See [crates/bashkit-js](crates/bashkit-js/) for details.

## Security

Bashkit is built for running untrusted scripts from AI agents and users. Security is a core design goal, not an afterthought.

### Defense in Depth

| Layer | Protection |
|-------|------------|
| **No process spawning** | All 150 commands are reimplemented in Rust — no `fork`, `exec`, or shell escape |
| **Virtual filesystem** | Scripts see an in-memory FS by default; no host filesystem access unless explicitly mounted |
| **Network allowlist** | HTTP access is denied by default; each domain must be explicitly allowed |
| **Resource limits** | Configurable caps on commands (10K), loop iterations (100K), function depth (100), output (10MB), input (10MB) |
| **Filesystem limits** | Max total bytes (100MB), max file size (10MB), max file count (10K) — prevents zip bombs, tar bombs, and append floods |
| **Parser limits** | Timeout (5s), fuel budget (100K ops), AST depth (100) — prevents pathological input from hanging the interpreter |
| **Multi-tenant isolation** | Each `Bash` instance is fully isolated — no shared state between tenants |
| **Panic recovery** | All builtins wrapped in `catch_unwind` — a panic in one command doesn't crash the host |
| **Path traversal prevention** | RealFs backend canonicalizes paths to prevent `../../etc/passwd` escapes |
| **Unicode security** | 68 byte-boundary tests across builtins; zero-width character rejection in VFS paths |

### Threat Model

60+ identified threats across 11 categories (DoS, sandbox escape, info disclosure, injection, network, isolation, internal errors, git, logging, Python, Unicode) — each with a stable ID, mitigation status, and test coverage.

See the [threat model](specs/006-threat-model.md) for the full analysis and [security policy](SECURITY.md) for reporting vulnerabilities.

## Other Virtual Bash Implementations

- **[just-bash](https://github.com/vercel-labs/just-bash)** (TypeScript, Apache-2.0) — Virtual bash interpreter for AI agents by Vercel Labs. Custom recursive descent parser, 75+ reimplemented commands (including full awk/sed/jq), in-memory VFS, defense-in-depth sandboxing, AST transform plugins. Runs in Node.js and browser.
- **[gbash](https://github.com/ewhauser/gbash)** (Go, Apache-2.0) — Deterministic, sandbox-only bash runtime for AI agents. Delegates parsing to `mvdan/sh`. Registry-backed commands, policy enforcement, structured tracing, JSON-RPC server mode.

## Acknowledgments

Bashkit is an independent implementation that draws design inspiration from several open source projects:

- **[just-bash](https://github.com/vercel-labs/just-bash)** (Vercel Labs, Apache-2.0) — Pioneered the idea of a virtual bash interpreter for AI-powered environments. Bashkit's sandboxing architecture and multi-tenant design was inspired by their approach.
- **[Oils](https://github.com/oilshell/oil)** (Andy Chu, Apache-2.0) — Comprehensive bash compatibility testing approach inspired our spec test methodology.
- **[One True AWK](https://github.com/onetrueawk/awk)** (Lucent Technologies) — AWK language semantics reference for our awk builtin.
- **[jq](https://github.com/jqlang/jq)** (Stephen Dolan, MIT) — jq query syntax and behavior reference. Our implementation uses the [jaq](https://github.com/01mf02/jaq) Rust crates.

No code was copied from any of these projects. See [NOTICE](NOTICE) for full details.

## Ecosystem

Bashkit is part of the [Everruns](https://everruns.com) ecosystem.

## License

MIT
