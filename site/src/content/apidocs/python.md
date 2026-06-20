# Python API reference

Auto-generated reference for the [`bashkit`](https://pypi.org/project/bashkit/) PyPI package, covering the public classes and functions exported from `bashkit`. Reflects the latest published release.

> Install with `pip install bashkit`. See the [Embedding guide](/docs/embedding/) and [LLM tools guide](/docs/llm-tools/) for task-oriented walkthroughs.

## Bash

Core bash interpreter with virtual filesystem.

State persists between calls — files created in one execute() are
available in subsequent calls.

Example (basic):

```python
>>> bash = Bash()
>>> result = await bash.execute("echo 'Hello!'")
>>> print(result.stdout)
Hello!
```


Example (Python execution with external function handler):

```python
>>> async def handler(fn_name: str, args: list, kwargs: dict) -> Any:
...     return await tool_executor.call(fn_name, kwargs)
>>> bash = Bash(
...     python=True,
...     external_functions=["api_request"],
...     external_handler=handler,
... )
>>> result = await bash.execute("python3 -c 'print(api_request())'")
```

### Constructor

```python
Bash(username: str | None = None, hostname: str | None = None, cwd: str | None = None, env: Mapping[str, str] | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, max_memory: int | None = None, timeout_seconds: float | None = None, python: bool = False, sqlite: bool = False, external_functions: list[str] | None = None, external_handler: ExternalHandler | None = None, files: dict[str, str | Callable[[], str]] | None = None, mounts: list[dict[str, Any]] | None = None, allowed_mount_paths: list[str] | None = None, readonly_filesystem: bool = False, custom_builtins: Mapping[str, BuiltinCallback] | None = None, network: NetworkConfig | None = None) -> None
```

Create a new Bash interpreter.

**Parameters:**

- **`username`** — Custom username (default ``"user"``).
- **`hostname`** — Custom hostname (default ``"bashkit"``).
- **`cwd`** — Initial working directory for the shell. Sets the starting directory directly instead of running a leading ``cd``.
- **`env`** — Initial environment variables applied before execution, so scripts see them without an ``export`` prelude.
- **`max_commands`** — Limit total commands executed.
- **`max_loop_iterations`** — Limit iterations per loop.
- **`max_memory`** — Memory limit in bytes for the VFS.
- **`timeout_seconds`** — Abort execution after this duration.
- **`python`** — Enable embedded Python (``python3`` builtin).
- **`sqlite`** — Enable embedded SQLite (``sqlite``/``sqlite3`` builtin). Defaults to ``False``. When ``True``, the Turso-backed engine is registered and ``BASHKIT_ALLOW_INPROCESS_SQLITE=1`` is injected automatically. Default ``SqliteLimits`` apply: 4 MiB script cap, 256 MiB DB cap, 30 s wall-clock budget, resource-affecting PRAGMAs (`cache_size`, `mmap_size`, …) rejected, ``ATTACH``/``DETACH`` rejected.
- **`external_functions`** — Function names callable from Python code.
- **`external_handler`** — Async callback for external function calls. The callback must not call back into the same ``Bash`` instance via live methods like ``read_file()``, ``fs()``, or ``execute()``; those re-entrant calls are rejected.
- **`files`** — Dict mapping VFS paths to file contents or lazy callables.
- **`mounts`** — List of real host directory mount configs.
- **`allowed_mount_paths`** — Host path prefixes allowed for real filesystem mounts. Required when mounting sensitive host locations such as paths under a user home directory.
- **`readonly_filesystem`** — Deny all filesystem mutations after configured files and mounts are applied.
- **`custom_builtins`** — Constructor-time Python callbacks exposed as bash builtins. Each callback receives a ``BuiltinContext`` with raw ``argv`` tokens, optional pipeline ``stdin``, and a live ``fs`` handle to the virtual filesystem, and must return a stdout string, a ``BuiltinResult``, or await either. Async callbacks run on the caller's active asyncio loop for ``await execute()`` and on a private loop for ``execute_sync()``.
- **`network`** — Optional outbound HTTP / network configuration. Pass ``{"allow": [...]}`` for an explicit allowlist or ``{"allow_all": True}`` to allow every URL (mirrors ``NetworkAllowlist::allow_all()`` in the Rust core). Set ``"block_private_ips": False`` to relax the SSRF guard. Add ``"credentials": [...]`` to inject headers transparently for matching URLs and ``"credential_placeholders": [...]`` to expose opaque placeholder env vars that are replaced with the real secret on the wire. When omitted, network access is disabled (current default). Preserved across ``reset()`` and ``from_snapshot()`` — placeholder env vars are regenerated on each rebuild.

Example:

```python
>>> bash = Bash(
...     timeout_seconds=30,
...     files={"/input.txt": "some data"},
...     custom_builtins={"ping": lambda ctx: "pong\n"},
...     network={
...         "allow": ["https://api.github.com"],
...         "credentials": [
...             {
...                 "pattern": "https://api.github.com",
...                 "kind": "bearer",
...                 "token": "ghp_xxx",
...             }
...         ],
...     },
... )
```

### `execute`

```python
Bash.execute(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute bash commands asynchronously.

**Parameters:**

- **`commands`** — Bash script to run (like ``bash -c "commands"``).
- **`on_output`** — Optional callback receiving chunked ``(stdout, stderr)`` pairs during execution. Must be synchronous.

Async ``custom_builtins`` callbacks run on the caller's active asyncio
loop.

**Returns:** ExecResult with stdout, stderr, exit_code.

Example:

```python
>>> bash = Bash()
>>> result = await bash.execute("echo hello && echo world")
>>> print(result.stdout)
hello
world
```

### `execute_sync`

```python
Bash.execute_sync(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute bash commands synchronously (blocking).

Not supported when ``external_handler`` is configured — use
``execute()`` (async) instead. ``on_output`` must be synchronous.
Async ``custom_builtins`` callbacks run on a private loop here.
When called from inside a running event loop (e.g. Jupyter / IPython),
callbacks are dispatched to a background thread with their own loop so
that asyncio's "cannot run while another loop is running" restriction
is not triggered.

Example:

```python
>>> bash = Bash()
>>> result = bash.execute_sync("date +%Y")
>>> print(result.exit_code)
0
```

### `execute_or_throw`

```python
Bash.execute_or_throw(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute commands asynchronously; raise ``BashError`` on non-zero exit.

``on_output`` must be synchronous.

Example:

```python
>>> bash = Bash()
>>> result = await bash.execute_or_throw("echo ok")
>>> # Raises BashError if the command fails:
>>> await bash.execute_or_throw("false")  # doctest: +SKIP
Traceback (most recent call last):
    ...
BashError: ...
```

### `execute_sync_or_throw`

```python
Bash.execute_sync_or_throw(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute commands synchronously; raise ``BashError`` on non-zero exit.

``on_output`` must be synchronous.

Example:

```python
>>> bash = Bash()
>>> result = bash.execute_sync_or_throw("echo ok")
>>> print(result.stdout.strip())
ok
```

### `cancel`

```python
Bash.cancel() -> None
```

Cancel the currently running execution.

Safe to call from any thread. Execution aborts at the next
command boundary.

Example:

```python
>>> import threading
>>> bash = Bash()
>>> threading.Timer(1.0, bash.cancel).start()
>>> # Long-running command will be cancelled after 1 second
```

### `clear_cancel`

```python
Bash.clear_cancel() -> None
```

Clear the cancellation flag so subsequent executions proceed normally.

Call this after a ``cancel()`` once the in-flight execution has
finished and you want to reuse the same ``Bash`` instance
(preserving VFS state). Without this, every future ``execute()``
will immediately fail with ``"execution cancelled"``.

**Note:** Calling this while an execution is still in-flight may
allow that execution to continue past the cancellation point.
Wait for the cancelled execution to finish before clearing
(await the async call or let ``execute_sync`` return).

Example:

```python
>>> bash = Bash()
>>> bash.cancel()
>>> bash.clear_cancel()
>>> result = bash.execute_sync("echo ok")
>>> result.exit_code
0
```

### `reset`

```python
Bash.reset() -> None
```

Reset interpreter to initial state.

Clears all VFS contents, environment variables, and shell state.
Re-applies the original ``files``, ``mounts``, and
``custom_builtins`` configuration.

Example:

```python
>>> bash = Bash()
>>> bash.execute_sync("echo hi > /tmp/file.txt")
>>> bash.reset()
>>> result = bash.execute_sync("cat /tmp/file.txt")
>>> result.exit_code  # file is gone after reset
1
```

### `snapshot`

```python
Bash.snapshot(exclude_filesystem: bool = False, exclude_functions: bool = False) -> bytes
```

Serialize interpreter state to bytes.

### `snapshot_keyed`

```python
Bash.snapshot_keyed(key: bytes, exclude_filesystem: bool = False, exclude_functions: bool = False) -> bytes
```

Serialize interpreter state to HMAC-protected bytes.

### `shell_state`

```python
Bash.shell_state() -> ShellState
```

Capture a read-only shell-state snapshot.

### `restore_snapshot`

```python
Bash.restore_snapshot(data: bytes) -> None
```

Restore interpreter state from bytes produced by ``snapshot()``.

### `restore_snapshot_keyed`

```python
Bash.restore_snapshot_keyed(data: bytes, key: bytes) -> None
```

Restore interpreter state from bytes produced by ``snapshot_keyed()``.

### `from_snapshot`

```python
Bash.from_snapshot(data: bytes, username: str | None = None, hostname: str | None = None, cwd: str | None = None, env: Mapping[str, str] | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, max_memory: int | None = None, timeout_seconds: float | None = None, python: bool = False, sqlite: bool = False, external_functions: list[str] | None = None, external_handler: ExternalHandler | None = None, files: dict[str, str] | None = None, mounts: list[dict[str, Any]] | None = None, allowed_mount_paths: list[str] | None = None, readonly_filesystem: bool = False, custom_builtins: Mapping[str, BuiltinCallback] | None = None, network: NetworkConfig | None = None) -> Bash
```

Create a new ``Bash`` from snapshot bytes and optional constructor kwargs.

### `from_snapshot_keyed`

```python
Bash.from_snapshot_keyed(data: bytes, key: bytes, username: str | None = None, hostname: str | None = None, cwd: str | None = None, env: Mapping[str, str] | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, max_memory: int | None = None, timeout_seconds: float | None = None, python: bool = False, sqlite: bool = False, external_functions: list[str] | None = None, external_handler: ExternalHandler | None = None, files: dict[str, str] | None = None, mounts: list[dict[str, Any]] | None = None, allowed_mount_paths: list[str] | None = None, readonly_filesystem: bool = False, custom_builtins: Mapping[str, BuiltinCallback] | None = None, network: NetworkConfig | None = None) -> Bash
```

Create a new ``Bash`` from HMAC-protected snapshot bytes.

### `read_file`

```python
Bash.read_file(path: str) -> str
```

Read a VFS file as UTF-8 text.

### `write_file`

```python
Bash.write_file(path: str, content: str) -> None
```

Write UTF-8 text into the VFS.

### `append_file`

```python
Bash.append_file(path: str, content: str) -> None
```

Append UTF-8 text to a VFS file.

### `mkdir`

```python
Bash.mkdir(path: str, recursive: bool = False) -> None
```

Create a directory in the VFS.

### `exists`

```python
Bash.exists(path: str) -> bool
```

Return whether a VFS path exists.

### `remove`

```python
Bash.remove(path: str, recursive: bool = False) -> None
```

Remove a VFS file or directory.

### `stat`

```python
Bash.stat(path: str) -> dict[str, Any]
```

Return metadata for a VFS path.

### `chmod`

```python
Bash.chmod(path: str, mode: int) -> None
```

Change VFS permissions for a path.

### `symlink`

```python
Bash.symlink(target: str, link: str) -> None
```

Create a symlink in the VFS.

### `read_link`

```python
Bash.read_link(path: str) -> str
```

Return the symlink target for a VFS path.

### `read_dir`

```python
Bash.read_dir(path: str) -> list[dict[str, Any]]
```

Return directory entries with metadata.

### `ls`

```python
Bash.ls(path: str = '.') -> list[str]
```

Return entry names for a directory, or an empty list if it is missing.

### `glob`

```python
Bash.glob(pattern: str) -> list[str]
```

Return file paths matching a safe glob pattern.

### `fs`

```python
Bash.fs() -> FileSystem
```

Return a live filesystem handle.

Each operation acquires the interpreter lock, so the handle always
reflects the latest state (including after ``reset()``).

Example:

```python
>>> bash = Bash()
>>> bash.execute_sync("echo hello > /greeting.txt")
>>> fs = bash.fs()
>>> fs.read_file("/greeting.txt")
b'hello\n'
```

### `mount`

```python
Bash.mount(vfs_path: str, fs: FileSystem) -> None
```

Mount an external filesystem at the given VFS path.

**Parameters:**

- **`vfs_path`** — Mount point inside the VFS.
- **`fs`** — FileSystem instance to mount.

Example:

```python
>>> bash = Bash()
>>> overlay = FileSystem()
>>> overlay.write_file("/data.csv", b"a,b,c")
>>> bash.mount("/mnt/data", overlay)
>>> result = bash.execute_sync("cat /mnt/data/data.csv")
>>> print(result.stdout)
a,b,c
```

### `unmount`

```python
Bash.unmount(vfs_path: str) -> None
```

Unmount a previously mounted filesystem.

Example:

```python
>>> bash = Bash()
>>> overlay = FileSystem()
>>> bash.mount("/mnt/ext", overlay)
>>> bash.unmount("/mnt/ext")
```

## BashTool

Sandboxed bash interpreter for AI agents.

BashTool provides a safe execution environment for running bash commands
with a virtual filesystem. All file operations are contained within the
sandbox - no access to the real filesystem.

Adds LLM-facing contract metadata (``description``, ``system_prompt``,
``input_schema``, ``output_schema``) on top of the core interpreter.

### Fields

- **`name`** — `str`
- **`short_description`** — `str`
- **`version`** — `str`

### Constructor

```python
BashTool(username: str | None = None, hostname: str | None = None, cwd: str | None = None, env: Mapping[str, str] | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, max_memory: int | None = None, timeout_seconds: float | None = None, files: dict[str, str | Callable[[], str]] | None = None, mounts: list[dict[str, Any]] | None = None, allowed_mount_paths: list[str] | None = None, readonly_filesystem: bool = False, custom_builtins: Mapping[str, BuiltinCallback] | None = None, network: NetworkConfig | None = None) -> None
```

Create a new BashTool.

**Parameters:**

- **`username`** — Custom username (default ``"user"``).
- **`hostname`** — Custom hostname (default ``"bashkit"``).
- **`cwd`** — Initial working directory for the shell. Sets the starting directory directly instead of running a leading ``cd``.
- **`env`** — Initial environment variables applied before execution, so scripts see them without an ``export`` prelude.
- **`max_commands`** — Limit total commands executed.
- **`max_loop_iterations`** — Limit iterations per loop.
- **`max_memory`** — Memory limit in bytes for the VFS.
- **`timeout_seconds`** — Abort execution after this duration.
- **`files`** — Dict mapping VFS paths to file contents or lazy callables.
- **`mounts`** — List of real host directory mount configs.
- **`allowed_mount_paths`** — Host path prefixes allowed for real filesystem mounts. Required when mounting sensitive host locations such as paths under a user home directory.
- **`readonly_filesystem`** — Deny all filesystem mutations after configured files and mounts are applied.
- **`custom_builtins`** — Constructor-time Python callbacks exposed as bash builtins. Each callback receives a ``BuiltinContext`` (including a live ``fs`` handle to the virtual filesystem) and must return a stdout string, a ``BuiltinResult``, or await either. Async callbacks run on the caller's active asyncio loop for ``await execute()`` and on a private loop for ``execute_sync()``.
- **`network`** — Optional outbound HTTP / network configuration. See ``Bash.__init__`` for accepted keys. Preserved across ``reset()`` and ``from_snapshot()``.

Example:

```python
>>> tool = BashTool(
...     timeout_seconds=30,
...     custom_builtins={"ping": lambda ctx: "pong\n"},
...     network={"allow_all": True},
... )
>>> print(tool.name)
bash
```

### `execute`

```python
BashTool.execute(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute bash commands asynchronously.

Async ``custom_builtins`` callbacks run on the caller's active asyncio
loop.

``on_output`` must be synchronous.

Example:

```python
>>> tool = BashTool()
>>> result = await tool.execute("ls /")
>>> result.success
True
```

### `execute_sync`

```python
BashTool.execute_sync(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute bash commands synchronously (blocking).

Async ``custom_builtins`` callbacks run on a private loop here.
When called from inside a running event loop (e.g. Jupyter / IPython),
callbacks are dispatched to a background thread with their own loop so
that asyncio's "cannot run while another loop is running" restriction
is not triggered.

``on_output`` must be synchronous.

Example:

```python
>>> tool = BashTool()
>>> result = tool.execute_sync("echo 42")
>>> result.stdout.strip()
'42'
```

### `execute_or_throw`

```python
BashTool.execute_or_throw(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute commands asynchronously; raise ``BashError`` on non-zero exit.

``on_output`` must be synchronous.

Example:

```python
>>> tool = BashTool()
>>> result = await tool.execute_or_throw("echo ok")
>>> result.success
True
```

### `execute_sync_or_throw`

```python
BashTool.execute_sync_or_throw(commands: str, on_output: OutputHandler | None = None) -> ExecResult
```

Execute commands synchronously; raise ``BashError`` on non-zero exit.

``on_output`` must be synchronous.

Example:

```python
>>> tool = BashTool()
>>> result = tool.execute_sync_or_throw("echo ok")
>>> result.stdout.strip()
'ok'
```

### `cancel`

```python
BashTool.cancel() -> None
```

Cancel the currently running execution.

Safe to call from any thread.

Example:

```python
>>> tool = BashTool()
>>> tool.cancel()  # no-op if nothing is running
```

### `clear_cancel`

```python
BashTool.clear_cancel() -> None
```

Clear the cancellation flag so subsequent executions proceed normally.

Call this after a ``cancel()`` once the in-flight execution has
finished and you want to reuse the same ``BashTool`` instance
(preserving VFS state). Without this, every future ``execute()``
will immediately fail with ``"execution cancelled"``.

**Note:** Calling this while an execution is still in-flight may
allow that execution to continue past the cancellation point.
Wait for the cancelled execution to finish before clearing
(await the async call or let ``execute_sync`` return).

Example:

```python
>>> tool = BashTool()
>>> tool.cancel()
>>> tool.clear_cancel()
>>> result = tool.execute_sync("echo ok")
>>> result.exit_code
0
```

### `description`

```python
BashTool.description() -> str
```

Return the tool description for LLM consumption.

Example:

```python
>>> tool = BashTool()
>>> desc = tool.description()
>>> "bash" in desc.lower()
True
```

### `help`

```python
BashTool.help() -> str
```

Return extended help text.

Example:

```python
>>> tool = BashTool()
>>> help_text = tool.help()
>>> len(help_text) > 0
True
```

### `system_prompt`

```python
BashTool.system_prompt() -> str
```

Return the system prompt for LLM agents.

Includes tool description, usage guidelines, and capabilities.

Example:

```python
>>> tool = BashTool()
>>> prompt = tool.system_prompt()
>>> "sandbox" in prompt.lower() or "bash" in prompt.lower()
True
```

### `input_schema`

```python
BashTool.input_schema() -> str
```

Return the JSON Schema for tool input.

Example:

```python
>>> import json
>>> tool = BashTool()
>>> schema = json.loads(tool.input_schema())
>>> "commands" in str(schema)
True
```

### `output_schema`

```python
BashTool.output_schema() -> str
```

Return the JSON Schema for tool output.

Example:

```python
>>> import json
>>> tool = BashTool()
>>> schema = json.loads(tool.output_schema())
>>> isinstance(schema, dict)
True
```

### `reset`

```python
BashTool.reset() -> None
```

Reset the tool to initial state.

Clears VFS, environment, and shell state while re-applying
constructor-time ``custom_builtins``.

Example:

```python
>>> tool = BashTool()
>>> tool.execute_sync("touch /tmp/file")
>>> tool.reset()
>>> result = tool.execute_sync("test -f /tmp/file")
>>> result.exit_code  # file is gone
1
```

### `snapshot`

```python
BashTool.snapshot(exclude_filesystem: bool = False, exclude_functions: bool = False) -> bytes
```

Serialize interpreter state to bytes.

### `snapshot_keyed`

```python
BashTool.snapshot_keyed(key: bytes, exclude_filesystem: bool = False, exclude_functions: bool = False) -> bytes
```

Serialize interpreter state to HMAC-protected bytes.

### `shell_state`

```python
BashTool.shell_state() -> ShellState
```

Capture a read-only shell-state snapshot.

### `restore_snapshot`

```python
BashTool.restore_snapshot(data: bytes) -> None
```

Restore interpreter state from bytes produced by ``snapshot()``.

### `restore_snapshot_keyed`

```python
BashTool.restore_snapshot_keyed(data: bytes, key: bytes) -> None
```

Restore interpreter state from bytes produced by ``snapshot_keyed()``.

### `from_snapshot`

```python
BashTool.from_snapshot(data: bytes, username: str | None = None, hostname: str | None = None, cwd: str | None = None, env: Mapping[str, str] | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, max_memory: int | None = None, timeout_seconds: float | None = None, files: dict[str, str] | None = None, mounts: list[dict[str, Any]] | None = None, allowed_mount_paths: list[str] | None = None, readonly_filesystem: bool = False, custom_builtins: Mapping[str, BuiltinCallback] | None = None, network: NetworkConfig | None = None) -> BashTool
```

Create a new ``BashTool`` from snapshot bytes and optional constructor kwargs.

### `from_snapshot_keyed`

```python
BashTool.from_snapshot_keyed(data: bytes, key: bytes, username: str | None = None, hostname: str | None = None, cwd: str | None = None, env: Mapping[str, str] | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, max_memory: int | None = None, timeout_seconds: float | None = None, files: dict[str, str] | None = None, mounts: list[dict[str, Any]] | None = None, allowed_mount_paths: list[str] | None = None, readonly_filesystem: bool = False, custom_builtins: Mapping[str, BuiltinCallback] | None = None, network: NetworkConfig | None = None) -> BashTool
```

Create a new ``BashTool`` from HMAC-protected snapshot bytes.

### `read_file`

```python
BashTool.read_file(path: str) -> str
```

Read a VFS file as UTF-8 text.

### `write_file`

```python
BashTool.write_file(path: str, content: str) -> None
```

Write UTF-8 text into the VFS.

### `append_file`

```python
BashTool.append_file(path: str, content: str) -> None
```

Append UTF-8 text to a VFS file.

### `mkdir`

```python
BashTool.mkdir(path: str, recursive: bool = False) -> None
```

Create a directory in the VFS.

### `exists`

```python
BashTool.exists(path: str) -> bool
```

Return whether a VFS path exists.

### `remove`

```python
BashTool.remove(path: str, recursive: bool = False) -> None
```

Remove a VFS file or directory.

### `stat`

```python
BashTool.stat(path: str) -> dict[str, Any]
```

Return metadata for a VFS path.

### `chmod`

```python
BashTool.chmod(path: str, mode: int) -> None
```

Change VFS permissions for a path.

### `symlink`

```python
BashTool.symlink(target: str, link: str) -> None
```

Create a symlink in the VFS.

### `read_link`

```python
BashTool.read_link(path: str) -> str
```

Return the symlink target for a VFS path.

### `read_dir`

```python
BashTool.read_dir(path: str) -> list[dict[str, Any]]
```

Return directory entries with metadata.

### `ls`

```python
BashTool.ls(path: str = '.') -> list[str]
```

Return entry names for a directory, or an empty list if it is missing.

### `glob`

```python
BashTool.glob(pattern: str) -> list[str]
```

Return file paths matching a safe glob pattern.

### `fs`

```python
BashTool.fs() -> FileSystem
```

Return a live filesystem handle.

Each operation acquires the interpreter lock, so the handle always
reflects the latest state (including after ``reset()``).

Example:

```python
>>> tool = BashTool()
>>> tool.execute_sync("echo data > /out.txt")
>>> fs = tool.fs()
>>> fs.read_file("/out.txt")
b'data\n'
```

### `mount`

```python
BashTool.mount(vfs_path: str, fs: FileSystem) -> None
```

Mount an external filesystem at the given VFS path.

Example:

```python
>>> tool = BashTool()
>>> ext = FileSystem()
>>> ext.write_file("/info.txt", b"external")
>>> tool.mount("/mnt/ext", ext)
>>> result = tool.execute_sync("cat /mnt/ext/info.txt")
>>> result.stdout.strip()
'external'
```

### `unmount`

```python
BashTool.unmount(vfs_path: str) -> None
```

Unmount a previously mounted filesystem.

Example:

```python
>>> tool = BashTool()
>>> ext = FileSystem()
>>> tool.mount("/mnt/ext", ext)
>>> tool.unmount("/mnt/ext")
```

## ScriptedTool

Compose Python callbacks as bash builtins for multi-tool orchestration.

Each registered tool becomes a bash builtin command. An LLM (or user)
writes a single bash script that pipes, loops, and branches across tools.

### Fields

- **`name`** — `str`
- **`short_description`** — `str`
- **`version`** — `str`

### Constructor

```python
ScriptedTool(name: str, short_description: str | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None) -> None
```

Create a new ScriptedTool.

**Parameters:**

- **`name`** — Tool name (used as the LLM tool identifier).
- **`short_description`** — One-line description of the tool.
- **`max_commands`** — Limit total commands per execution.
- **`max_loop_iterations`** — Limit iterations per loop.

Example:

```python
>>> tool = ScriptedTool("data_pipeline", short_description="ETL tools")
>>> print(tool.name)
data_pipeline
```

### `add_tool`

```python
ScriptedTool.add_tool(name: str, description: str, callback: Callable[[dict[str, Any], str | None], str], schema: dict[str, Any] | None = None) -> None
```

Register a Python callback as a bash builtin command.

**Parameters:**

- **`name`** — Command name (becomes a bash builtin).
- **`description`** — Human-readable description of the sub-tool.
- **`callback`** — ``(params_dict, stdin_or_none) -> output_string`` or an async callback that resolves to one. Async callbacks run on the caller's active asyncio loop for ``await execute()`` and on a private loop for ``execute_sync()``.
- **`schema`** — Optional JSON Schema for the tool's parameters.

Example:

```python
>>> tool = ScriptedTool("math")
>>> tool.add_tool(
...     "add", "Add two numbers",
...     callback=lambda p, s=None: str(int(p["a"]) + int(p["b"])) + "\n",
...     schema={
...         "type": "object",
...         "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
...     },
... )
>>> result = tool.execute_sync("add --a 2 --b 3")
>>> result.stdout.strip()
'5'
```

### `env`

```python
ScriptedTool.env(key: str, value: str) -> None
```

Set an environment variable for subsequent executions.

Example:

```python
>>> tool = ScriptedTool("demo")
>>> tool.env("API_KEY", "secret-123")
>>> result = tool.execute_sync("echo $API_KEY")
>>> result.stdout.strip()
'secret-123'
```

### `execute`

```python
ScriptedTool.execute(commands: str) -> ExecResult
```

Execute commands asynchronously.

Async callbacks run on the caller's active asyncio loop.

Example:

```python
>>> tool = ScriptedTool("demo")
>>> tool.add_tool("hi", "Say hi", callback=lambda p, s=None: "hi\n")
>>> result = await tool.execute("hi")
>>> result.stdout.strip()
'hi'
```

### `execute_sync`

```python
ScriptedTool.execute_sync(commands: str) -> ExecResult
```

Execute commands synchronously (blocking).

Async callbacks run on a private loop here.

Example:

```python
>>> tool = ScriptedTool("demo")
>>> tool.add_tool("ping", "Ping", callback=lambda p, s=None: "pong\n")
>>> result = tool.execute_sync("ping")
>>> result.stdout.strip()
'pong'
```

### `tool_count`

```python
ScriptedTool.tool_count() -> int
```

Return the number of registered sub-tools.

Example:

```python
>>> tool = ScriptedTool("demo")
>>> tool.tool_count()
0
>>> tool.add_tool("a", "A", callback=lambda p, s=None: "")
>>> tool.tool_count()
1
```

### `description`

```python
ScriptedTool.description() -> str
```

Return the tool description for LLM consumption.

Example:

```python
>>> tool = ScriptedTool("api", short_description="API tools")
>>> desc = tool.description()
>>> len(desc) > 0
True
```

### `help`

```python
ScriptedTool.help() -> str
```

Return extended help text listing all registered sub-tools.

Example:

```python
>>> tool = ScriptedTool("api")
>>> tool.add_tool("fetch", "Fetch URL", callback=lambda p, s=None: "")
>>> "fetch" in tool.help()
True
```

### `system_prompt`

```python
ScriptedTool.system_prompt() -> str
```

Return the system prompt for LLM agents.

Includes descriptions of all registered sub-tools and usage examples.

Example:

```python
>>> tool = ScriptedTool("api")
>>> tool.add_tool("fetch", "Fetch URL", callback=lambda p, s=None: "")
>>> prompt = tool.system_prompt()
>>> "fetch" in prompt.lower()
True
```

### `input_schema`

```python
ScriptedTool.input_schema() -> str
```

Return the JSON Schema for tool input.

Example:

```python
>>> import json
>>> tool = ScriptedTool("api")
>>> schema = json.loads(tool.input_schema())
>>> "commands" in str(schema)
True
```

### `output_schema`

```python
ScriptedTool.output_schema() -> str
```

Return the JSON Schema for tool output.

Example:

```python
>>> import json
>>> tool = ScriptedTool("api")
>>> schema = json.loads(tool.output_schema())
>>> isinstance(schema, dict)
True
```

## FileSystem

Direct access to Bashkit's virtual filesystem or a standalone mountable FS.

Two ways to create:

1. In-memory (default) — starts empty:

```python
>>> fs = FileSystem()
>>> fs.write_file("/hello.txt", b"hi")
>>> fs.read_file("/hello.txt")
b'hi'
```


2. Backed by a real host directory:

```python
>>> fs = FileSystem.real("/tmp/data", writable=False)
>>> fs.exists("/some-host-file.txt")
True
```

### Constructor

```python
FileSystem() -> None
```

Create a new empty in-memory filesystem.

Example:

```python
>>> fs = FileSystem()
>>> fs.exists("/anything")
False
```

### `real`

```python
FileSystem.real(host_path: str, writable: bool = False) -> FileSystem
```

Create a filesystem backed by a real host directory.

**Parameters:**

- **`host_path`** — Absolute path on the host to expose.
- **`writable`** — Allow write operations (default read-only).

Example:

```python
>>> fs = FileSystem.real("/tmp/project", writable=True)
>>> fs.write_file("/tmp/project/out.txt", b"data")
```

### `from_capsule`

```python
FileSystem.from_capsule(capsule: Any) -> FileSystem
```

Create a filesystem from a ``PyCapsule`` exported by a native extension.

The capsule must wrap a ``bashkit.FileSystem.v1`` stable ABI handle.

### `to_capsule`

```python
FileSystem.to_capsule() -> Any
```

Export this filesystem as a stable-ABI ``PyCapsule`` for native extension interop.

### `read_file`

```python
FileSystem.read_file(path: str) -> bytes
```

Read the entire contents of a file.

**Parameters:**

- **`path`** — Absolute path in the filesystem.

**Returns:** File contents as bytes.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/demo.txt", b"hello")
>>> fs.read_file("/demo.txt")
b'hello'
```

### `write_file`

```python
FileSystem.write_file(path: str, content: bytes) -> None
```

Write content to a file, creating or overwriting it.

**Parameters:**

- **`path`** — Absolute path in the filesystem.
- **`content`** — Data to write.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/output.txt", b"result data")
```

### `append_file`

```python
FileSystem.append_file(path: str, content: bytes) -> None
```

Append content to an existing file.

**Parameters:**

- **`path`** — Absolute path in the filesystem.
- **`content`** — Data to append.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/log.txt", b"line1\n")
>>> fs.append_file("/log.txt", b"line2\n")
>>> fs.read_file("/log.txt")
b'line1\nline2\n'
```

### `mkdir`

```python
FileSystem.mkdir(path: str, recursive: bool = False) -> None
```

Create a directory.

**Parameters:**

- **`path`** — Absolute path for the new directory.
- **`recursive`** — Create parent directories as needed.

Example:

```python
>>> fs = FileSystem()
>>> fs.mkdir("/a/b/c", recursive=True)
>>> fs.exists("/a/b/c")
True
```

### `remove`

```python
FileSystem.remove(path: str, recursive: bool = False) -> None
```

Remove a file or directory.

**Parameters:**

- **`path`** — Absolute path to remove.
- **`recursive`** — Remove directory contents recursively.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/tmp.txt", b"x")
>>> fs.remove("/tmp.txt")
>>> fs.exists("/tmp.txt")
False
```

### `stat`

```python
FileSystem.stat(path: str) -> dict[str, Any]
```

Get file metadata.

**Returns:** Dict with ``file_type``, ``size``, ``mode``, ``modified``, ``created``.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/f.txt", b"data")
>>> info = fs.stat("/f.txt")
>>> info["file_type"]
'file'
>>> info["size"]
4
```

### `read_dir`

```python
FileSystem.read_dir(path: str) -> list[dict[str, Any]]
```

List directory entries.

**Returns:** List of dicts, each with ``name`` and ``metadata`` keys.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/dir/a.txt", b"a")
>>> entries = fs.read_dir("/dir")
>>> entries[0]["name"]
'a.txt'
```

### `exists`

```python
FileSystem.exists(path: str) -> bool
```

Check whether a path exists.

Example:

```python
>>> fs = FileSystem()
>>> fs.exists("/nope")
False
>>> fs.write_file("/yes.txt", b"")
>>> fs.exists("/yes.txt")
True
```

### `rename`

```python
FileSystem.rename(from_path: str, to_path: str) -> None
```

Rename (move) a file or directory.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/old.txt", b"data")
>>> fs.rename("/old.txt", "/new.txt")
>>> fs.exists("/new.txt")
True
```

### `copy`

```python
FileSystem.copy(from_path: str, to_path: str) -> None
```

Copy a file.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/src.txt", b"data")
>>> fs.copy("/src.txt", "/dst.txt")
>>> fs.read_file("/dst.txt")
b'data'
```

### `symlink`

```python
FileSystem.symlink(target: str, link: str) -> None
```

Create a symbolic link.

**Parameters:**

- **`target`** — Path the symlink points to.
- **`link`** — Path of the symlink itself.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/real.txt", b"data")
>>> fs.symlink("/real.txt", "/link.txt")
>>> fs.read_file("/link.txt")
b'data'
```

### `chmod`

```python
FileSystem.chmod(path: str, mode: int) -> None
```

Change file permissions.

**Parameters:**

- **`path`** — Absolute path.
- **`mode`** — Octal permission bits (e.g. ``0o755``).

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/script.sh", b"#!/bin/bash")
>>> fs.chmod("/script.sh", 0o755)
```

### `read_link`

```python
FileSystem.read_link(path: str) -> str
```

Read the target of a symbolic link.

Example:

```python
>>> fs = FileSystem()
>>> fs.write_file("/target.txt", b"data")
>>> fs.symlink("/target.txt", "/link.txt")
>>> fs.read_link("/link.txt")
'/target.txt'
```

## ExecResult

Result from executing bash commands.

Example:

```python
>>> bash = Bash()
>>> result = bash.execute_sync("echo hello")
>>> result.success
True
>>> result.stdout
'hello\n'
>>> result.exit_code
0
```

### Fields

- **`stdout`** — `str`
- **`stderr`** — `str`
- **`exit_code`** — `int`
- **`error`** — `str | None`
- **`success`** — `bool`

### `to_dict`

```python
ExecResult.to_dict() -> dict[str, Any]
```

Convert result to a plain dictionary.

**Returns:** Dict with ``stdout``, ``stderr``, ``exit_code``, ``error``, ``stdout_truncated``, ``stderr_truncated``, ``final_env``.

Example:

```python
>>> bash = Bash()
>>> result = bash.execute_sync("echo hi")
>>> d = result.to_dict()
>>> d["stdout"]
'hi\n'
>>> d["exit_code"]
0
```

## ShellState

Read-only snapshot of shell state.

Returned by ``Bash.shell_state()`` and ``BashTool.shell_state()`` for
prompt rendering and state inspection. This is a Python-friendly
inspection view, not a full-fidelity Rust ``ShellState`` mirror.
Mapping fields are immutable views. Use
``snapshot(exclude_filesystem=True)`` when you need shell-only restore
bytes. Transient fields like ``last_exit_code`` and ``traps`` reflect the
captured snapshot, but the next top-level ``execute()`` / ``execute_sync()``
clears them before running a new command.

### Fields

- **`env`** — `Mapping[str, str]`
- **`variables`** — `Mapping[str, str]`
- **`arrays`** — `Mapping[str, Mapping[int, str]]`
- **`assoc_arrays`** — `Mapping[str, Mapping[str, str]]`
- **`cwd`** — `str`
- **`last_exit_code`** — `int`
- **`aliases`** — `Mapping[str, str]`
- **`traps`** — `Mapping[str, str]`

## BuiltinContext

Invocation context for a custom builtin callback.

### Fields

- **`name`** — `str`
- **`argv`** — `list[str]`
- **`stdin`** — `str | None`
- **`env`** — `dict[str, str]`
- **`cwd`** — `str`
- **`fs`** — `FileSystem`

## BuiltinResult

Shell-facing result for a custom builtin callback.

### Fields

- **`stdout`** — `str`
- **`stderr`** — `str`
- **`exit_code`** — `int`

### Constructor

```python
BuiltinResult(stdout: str = '', stderr: str = '', exit_code: int = 0) -> None
```

## BashError

Exception raised when a bash command exits with non-zero status.

Example:

```python
>>> bash = Bash()
>>> try:
...     bash.execute_sync_or_throw("exit 42")
... except BashError as e:
...     print(e.exit_code)
42
```

### Fields

- **`exit_code`** — `int`
- **`stderr`** — `str`
- **`stdout`** — `str`

## create_langchain_tool_spec()

```python
create_langchain_tool_spec() -> dict[str, Any]
```

Create a LangChain-compatible tool specification.

**Returns:** Dict with name, description, and args_schema.

Example:

```python
>>> spec = create_langchain_tool_spec()
>>> spec["name"]
'bash'
```

## get_version()

```python
get_version() -> str
```

Get the bashkit version string.

Example:

```python
>>> version = get_version()
>>> isinstance(version, str)
True
```

---

# Framework integrations

## `bashkit.langchain`

LangChain integration for Bashkit.

### `Field`

```python
bashkit.langchain.Field(*args, **kwargs)
```

### `PrivateAttr`

```python
bashkit.langchain.PrivateAttr(*args, **kwargs)
```

### `create_bash_tool`

```python
bashkit.langchain.create_bash_tool(username: str | None = None, hostname: str | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, timeout_seconds: float | None = None, files: dict[str, str | Callable[[], str]] | None = None, mounts: list[dict[str, Any]] | None = None, allowed_mount_paths: list[str] | None = None, readonly_filesystem: bool = False, max_output_length: int = 100000) -> BashkitTool
```

Create a LangChain-compatible Bashkit tool.

**Parameters:**

- **`username`** — Custom username for sandbox
- **`hostname`** — Custom hostname for sandbox
- **`max_commands`** — Max commands to execute
- **`max_loop_iterations`** — Max loop iterations
- **`timeout_seconds`** — Execution timeout in seconds. When set, commands that exceed this duration are aborted with exit code 124.
- **`files`** — Static VFS file mounts keyed by sandbox path.
- **`mounts`** — Real host directory mounts exposed inside the sandbox. Mounts are read-only by default; pass ``{"writable": True}`` on a mount config to allow writes.
- **`allowed_mount_paths`** — Host path prefixes allowed for real filesystem mounts. Required when mounting sensitive host locations such as paths under a user home directory.
- **`readonly_filesystem`** — Deny all filesystem mutations after configured files and mounts are applied.
- **`max_output_length`** — Maximum number of characters returned to the LangChain agent from one bash tool call before truncation.

**Returns:** BashkitTool instance for use with LangChain agents

**Raises:**

- **`ImportError`** — If langchain-core is not installed

### `create_scripted_tool`

```python
bashkit.langchain.create_scripted_tool(scripted_tool: NativeScriptedTool) -> ScriptedToolLangChain
```

Create a LangChain-compatible tool from a configured ScriptedTool.

**Parameters:**

- **`scripted_tool`** — A ScriptedTool with registered tool callbacks

**Returns:** ScriptedToolLangChain instance for use with LangChain agents

**Raises:**

- **`ImportError`** — If langchain-core is not installed

## `bashkit.pydantic_ai`

PydanticAI integration for Bashkit.

### `create_bash_tool`

```python
bashkit.pydantic_ai.create_bash_tool(username: str | None = None, hostname: str | None = None, max_commands: int | None = None, max_loop_iterations: int | None = None, timeout_seconds: float | None = None, max_output_length: int = 100000) -> Tool
```

Create a PydanticAI Tool wrapping Bashkit.

**Parameters:**

- **`username`** — Custom username for sandbox
- **`hostname`** — Custom hostname for sandbox
- **`max_commands`** — Max commands to execute
- **`max_loop_iterations`** — Max loop iterations
- **`timeout_seconds`** — Execution timeout in seconds. When set, commands that exceed this duration are aborted with exit code 124.

**Returns:** Tool for use with ``Agent(tools=[...])``

**Raises:**

- **`ImportError`** — If pydantic-ai is not installed

## `bashkit.deepagents`

Deep Agents integration for Bashkit.

### `create_bash_middleware`

```python
bashkit.deepagents.create_bash_middleware(**kwargs) -> BashkitMiddleware
```

Create BashkitMiddleware for Deep Agents.

### `create_bashkit_backend`

```python
bashkit.deepagents.create_bashkit_backend(**kwargs) -> BashkitBackend
```

Create BashkitBackend for Deep Agents.
