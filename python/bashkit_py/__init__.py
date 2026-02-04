"""
BashKit Python Bindings

A sandboxed bash interpreter for AI agents with virtual filesystem.

Example:
    >>> from bashkit_py import BashTool
    >>> tool = BashTool()
    >>> result = await tool.execute("echo 'Hello, World!'")
    >>> print(result.stdout)
    Hello, World!

For LangChain integration:
    >>> from bashkit_py.langchain import create_bash_tool
    >>> tool = create_bash_tool()
"""

from bashkit_py._bashkit_py import (
    BashTool,
    ExecResult,
    create_langchain_tool_spec,
)

__version__ = "0.1.0"
__all__ = [
    "BashTool",
    "ExecResult",
    "create_langchain_tool_spec",
]
