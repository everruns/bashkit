# Pi + Bashkit Integration

Run [pi](https://pi.dev/) (terminal coding agent) with bashkit's virtual bash interpreter and virtual filesystem instead of real shell access.

## What This Does

Replaces pi's built-in `bash` tool with bashkit. When the LLM calls the bash tool, commands execute in bashkit's sandboxed virtual environment:

- **Virtual filesystem** — all file operations are in-memory, no real FS access
- **100+ builtins** — echo, grep, sed, awk, jq, curl, find, ls, cat, etc.
- **State persistence** — variables, files, and cwd persist across tool calls within a session
- **Resource limits** — bounded command count, loop iterations, function depth

## Setup

### Prerequisites

```bash
# Install pi
npm install -g @mariozechner/pi-coding-agent

# Install bashkit Python package
pip install bashkit
# Or build from source:
cd crates/bashkit-python && maturin develop --release
```

### Run

```bash
# With OpenAI
pi --provider openai --model gpt-4o \
  -e pi-integration/bashkit-extension.ts \
  --api-key "$OPENAI_API_KEY"

# With Anthropic
pi --provider anthropic --model claude-sonnet-4-20250514 \
  -e pi-integration/bashkit-extension.ts \
  --api-key "$ANTHROPIC_API_KEY"

# Non-interactive (print mode)
pi --provider openai --model gpt-4o-mini \
  -e pi-integration/bashkit-extension.ts \
  -p "Create a directory structure and write some files" \
  --no-session
```

## Architecture

```
pi (LLM agent) → bash tool call → bashkit-extension.ts → bashkit_server.py → bashkit (Rust via PyO3)
```

1. **bashkit-extension.ts** — Pi extension that registers a `bash` tool, replacing the built-in
2. **bashkit_server.py** — Persistent Python process running bashkit, communicates via JSON-line protocol over stdin/stdout
3. **bashkit** — Rust virtual bash interpreter with in-memory VFS

The server process stays alive for the session, maintaining VFS and shell state across tool calls.

## Configuration

Set `BASHKIT_PYTHON` env var to override the Python path:

```bash
BASHKIT_PYTHON=/path/to/venv/bin/python3 pi -e pi-integration/bashkit-extension.ts
```

## Limitations

- No real filesystem access (by design — that's the point)
- No background processes or job control
- Network access (curl/wget) requires allowlist configuration in bashkit
- pi's `read`, `write`, `edit` tools still use real FS — only `bash` is virtualized
