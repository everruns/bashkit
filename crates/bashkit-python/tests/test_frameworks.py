"""Tests for framework integration modules (langchain, deepagents, pydantic_ai).

These tests verify the integration modules work without the external frameworks
by testing the import-guarding, factory functions, and mock behavior.
"""

import pytest

from bashkit import BashTool, ScriptedTool


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
