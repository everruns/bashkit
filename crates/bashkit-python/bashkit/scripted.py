"""
Multi-tool orchestration for Bashkit (``ScriptedTool``).

Each registered Python callback becomes a bash builtin; an LLM writes one bash
script that pipes, loops, and branches across all of them in a logic-only
shell.

    >>> from bashkit.scripted import ScriptedTool
    >>> tool = ScriptedTool("api")
    >>> tool.add_tool("greet", "Greet user",
    ...     callback=lambda p, s=None: f"hello {p.get('name', 'world')}\\n")
    >>> result = tool.execute_sync("greet --name Alice")
    >>> print(result.stdout.strip())
    hello Alice

Decision: ``ScriptedTool`` lives here, not in the top-level ``bashkit``
namespace, mirroring the Rust ``bashkit-scripted-tool`` crate. ``bashkit``
keeps the interpreter and ``BashTool``. ``bashkit.ScriptedTool`` still resolves
with a ``FutureWarning`` during the transition.
"""

from bashkit._bashkit import ScriptedTool

__all__ = ["ScriptedTool"]
