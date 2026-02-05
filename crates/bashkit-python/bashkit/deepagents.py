"""
Deep Agents middleware for BashKit.

Provides a middleware component that adds sandboxed bash execution
capabilities to deep agents with virtual filesystem support.

Example:
    >>> from bashkit.deepagents import BashKitMiddleware, create_bash_middleware
    >>> from deepagents import create_deep_agent
    >>>
    >>> middleware = create_bash_middleware()
    >>> agent = create_deep_agent(
    ...     model="anthropic:claude-sonnet-4-20250514",
    ...     middleware=[middleware]
    ... )
"""

from __future__ import annotations

from typing import Optional

try:
    from deepagents.middleware import AgentMiddleware
    from langchain_core.tools import tool as langchain_tool

    DEEPAGENTS_AVAILABLE = True
except ImportError:
    DEEPAGENTS_AVAILABLE = False
    AgentMiddleware = object

    def langchain_tool(func):
        return func


from bashkit import BashTool as NativeBashTool


def _create_bash_tools(
    username: Optional[str] = None,
    hostname: Optional[str] = None,
    max_commands: Optional[int] = None,
    max_loop_iterations: Optional[int] = None,
):
    """Create bash tools with a shared BashTool instance.

    Returns a tuple of (execute_tool, reset_tool, bash_instance).
    """
    bash_instance = NativeBashTool(
        username=username,
        hostname=hostname,
        max_commands=max_commands,
        max_loop_iterations=max_loop_iterations,
    )

    @langchain_tool
    def execute(commands: str) -> str:
        """Execute bash commands in a sandboxed virtual filesystem.

        This tool provides a safe bash execution environment with:
        - Virtual filesystem (no real filesystem access)
        - 66+ built-in commands (echo, cat, grep, sed, awk, jq, curl, etc.)
        - Full bash syntax: variables, pipelines, redirects, loops, functions
        - Persistent state between calls (files and variables are retained)

        Common commands:
        - Navigation: ls, cd, pwd, find
        - File ops: cat, head, tail, touch, mkdir, rm, cp, mv
        - Text processing: grep, sed, awk, jq, sort, uniq, cut, tr, wc
        - Utilities: echo, printf, date, sleep, curl, wget

        Args:
            commands: Bash commands to execute (like `bash -c 'commands'`)

        Returns:
            Command output (stdout, stderr combined) with exit code if non-zero
        """
        result = bash_instance.execute_sync(commands)

        if result.error:
            return f"Error: {result.error}"

        output = result.stdout
        if result.stderr:
            output += f"\nSTDERR: {result.stderr}"
        if result.exit_code != 0:
            output += f"\n[Exit code: {result.exit_code}]"

        return output

    @langchain_tool
    def reset_filesystem() -> str:
        """Reset the virtual filesystem to its initial state.

        This clears all files and directories created during the session
        and resets shell variables. Use this when you need a clean slate.

        Returns:
            Confirmation message
        """
        bash_instance.reset()
        return "Virtual filesystem has been reset to initial state."

    return execute, reset_filesystem, bash_instance


if DEEPAGENTS_AVAILABLE:

    class BashKitMiddleware(AgentMiddleware):
        """Deep Agents middleware for sandboxed bash execution.

        Provides tools for executing bash commands in an isolated virtual
        filesystem. All file operations are sandboxed - no real filesystem
        access occurs.

        The middleware maintains state between tool calls, so files created
        in one command are available in subsequent commands.

        Example:
            >>> from bashkit.deepagents import BashKitMiddleware
            >>> from deepagents import create_deep_agent
            >>>
            >>> agent = create_deep_agent(
            ...     model="anthropic:claude-sonnet-4-20250514",
            ...     middleware=[BashKitMiddleware()]
            ... )

        Attributes:
            tools: List of tools exposed to the agent (execute, reset_filesystem)
        """

        def __init__(
            self,
            username: Optional[str] = None,
            hostname: Optional[str] = None,
            max_commands: Optional[int] = None,
            max_loop_iterations: Optional[int] = None,
        ):
            """Initialize BashKitMiddleware.

            Args:
                username: Custom username for sandbox (default: "user")
                hostname: Custom hostname for sandbox (default: "sandbox")
                max_commands: Maximum commands to execute per session
                max_loop_iterations: Maximum loop iterations allowed
            """
            execute, reset, self._bash_instance = _create_bash_tools(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
            )
            self._tools = [execute, reset]

        @property
        def tools(self):
            """Tools provided by this middleware."""
            return self._tools

        def execute_sync(self, commands: str) -> str:
            """Execute commands synchronously (for setup scripts).

            Args:
                commands: Bash commands to execute

            Returns:
                Command output
            """
            result = self._bash_instance.execute_sync(commands)
            output = result.stdout
            if result.stderr:
                output += f"\nSTDERR: {result.stderr}"
            return output


def create_bash_middleware(
    username: Optional[str] = None,
    hostname: Optional[str] = None,
    max_commands: Optional[int] = None,
    max_loop_iterations: Optional[int] = None,
) -> "BashKitMiddleware":
    """Create a Deep Agents middleware for BashKit.

    Args:
        username: Custom username for sandbox
        hostname: Custom hostname for sandbox
        max_commands: Maximum commands to execute
        max_loop_iterations: Maximum loop iterations

    Returns:
        BashKitMiddleware instance for use with create_deep_agent

    Raises:
        ImportError: If deepagents is not installed

    Example:
        >>> from bashkit.deepagents import create_bash_middleware
        >>> from deepagents import create_deep_agent
        >>>
        >>> middleware = create_bash_middleware(username="dev", hostname="sandbox")
        >>> agent = create_deep_agent(middleware=[middleware])
    """
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError(
            "deepagents is required for Deep Agents integration. "
            "Install with: pip install 'bashkit[deepagents]'"
        )

    return BashKitMiddleware(
        username=username,
        hostname=hostname,
        max_commands=max_commands,
        max_loop_iterations=max_loop_iterations,
    )


__all__ = ["BashKitMiddleware", "create_bash_middleware"]
