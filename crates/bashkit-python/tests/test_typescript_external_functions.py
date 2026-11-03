"""TypeScript host callbacks in the Python agent integrations."""

import asyncio

import pytest

from bashkit import BashTool


def test_typescript_external_handler():
    calls = []

    async def handler(name, args):
        calls.append((name, args))
        await asyncio.sleep(0)
        return {"value": args[0] * args[1]["multiplier"]}

    tool = BashTool(
        typescript=True,
        typescript_external_functions=["lookup"],
        typescript_external_handler=handler,
    )
    result = tool.execute_sync("ts -c 'const r = await lookup(21, {multiplier: 2}); r.value'")
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"
    assert calls == [("lookup", [21, {"multiplier": 2}])]


@pytest.mark.parametrize(
    "options, message",
    [
        ({"typescript_external_functions": ["lookup"]}, "typescript_external_handler"),
        ({"typescript_external_handler": lambda *args: None}, "typescript=True"),
        ({"typescript": True, "typescript_external_handler": 42}, "callable"),
        ({"typescript": True, "typescript_external_handler": lambda *args: 42}, "async"),
    ],
)
def test_invalid_typescript_external_configuration(options, message):
    with pytest.raises(ValueError, match=message):
        BashTool(**options)


def test_both_languages_dispatch_independently_and_share_vfs():
    calls = []

    async def python_handler(name, args, kwargs):
        calls.append(("python", name, args, kwargs))
        return {"value": 21}

    async def typescript_handler(name, args):
        calls.append(("typescript", name, args))
        return {"value": int(args[0]["value"]) * 2}

    tool = BashTool(
        python=True,
        typescript=True,
        env={"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"},
        external_functions=["lookup"],
        external_handler=python_handler,
        typescript_external_functions=["lookup"],
        typescript_external_handler=typescript_handler,
    )
    result = tool.execute_sync(
        'python -c \'from pathlib import Path; n = Path("/tmp/value").write_text(str(lookup()["value"]))\'; '
        'ts -c \'const r = await lookup({value: await readFile("/tmp/value")}); '
        'await writeFile("/tmp/result", "" + r.value); r.value\''
    )
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"
    assert tool.read_file("/tmp/result") == "42"
    assert calls == [("python", "lookup", [], {}), ("typescript", "lookup", [{"value": "21"}])]
    for metadata in (tool.description(), tool.help(), tool.system_prompt()):
        assert "Python host functions: lookup" in metadata
        assert "TypeScript host functions: lookup" in metadata


def test_typescript_value_conversion_preserves_types():
    received = []

    class Handler:
        async def __call__(self, name, args):
            received.append(args)
            return {"items": args, "ok": True}

    tool = BashTool(typescript=True, typescript_external_functions=["lookup"], typescript_external_handler=Handler())
    result = tool.execute_sync(
        'ts -c \'const r = await lookup(null, undefined, true, false, 21, 0.5, "text", [1, 2], {key: "value"}); '
        "[r.items[4] * 2, r.ok]'"
    )
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42,true\n"
    assert received == [[None, None, True, False, 21, 0.5, "text", [1, 2], {"key": "value"}]]


@pytest.mark.parametrize("value", [object(), float("nan"), float("inf"), 2**100, {1: "invalid-key"}])
def test_typescript_rejects_non_json_handler_returns(value):
    async def handler(name, args):
        return value

    tool = BashTool(typescript=True, typescript_external_functions=["lookup"], typescript_external_handler=handler)
    assert tool.execute_sync("ts -c 'await lookup()'").exit_code != 0


def test_typescript_rejects_function_arguments_before_callback():
    calls = []

    async def handler(name, args):
        calls.append(args)

    tool = BashTool(typescript=True, typescript_external_functions=["lookup"], typescript_external_handler=handler)
    result = tool.execute_sync("ts -c 'await lookup(() => 42)'")
    assert result.exit_code != 0
    assert calls == []


@pytest.mark.parametrize("direction", ["input", "output"])
def test_typescript_conversion_depth_is_bounded(direction):
    async def handler(name, args):
        result = None
        for _ in range(70):
            result = [result]
        return result

    tool = BashTool(typescript=True, typescript_external_functions=["lookup"], typescript_external_handler=handler)
    script = (
        "await lookup()"
        if direction == "output"
        else ("let x = null; for (let i = 0; i < 70; i++) { x = [x]; } await lookup(x)")
    )
    result = tool.execute_sync(f"ts -c '{script}'")
    assert result.exit_code != 0
    assert "nesting depth" in result.stderr + (result.error or "")
