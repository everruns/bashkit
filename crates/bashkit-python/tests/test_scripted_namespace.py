"""ScriptedTool lives in bashkit.scripted; bashkit.ScriptedTool is a warning shim."""

import importlib
import warnings

import pytest

import bashkit
from bashkit.scripted import ScriptedTool


def test_scripted_module_exports_scripted_tool():
    import bashkit.scripted as scripted

    assert scripted.__all__ == ["ScriptedTool"]
    tool = ScriptedTool("api")
    tool.add_tool("greet", "Greet user", callback=lambda p, s=None: "hi\n")
    assert tool.execute_sync("greet").stdout == "hi\n"


def test_top_level_scripted_tool_warns_and_resolves_same_class():
    with pytest.warns(FutureWarning, match="bashkit.scripted"):
        legacy = bashkit.ScriptedTool
    assert legacy is ScriptedTool


def test_top_level_from_import_warns():
    with pytest.warns(FutureWarning):
        exec("from bashkit import ScriptedTool as _legacy", {})


def test_scripted_tool_not_in_top_level_all():
    assert "ScriptedTool" not in bashkit.__all__


def test_star_import_does_not_warn():
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        exec("from bashkit import *", {})


def test_unknown_attribute_still_raises():
    with pytest.raises(AttributeError, match="no attribute 'NoSuchThing'"):
        bashkit.NoSuchThing  # noqa: B018


def test_core_imports_do_not_warn():
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        importlib.reload(bashkit)
        from bashkit import Bash, BashTool  # noqa: F401
