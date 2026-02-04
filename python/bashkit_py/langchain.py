"""
LangChain integration for BashKit.

Provides a LangChain-compatible tool that wraps BashTool for use with
LangChain agents and chains.

Example:
    >>> from bashkit_py.langchain import create_bash_tool
    >>> from langchain.agents import create_tool_calling_agent
    >>>
    >>> tool = create_bash_tool()
    >>> agent = create_tool_calling_agent(llm, [tool], prompt)
"""

from __future__ import annotations

import asyncio
from typing import Optional, Type

try:
    from langchain_core.tools import BaseTool, ToolException
    from pydantic import BaseModel, Field

    LANGCHAIN_AVAILABLE = True
except ImportError:
    LANGCHAIN_AVAILABLE = False
    BaseTool = object
    BaseModel = object

    def Field(*args, **kwargs):
        return None


from bashkit_py import BashTool as NativeBashTool


class BashToolInput(BaseModel):
    """Input schema for BashTool."""

    commands: str = Field(
        description="Bash commands to execute (like `bash -c 'commands'`)"
    )


if LANGCHAIN_AVAILABLE:

    class BashKitTool(BaseTool):
        """LangChain tool wrapper for BashKit sandboxed bash interpreter.

        This tool provides a safe bash execution environment with:
        - Virtual filesystem (no real filesystem access)
        - Resource limits (command count, loop iterations)
        - 66+ built-in commands (echo, cat, grep, sed, awk, jq, etc.)

        Example:
            >>> tool = BashKitTool()
            >>> result = tool.invoke({"commands": "echo 'Hello!'"})
            >>> print(result)  # Hello!
        """

        name: str = "bashkit"
        description: str = """Sandboxed bash interpreter with virtual filesystem.
Execute bash commands safely. Supports variables, pipelines, redirects, loops,
conditionals, functions, and arrays. Built-in commands include: echo, cat, grep,
sed, awk, jq, head, tail, sort, uniq, cut, tr, wc, find, xargs, and more.
All file operations use a virtual filesystem - changes don't affect real files."""
        args_schema: Type[BaseModel] = BashToolInput
        handle_tool_error: bool = True

        # Internal state
        _bash_tool: Optional[NativeBashTool] = None

        def __init__(
            self,
            username: Optional[str] = None,
            hostname: Optional[str] = None,
            max_commands: Optional[int] = None,
            max_loop_iterations: Optional[int] = None,
            **kwargs,
        ):
            """Initialize BashKitTool.

            Args:
                username: Custom username for sandbox
                hostname: Custom hostname for sandbox
                max_commands: Max commands to execute
                max_loop_iterations: Max loop iterations
            """
            super().__init__(**kwargs)
            self._bash_tool = NativeBashTool(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
            )

        def _run(self, commands: str) -> str:
            """Execute bash commands synchronously."""
            if self._bash_tool is None:
                raise ToolException("BashTool not initialized")

            result = self._bash_tool.execute_sync(commands)

            if result.error:
                raise ToolException(f"Execution error: {result.error}")

            # Return combined output for the agent
            output = result.stdout
            if result.stderr:
                output += f"\nSTDERR: {result.stderr}"
            if result.exit_code != 0:
                output += f"\n[Exit code: {result.exit_code}]"

            return output

        async def _arun(self, commands: str) -> str:
            """Execute bash commands asynchronously."""
            if self._bash_tool is None:
                raise ToolException("BashTool not initialized")

            result = await self._bash_tool.execute(commands)

            if result.error:
                raise ToolException(f"Execution error: {result.error}")

            # Return combined output for the agent
            output = result.stdout
            if result.stderr:
                output += f"\nSTDERR: {result.stderr}"
            if result.exit_code != 0:
                output += f"\n[Exit code: {result.exit_code}]"

            return output


def create_bash_tool(
    username: Optional[str] = None,
    hostname: Optional[str] = None,
    max_commands: Optional[int] = None,
    max_loop_iterations: Optional[int] = None,
) -> "BashKitTool":
    """Create a LangChain-compatible BashKit tool.

    Args:
        username: Custom username for sandbox
        hostname: Custom hostname for sandbox
        max_commands: Max commands to execute
        max_loop_iterations: Max loop iterations

    Returns:
        BashKitTool instance for use with LangChain agents

    Raises:
        ImportError: If langchain-core is not installed

    Example:
        >>> from bashkit_py.langchain import create_bash_tool
        >>> tool = create_bash_tool()
        >>> result = tool.invoke({"commands": "ls -la"})
    """
    if not LANGCHAIN_AVAILABLE:
        raise ImportError(
            "langchain-core is required for LangChain integration. "
            "Install with: pip install 'bashkit-py[langchain]'"
        )

    return BashKitTool(
        username=username,
        hostname=hostname,
        max_commands=max_commands,
        max_loop_iterations=max_loop_iterations,
    )


__all__ = ["BashKitTool", "BashToolInput", "create_bash_tool"]
