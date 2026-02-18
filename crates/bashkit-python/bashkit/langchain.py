"""
LangChain integration for Bashkit.

Provides LangChain-compatible tools wrapping BashTool and ScriptedTool
for use with LangChain agents and chains.

Example (BashTool):
    >>> from bashkit.langchain import create_bash_tool
    >>> tool = create_bash_tool()
    >>> result = tool.invoke({"commands": "echo hello"})

Example (ScriptedTool):
    >>> from bashkit import ScriptedTool
    >>> from bashkit.langchain import create_scripted_tool
    >>>
    >>> st = ScriptedTool("api")
    >>> st.add_tool("greet", "Greet user", callback=lambda p, s=None: f"hello {p.get('name')}\\n")
    >>> tool = create_scripted_tool(st)
    >>> result = tool.invoke({"commands": "greet --name Alice"})
"""

from __future__ import annotations

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
from bashkit import ScriptedTool as NativeScriptedTool


class BashToolInput(BaseModel):
    """Input schema for BashTool."""

    commands: str = Field(description="Bash commands to execute (like `bash -c 'commands'`)")


if LANGCHAIN_AVAILABLE:

    class BashkitTool(BaseTool):
        """LangChain tool wrapper for Bashkit sandboxed bash interpreter.

        Example:
            >>> tool = BashkitTool()
            >>> result = tool.invoke({"commands": "echo 'Hello!'"})
            >>> print(result)  # Hello!
        """

        name: str = ""
        description: str = ""
        args_schema: type[BaseModel] = BashToolInput
        handle_tool_error: bool = True

        _bash_tool: NativeBashTool = PrivateAttr()

        def __init__(
            self,
            username: str | None = None,
            hostname: str | None = None,
            max_commands: int | None = None,
            max_loop_iterations: int | None = None,
            **kwargs,
        ):
            bash_tool = NativeBashTool(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
            )
            kwargs["name"] = bash_tool.name
            kwargs["description"] = bash_tool.description()
            super().__init__(**kwargs)
            object.__setattr__(self, "_bash_tool", bash_tool)

        def _run(self, commands: str) -> str:
            """Execute bash commands synchronously."""
            result = self._bash_tool.execute_sync(commands)

            if result.error:
                raise ToolException(f"Execution error: {result.error}")

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

            output = result.stdout
            if result.stderr:
                output += f"\nSTDERR: {result.stderr}"
            if result.exit_code != 0:
                output += f"\n[Exit code: {result.exit_code}]"

            return output

    class ScriptedToolLangChain(BaseTool):
        """LangChain tool wrapper for Bashkit ScriptedTool.

        Wraps a pre-configured ScriptedTool (with registered Python callbacks)
        as a LangChain tool. The LLM sends bash scripts that orchestrate all
        registered sub-tools in one call.

        Example:
            >>> from bashkit import ScriptedTool
            >>> st = ScriptedTool("k8s")
            >>> st.add_tool("get_pods", "List pods", callback=my_callback)
            >>> tool = ScriptedToolLangChain(st)
            >>> result = tool.invoke({"commands": "get_pods --namespace default | jq '.items | length'"})
        """

        name: str = ""
        description: str = ""
        args_schema: type[BaseModel] = BashToolInput
        handle_tool_error: bool = True

        _scripted_tool: NativeScriptedTool = PrivateAttr()

        def __init__(self, scripted_tool: NativeScriptedTool, **kwargs):
            kwargs["name"] = scripted_tool.name
            kwargs["description"] = scripted_tool.system_prompt()
            super().__init__(**kwargs)
            object.__setattr__(self, "_scripted_tool", scripted_tool)

        def _run(self, commands: str) -> str:
            """Execute scripted tool commands synchronously."""
            result = self._scripted_tool.execute_sync(commands)

            if result.error:
                raise ToolException(f"Execution error: {result.error}")

            output = result.stdout
            if result.stderr:
                output += f"\nSTDERR: {result.stderr}"
            if result.exit_code != 0:
                output += f"\n[Exit code: {result.exit_code}]"

            return output

        async def _arun(self, commands: str) -> str:
            """Execute scripted tool commands asynchronously."""
            result = await self._scripted_tool.execute(commands)

            if result.error:
                raise ToolException(f"Execution error: {result.error}")

            output = result.stdout
            if result.stderr:
                output += f"\nSTDERR: {result.stderr}"
            if result.exit_code != 0:
                output += f"\n[Exit code: {result.exit_code}]"

            return output


def create_bash_tool(
    username: str | None = None,
    hostname: str | None = None,
    max_commands: int | None = None,
    max_loop_iterations: int | None = None,
) -> BashkitTool:
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
            "langchain-core is required for LangChain integration. Install with: pip install 'bashkit[langchain]'"
        )

    return BashkitTool(
        username=username,
        hostname=hostname,
        max_commands=max_commands,
        max_loop_iterations=max_loop_iterations,
    )


def create_scripted_tool(scripted_tool: NativeScriptedTool) -> ScriptedToolLangChain:
    """Create a LangChain-compatible tool from a configured ScriptedTool.

    Args:
        scripted_tool: A ScriptedTool with registered tool callbacks

    Returns:
        ScriptedToolLangChain instance for use with LangChain agents

    Raises:
        ImportError: If langchain-core is not installed

    Example:
        >>> from bashkit import ScriptedTool
        >>> from bashkit.langchain import create_scripted_tool
        >>>
        >>> st = ScriptedTool("api")
        >>> st.add_tool("get_data", "Fetch data", callback=my_fn)
        >>> tool = create_scripted_tool(st)
        >>> # Use with: create_react_agent(model, [tool])
    """
    if not LANGCHAIN_AVAILABLE:
        raise ImportError(
            "langchain-core is required for LangChain integration. Install with: pip install 'bashkit[langchain]'"
        )

    return ScriptedToolLangChain(scripted_tool)


__all__ = [
    "BashkitTool",
    "BashToolInput",
    "ScriptedToolLangChain",
    "create_bash_tool",
    "create_scripted_tool",
]
