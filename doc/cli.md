# bashkit-cli

Quick CLI for running BashKit scripts in a sandboxed virtual filesystem.

## Defaults

`bashkit-cli` enables these by default:

- HTTP builtins (`curl`, `wget`)
- Git builtin (`git`)
- Python command (`python`) via `monty python` when `monty` exists, else `python3`

Disable any default per run:

- `--no-http`
- `--no-git`
- `--no-python`

## Quick install

From source:

```bash
git clone https://github.com/everruns/bashkit
cd bashkit
cargo install --path crates/bashkit-cli
```

Run:

```bash
bashkit --version
```

## Examples

Works everywhere (no network):

```bash
bashkit -c 'echo "hello" | tr a-z A-Z'
```

Python enabled by default:

```bash
bashkit -c 'python -c "print(2 + 2)"'
```

Disable python:

```bash
bashkit --no-python -c 'python --version'
```

Git enabled by default:

```bash
bashkit -c 'git init /repo && cd /repo && git status'
```

HTTP enabled by default:

```bash
bashkit -c 'curl -s https://example.com | head -n 1'
```

Disable HTTP:

```bash
bashkit --no-http -c 'curl -s https://example.com'
```
