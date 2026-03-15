# @everruns/bashkit-pi

Run [pi](https://pi.dev/) coding agent with bashkit's sandboxed virtual bash + VFS. No real filesystem access, no subprocesses.

## Quick Start

```bash
npx @everruns/bashkit-pi --provider openai --model gpt-5.4 --api-key "$OPENAI_API_KEY"
```

```bash
npx @everruns/bashkit-pi --provider anthropic --model claude-sonnet-4-20250514 --api-key "$ANTHROPIC_API_KEY"
```

## What It Does

Replaces all four of pi's core tools with bashkit-backed virtual implementations:

- **bash** — commands execute in bashkit's sandboxed virtual bash (100+ builtins)
- **read** — reads files from bashkit's in-memory VFS
- **write** — writes files to bashkit's in-memory VFS
- **edit** — edits files in bashkit's in-memory VFS (find-and-replace)

## Prerequisites

- Node.js 18+
- [pi](https://pi.dev/) installed globally: `npm install -g @mariozechner/pi-coding-agent`

## All CLI args are forwarded to pi

```bash
# Non-interactive
npx @everruns/bashkit-pi --provider openai --model gpt-5.4 \
  -p "Create a project structure, write some code, and grep for patterns" \
  --no-session

# With additional extensions
npx @everruns/bashkit-pi --provider openai --model gpt-5.4 \
  -e ./my-other-extension.ts
```
