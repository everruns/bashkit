"""Type stubs for bashkit native module."""

from collections.abc import Callable
from typing import Any, Protocol

class FileSystem:
    """Direct access to BashKit's virtual filesystem or a standalone mountable FS."""

    def __init__(self) -> None: ...
    @staticmethod
    def real(host_path: str, writable: bool = False) -> FileSystem: ...
    def read_file(self, path: str) -> bytes: ...
    def write_file(self, path: str, content: bytes) -> None: ...
    def append_file(self, path: str, content: bytes) -> None: ...
    def mkdir(self, path: str, recursive: bool = False) -> None: ...
    def remove(self, path: str, recursive: bool = False) -> None: ...
    def stat(self, path: str) -> dict[str, Any]: ...
    def read_dir(self, path: str) -> list[dict[str, Any]]: ...
    def exists(self, path: str) -> bool: ...
    def rename(self, from_path: str, to_path: str) -> None: ...
    def copy(self, from_path: str, to_path: str) -> None: ...
    def symlink(self, target: str, link: str) -> None: ...
    def chmod(self, path: str, mode: int) -> None: ...
    def read_link(self, path: str) -> str: ...

class ExternalHandler(Protocol):
    """Protocol for the external function handler passed to Bash.

    Called when Monty Python code invokes a registered external function.
    Must be an async callable with this exact signature.
    """

    async def __call__(self, fn_name: str, args: list[Any], kwargs: dict[str, Any]) -> Any: ...

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
        python: bool = False,
        external_functions: list[str] | None = None,
        external_handler: ExternalHandler | None = None,
        files: dict[str, str] | None = None,
        mounts: list[dict[str, Any]] | None = None,
    ) -> None: ...
    async def execute(self, commands: str) -> ExecResult: ...
    def execute_sync(self, commands: str) -> ExecResult: ...
    async def execute_or_throw(self, commands: str) -> ExecResult: ...
    def execute_sync_or_throw(self, commands: str) -> ExecResult: ...
    def cancel(self) -> None: ...
    def reset(self) -> None: ...
    def fs(self) -> FileSystem:
        """Return a live filesystem handle.

        Each operation acquires the interpreter lock, so the handle always
        reflects the latest state (including after ``reset()``).
        """
        ...
    def mount(self, vfs_path: str, fs: FileSystem) -> None: ...
    def unmount(self, vfs_path: str) -> None: ...

class ExecResult:
    """Result from executing bash commands."""

    stdout: str
    stderr: str
    exit_code: int
    error: str | None
    success: bool

    def to_dict(self) -> dict[str, Any]: ...

class BashTool:
    """Sandboxed bash interpreter for AI agents.

    BashTool provides a safe execution environment for running bash commands
    with a virtual filesystem. All file operations are contained within the
    sandbox - no access to the real filesystem.

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
        files: dict[str, str] | None = None,
        mounts: list[dict[str, Any]] | None = None,
    ) -> None: ...
    async def execute(self, commands: str) -> ExecResult: ...
    def execute_sync(self, commands: str) -> ExecResult: ...
    async def execute_or_throw(self, commands: str) -> ExecResult: ...
    def execute_sync_or_throw(self, commands: str) -> ExecResult: ...
    def cancel(self) -> None: ...
    def description(self) -> str: ...
    def help(self) -> str: ...
    def system_prompt(self) -> str: ...
    def input_schema(self) -> str: ...
    def output_schema(self) -> str: ...
    def reset(self) -> None: ...
    def fs(self) -> FileSystem:
        """Return a live filesystem handle.

        Each operation acquires the interpreter lock, so the handle always
        reflects the latest state (including after ``reset()``).
        """
        ...
    def mount(self, vfs_path: str, fs: FileSystem) -> None: ...
    def unmount(self, vfs_path: str) -> None: ...

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
    ) -> None: ...
    def add_tool(
        self,
        name: str,
        description: str,
        callback: Callable[[dict[str, Any], str | None], str],
        schema: dict[str, Any] | None = None,
    ) -> None: ...
    def env(self, key: str, value: str) -> None: ...
    async def execute(self, commands: str) -> ExecResult: ...
    def execute_sync(self, commands: str) -> ExecResult: ...
    def tool_count(self) -> int: ...
    def description(self) -> str: ...
    def help(self) -> str: ...
    def system_prompt(self) -> str: ...
    def input_schema(self) -> str: ...
    def output_schema(self) -> str: ...

class BashError(Exception):
    """Exception raised when a bash command exits with non-zero status."""

    exit_code: int
    stderr: str
    stdout: str

def create_langchain_tool_spec() -> dict[str, Any]:
    """Create a LangChain-compatible tool specification.

    Returns:
        Dict with name, description, and args_schema
    """
    ...

def get_version() -> str:
    """Get the bashkit version string."""
    ...
