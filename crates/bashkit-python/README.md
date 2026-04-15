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
- **160 built-in commands** — echo, cat, grep, sed, awk, jq, curl, find, and more
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

### Direct Filesystem Access

```python
from bashkit import Bash

bash = Bash()
bash.mkdir("/data", recursive=True)
bash.write_file("/data/config.json", '{"debug": true}\n')
assert bash.read_file("/data/config.json") == '{"debug": true}\n'
assert bash.ls("/data") == ["config.json"]
assert bash.glob("/data/*.json") == ["/data/config.json"]

# The same convenience methods exist on BashTool()
```

### FileSystem Accessor

```python
from bashkit import Bash

bash = Bash()
fs = bash.fs()
fs.mkdir("/data", recursive=True)
fs.write_file("/data/blob.bin", b"\x00\x01hello")
assert fs.read_file("/data/blob.bin") == b"\x00\x01hello"

# Additional filesystem operations
fs.append_file("/data/blob.bin", b" world")
fs.copy("/data/blob.bin", "/data/backup.bin")
fs.rename("/data/backup.bin", "/data/copy.bin")
info = fs.stat("/data/blob.bin")       # dict with size, type, etc.
entries = fs.read_dir("/data")         # detailed directory listing
fs.symlink("/data/link", "/data/blob.bin")
fs.chmod("/data/blob.bin", 0o644)
```

### Files and Mounts

```python
from bashkit import Bash, FileSystem

# Text files (in-memory, writable)
bash = Bash(files={"/config/app.conf": "debug=true\n"})

# Lazy file providers (called on first read, then cached)
bash = Bash(files={
    "/config/static.txt": "ready\n",
    "/config/report.json": lambda: '{"ok": true}\n',
})

# Real filesystem mounts (read-only by default)
bash = Bash(mounts=[
    {"host_path": "/path/to/data", "vfs_path": "/data"},
    {"host_path": "/path/to/workspace", "vfs_path": "/workspace", "writable": True},
])
```

### Live Mounts

```python
from bashkit import Bash, FileSystem

bash = Bash()
workspace = FileSystem.real("/path/to/workspace", writable=True)
bash.mount("/workspace", workspace)

bash.execute_sync("echo 'hello' > /workspace/demo.txt")
bash.unmount("/workspace")
```

### BashTool — Convenience Wrapper for AI Agents

`BashTool` is a convenience wrapper specifically designed for AI agents. It wraps `Bash` and adds contract metadata (`description`, Markdown `help`, `system_prompt`, JSON schemas) needed by tool-use protocols. Use this when integrating with LangChain, PydanticAI, or similar agent frameworks.

```python
from bashkit import BashTool

tool = BashTool()
print(tool.input_schema())    # JSON schema for LLM tool-use
print(tool.description())     # Token-efficient tool description
print(tool.system_prompt())   # Token-efficient prompt
print(tool.help())            # Markdown help document

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

### Snapshot / Restore

```python
from bashkit import Bash

bash = Bash(username="agent", max_commands=100)
bash.execute_sync("export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt")

snapshot = bash.snapshot()

restored = Bash.from_snapshot(snapshot, username="agent", max_commands=100)
assert restored.execute_sync("echo $BUILD_ID").stdout.strip() == "42"
assert restored.execute_sync("cat /workspace/state.txt").stdout.strip() == "ready"

restored.reset()
restored.restore_snapshot(snapshot)
assert restored.execute_sync("pwd").stdout.strip() == "/workspace"

# BashTool exposes the same snapshot/restore API.
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

### Deep Agents

```python
from bashkit.deepagents import BashkitBackend, BashkitMiddleware
# Use with Deep Agents framework
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
- **160 built-in commands**: echo, cat, grep, sed, awk, jq, curl, find, and more
- **Full bash syntax**: Variables, pipelines, redirects, loops, functions, arrays
- **Resource limits**: Protect against infinite loops and runaway scripts

## API Reference

### Bash

- `execute(commands: str) -> ExecResult` — execute commands asynchronously
- `execute_sync(commands: str) -> ExecResult` — execute commands synchronously
- `execute_or_throw(commands: str) -> ExecResult` — async, raises on non-zero exit
- `execute_sync_or_throw(commands: str) -> ExecResult` — sync, raises on non-zero exit
- `reset()` — reset interpreter state
- `snapshot() -> bytes` — serialize interpreter state
- `restore_snapshot(data: bytes)` — restore serialized interpreter state
- `from_snapshot(data: bytes, **kwargs) -> Bash` — construct and restore in one step
- `fs() -> FileSystem` — direct filesystem access

### BashTool

Convenience wrapper for AI agents. Inherits all execution methods from `Bash`, plus:

- `description() -> str` — token-efficient tool description
- `help() -> str` — Markdown help document
- `system_prompt() -> str` — token-efficient system prompt for LLM integration
- `input_schema() -> str` — JSON input schema
- `output_schema() -> str` — JSON output schema
- `snapshot() -> bytes` / `restore_snapshot(data: bytes)` / `from_snapshot(...)` — checkpoint and resume interpreter state

### ExecResult

- `stdout: str` — standard output
- `stderr: str` — standard error
- `exit_code: int` — exit code (0 = success)
- `error: Optional[str]` — error message if execution failed
- `success: bool` — True if exit_code == 0
- `to_dict() -> dict` — convert to dictionary

### FileSystem

- `mkdir(path, recursive=False)` — create directory
- `write_file(path, content)` — write bytes to file
- `read_file(path) -> bytes` — read file contents
- `append_file(path, content)` — append bytes to file
- `exists(path) -> bool` — check if path exists
- `remove(path, recursive=False)` — delete file/directory
- `stat(path) -> dict` — file metadata (size, type, etc.)
- `read_dir(path) -> list` — detailed directory listing
- `rename(src, dst)` — rename file/directory
- `copy(src, dst)` — copy file
- `symlink(link, target)` — create symlink
- `chmod(path, mode)` — change file permissions
- `read_link(path) -> str` — read symlink target

### ScriptedTool

- `add_tool(name, description, callback, schema=None)` — register a tool
- `execute(script: str) -> ExecResult` — execute script asynchronously
- `execute_sync(script: str) -> ExecResult` — execute script synchronously
- `execute_or_throw(script: str) -> ExecResult` — async, raises on non-zero exit
- `execute_sync_or_throw(script: str) -> ExecResult` — sync, raises on non-zero exit
- `env(key: str, value: str)` — set environment variable
- `tool_count() -> int` — number of registered tools

## How it works

Bashkit is built on top of [Bashkit core](https://github.com/everruns/bashkit), a bash interpreter written in Rust. The Python package provides a native extension for fast, sandboxed execution without spawning subprocesses or containers.

## Part of Everruns

Bashkit is part of the [Everruns](https://github.com/everruns) ecosystem — tools and runtimes for building reliable AI agents. See the [bashkit monorepo](https://github.com/everruns/bashkit) for the Rust core, Node.js package (`@everruns/bashkit`), and more.

## License

MIT
