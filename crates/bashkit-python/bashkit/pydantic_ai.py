"""
PydanticAI integration for Bashkit.

Provides a Tool that wraps BashTool for use with PydanticAI agents.

Example:
    >>> from bashkit.pydantic_ai import create_bash_tool
    >>> from pydantic_ai import Agent
    >>>
    >>> tool = create_bash_tool()
    >>> agent = Agent('anthropic:claude-sonnet-4-20250514', tools=[tool])
"""

from __future__ import annotations

from typing import Optional

try:
    from pydantic_ai import Tool

    PYDANTIC_AI_AVAILABLE = True
except ImportError:
    PYDANTIC_AI_AVAILABLE = False

from bashkit import BashTool as NativeBashTool


def create_bash_tool(
    username: Optional[str] = None,
    hostname: Optional[str] = None,
    max_commands: Optional[int] = None,
    max_loop_iterations: Optional[int] = None,
) -> "Tool":
    """Create a PydanticAI Tool wrapping Bashkit.

    Args:
        username: Custom username for sandbox
        hostname: Custom hostname for sandbox
        max_commands: Max commands to execute
        max_loop_iterations: Max loop iterations

    Returns:
        Tool for use with ``Agent(tools=[...])``

    Raises:
        ImportError: If pydantic-ai is not installed

    Example:
        >>> from bashkit.pydantic_ai import create_bash_tool
        >>> tool = create_bash_tool()
        >>> from pydantic_ai import Agent
        >>> agent = Agent('anthropic:claude-sonnet-4-20250514', tools=[tool])
    """
    if not PYDANTIC_AI_AVAILABLE:
        raise ImportError(
            "pydantic-ai is required for PydanticAI integration. "
            "Install with: pip install 'bashkit[pydantic-ai]'"
        )

    native = NativeBashTool(
        username=username,
        hostname=hostname,
        max_commands=max_commands,
        max_loop_iterations=max_loop_iterations,
    )

    async def bash(commands: str) -> str:
        """Execute bash commands in a sandboxed virtual environment.

        Runs commands like ``bash -c "commands"``. All file operations happen
        in a virtual filesystem — nothing touches the real host.

        Args:
            commands: Bash commands to execute
        """
        result = await native.execute(commands)

        output = result.stdout
        if result.stderr:
            output += f"\nSTDERR: {result.stderr}"
        if result.exit_code != 0:
            output += f"\n[Exit code: {result.exit_code}]"

        return output if output else "[No output]"

    return Tool(bash, takes_ctx=False, name="bash")


__all__ = ["create_bash_tool"]
