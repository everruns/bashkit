# bashkit-cli

Command-line interface for running bash scripts in a sandboxed virtual filesystem.

## Defaults

Enabled out of the box:

- **HTTP** (`curl`, `wget`) — all URLs allowed
- **Git** (`git`) — local VFS operations (init, add, commit, log, etc.)
- **Python** (`python`, `python3`) — embedded via [Monty](https://github.com/pydantic/monty)

Disable per-run:

| Flag | Effect |
|------|--------|
| `--no-http` | Disable curl/wget builtins |
| `--no-git` | Disable git builtin |
| `--no-python` | Disable python/python3 builtins |

## Install

From source:

```bash
git clone https://github.com/everruns/bashkit
cd bashkit
cargo install --path crates/bashkit-cli
```

## Examples

Text processing:

```bash
bashkit -c 'echo "hello world" | tr a-z A-Z'
# HELLO WORLD
```

Python (enabled by default):

```bash
bashkit -c 'python3 -c "print(2 + 2)"'
# 4
```

Git on the virtual filesystem:

```bash
bashkit -c '
git init /repo
cd /repo
echo "# readme" > README.md
git add README.md
git commit -m "init"
git log --oneline
'
```

Disable python:

```bash
bashkit --no-python -c 'python --version'
# python: command not found
```

Run a script file:

```bash
bashkit script.sh
```

MCP server mode:

```bash
bashkit mcp
```
