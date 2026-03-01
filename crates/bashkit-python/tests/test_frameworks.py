"""Tests for framework integration modules (langchain, deepagents, pydantic_ai).

These tests verify the integration modules work without the external frameworks
by testing the import-guarding, factory functions, and mock behavior.
"""

import pytest

from bashkit import ScriptedTool

# ===========================================================================
# langchain.py tests
# ===========================================================================


def test_langchain_import():
    """langchain module imports without langchain installed."""
    from bashkit import langchain  # noqa: F401


def test_langchain_create_bash_tool_without_langchain():
    """create_bash_tool raises ImportError when langchain not installed."""
    from bashkit.langchain import LANGCHAIN_AVAILABLE, create_bash_tool

    if not LANGCHAIN_AVAILABLE:
        with pytest.raises(ImportError, match="langchain-core"):
            create_bash_tool()


def test_langchain_create_scripted_tool_without_langchain():
    """create_scripted_tool raises ImportError when langchain not installed."""
    from bashkit.langchain import LANGCHAIN_AVAILABLE, create_scripted_tool

    if not LANGCHAIN_AVAILABLE:
        st = ScriptedTool("api")
        st.add_tool("noop", "No-op", callback=lambda p, s=None: "ok\n")
        with pytest.raises(ImportError, match="langchain-core"):
            create_scripted_tool(st)


def test_langchain_all_exports():
    """langchain __all__ contains expected symbols."""
    from bashkit.langchain import __all__

    assert "create_bash_tool" in __all__
    assert "create_scripted_tool" in __all__
    assert "BashkitTool" in __all__
    assert "BashToolInput" in __all__


# ===========================================================================
# deepagents.py tests
# ===========================================================================


def test_deepagents_import():
    """deepagents module imports without deepagents installed."""
    from bashkit import deepagents  # noqa: F401


def test_deepagents_create_bash_middleware_without_deepagents():
    """create_bash_middleware raises ImportError when deepagents not installed."""
    from bashkit.deepagents import DEEPAGENTS_AVAILABLE, create_bash_middleware

    if not DEEPAGENTS_AVAILABLE:
        with pytest.raises(ImportError, match="deepagents"):
            create_bash_middleware()


def test_deepagents_create_bashkit_backend_without_deepagents():
    """create_bashkit_backend raises ImportError when deepagents not installed."""
    from bashkit.deepagents import DEEPAGENTS_AVAILABLE, create_bashkit_backend

    if not DEEPAGENTS_AVAILABLE:
        with pytest.raises(ImportError, match="deepagents"):
            create_bashkit_backend()


def test_deepagents_all_exports():
    """deepagents __all__ contains expected symbols."""
    from bashkit.deepagents import __all__

    assert "create_bash_middleware" in __all__
    assert "create_bashkit_backend" in __all__
    assert "BashkitMiddleware" in __all__
    assert "BashkitBackend" in __all__


def test_deepagents_now_iso():
    """_now_iso returns ISO format string."""
    from bashkit.deepagents import _now_iso

    ts = _now_iso()
    assert isinstance(ts, str)
    assert "T" in ts  # ISO format has T separator


def test_deepagents_write_heredoc_injection():
    """Content containing the heredoc delimiter must not cause injection."""
    from bashkit import BashTool
    from bashkit.deepagents import _build_write_cmd

    # Content that would terminate a fixed BASHKIT_EOF heredoc early
    malicious = "line1\nBASHKIT_EOF\necho INJECTED\nmore"
    cmd = _build_write_cmd("/tmp/test_inject.txt", malicious)

    # The generated delimiter must not be the plain "BASHKIT_EOF"
    # so content containing that literal cannot terminate it early
    tool = BashTool()
    tool.execute_sync(cmd)
    r = tool.execute_sync("cat /tmp/test_inject.txt")
    assert r.exit_code == 0
    # The file must contain the literal BASHKIT_EOF line, not execute it
    assert "BASHKIT_EOF" in r.stdout
    assert "INJECTED" not in r.stdout or "echo INJECTED" in r.stdout
    # All original lines present
    assert "line1" in r.stdout
    assert "more" in r.stdout


def test_deepagents_write_cmd_uses_shlex_quote():
    """_build_write_cmd must quote file paths with special characters."""
    from bashkit.deepagents import _build_write_cmd

    cmd = _build_write_cmd("/tmp/my file.txt", "hello")
    # shlex.quote wraps in single quotes for paths with spaces
    assert "'/tmp/my file.txt'" in cmd


def test_deepagents_write_cmd_unique_delimiters():
    """Each call should produce a unique delimiter."""
    from bashkit.deepagents import _build_write_cmd

    cmd1 = _build_write_cmd("/tmp/a.txt", "x")
    cmd2 = _build_write_cmd("/tmp/b.txt", "y")
    # Extract delimiter from first line: cat > path << 'DELIM'
    delim1 = cmd1.split("'")[-2]
    delim2 = cmd2.split("'")[-2]
    assert delim1 != delim2


# ===========================================================================
# pydantic_ai.py tests
# ===========================================================================


def test_pydantic_ai_import():
    """pydantic_ai module imports without pydantic-ai installed."""
    from bashkit import pydantic_ai  # noqa: F401


def test_pydantic_ai_create_bash_tool_without_pydantic():
    """create_bash_tool raises ImportError when pydantic-ai not installed."""
    from bashkit.pydantic_ai import PYDANTIC_AI_AVAILABLE
    from bashkit.pydantic_ai import create_bash_tool as create_pydantic_tool

    if not PYDANTIC_AI_AVAILABLE:
        with pytest.raises(ImportError, match="pydantic-ai"):
            create_pydantic_tool()


def test_pydantic_ai_all_exports():
    """pydantic_ai __all__ contains expected symbols."""
    from bashkit.pydantic_ai import __all__

    assert "create_bash_tool" in __all__
