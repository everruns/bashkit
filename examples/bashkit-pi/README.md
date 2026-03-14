# Pi + Bashkit Integration

Run [pi](https://pi.dev/) (terminal coding agent) with bashkit's virtual bash interpreter and virtual filesystem instead of real shell/filesystem access.

## What This Does

Replaces all four of pi's core tools (bash, read, write, edit) with bashkit-backed virtual implementations:

- **bash** — commands execute in bashkit's sandboxed virtual bash (100+ builtins)
- **read** — reads files from bashkit's in-memory VFS
- **write** — writes files to bashkit's in-memory VFS
- **edit** — edits files in bashkit's in-memory VFS (find-and-replace)

No real filesystem access. All state persists across tool calls within a session.

## Setup

```bash
# 1. Build the server binary
cargo build --example pi_server --release

# 2. Install pi
npm install -g @mariozechner/pi-coding-agent
```

## Run

```bash
# With OpenAI
pi --provider openai --model gpt-5.4 \
  -e examples/bashkit-pi/bashkit-extension.ts \
  --api-key "$OPENAI_API_KEY"

# With Anthropic
pi --provider anthropic --model claude-sonnet-4-20250514 \
  -e examples/bashkit-pi/bashkit-extension.ts \
  --api-key "$ANTHROPIC_API_KEY"

# Non-interactive
pi --provider openai --model gpt-5.4 \
  -e examples/bashkit-pi/bashkit-extension.ts \
  -p "Create a project structure, write some code, and grep for patterns" \
  --no-session
```

## Architecture

```
pi (LLM agent)
  ├── bash tool  ──→ pi_server (Rust binary) ──→ bashkit virtual bash
  ├── read tool  ──→ pi_server ──→ bashkit VFS read
  ├── write tool ──→ pi_server ──→ bashkit VFS write
  └── edit tool  ──→ pi_server ──→ bashkit VFS read+write
```

The `pi_server` binary (`crates/bashkit/examples/pi_server.rs`) is a JSON-line protocol server that keeps bashkit state alive for the session. The TypeScript extension talks to it over stdin/stdout.

## Configuration

Override the server binary path:

```bash
BASHKIT_PI_SERVER=/path/to/pi_server pi -e examples/bashkit-pi/bashkit-extension.ts
```

## How It Works

1. Extension starts `pi_server` as a child process on first tool call
2. Each tool call sends a JSON request over stdin: `{"id":"...","op":"bash","command":"echo hi"}`
3. Server executes in bashkit, returns JSON response: `{"id":"...","stdout":"hi\n","exit_code":0}`
4. VFS and shell state persist across all calls — files created by bash are visible to read/write/edit and vice versa
