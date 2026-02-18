"""Tests for bashkit Python bindings."""

import json

import pytest

from bashkit import BashTool, create_langchain_tool_spec

# -- Construction -----------------------------------------------------------


def test_default_construction():
    tool = BashTool()
    assert tool.name == "bashkit"
    assert isinstance(tool.short_description, str)
    assert isinstance(tool.version, str)


def test_custom_construction():
    tool = BashTool(username="alice", hostname="box", max_commands=100, max_loop_iterations=500)
    assert repr(tool) == 'BashTool(username="alice", hostname="box")'


# -- Sync execution ---------------------------------------------------------


def test_echo():
    tool = BashTool()
    r = tool.execute_sync("echo hello")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hello"
    assert r.stderr == ""
    assert r.error is None
    assert r.success is True


def test_exit_code():
    tool = BashTool()
    r = tool.execute_sync("exit 42")
    assert r.exit_code == 42
    assert r.success is False


def test_stderr():
    tool = BashTool()
    r = tool.execute_sync("echo err >&2")
    assert "err" in r.stderr


def test_multiline():
    tool = BashTool()
    r = tool.execute_sync("echo a; echo b; echo c")
    assert r.exit_code == 0
    lines = r.stdout.strip().splitlines()
    assert lines == ["a", "b", "c"]


def test_state_persists():
    """Filesystem and variables persist across calls."""
    tool = BashTool()
    tool.execute_sync("export FOO=bar")
    r = tool.execute_sync("echo $FOO")
    assert r.stdout.strip() == "bar"


def test_file_persistence():
    """Files created in one call are visible in the next."""
    tool = BashTool()
    tool.execute_sync("echo content > /tmp/test.txt")
    r = tool.execute_sync("cat /tmp/test.txt")
    assert r.stdout.strip() == "content"


# -- Async execution --------------------------------------------------------


@pytest.mark.asyncio
async def test_async_execute():
    tool = BashTool()
    r = await tool.execute("echo async_hello")
    assert r.exit_code == 0
    assert r.stdout.strip() == "async_hello"


@pytest.mark.asyncio
async def test_async_state_persists():
    tool = BashTool()
    await tool.execute("X=123")
    r = await tool.execute("echo $X")
    assert r.stdout.strip() == "123"


# -- ExecResult -------------------------------------------------------------


def test_exec_result_to_dict():
    tool = BashTool()
    r = tool.execute_sync("echo hi")
    d = r.to_dict()
    assert d["stdout"].strip() == "hi"
    assert d["exit_code"] == 0
    assert d["stderr"] == ""
    assert d["error"] is None


def test_exec_result_repr():
    tool = BashTool()
    r = tool.execute_sync("echo hi")
    assert "ExecResult" in repr(r)


def test_exec_result_str_success():
    tool = BashTool()
    r = tool.execute_sync("echo ok")
    assert str(r).strip() == "ok"


def test_exec_result_str_failure():
    tool = BashTool()
    r = tool.execute_sync("exit 1")
    assert "Error" in str(r)


# -- Reset ------------------------------------------------------------------


def test_reset():
    tool = BashTool()
    tool.execute_sync("export KEEP=1")
    tool.reset()
    r = tool.execute_sync("echo ${KEEP:-empty}")
    assert r.stdout.strip() == "empty"


# -- LLM metadata ----------------------------------------------------------


def test_description():
    tool = BashTool()
    desc = tool.description()
    assert isinstance(desc, str)
    assert len(desc) > 0


def test_help():
    tool = BashTool()
    h = tool.help()
    assert isinstance(h, str)
    assert len(h) > 0


def test_system_prompt():
    tool = BashTool()
    sp = tool.system_prompt()
    assert isinstance(sp, str)
    assert len(sp) > 0


def test_input_schema():
    tool = BashTool()
    schema = tool.input_schema()
    parsed = json.loads(schema)
    assert "type" in parsed or "properties" in parsed


def test_output_schema():
    tool = BashTool()
    schema = tool.output_schema()
    parsed = json.loads(schema)
    assert "type" in parsed or "properties" in parsed


# -- LangChain tool spec ---------------------------------------------------


def test_langchain_tool_spec():
    spec = create_langchain_tool_spec()
    assert "name" in spec
    assert "description" in spec
    assert "args_schema" in spec
    assert spec["name"] == "bashkit"
