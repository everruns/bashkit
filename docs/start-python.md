# Get started in Python

Embed the Bashkit sandbox in a Python application or agent. The package ships as
pre-built binary wheels on PyPI — install and go, no Rust toolchain needed.

## Install

```bash
pip install bashkit
```

## First script

```python
from bashkit import Bash

bash = Bash()
result = bash.execute_sync("echo 'Hello, World!'")
print(result.stdout)
```

## Persistent state

A `Bash` instance keeps its environment and virtual filesystem across calls:

```python
from bashkit import Bash

bash = Bash()
bash.execute_sync("export APP_ENV=dev")
print(bash.execute_sync("echo $APP_ENV").stdout)  # dev
```

## Sync vs async

`execute_sync()` runs scripts that complete without suspending — plain bash and
`jq`. If you register an async custom builtin (e.g. one that issues an HTTP
request), use the awaitable `execute()` instead:

```python
result = await bash.execute("echo hi | my_async_tool")
```

## Embedded Python

Bashkit can also run Python *inside* the shell via the embedded Monty runtime —
enable it with `Bash(python=True)`. That is a different feature from embedding
Bashkit in your Python app; see the [Python builtin](python.md) guide.

## Examples

Runnable Python examples in the repo:

- [`bash_basics.py`](https://github.com/everruns/bashkit/blob/main/crates/bashkit-python/examples/bash_basics.py) — first scripts and persistent state
- [`data_pipeline.py`](https://github.com/everruns/bashkit/blob/main/crates/bashkit-python/examples/data_pipeline.py) — pipes and data processing
- [`llm_tool.py`](https://github.com/everruns/bashkit/blob/main/crates/bashkit-python/examples/llm_tool.py) — exposing Bashkit as an LLM tool
- Agent integrations: [deepagents](https://github.com/everruns/bashkit/blob/main/examples/deepagent_coding_agent.py), [Pydantic AI](https://github.com/everruns/bashkit/blob/main/examples/pydantic_ai_bash_agent.py)

## Next steps

- [Sandbox configuration & limits](configuration.md) — resource limits and sandbox options.
- [LLM tools](llm-tools.md) — expose Bashkit as a sandboxed tool for agent frameworks.
- [Security](security.md) — sandbox boundaries and what scripts cannot do.
