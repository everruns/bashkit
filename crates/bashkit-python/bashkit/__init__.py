"""
Bashkit Python Bindings

A sandboxed bash interpreter for AI agents with virtual filesystem.

Example:
    >>> from bashkit import BashTool
    >>> tool = BashTool()
    >>> result = await tool.execute("echo 'Hello, World!'")
    >>> print(result.stdout)
    Hello, World!

For scripted multi-tool orchestration:
    >>> from bashkit import ScriptedTool
    >>> tool = ScriptedTool("api")
    >>> tool.add_tool("greet", "Greet user", callback=lambda p, s=None: f"hello {p.get('name', 'world')}")
    >>> result = tool.execute_sync("greet --name Alice")

For LangChain integration:
    >>> from bashkit.langchain import create_bash_tool, create_scripted_tool

For Deep Agents integration:
    >>> from bashkit.deepagents import create_bash_middleware

For PydanticAI integration:
    >>> from bashkit.pydantic_ai import create_bash_tool
"""

from bashkit._bashkit import (
    BashTool,
    ExecResult,
    ScriptedTool,
    create_langchain_tool_spec,
)

__version__ = "0.1.2"
__all__ = [
    "BashTool",
    "ExecResult",
    "ScriptedTool",
    "create_langchain_tool_spec",
]
