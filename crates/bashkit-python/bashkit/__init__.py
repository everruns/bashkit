"""
Bashkit — a sandboxed bash interpreter for AI agents.

Example:
    >>> from bashkit import BashTool
    >>> tool = BashTool()
    >>> result = tool.execute_sync("echo 'Hello, World!'")
    >>> print(result.stdout)
    Hello, World!

LLM tool wrapper (adds schema, description, system_prompt):
    >>> from bashkit import BashTool
    >>> tool = BashTool()
    >>> print(tool.input_schema())

Multi-tool orchestration:
    >>> from bashkit import ScriptedTool
    >>> tool = ScriptedTool("api")
    >>> tool.add_tool("greet", "Greet user", callback=lambda p, s=None: f"hello {p.get('name', 'world')}")
    >>> result = tool.execute_sync("greet --name Alice")

Framework integrations:
    >>> from bashkit.langchain import create_bash_tool, create_scripted_tool
    >>> from bashkit.pydantic_ai import create_bash_tool
"""

from bashkit._bashkit import (
    Bash,
    BashError,
    BashTool,
    ExecResult,
    FileSystem,
    ScriptedTool,
    create_langchain_tool_spec,
    get_version,
)

__version__ = "0.1.2"
__all__ = [
    "Bash",
    "BashError",
    "BashTool",
    "ExecResult",
    "FileSystem",
    "ScriptedTool",
    "create_langchain_tool_spec",
    "get_version",
]
