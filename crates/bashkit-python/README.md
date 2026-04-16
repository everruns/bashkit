# Bashkit

[![PyPI](https://img.shields.io/pypi/v/bashkit)](https://pypi.org/project/bashkit/)

Sandboxed bash interpreter for Python. Native bindings to the `bashkit` Rust core for fast, in-process execution with a virtual filesystem.

## Features

- Sandboxed execution in-process, without containers or subprocess orchestration
- Full bash syntax: variables, pipelines, redirects, loops, functions, and arrays
- 160 built-in commands including `grep`, `sed`, `awk`, `jq`, `curl`, and `find`
- Persistent interpreter state across calls, including variables, cwd, and VFS contents
- Direct virtual filesystem APIs, constructor mounts, and live host mounts
- Snapshot and restore support on `Bash` and `BashTool`
- AI integrations for LangChain, PydanticAI, and Deep Agents

## Installation

```bash
pip install bashkit

# Optional integrations
pip install 'bashkit[langchain]'
pip install 'bashkit[pydantic-ai]'
pip install 'bashkit[deepagents]'
```

## Quick Start

### Sync Execution

```python
from bashkit import Bash

bash = Bash()

result = bash.execute_sync("echo 'Hello, World!'")
print(result.stdout)  # Hello, World!

bash.execute_sync("export APP_ENV=dev")
print(bash.execute_sync("echo $APP_ENV").stdout)  # dev
```

### Async Execution

```python
import asyncio
from bashkit import Bash


async def main():
    bash = Bash()

    result = await bash.execute("echo -e 'banana\\napple\\ncherry' | sort")
    print(result.stdout)  # apple\nbanana\ncherry

    await bash.execute("printf 'data\\n' > /tmp/file.txt")
    saved = await bash.execute("cat /tmp/file.txt")
    print(saved.stdout)  # data


asyncio.run(main())
```

## Configuration

### Constructor Options

```python
from bashkit import Bash

bash = Bash(
    username="agent",
    hostname="sandbox",
    max_commands=1000,
    max_loop_iterations=10000,
    max_memory=10 * 1024 * 1024,
    timeout_seconds=30,
    python=False,
)
```

### Live Output

```python
from bashkit import Bash

bash = Bash()

def on_output(stdout: str, stderr: str) -> None:
    if stdout:
        print(stdout, end="", flush=True)
    if stderr:
        print(stderr, end="", flush=True)

result = bash.execute_sync(
    "for i in 1 2 3; do echo out-$i; echo err-$i >&2; done",
    on_output=on_output,
)
```

`on_output` is optional and fires during execution with chunked `(stdout, stderr)`
pairs. Chunks are not line-aligned or exact terminal interleaving, but
concatenating all callback chunks matches the final `ExecResult.stdout` and
`ExecResult.stderr`. The handler must be synchronous; `async def` callbacks and
callbacks that return awaitables are rejected.

## Virtual Filesystem

### Direct Methods on Bash and BashTool

```python
from bashkit import Bash

bash = Bash()
bash.mkdir("/data", recursive=True)
bash.write_file("/data/config.json", '{"debug": true}\n')
bash.append_file("/data/config.json", '{"trace": false}\n')

print(bash.read_file("/data/config.json"))
print(bash.exists("/data/config.json"))
print(bash.ls("/data"))
print(bash.glob("/data/*.json"))
```

The same direct filesystem helpers are available on `BashTool()`.

### FileSystem Accessor

```python
from bashkit import Bash

bash = Bash()
fs = bash.fs()

fs.mkdir("/data", recursive=True)
fs.write_file("/data/blob.bin", b"\x00\x01hello")
fs.copy("/data/blob.bin", "/data/backup.bin")

assert fs.read_file("/data/blob.bin") == b"\x00\x01hello"
assert fs.exists("/data/backup.bin")
```

### Pre-Initialized Files

```python
from bashkit import Bash

bash = Bash(files={
    "/config/static.txt": "ready\n",
    "/config/report.json": lambda: '{"ok": true}\n',
})

print(bash.execute_sync("cat /config/static.txt").stdout)
print(bash.execute_sync("cat /config/report.json").stdout)
```

### Real Filesystem Mounts

```python
from bashkit import Bash

bash = Bash(mounts=[
    {"host_path": "/path/to/data", "vfs_path": "/data"},
    {"host_path": "/path/to/workspace", "vfs_path": "/workspace", "writable": True},
])

print(bash.execute_sync("ls /workspace").stdout)
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

## Error Handling

```python
from bashkit import Bash, BashError

bash = Bash()

try:
    bash.execute_sync_or_throw("exit 42")
except BashError as err:
    print(err.exit_code)  # 42
    print(err.stderr)
    print(str(err))
```

Use `execute_or_throw()` and `execute_sync_or_throw()` when you want failures surfaced as exceptions instead of inspecting `exit_code` manually.

## Cancellation

```python
from bashkit import Bash

bash = Bash()

bash.cancel()        # abort in-flight execution (no-op if idle)
bash.clear_cancel()  # clear the sticky flag so subsequent executions work
```

`cancel()` sets a sticky flag that causes every future `execute()` to fail
immediately with `"execution cancelled"`. Call `clear_cancel()` after the
cancelled execution finishes to restore the instance for reuse — this
preserves all VFS state. Use `reset()` only when you want to discard VFS
and shell state entirely.

`BashTool` exposes the same `cancel()`, `clear_cancel()`, and `reset()` methods.

## BashTool

`BashTool` wraps `Bash` and adds tool-contract metadata for agent frameworks:

- `name`
- `short_description`
- `version`
- `description()`
- `help()`
- `system_prompt()`
- `input_schema()`
- `output_schema()`

```python
from bashkit import BashTool

tool = BashTool()

print(tool.description())
print(tool.input_schema())

result = tool.execute_sync("echo 'Hello from BashTool'")
print(result.stdout)
```

## ScriptedTool

Use `ScriptedTool` to register Python callbacks as bash-callable tools:

```python
from bashkit import ScriptedTool


def get_user(params, stdin=None):
    return '{"id": 1, "name": "Alice"}'


tool = ScriptedTool("api")
tool.add_tool(
    "get_user",
    "Fetch user by ID",
    callback=get_user,
    schema={"type": "object", "properties": {"id": {"type": "integer"}}},
)

result = tool.execute_sync("get_user --id 1 | jq -r '.name'")
print(result.stdout)  # Alice
```

`ScriptedTool` callbacks receive `(params_dict, stdin_or_none)` and must return
the tool stdout string. Async callbacks are also supported. `await tool.execute(...)`
runs async callbacks on the caller's active asyncio loop; `execute_sync()` falls
back to a private loop because there is no caller loop to reuse.

## Snapshot / Restore

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
```

`BashTool` exposes the same `snapshot()`, `restore_snapshot(...)`, and `from_snapshot(...)` APIs.

## Framework Integrations

### LangChain

```python
from bashkit.langchain import create_bash_tool

tool = create_bash_tool()
```

### PydanticAI

```python
from bashkit.pydantic_ai import create_bash_tool

tool = create_bash_tool()
```

### Deep Agents

```python
from bashkit.deepagents import BashkitBackend, BashkitMiddleware
```

## API Reference

### Bash

- `execute(commands: str) -> ExecResult`
- `execute_sync(commands: str) -> ExecResult`
- `execute_or_throw(commands: str) -> ExecResult`
- `execute_sync_or_throw(commands: str) -> ExecResult`
- `cancel()`
- `clear_cancel()`
- `reset()`
- `snapshot() -> bytes`
- `restore_snapshot(data: bytes)`
- `from_snapshot(data: bytes, **kwargs) -> Bash`
- `mount(vfs_path: str, fs: FileSystem)`
- `unmount(vfs_path: str)`
- Direct VFS helpers: `read_file`, `write_file`, `append_file`, `mkdir`, `remove`, `exists`, `stat`, `read_dir`, `ls`, `glob`, `copy`, `rename`, `symlink`, `chmod`, `read_link`

### BashTool

- All execution, cancellation (`cancel()`, `clear_cancel()`), reset, snapshot, restore, mount, and direct VFS helpers from `Bash`
- Tool metadata: `name`, `short_description`, `version`
- `description() -> str`
- `help() -> str`
- `system_prompt() -> str`
- `input_schema() -> str`
- `output_schema() -> str`

### ScriptedTool

- `add_tool(name, description, callback, schema=None)`
- `execute(script: str) -> ExecResult`
- `execute_sync(script: str) -> ExecResult`
- `execute_or_throw(script: str) -> ExecResult`
- `execute_sync_or_throw(script: str) -> ExecResult`
- `env(key: str, value: str)`
- `tool_count() -> int`

### FileSystem

- `mkdir(path, recursive=False)`
- `write_file(path, content)`
- `read_file(path) -> bytes`
- `append_file(path, content)`
- `exists(path) -> bool`
- `remove(path, recursive=False)`
- `stat(path) -> dict`
- `read_dir(path) -> list`
- `rename(src, dst)`
- `copy(src, dst)`
- `symlink(target, link)`
- `chmod(path, mode)`
- `read_link(path) -> str`
- `FileSystem.real(host_path, writable=False) -> FileSystem`

### ExecResult and BashError

- `ExecResult.stdout`
- `ExecResult.stderr`
- `ExecResult.exit_code`
- `ExecResult.error`
- `ExecResult.success`
- `ExecResult.to_dict()`
- `BashError.exit_code`
- `BashError.stderr`

## Platform Support

- Linux: `x86_64`, `aarch64` (glibc and musl wheels)
- macOS: `x86_64`, `aarch64`
- Windows: `x86_64`
- Python: `3.9` through `3.13`

## How It Works

Bashkit is built on the `bashkit` Rust core, which implements a sandboxed bash interpreter and virtual filesystem. The Python package exposes that engine through a native extension, so commands run in-process with persistent state and resource limits, without shelling out to the host system.

## Part of Everruns

Bashkit is part of the [Everruns](https://github.com/everruns) ecosystem. See the [bashkit monorepo](https://github.com/everruns/bashkit) for the Rust core, the JavaScript package (`@everruns/bashkit`), and related tooling.

## License

MIT
