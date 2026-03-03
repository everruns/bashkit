# Bashkit

[![PyPI](https://img.shields.io/pypi/v/bashkit)](https://pypi.org/project/bashkit/)

A sandboxed bash interpreter for AI agents.

```python
from bashkit import BashTool

tool = BashTool()
result = tool.execute_sync("echo 'Hello, World!'")
print(result.stdout)  # Hello, World!
```

## Features

- **Sandboxed execution** — all commands run in-process with a virtual filesystem, no containers needed
- **100+ built-in commands** — echo, cat, grep, sed, awk, jq, curl, find, and more
- **Full bash syntax** — variables, pipelines, redirects, loops, functions, arrays
- **Resource limits** — protect against infinite loops and runaway scripts
- **Framework integrations** — LangChain, PydanticAI, and Deep Agents

## Installation

```bash
pip install bashkit

# With framework support
pip install 'bashkit[langchain]'
pip install 'bashkit[pydantic-ai]'
```

## Usage

### Async

```python
import asyncio
from bashkit import Bash

async def main():
    bash = Bash()

    # Simple command
    result = await bash.execute("echo 'Hello, World!'")
    print(result.stdout)  # Hello, World!

    # Pipeline
    result = await bash.execute("echo -e 'banana\\napple\\ncherry' | sort")
    print(result.stdout)  # apple\nbanana\ncherry

    # Virtual filesystem persists between calls
    await bash.execute("echo 'data' > /tmp/file.txt")
    result = await bash.execute("cat /tmp/file.txt")
    print(result.stdout)  # data

asyncio.run(main())
```

### Sync

```python
from bashkit import BashTool

tool = BashTool()
result = tool.execute_sync("echo 'Hello!'")
print(result.stdout)
```

### Configuration

```python
bash = Bash(
    username="agent",           # Custom username (whoami)
    hostname="sandbox",         # Custom hostname
    max_commands=1000,          # Limit total commands
    max_loop_iterations=10000,  # Limit loop iterations
)
```

### BashTool — Convenience Wrapper for AI Agents

`BashTool` is a convenience wrapper specifically designed for AI agents. It wraps `Bash` and adds LLM tool metadata (schema, description, system prompt) needed by tool-use protocols. Use this when integrating with LangChain, PydanticAI, or similar agent frameworks.

```python
from bashkit import BashTool

tool = BashTool()
print(tool.input_schema())    # JSON schema for LLM tool-use
print(tool.system_prompt())   # Token-efficient prompt

result = await tool.execute("echo 'Hello!'")
```

### Scripted Tool Orchestration

Compose multiple tools into a single bash-scriptable interface:

```python
from bashkit import ScriptedTool

tool = ScriptedTool("api")
tool.add_tool("greet", "Greet a user", callback=lambda p, s=None: f"hello {p.get('name', 'world')}")
result = tool.execute_sync("greet --name Alice")
print(result.stdout)  # hello Alice
```

### LangChain

```python
from bashkit.langchain import create_bash_tool

bash_tool = create_bash_tool()
# Use with any LangChain agent
```

### PydanticAI

```python
from bashkit.pydantic_ai import create_bash_tool

bash_tool = create_bash_tool()
# Use with any PydanticAI agent
```

## ScriptedTool — Multi-Tool Orchestration

Compose Python callbacks as bash builtins. An LLM writes a single bash script that pipes, loops, and branches across all registered tools.

```python
from bashkit import ScriptedTool

def get_user(params, stdin=None):
    return '{"id": 1, "name": "Alice"}'

tool = ScriptedTool("api")
tool.add_tool("get_user", "Fetch user by ID",
    callback=get_user,
    schema={"type": "object", "properties": {"id": {"type": "integer"}}})

result = tool.execute_sync("get_user --id 1 | jq -r '.name'")
print(result.stdout)  # Alice
```

## Features

- **Sandboxed, in-process execution**: All commands run in isolation with a virtual filesystem
- **100+ built-in commands**: echo, cat, grep, sed, awk, jq, curl, find, and more
- **Full bash syntax**: Variables, pipelines, redirects, loops, functions, arrays
- **Resource limits**: Protect against infinite loops and runaway scripts

## API Reference

### Bash

- `execute(commands: str) -> ExecResult` — execute commands asynchronously
- `execute_sync(commands: str) -> ExecResult` — execute commands synchronously
- `reset()` — reset interpreter state
- `description() -> str` — tool description for LLM integration
- `help() -> str` — detailed documentation
- `input_schema() -> str` — JSON input schema
- `output_schema() -> str` — JSON output schema

### BashTool

Convenience wrapper for AI agents. Inherits all execution methods from `Bash`, plus:

- `system_prompt() -> str` — token-efficient system prompt for LLM integration

### ExecResult

- `stdout: str` — standard output
- `stderr: str` — standard error
- `exit_code: int` — exit code (0 = success)
- `error: Optional[str]` — error message if execution failed
- `success: bool` — True if exit_code == 0
- `to_dict() -> dict` — convert to dictionary

### ScriptedTool

- `add_tool(name, description, callback, schema=None)` — register a tool
- `execute(script: str) -> ExecResult` — execute script asynchronously
- `execute_sync(script: str) -> ExecResult` — execute script synchronously
- `env(key: str, value: str)` — set environment variable

## How it works

Bashkit is built on top of [Bashkit core](https://github.com/everruns/bashkit), a bash interpreter written in Rust. The Python package provides a native extension for fast, sandboxed execution without spawning subprocesses or containers.

## License

MIT
