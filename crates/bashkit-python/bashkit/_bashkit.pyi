"""Type stubs for bashkit_py native module."""

class ExecResult:
    """Result from executing bash commands."""

    stdout: str
    stderr: str
    exit_code: int
    error: str | None
    success: bool

    def to_dict(self) -> dict[str, any]: ...

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
    ) -> None:
        """Create a new BashTool instance.

        Args:
            username: Custom username for sandbox (default: "user")
            hostname: Custom hostname for sandbox (default: "sandbox")
            max_commands: Maximum commands to execute (default: 10000)
            max_loop_iterations: Maximum loop iterations (default: 100000)
        """
        ...

    async def execute(self, commands: str) -> ExecResult:
        """Execute bash commands asynchronously.

        Args:
            commands: Bash commands to execute (like `bash -c "commands"`)

        Returns:
            ExecResult with stdout, stderr, exit_code
        """
        ...

    def execute_sync(self, commands: str) -> ExecResult:
        """Execute bash commands synchronously (blocking).

        Note: Prefer `execute()` for async contexts. This method blocks.

        Args:
            commands: Bash commands to execute

        Returns:
            ExecResult with stdout, stderr, exit_code
        """
        ...

    def description(self) -> str:
        """Get the full description."""
        ...

    def help(self) -> str:
        """Get LLM documentation."""
        ...

    def system_prompt(self) -> str:
        """Get system prompt for LLMs."""
        ...

    def input_schema(self) -> str:
        """Get JSON schema for input validation."""
        ...

    def output_schema(self) -> str:
        """Get JSON schema for output."""
        ...

def create_langchain_tool_spec() -> dict[str, any]:
    """Create a LangChain-compatible tool specification.

    Returns:
        Dict with name, description, and args_schema
    """
    ...
