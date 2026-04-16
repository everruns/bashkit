"""Type stubs for bashkit native module."""

from collections.abc import Awaitable, Callable, Mapping
from typing import Any, Protocol

# Synchronous chunk callback for live stdout/stderr streaming.
OutputHandler = Callable[[str, str], None]

class BuiltinContext:
    """Invocation context for a custom builtin callback.

    Attributes:
        name: Builtin command name.
        argv: Raw argv tokens after shell parsing, excluding the command name.
        stdin: Pipeline input from the previous command, if any.
        env: Environment variables visible to the builtin.
        cwd: Current working directory at invocation time.
    """

    name: str
    argv: list[str]
    stdin: str | None
    env: dict[str, str]
    cwd: str

BuiltinCallback = Callable[[BuiltinContext], str | Awaitable[str]]

class FileSystem:
    """Direct access to BashKit's virtual filesystem or a standalone mountable FS.

    Two ways to create:

    1. In-memory (default) — starts empty::

        >>> fs = FileSystem()
        >>> fs.write_file("/hello.txt", b"hi")
        >>> fs.read_file("/hello.txt")
        b'hi'

    2. Backed by a real host directory::

        >>> fs = FileSystem.real("/tmp/data", writable=False)
        >>> fs.exists("/some-host-file.txt")
        True
    """

    def __init__(self) -> None:
        """Create a new empty in-memory filesystem.

        Example::

            >>> fs = FileSystem()
            >>> fs.exists("/anything")
            False
        """
        ...

    @staticmethod
    def real(host_path: str, writable: bool = False) -> FileSystem:
        """Create a filesystem backed by a real host directory.

        Args:
            host_path: Absolute path on the host to expose.
            writable: Allow write operations (default read-only).

        Example::

            >>> fs = FileSystem.real("/tmp/project", writable=True)
            >>> fs.write_file("/tmp/project/out.txt", b"data")
        """
        ...

    def read_file(self, path: str) -> bytes:
        """Read the entire contents of a file.

        Args:
            path: Absolute path in the filesystem.

        Returns:
            File contents as bytes.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/demo.txt", b"hello")
            >>> fs.read_file("/demo.txt")
            b'hello'
        """
        ...

    def write_file(self, path: str, content: bytes) -> None:
        """Write content to a file, creating or overwriting it.

        Args:
            path: Absolute path in the filesystem.
            content: Data to write.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/output.txt", b"result data")
        """
        ...

    def append_file(self, path: str, content: bytes) -> None:
        """Append content to an existing file.

        Args:
            path: Absolute path in the filesystem.
            content: Data to append.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/log.txt", b"line1\\n")
            >>> fs.append_file("/log.txt", b"line2\\n")
            >>> fs.read_file("/log.txt")
            b'line1\\nline2\\n'
        """
        ...

    def mkdir(self, path: str, recursive: bool = False) -> None:
        """Create a directory.

        Args:
            path: Absolute path for the new directory.
            recursive: Create parent directories as needed.

        Example::

            >>> fs = FileSystem()
            >>> fs.mkdir("/a/b/c", recursive=True)
            >>> fs.exists("/a/b/c")
            True
        """
        ...

    def remove(self, path: str, recursive: bool = False) -> None:
        """Remove a file or directory.

        Args:
            path: Absolute path to remove.
            recursive: Remove directory contents recursively.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/tmp.txt", b"x")
            >>> fs.remove("/tmp.txt")
            >>> fs.exists("/tmp.txt")
            False
        """
        ...

    def stat(self, path: str) -> dict[str, Any]:
        """Get file metadata.

        Returns:
            Dict with ``file_type``, ``size``, ``mode``, ``modified``, ``created``.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/f.txt", b"data")
            >>> info = fs.stat("/f.txt")
            >>> info["file_type"]
            'file'
            >>> info["size"]
            4
        """
        ...

    def read_dir(self, path: str) -> list[dict[str, Any]]:
        """List directory entries.

        Returns:
            List of dicts, each with ``name`` and ``metadata`` keys.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/dir/a.txt", b"a")
            >>> entries = fs.read_dir("/dir")
            >>> entries[0]["name"]
            'a.txt'
        """
        ...

    def exists(self, path: str) -> bool:
        """Check whether a path exists.

        Example::

            >>> fs = FileSystem()
            >>> fs.exists("/nope")
            False
            >>> fs.write_file("/yes.txt", b"")
            >>> fs.exists("/yes.txt")
            True
        """
        ...

    def rename(self, from_path: str, to_path: str) -> None:
        """Rename (move) a file or directory.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/old.txt", b"data")
            >>> fs.rename("/old.txt", "/new.txt")
            >>> fs.exists("/new.txt")
            True
        """
        ...

    def copy(self, from_path: str, to_path: str) -> None:
        """Copy a file.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/src.txt", b"data")
            >>> fs.copy("/src.txt", "/dst.txt")
            >>> fs.read_file("/dst.txt")
            b'data'
        """
        ...

    def symlink(self, target: str, link: str) -> None:
        """Create a symbolic link.

        Args:
            target: Path the symlink points to.
            link: Path of the symlink itself.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/real.txt", b"data")
            >>> fs.symlink("/real.txt", "/link.txt")
            >>> fs.read_file("/link.txt")
            b'data'
        """
        ...

    def chmod(self, path: str, mode: int) -> None:
        """Change file permissions.

        Args:
            path: Absolute path.
            mode: Octal permission bits (e.g. ``0o755``).

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/script.sh", b"#!/bin/bash")
            >>> fs.chmod("/script.sh", 0o755)
        """
        ...

    def read_link(self, path: str) -> str:
        """Read the target of a symbolic link.

        Example::

            >>> fs = FileSystem()
            >>> fs.write_file("/target.txt", b"data")
            >>> fs.symlink("/target.txt", "/link.txt")
            >>> fs.read_link("/link.txt")
            '/target.txt'
        """
        ...

class ExternalHandler(Protocol):
    """Protocol for the external function handler passed to Bash.

    Called when Monty Python code invokes a registered external function.
    Must be an async callable with this exact signature.

    Example::

        >>> async def my_handler(fn_name: str, args: list, kwargs: dict) -> Any:
        ...     if fn_name == "fetch":
        ...         return {"status": 200, "body": "ok"}
        ...     return None
        >>> bash = Bash(
        ...     python=True,
        ...     external_functions=["fetch"],
        ...     external_handler=my_handler,
        ... )
    """

    async def __call__(self, fn_name: str, args: list[Any], kwargs: dict[str, Any]) -> Any: ...

class ShellState:
    """Read-only snapshot of shell state.

    Returned by ``Bash.shell_state()`` and ``BashTool.shell_state()`` for
    prompt rendering and state inspection. Mapping fields are immutable views.
    Transient fields like ``last_exit_code`` and ``traps`` reflect the captured
    snapshot, but the next top-level ``execute()`` / ``execute_sync()`` clears
    them before running a new command.
    """

    @property
    def env(self) -> Mapping[str, str]: ...
    @property
    def variables(self) -> Mapping[str, str]: ...
    @property
    def arrays(self) -> Mapping[str, Mapping[int, str]]: ...
    @property
    def assoc_arrays(self) -> Mapping[str, Mapping[str, str]]: ...
    @property
    def cwd(self) -> str: ...
    @property
    def last_exit_code(self) -> int: ...
    @property
    def aliases(self) -> Mapping[str, str]: ...
    @property
    def traps(self) -> Mapping[str, str]: ...

class Bash:
    """Core bash interpreter with virtual filesystem.

    State persists between calls — files created in one execute() are
    available in subsequent calls.

    Example (basic):
        >>> bash = Bash()
        >>> result = await bash.execute("echo 'Hello!'")
        >>> print(result.stdout)
        Hello!

    Example (Python execution with external function handler):
        >>> async def handler(fn_name: str, args: list, kwargs: dict) -> Any:
        ...     return await tool_executor.call(fn_name, kwargs)
        >>> bash = Bash(
        ...     python=True,
        ...     external_functions=["api_request"],
        ...     external_handler=handler,
        ... )
        >>> result = await bash.execute("python3 -c 'print(api_request(url=\"/data\"))'")
    """

    def __init__(
        self,
        username: str | None = None,
        hostname: str | None = None,
        max_commands: int | None = None,
        max_loop_iterations: int | None = None,
        max_memory: int | None = None,
        timeout_seconds: float | None = None,
        python: bool = False,
        external_functions: list[str] | None = None,
        external_handler: ExternalHandler | None = None,
        files: dict[str, str | Callable[[], str]] | None = None,
        mounts: list[dict[str, Any]] | None = None,
        custom_builtins: dict[str, BuiltinCallback] | None = None,
    ) -> None:
        """Create a new Bash interpreter.

        Args:
            username: Custom username (default ``"user"``).
            hostname: Custom hostname (default ``"bashkit"``).
            max_commands: Limit total commands executed.
            max_loop_iterations: Limit iterations per loop.
            max_memory: Memory limit in bytes for the VFS.
            timeout_seconds: Abort execution after this duration.
            python: Enable embedded Python (``python3`` builtin).
            external_functions: Function names callable from Python code.
            external_handler: Async callback for external function calls.
            files: Dict mapping VFS paths to file contents or lazy callables.
            mounts: List of real host directory mount configs.
            custom_builtins: Constructor-time Python callbacks exposed as
                bash builtins. Each callback receives a ``BuiltinContext``
                with raw ``argv`` tokens and optional pipeline ``stdin``,
                and must return a stdout string or await one. Async callbacks
                run on the caller's active asyncio loop for ``await execute()``
                and on a private loop for ``execute_sync()``.

        Example::

            >>> bash = Bash(
            ...     timeout_seconds=30,
            ...     files={"/input.txt": "some data"},
            ...     custom_builtins={"ping": lambda ctx: "pong\\n"},
            ... )
        """
        ...

    async def execute(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute bash commands asynchronously.

        Args:
            commands: Bash script to run (like ``bash -c "commands"``).
            on_output: Optional callback receiving chunked ``(stdout, stderr)``
                pairs during execution. Must be synchronous.

        Async ``custom_builtins`` callbacks run on the caller's active asyncio
        loop.

        Returns:
            ExecResult with stdout, stderr, exit_code.

        Example::

            >>> bash = Bash()
            >>> result = await bash.execute("echo hello && echo world")
            >>> print(result.stdout)
            hello
            world
        """
        ...

    def execute_sync(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute bash commands synchronously (blocking).

        Not supported when ``external_handler`` is configured — use
        ``execute()`` (async) instead. ``on_output`` must be synchronous.
        Async ``custom_builtins`` callbacks run on a private loop here.

        Example::

            >>> bash = Bash()
            >>> result = bash.execute_sync("date +%Y")
            >>> print(result.exit_code)
            0
        """
        ...

    async def execute_or_throw(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute commands asynchronously; raise ``BashError`` on non-zero exit.

        ``on_output`` must be synchronous.

        Example::

            >>> bash = Bash()
            >>> result = await bash.execute_or_throw("echo ok")
            >>> # Raises BashError if the command fails:
            >>> await bash.execute_or_throw("false")  # doctest: +SKIP
            Traceback (most recent call last):
                ...
            BashError: ...
        """
        ...

    def execute_sync_or_throw(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute commands synchronously; raise ``BashError`` on non-zero exit.

        ``on_output`` must be synchronous.

        Example::

            >>> bash = Bash()
            >>> result = bash.execute_sync_or_throw("echo ok")
            >>> print(result.stdout.strip())
            ok
        """
        ...

    def cancel(self) -> None:
        """Cancel the currently running execution.

        Safe to call from any thread. Execution aborts at the next
        command boundary.

        Example::

            >>> import threading
            >>> bash = Bash()
            >>> threading.Timer(1.0, bash.cancel).start()
            >>> # Long-running command will be cancelled after 1 second
        """
        ...

    def clear_cancel(self) -> None:
        """Clear the cancellation flag so subsequent executions proceed normally.

        Call this after a ``cancel()`` once the in-flight execution has
        finished and you want to reuse the same ``Bash`` instance
        (preserving VFS state). Without this, every future ``execute()``
        will immediately fail with ``"execution cancelled"``.

        **Note:** Calling this while an execution is still in-flight may
        allow that execution to continue past the cancellation point.
        Wait for the cancelled execution to finish before clearing
        (await the async call or let ``execute_sync`` return).

        Example::

            >>> bash = Bash()
            >>> bash.cancel()
            >>> bash.clear_cancel()
            >>> result = bash.execute_sync("echo ok")
            >>> result.exit_code
            0
        """
        ...

    def reset(self) -> None:
        """Reset interpreter to initial state.

        Clears all VFS contents, environment variables, and shell state.
        Re-applies the original ``files``, ``mounts``, and
        ``custom_builtins`` configuration.

        Example::

            >>> bash = Bash()
            >>> bash.execute_sync("echo hi > /tmp/file.txt")
            >>> bash.reset()
            >>> result = bash.execute_sync("cat /tmp/file.txt")
            >>> result.exit_code  # file is gone after reset
            1
        """
        ...

    def snapshot(self, exclude_filesystem: bool = False) -> bytes:
        """Serialize interpreter state to bytes."""
        ...

    def shell_state(self) -> ShellState:
        """Capture a read-only shell-state snapshot."""
        ...

    def restore_shell_state(self, state: ShellState) -> None:
        """Restore shell state from a previous ``shell_state()`` capture.

        Restores the captured shell-state object. The next top-level
        ``execute()`` / ``execute_sync()`` still clears transient fields like
        ``last_exit_code`` and ``traps`` before running the new command.
        """
        ...

    def restore_snapshot(self, data: bytes) -> None:
        """Restore interpreter state from bytes produced by ``snapshot()``."""
        ...

    @staticmethod
    def from_snapshot(
        data: bytes,
        username: str | None = None,
        hostname: str | None = None,
        max_commands: int | None = None,
        max_loop_iterations: int | None = None,
        max_memory: int | None = None,
        timeout_seconds: float | None = None,
        python: bool = False,
        external_functions: list[str] | None = None,
        external_handler: ExternalHandler | None = None,
        files: dict[str, str] | None = None,
        mounts: list[dict[str, Any]] | None = None,
        custom_builtins: dict[str, BuiltinCallback] | None = None,
    ) -> Bash:
        """Create a new ``Bash`` from snapshot bytes and optional constructor kwargs."""
        ...

    def read_file(self, path: str) -> str:
        """Read a VFS file as UTF-8 text."""
        ...

    def write_file(self, path: str, content: str) -> None:
        """Write UTF-8 text into the VFS."""
        ...

    def append_file(self, path: str, content: str) -> None:
        """Append UTF-8 text to a VFS file."""
        ...

    def mkdir(self, path: str, recursive: bool = False) -> None:
        """Create a directory in the VFS."""
        ...

    def exists(self, path: str) -> bool:
        """Return whether a VFS path exists."""
        ...

    def remove(self, path: str, recursive: bool = False) -> None:
        """Remove a VFS file or directory."""
        ...

    def stat(self, path: str) -> dict[str, Any]:
        """Return metadata for a VFS path."""
        ...

    def chmod(self, path: str, mode: int) -> None:
        """Change VFS permissions for a path."""
        ...

    def symlink(self, target: str, link: str) -> None:
        """Create a symlink in the VFS."""
        ...

    def read_link(self, path: str) -> str:
        """Return the symlink target for a VFS path."""
        ...

    def read_dir(self, path: str) -> list[dict[str, Any]]:
        """Return directory entries with metadata."""
        ...

    def ls(self, path: str = ".") -> list[str]:
        """Return entry names for a directory, or an empty list if it is missing."""
        ...

    def glob(self, pattern: str) -> list[str]:
        """Return file paths matching a safe glob pattern."""
        ...

    def fs(self) -> FileSystem:
        """Return a live filesystem handle.

        Each operation acquires the interpreter lock, so the handle always
        reflects the latest state (including after ``reset()``).

        Example::

            >>> bash = Bash()
            >>> bash.execute_sync("echo hello > /greeting.txt")
            >>> fs = bash.fs()
            >>> fs.read_file("/greeting.txt")
            b'hello\\n'
        """
        ...

    def mount(self, vfs_path: str, fs: FileSystem) -> None:
        """Mount an external filesystem at the given VFS path.

        Args:
            vfs_path: Mount point inside the VFS.
            fs: FileSystem instance to mount.

        Example::

            >>> bash = Bash()
            >>> overlay = FileSystem()
            >>> overlay.write_file("/data.csv", b"a,b,c")
            >>> bash.mount("/mnt/data", overlay)
            >>> result = bash.execute_sync("cat /mnt/data/data.csv")
            >>> print(result.stdout)
            a,b,c
        """
        ...

    def unmount(self, vfs_path: str) -> None:
        """Unmount a previously mounted filesystem.

        Example::

            >>> bash = Bash()
            >>> overlay = FileSystem()
            >>> bash.mount("/mnt/ext", overlay)
            >>> bash.unmount("/mnt/ext")
        """
        ...

class ExecResult:
    """Result from executing bash commands.

    Example::

        >>> bash = Bash()
        >>> result = bash.execute_sync("echo hello")
        >>> result.success
        True
        >>> result.stdout
        'hello\\n'
        >>> result.exit_code
        0
    """

    stdout: str
    stderr: str
    exit_code: int
    error: str | None
    success: bool

    def to_dict(self) -> dict[str, Any]:
        """Convert result to a plain dictionary.

        Returns:
            Dict with ``stdout``, ``stderr``, ``exit_code``, ``error``,
            ``stdout_truncated``, ``stderr_truncated``, ``final_env``.

        Example::

            >>> bash = Bash()
            >>> result = bash.execute_sync("echo hi")
            >>> d = result.to_dict()
            >>> d["stdout"]
            'hi\\n'
            >>> d["exit_code"]
            0
        """
        ...

class BashTool:
    """Sandboxed bash interpreter for AI agents.

    BashTool provides a safe execution environment for running bash commands
    with a virtual filesystem. All file operations are contained within the
    sandbox - no access to the real filesystem.

    Adds LLM-facing contract metadata (``description``, ``system_prompt``,
    ``input_schema``, ``output_schema``) on top of the core interpreter.

    Example:
        >>> tool = BashTool()
        >>> result = await tool.execute("echo 'Hello!'")
        >>> print(result.stdout)
        Hello!
    """

    name: str
    short_description: str
    version: str

    def __init__(
        self,
        username: str | None = None,
        hostname: str | None = None,
        max_commands: int | None = None,
        max_loop_iterations: int | None = None,
        max_memory: int | None = None,
        timeout_seconds: float | None = None,
        files: dict[str, str | Callable[[], str]] | None = None,
        mounts: list[dict[str, Any]] | None = None,
        custom_builtins: dict[str, BuiltinCallback] | None = None,
    ) -> None:
        """Create a new BashTool.

        Args:
            username: Custom username (default ``"user"``).
            hostname: Custom hostname (default ``"bashkit"``).
            max_commands: Limit total commands executed.
            max_loop_iterations: Limit iterations per loop.
            max_memory: Memory limit in bytes for the VFS.
            timeout_seconds: Abort execution after this duration.
            files: Dict mapping VFS paths to file contents or lazy callables.
            mounts: List of real host directory mount configs.
            custom_builtins: Constructor-time Python callbacks exposed as
                bash builtins. Each callback receives a ``BuiltinContext``
                and must return a stdout string or await one. Async callbacks
                run on the caller's active asyncio loop for ``await execute()``
                and on a private loop for ``execute_sync()``.

        Example::

            >>> tool = BashTool(
            ...     timeout_seconds=30,
            ...     custom_builtins={"ping": lambda ctx: "pong\\n"},
            ... )
            >>> print(tool.name)
            bash
        """
        ...

    async def execute(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute bash commands asynchronously.

        Async ``custom_builtins`` callbacks run on the caller's active asyncio
        loop.

        ``on_output`` must be synchronous.

        Example::

            >>> tool = BashTool()
            >>> result = await tool.execute("ls /")
            >>> result.success
            True
        """
        ...

    def execute_sync(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute bash commands synchronously (blocking).

        Async ``custom_builtins`` callbacks run on a private loop here.

        ``on_output`` must be synchronous.

        Example::

            >>> tool = BashTool()
            >>> result = tool.execute_sync("echo 42")
            >>> result.stdout.strip()
            '42'
        """
        ...

    async def execute_or_throw(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute commands asynchronously; raise ``BashError`` on non-zero exit.

        ``on_output`` must be synchronous.

        Example::

            >>> tool = BashTool()
            >>> result = await tool.execute_or_throw("echo ok")
            >>> result.success
            True
        """
        ...

    def execute_sync_or_throw(self, commands: str, on_output: OutputHandler | None = None) -> ExecResult:
        """Execute commands synchronously; raise ``BashError`` on non-zero exit.

        ``on_output`` must be synchronous.

        Example::

            >>> tool = BashTool()
            >>> result = tool.execute_sync_or_throw("echo ok")
            >>> result.stdout.strip()
            'ok'
        """
        ...

    def cancel(self) -> None:
        """Cancel the currently running execution.

        Safe to call from any thread.

        Example::

            >>> tool = BashTool()
            >>> tool.cancel()  # no-op if nothing is running
        """
        ...

    def clear_cancel(self) -> None:
        """Clear the cancellation flag so subsequent executions proceed normally.

        Call this after a ``cancel()`` once the in-flight execution has
        finished and you want to reuse the same ``BashTool`` instance
        (preserving VFS state). Without this, every future ``execute()``
        will immediately fail with ``"execution cancelled"``.

        **Note:** Calling this while an execution is still in-flight may
        allow that execution to continue past the cancellation point.
        Wait for the cancelled execution to finish before clearing
        (await the async call or let ``execute_sync`` return).

        Example::

            >>> tool = BashTool()
            >>> tool.cancel()
            >>> tool.clear_cancel()
            >>> result = tool.execute_sync("echo ok")
            >>> result.exit_code
            0
        """
        ...

    def description(self) -> str:
        """Return the tool description for LLM consumption.

        Example::

            >>> tool = BashTool()
            >>> desc = tool.description()
            >>> "bash" in desc.lower()
            True
        """
        ...

    def help(self) -> str:
        """Return extended help text.

        Example::

            >>> tool = BashTool()
            >>> help_text = tool.help()
            >>> len(help_text) > 0
            True
        """
        ...

    def system_prompt(self) -> str:
        """Return the system prompt for LLM agents.

        Includes tool description, usage guidelines, and capabilities.

        Example::

            >>> tool = BashTool()
            >>> prompt = tool.system_prompt()
            >>> "sandbox" in prompt.lower() or "bash" in prompt.lower()
            True
        """
        ...

    def input_schema(self) -> str:
        """Return the JSON Schema for tool input.

        Example::

            >>> import json
            >>> tool = BashTool()
            >>> schema = json.loads(tool.input_schema())
            >>> "commands" in str(schema)
            True
        """
        ...

    def output_schema(self) -> str:
        """Return the JSON Schema for tool output.

        Example::

            >>> import json
            >>> tool = BashTool()
            >>> schema = json.loads(tool.output_schema())
            >>> isinstance(schema, dict)
            True
        """
        ...

    def reset(self) -> None:
        """Reset the tool to initial state.

        Clears VFS, environment, and shell state while re-applying
        constructor-time ``custom_builtins``.

        Example::

            >>> tool = BashTool()
            >>> tool.execute_sync("touch /tmp/file")
            >>> tool.reset()
            >>> result = tool.execute_sync("test -f /tmp/file")
            >>> result.exit_code  # file is gone
            1
        """
        ...

    def snapshot(self, exclude_filesystem: bool = False) -> bytes:
        """Serialize interpreter state to bytes."""
        ...

    def shell_state(self) -> ShellState:
        """Capture a read-only shell-state snapshot."""
        ...

    def restore_shell_state(self, state: ShellState) -> None:
        """Restore shell state from a previous ``shell_state()`` capture.

        Restores the captured shell-state object. The next top-level
        ``execute()`` / ``execute_sync()`` still clears transient fields like
        ``last_exit_code`` and ``traps`` before running the new command.
        """
        ...

    def restore_snapshot(self, data: bytes) -> None:
        """Restore interpreter state from bytes produced by ``snapshot()``."""
        ...

    @staticmethod
    def from_snapshot(
        data: bytes,
        username: str | None = None,
        hostname: str | None = None,
        max_commands: int | None = None,
        max_loop_iterations: int | None = None,
        max_memory: int | None = None,
        timeout_seconds: float | None = None,
        files: dict[str, str] | None = None,
        mounts: list[dict[str, Any]] | None = None,
        custom_builtins: dict[str, BuiltinCallback] | None = None,
    ) -> BashTool:
        """Create a new ``BashTool`` from snapshot bytes and optional constructor kwargs."""
        ...

    def read_file(self, path: str) -> str:
        """Read a VFS file as UTF-8 text."""
        ...

    def write_file(self, path: str, content: str) -> None:
        """Write UTF-8 text into the VFS."""
        ...

    def append_file(self, path: str, content: str) -> None:
        """Append UTF-8 text to a VFS file."""
        ...

    def mkdir(self, path: str, recursive: bool = False) -> None:
        """Create a directory in the VFS."""
        ...

    def exists(self, path: str) -> bool:
        """Return whether a VFS path exists."""
        ...

    def remove(self, path: str, recursive: bool = False) -> None:
        """Remove a VFS file or directory."""
        ...

    def stat(self, path: str) -> dict[str, Any]:
        """Return metadata for a VFS path."""
        ...

    def chmod(self, path: str, mode: int) -> None:
        """Change VFS permissions for a path."""
        ...

    def symlink(self, target: str, link: str) -> None:
        """Create a symlink in the VFS."""
        ...

    def read_link(self, path: str) -> str:
        """Return the symlink target for a VFS path."""
        ...

    def read_dir(self, path: str) -> list[dict[str, Any]]:
        """Return directory entries with metadata."""
        ...

    def ls(self, path: str = ".") -> list[str]:
        """Return entry names for a directory, or an empty list if it is missing."""
        ...

    def glob(self, pattern: str) -> list[str]:
        """Return file paths matching a safe glob pattern."""
        ...

    def fs(self) -> FileSystem:
        """Return a live filesystem handle.

        Each operation acquires the interpreter lock, so the handle always
        reflects the latest state (including after ``reset()``).

        Example::

            >>> tool = BashTool()
            >>> tool.execute_sync("echo data > /out.txt")
            >>> fs = tool.fs()
            >>> fs.read_file("/out.txt")
            b'data\\n'
        """
        ...

    def mount(self, vfs_path: str, fs: FileSystem) -> None:
        """Mount an external filesystem at the given VFS path.

        Example::

            >>> tool = BashTool()
            >>> ext = FileSystem()
            >>> ext.write_file("/info.txt", b"external")
            >>> tool.mount("/mnt/ext", ext)
            >>> result = tool.execute_sync("cat /mnt/ext/info.txt")
            >>> result.stdout.strip()
            'external'
        """
        ...

    def unmount(self, vfs_path: str) -> None:
        """Unmount a previously mounted filesystem.

        Example::

            >>> tool = BashTool()
            >>> ext = FileSystem()
            >>> tool.mount("/mnt/ext", ext)
            >>> tool.unmount("/mnt/ext")
        """
        ...

class ScriptedTool:
    """Compose Python callbacks as bash builtins for multi-tool orchestration.

    Each registered tool becomes a bash builtin command. An LLM (or user)
    writes a single bash script that pipes, loops, and branches across tools.

    Example:
        >>> tool = ScriptedTool("api")
        >>> tool.add_tool("greet", "Greet user",
        ...     callback=lambda p, s=None: f"hello {p.get('name', 'world')}\\n",
        ...     schema={"type": "object", "properties": {"name": {"type": "string"}}})
        >>> result = tool.execute_sync("greet --name Alice")
        >>> print(result.stdout.strip())
        hello Alice
    """

    name: str
    short_description: str
    version: str

    def __init__(
        self,
        name: str,
        short_description: str | None = None,
        max_commands: int | None = None,
        max_loop_iterations: int | None = None,
    ) -> None:
        """Create a new ScriptedTool.

        Args:
            name: Tool name (used as the LLM tool identifier).
            short_description: One-line description of the tool.
            max_commands: Limit total commands per execution.
            max_loop_iterations: Limit iterations per loop.

        Example::

            >>> tool = ScriptedTool("data_pipeline", short_description="ETL tools")
            >>> print(tool.name)
            data_pipeline
        """
        ...

    def add_tool(
        self,
        name: str,
        description: str,
        callback: Callable[[dict[str, Any], str | None], str],
        schema: dict[str, Any] | None = None,
    ) -> None:
        """Register a Python callback as a bash builtin command.

        Args:
            name: Command name (becomes a bash builtin).
            description: Human-readable description of the sub-tool.
            callback: ``(params_dict, stdin_or_none) -> output_string`` or
                an async callback that resolves to one. Async callbacks run on
                the caller's active asyncio loop for ``await execute()`` and on
                a private loop for ``execute_sync()``.
            schema: Optional JSON Schema for the tool's parameters.

        Example::

            >>> tool = ScriptedTool("math")
            >>> tool.add_tool(
            ...     "add", "Add two numbers",
            ...     callback=lambda p, s=None: str(int(p["a"]) + int(p["b"])) + "\\n",
            ...     schema={
            ...         "type": "object",
            ...         "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
            ...     },
            ... )
            >>> result = tool.execute_sync("add --a 2 --b 3")
            >>> result.stdout.strip()
            '5'
        """
        ...

    def env(self, key: str, value: str) -> None:
        """Set an environment variable for subsequent executions.

        Example::

            >>> tool = ScriptedTool("demo")
            >>> tool.env("API_KEY", "secret-123")
            >>> result = tool.execute_sync("echo $API_KEY")
            >>> result.stdout.strip()
            'secret-123'
        """
        ...

    async def execute(self, commands: str) -> ExecResult:
        """Execute commands asynchronously.

        Async callbacks run on the caller's active asyncio loop.

        Example::

            >>> tool = ScriptedTool("demo")
            >>> tool.add_tool("hi", "Say hi", callback=lambda p, s=None: "hi\\n")
            >>> result = await tool.execute("hi")
            >>> result.stdout.strip()
            'hi'
        """
        ...

    def execute_sync(self, commands: str) -> ExecResult:
        """Execute commands synchronously (blocking).

        Async callbacks run on a private loop here.

        Example::

            >>> tool = ScriptedTool("demo")
            >>> tool.add_tool("ping", "Ping", callback=lambda p, s=None: "pong\\n")
            >>> result = tool.execute_sync("ping")
            >>> result.stdout.strip()
            'pong'
        """
        ...

    def tool_count(self) -> int:
        """Return the number of registered sub-tools.

        Example::

            >>> tool = ScriptedTool("demo")
            >>> tool.tool_count()
            0
            >>> tool.add_tool("a", "A", callback=lambda p, s=None: "")
            >>> tool.tool_count()
            1
        """
        ...

    def description(self) -> str:
        """Return the tool description for LLM consumption.

        Example::

            >>> tool = ScriptedTool("api", short_description="API tools")
            >>> desc = tool.description()
            >>> len(desc) > 0
            True
        """
        ...

    def help(self) -> str:
        """Return extended help text listing all registered sub-tools.

        Example::

            >>> tool = ScriptedTool("api")
            >>> tool.add_tool("fetch", "Fetch URL", callback=lambda p, s=None: "")
            >>> "fetch" in tool.help()
            True
        """
        ...

    def system_prompt(self) -> str:
        """Return the system prompt for LLM agents.

        Includes descriptions of all registered sub-tools and usage examples.

        Example::

            >>> tool = ScriptedTool("api")
            >>> tool.add_tool("fetch", "Fetch URL", callback=lambda p, s=None: "")
            >>> prompt = tool.system_prompt()
            >>> "fetch" in prompt.lower()
            True
        """
        ...

    def input_schema(self) -> str:
        """Return the JSON Schema for tool input.

        Example::

            >>> import json
            >>> tool = ScriptedTool("api")
            >>> schema = json.loads(tool.input_schema())
            >>> "commands" in str(schema)
            True
        """
        ...

    def output_schema(self) -> str:
        """Return the JSON Schema for tool output.

        Example::

            >>> import json
            >>> tool = ScriptedTool("api")
            >>> schema = json.loads(tool.output_schema())
            >>> isinstance(schema, dict)
            True
        """
        ...

class BashError(Exception):
    """Exception raised when a bash command exits with non-zero status.

    Example::

        >>> bash = Bash()
        >>> try:
        ...     bash.execute_sync_or_throw("exit 42")
        ... except BashError as e:
        ...     print(e.exit_code)
        42
    """

    exit_code: int
    stderr: str
    stdout: str

def create_langchain_tool_spec() -> dict[str, Any]:
    """Create a LangChain-compatible tool specification.

    Returns:
        Dict with name, description, and args_schema.

    Example::

        >>> spec = create_langchain_tool_spec()
        >>> spec["name"]
        'bash'
    """
    ...

def get_version() -> str:
    """Get the bashkit version string.

    Example::

        >>> version = get_version()
        >>> isinstance(version, str)
        True
    """
    ...
