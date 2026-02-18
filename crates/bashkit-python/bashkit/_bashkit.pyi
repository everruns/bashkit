"""Type stubs for bashkit_py native module."""

from typing import Any, Callable

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
    ) -> None: ...
    async def execute(self, commands: str) -> ExecResult: ...
    def execute_sync(self, commands: str) -> ExecResult: ...
    def description(self) -> str: ...
    def help(self) -> str: ...
    def system_prompt(self) -> str: ...
    def input_schema(self) -> str: ...
    def output_schema(self) -> str: ...

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

def create_langchain_tool_spec() -> dict[str, Any]:
    """Create a LangChain-compatible tool specification.

    Returns:
        Dict with name, description, and args_schema
    """
    ...
