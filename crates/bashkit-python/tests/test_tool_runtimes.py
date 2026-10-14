"""Opt-in tool runtimes share the VFS and survive interpreter rebuilds."""

import importlib.util

import pytest

from bashkit import BashTool

PYTHON_ENV = {"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"}


@pytest.mark.parametrize("command", ["python", "python3", "ts", "node"])
def test_runtimes_disabled_by_default(command):
    result = BashTool().execute_sync(f"{command} -c '1 + 1'")
    assert result.exit_code == 127


@pytest.mark.parametrize(
    ("runtime", "script", "other_script"),
    [
        ("python", "python -c 'print(42)'", "ts -c '42'"),
        ("typescript", "ts -c '42'", "python -c 'print(42)'"),
    ],
)
def test_runtime_execution_metadata_and_reset(runtime, script, other_script):
    tool = BashTool(env=PYTHON_ENV, **{runtime: True})
    for _ in range(2):
        result = tool.execute_sync(script)
        assert result.exit_code == 0, result.stderr
        assert result.stdout == "42\n"
        assert tool.execute_sync(other_script).exit_code == 127
        assert runtime in tool.description().lower()
        assert runtime in tool.help().lower()
        tool.reset()


def test_runtimes_share_virtual_filesystem():
    tool = BashTool(python=True, typescript=True, env=PYTHON_ENV)
    result = tool.execute_sync(
        "echo 'hello' > /tmp/input.txt\n"
        "ts -c \"await writeFile('/tmp/output.txt', "
        "(await readFile('/tmp/input.txt')).trim().toUpperCase())\"\n"
        "python -c 'from pathlib import Path; print(Path(\"/tmp/output.txt\").read_text())'"
    )
    assert result.exit_code == 0, result.stderr
    assert result.stdout.strip() == "HELLO"
    assert tool.execute_sync("cat /tmp/output.txt").stdout == "HELLO"


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("runtime", "script"),
    [("python", "python -c 'print(42)'"), ("typescript", "ts -c '42'")],
)
async def test_runtime_async(runtime, script):
    result = await BashTool(env=PYTHON_ENV, **{runtime: True}).execute(script)
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"


@pytest.mark.parametrize("keyed", [False, True])
def test_snapshot_restore_runtime_flags(keyed):
    original = BashTool(python=True, typescript=True, env=PYTHON_ENV)
    assert original.execute_sync("echo '40 + 2' > /tmp/main.ts").exit_code == 0
    if keyed:
        key = b"k" * 32
        restored = BashTool.from_snapshot_keyed(
            original.snapshot_keyed(key), key, python=True, typescript=True, env=PYTHON_ENV
        )
    else:
        restored = BashTool.from_snapshot(original.snapshot(), python=True, typescript=True, env=PYTHON_ENV)
    assert restored.execute_sync("ts /tmp/main.ts").stdout == "42\n"
    assert restored.execute_sync("python -c 'print(42)'").stdout == "42\n"


def test_langchain_forwards_runtime_flags():
    if importlib.util.find_spec("langchain_core") is None:
        pytest.skip("langchain-core not installed")
    from bashkit.langchain import BashkitTool, create_bash_tool

    for factory in (BashkitTool, create_bash_tool):
        tool = factory(python=True, typescript=True, env=PYTHON_ENV)
        assert "python" in tool.description.lower()
        assert "typescript" in tool.description.lower()
        output = tool.invoke({"commands": "python -c 'print(21)' && ts -c '21'"})
        assert output == "21\n21\n"


def test_deepagents_forwards_runtime_flags():
    if importlib.util.find_spec("deepagents") is None:
        pytest.skip("deepagents not installed")
    from bashkit.deepagents import BashkitBackend, BashkitMiddleware, create_bash_middleware

    script = "python -c 'print(21)' && ts -c '21'"
    for factory in (BashkitMiddleware, create_bash_middleware):
        middleware = factory(python=True, typescript=True, env=PYTHON_ENV)
        assert middleware.execute_sync(script) == "21\n21\n"
        assert middleware.tools[0].invoke({"command": script}) == "21\n21"
        assert "typescript" in middleware.tools[0].description.lower()

    backend = BashkitBackend(python=True, typescript=True, env=PYTHON_ENV)
    assert backend.execute(script).output == "21\n21\n"
    middleware = backend.create_middleware()
    assert backend.execute("echo '42' > /tmp/main.ts").exit_code == 0
    assert middleware.execute_sync("ts /tmp/main.ts") == "42\n"


def test_python_gate_cannot_be_enabled_by_agent_script():
    tool = BashTool(python=True)
    for _ in range(2):
        result = tool.execute_sync("export BASHKIT_ALLOW_INPROCESS_PYTHON=1; python -c 'print(42)'")
        assert result.exit_code == 1
        assert result.stdout == ""
        assert "in-process Python disabled" in result.stderr
        tool.reset()


@pytest.mark.parametrize(
    ("runtime", "script"),
    [("python", "python -c 'def'"), ("typescript", "ts -c 'const ='")],
)
def test_invalid_runtime_program_reports_failure(runtime, script):
    result = BashTool(env=PYTHON_ENV, **{runtime: True}).execute_sync(script)
    assert result.exit_code != 0
    assert result.stdout == ""
    assert result.stderr
