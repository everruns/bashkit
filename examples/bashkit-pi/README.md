# Pi + Bashkit Integration

Run [pi](https://pi.dev/) (terminal coding agent) with bashkit's virtual bash interpreter and virtual filesystem instead of real shell/filesystem access.

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
cd crates/bashkit-js && pnpm install && pnpm run build && cd -

# 2. Install this example's dependencies
cd examples/bashkit-pi && pnpm install && cd -

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

One `Bash` instance is active per Pi agent session and shared across all tools in that session. read/write/edit use direct VFS APIs (no shell quoting). bash tool uses `executeSync()`. Both share the same per-session VFS — files created by any tool are visible to all others in the same agent session.

## How It Works

1. Extension creates a fresh `Bash` instance for each `before_agent_start` event
2. All four tools (bash, read, write, edit) operate on that session's virtual filesystem
3. Files created by `write` are visible to `bash`, `read`, `edit` — and vice versa inside the same session
4. Shell state (variables, cwd, functions) persists across `bash` calls in the same session, then resets for the next session
