# Bashkit CLI

Use the CLI when the user wants to run Bashkit directly from a terminal.

## Install

```bash
cargo install bashkit-cli
```

Prebuilt binary with cargo-binstall:

```bash
cargo binstall bashkit-cli
```

The installed binary is `bashkit`.

## Modes

```bash
bashkit -c 'echo hello'
bashkit script.sh arg1 arg2
bashkit
```

Mode order: `-c` command string, then script file, then interactive REPL.

## Common Examples

```bash
bashkit -c 'printf "banana\napple\ncherry\n" | sort'
bashkit -c 'python3 -c "print(2 + 2)"'
bashkit -c "sqlite :memory: 'SELECT 1 + 2'"
```

Run a file:

```bash
bashkit ./script.sh
```

Mount a host directory read-only when built with `realfs` support:

```bash
bashkit --mount-ro "$PWD:/workspace" -c 'ls /workspace'
```

Mount read-write only for trusted scripts:

```bash
bashkit --mount-rw "$PWD:/workspace" -c 'echo ok > /workspace/out.txt'
```

## Security Defaults

- The default filesystem is virtual and in-memory.
- Host filesystem access requires `realfs` and `--mount-ro` or `--mount-rw`.
- HTTP builtins are disabled unless allowed.
- `--mount-rw` lets scripts modify host files.

## Useful Flags

```bash
bashkit --http-allow-all -c 'curl https://example.com'
bashkit --no-http -c 'curl https://example.com'
bashkit --no-git -c 'git status'
bashkit --no-python -c 'python3 -c "print(1)"'
bashkit --no-sqlite -c "sqlite :memory: 'SELECT 1'"
bashkit --max-commands 1000 --timeout 5 ./untrusted.sh
```

## MCP Server

Run Bashkit as a Model Context Protocol server:

```bash
bashkit mcp
```

Check current CLI docs before giving MCP flags beyond the command name.
