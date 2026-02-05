"""
LangChain integration for Bashkit.

Provides a LangChain-compatible tool that wraps BashTool for use with
LangChain agents and chains.

Example:
    >>> from bashkit.langchain import create_bash_tool
    >>> from langchain.agents import create_agent
    >>>
    >>> tool = create_bash_tool()
    >>> agent = create_agent(model="claude-sonnet-4-20250514", tools=[tool])
"""

from __future__ import annotations

import asyncio
from typing import Optional, Type

try:
    from langchain_core.tools import BaseTool, ToolException
    from pydantic import BaseModel, Field, PrivateAttr

    LANGCHAIN_AVAILABLE = True
except ImportError:
    LANGCHAIN_AVAILABLE = False
    BaseTool = object
    BaseModel = object

    def Field(*args, **kwargs):
        return None

    def PrivateAttr(*args, **kwargs):
        return None


from bashkit import BashTool as NativeBashTool


class BashToolInput(BaseModel):
    """Input schema for BashTool."""

    commands: str = Field(
        description="Bash commands to execute (like `bash -c 'commands'`)"
    )


if LANGCHAIN_AVAILABLE:

    class BashkitTool(BaseTool):
        """LangChain tool wrapper for Bashkit sandboxed bash interpreter.

        Example:
            >>> tool = BashkitTool()
            >>> result = tool.invoke({"commands": "echo 'Hello!'"})
            >>> print(result)  # Hello!
        """

        name: str = "Bash"
        description: str = ""  # Set in __init__ from bashkit
        args_schema: Type[BaseModel] = BashToolInput
        handle_tool_error: bool = True

        # Internal state - use PrivateAttr for pydantic v2 compatibility
        _bash_tool: NativeBashTool = PrivateAttr()

        def __init__(
            self,
            username: Optional[str] = None,
            hostname: Optional[str] = None,
            max_commands: Optional[int] = None,
            max_loop_iterations: Optional[int] = None,
            **kwargs,
        ):
            """Initialize BashkitTool.

            Args:
                username: Custom username for sandbox
                hostname: Custom hostname for sandbox
                max_commands: Max commands to execute
                max_loop_iterations: Max loop iterations
            """
            bash_tool = NativeBashTool(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
            )
            # Use description from bashkit lib
            kwargs["description"] = bash_tool.description()
            super().__init__(**kwargs)
            object.__setattr__(self, "_bash_tool", bash_tool)

        def _run(self, commands: str) -> str:
            """Execute bash commands synchronously."""
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
) -> "BashkitTool":
    """Create a LangChain-compatible Bashkit tool.

    Args:
        username: Custom username for sandbox
        hostname: Custom hostname for sandbox
        max_commands: Max commands to execute
        max_loop_iterations: Max loop iterations

    Returns:
        BashkitTool instance for use with LangChain agents

    Raises:
        ImportError: If langchain-core is not installed

    Example:
        >>> from bashkit.langchain import create_bash_tool
        >>> tool = create_bash_tool()
        >>> result = tool.invoke({"commands": "ls -la"})
    """
    if not LANGCHAIN_AVAILABLE:
        raise ImportError(
            "langchain-core is required for LangChain integration. "
            "Install with: pip install 'bashkit[langchain]'"
        )

    return BashkitTool(
        username=username,
        hostname=hostname,
        max_commands=max_commands,
        max_loop_iterations=max_loop_iterations,
    )


__all__ = ["BashkitTool", "BashToolInput", "create_bash_tool"]
