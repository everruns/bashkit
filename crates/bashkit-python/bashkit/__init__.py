"""
Bashkit — a sandboxed bash interpreter for AI agents.

Core interpreter (``Bash``)::

    >>> from bashkit import Bash
    >>> bash = Bash(timeout_seconds=30)
    >>> result = bash.execute_sync("echo 'Hello, World!'")
    >>> print(result.stdout)
    Hello, World!

LLM tool wrapper with schema and system prompt (``BashTool``)::

    >>> from bashkit import BashTool
    >>> tool = BashTool()
    >>> result = tool.execute_sync("echo 'Hello, World!'")
    >>> print(result.stdout)
    Hello, World!
    >>> print(tool.input_schema())   # JSON Schema for LLM function calling

Multi-tool orchestration (``ScriptedTool``, in ``bashkit.scripted``)::

    >>> from bashkit.scripted import ScriptedTool
    >>> tool = ScriptedTool("api")
    >>> tool.add_tool("greet", "Greet user",
    ...     callback=lambda p, s=None: f"hello {p.get('name', 'world')}\\n")
    >>> result = tool.execute_sync("greet --name Alice")
    >>> print(result.stdout.strip())
    hello Alice

Direct VFS access (``FileSystem``)::

    >>> from bashkit import FileSystem
    >>> fs = FileSystem()
    >>> fs.write_file("/data.txt", b"content")
    >>> fs.read_file("/data.txt")
    b'content'

Framework integrations::

    >>> from bashkit.langchain import create_bash_tool, create_scripted_tool
    >>> from bashkit.pydantic_ai import create_bash_tool
    >>> from bashkit.deepagents import create_bashkit_backend
"""

from bashkit._bashkit import (
    AnalyzedCommand,
    AnalyzedRedirect,
    Bash,
    BashError,
    BashTool,
    BuiltinContext,
    BuiltinResult,
    CapabilityFingerprint,
    ExecResult,
    ExecutionProfile,
    FileSystem,
    PackedCommit,
    ScriptAnalysis,
    ShellState,
    SnapshotDiff,
    SnapshotGraph,
    create_langchain_tool_spec,
    get_version,
)

__version__ = "0.1.2"
__all__ = [
    "AnalyzedCommand",
    "AnalyzedRedirect",
    "Bash",
    "BashError",
    "ExecutionProfile",
    "BuiltinContext",
    "BuiltinResult",
    "BashTool",
    "CapabilityFingerprint",
    "ExecResult",
    "FileSystem",
    "PackedCommit",
    "ShellState",
    "ScriptAnalysis",
    "SnapshotDiff",
    "SnapshotGraph",
    "create_langchain_tool_spec",
    "get_version",
]


_MOVED = {"ScriptedTool": "bashkit.scripted"}


def __getattr__(name: str):
    # Transition shim: ScriptedTool moved to bashkit.scripted.
    # TODO: remove after one release cycle (see docs/migrating-scripted-tool.md).
    if name in _MOVED:
        import importlib
        import warnings

        target = _MOVED[name]
        warnings.warn(
            f"bashkit.{name} moved to {target}; use `from {target} import {name}`",
            FutureWarning,
            stacklevel=2,
        )
        return getattr(importlib.import_module(target), name)
    raise AttributeError(f"module 'bashkit' has no attribute {name!r}")
