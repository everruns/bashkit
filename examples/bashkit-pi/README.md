# Pi + Bashkit Integration

Run [pi](https://pi.dev/) (terminal coding agent) with bashkit's virtual bash interpreter and virtual filesystem instead of real shell/filesystem access.

## Quick Start (from GitHub)

```bash
npx @everruns/bashkit-pi --provider openai --model gpt-5.4 --api-key "$OPENAI_API_KEY"
```

No clone needed. See [`packages/bashkit-pi`](../../packages/bashkit-pi/) for details.

## What This Does

Replaces all four of pi's core tools (bash, read, write, edit) with bashkit-backed virtual implementations:

- **bash** — commands execute in bashkit's sandboxed virtual bash (100+ builtins)
- **read** — reads files from bashkit's in-memory VFS
- **write** — writes files to bashkit's in-memory VFS
- **edit** — edits files in bashkit's in-memory VFS (find-and-replace)

No real filesystem access. No subprocess. Uses `@everruns/bashkit` Node.js native bindings (NAPI-RS) loaded directly in pi's process.

## Setup

```bash
# 1. Build the Node.js bindings
cd crates/bashkit-js && npm install && npm run build && cd -

# 2. Install this example's dependencies
cd examples/bashkit-pi && npm install && cd -

# 3. Install pi
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
  ├── bash tool  ──→ Bash.executeSync()  ──→ bashkit virtual bash
  ├── read tool  ──→ Bash.readFile()     ──→ bashkit VFS (direct)
  ├── write tool ──→ Bash.writeFile()    ──→ bashkit VFS (direct)
  └── edit tool  ──→ Bash.readFile() + writeFile()  ──→ bashkit VFS (direct)
```

Single `Bash` instance shared across all tools. read/write/edit use direct VFS APIs (no shell quoting). bash tool uses `executeSync()`. Both share the same VFS — files created by any tool are visible to all others.

## How It Works

1. Extension creates a single `Bash` instance on load
2. All four tools (bash, read, write, edit) operate on the same virtual filesystem
3. Files created by `write` are visible to `bash`, `read`, `edit` — and vice versa
4. Shell state (variables, cwd, functions) persists across `bash` calls
