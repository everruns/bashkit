"""Tests for bashkit Python bindings."""

import json

import pytest

from bashkit import BashTool, ScriptedTool, create_langchain_tool_spec

# -- BashTool: Construction -------------------------------------------------


def test_default_construction():
    tool = BashTool()
    assert tool.name == "bashkit"
    assert isinstance(tool.short_description, str)
    assert isinstance(tool.version, str)


def test_custom_construction():
    tool = BashTool(username="alice", hostname="box", max_commands=100, max_loop_iterations=500)
    assert repr(tool) == 'BashTool(username="alice", hostname="box")'


# -- BashTool: Sync execution -----------------------------------------------


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


# -- BashTool: Async execution ----------------------------------------------


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


# -- BashTool: Reset --------------------------------------------------------


def test_reset():
    tool = BashTool()
    tool.execute_sync("export KEEP=1")
    tool.reset()
    r = tool.execute_sync("echo ${KEEP:-empty}")
    assert r.stdout.strip() == "empty"


# -- BashTool: LLM metadata ------------------------------------------------


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


# ===========================================================================
# ScriptedTool tests
# ===========================================================================


def _make_echo_tool():
    """Helper: ScriptedTool with one 'greet' command."""
    tool = ScriptedTool("test_api", short_description="Test API")
    tool.add_tool(
        "greet",
        "Greet a user",
        callback=lambda params, stdin=None: f"hello {params.get('name', 'world')}\n",
        schema={"type": "object", "properties": {"name": {"type": "string"}}},
    )
    return tool


# -- ScriptedTool: Construction ---------------------------------------------


def test_scripted_tool_construction():
    tool = ScriptedTool("my_api")
    assert tool.name == "my_api"
    assert tool.tool_count() == 0
    assert "ScriptedTool" in tool.short_description


def test_scripted_tool_custom_description():
    tool = ScriptedTool("api", short_description="My custom API")
    assert tool.short_description == "My custom API"


def test_scripted_tool_repr():
    tool = _make_echo_tool()
    assert "test_api" in repr(tool)
    assert "1" in repr(tool)  # tool count


# -- ScriptedTool: add_tool -------------------------------------------------


def test_add_tool_increments_count():
    tool = ScriptedTool("api")
    assert tool.tool_count() == 0
    tool.add_tool("cmd1", "Command 1", callback=lambda p, s=None: "ok\n")
    assert tool.tool_count() == 1
    tool.add_tool("cmd2", "Command 2", callback=lambda p, s=None: "ok\n")
    assert tool.tool_count() == 2


def test_add_tool_with_schema():
    tool = ScriptedTool("api")
    tool.add_tool(
        "get_user",
        "Fetch user",
        callback=lambda p, s=None: json.dumps({"id": p.get("id", 0)}) + "\n",
        schema={"type": "object", "properties": {"id": {"type": "integer"}}},
    )
    assert tool.tool_count() == 1


def test_add_tool_no_schema():
    tool = ScriptedTool("api")
    tool.add_tool("noop", "No-op", callback=lambda p, s=None: "ok\n")
    assert tool.tool_count() == 1


# -- ScriptedTool: execute_sync --------------------------------------------


def test_scripted_tool_single_call():
    tool = _make_echo_tool()
    r = tool.execute_sync("greet --name Alice")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hello Alice"


def test_scripted_tool_pipeline_with_jq():
    tool = ScriptedTool("api")
    tool.add_tool(
        "get_user",
        "Fetch user",
        callback=lambda p, s=None: '{"id": 1, "name": "Alice"}\n',
    )
    r = tool.execute_sync("get_user | jq -r '.name'")
    assert r.exit_code == 0
    assert r.stdout.strip() == "Alice"


def test_scripted_tool_multi_step():
    tool = ScriptedTool("api")
    tool.add_tool(
        "get_user",
        "Fetch user",
        callback=lambda p, s=None: f'{{"id": {p.get("id", 0)}, "name": "Bob"}}\n',
        schema={"type": "object", "properties": {"id": {"type": "integer"}}},
    )
    tool.add_tool(
        "get_orders",
        "Fetch orders",
        callback=lambda p, s=None: '[{"total": 10}, {"total": 20}]\n',
        schema={"type": "object", "properties": {"user_id": {"type": "integer"}}},
    )
    r = tool.execute_sync("""
        user=$(get_user --id 1)
        name=$(echo "$user" | jq -r '.name')
        total=$(get_orders --user_id 1 | jq '[.[].total] | add')
        echo "$name: $total"
    """)
    assert r.exit_code == 0
    assert r.stdout.strip() == "Bob: 30"


def test_scripted_tool_callback_error():
    tool = ScriptedTool("api")
    tool.add_tool(
        "fail_cmd",
        "Always fails",
        callback=lambda p, s=None: (_ for _ in ()).throw(ValueError("service down")),
    )
    r = tool.execute_sync("fail_cmd")
    assert r.exit_code != 0
    assert "service down" in r.stderr


def test_scripted_tool_error_fallback():
    tool = ScriptedTool("api")
    tool.add_tool(
        "fail_cmd",
        "Always fails",
        callback=lambda p, s=None: (_ for _ in ()).throw(ValueError("boom")),
    )
    r = tool.execute_sync("fail_cmd || echo fallback")
    assert r.exit_code == 0
    assert "fallback" in r.stdout


def test_scripted_tool_stdin_pipe():
    tool = ScriptedTool("api")
    tool.add_tool(
        "upper",
        "Uppercase stdin",
        callback=lambda p, stdin=None: (stdin or "").upper(),
    )
    r = tool.execute_sync("echo hello | upper")
    assert r.exit_code == 0
    assert r.stdout.strip() == "HELLO"


def test_scripted_tool_env_var():
    tool = ScriptedTool("api")
    tool.env("API_URL", "https://example.com")
    tool.add_tool("noop", "No-op", callback=lambda p, s=None: "ok\n")
    r = tool.execute_sync("echo $API_URL")
    assert r.exit_code == 0
    assert r.stdout.strip() == "https://example.com"


def test_scripted_tool_loop():
    tool = ScriptedTool("api")
    tool.add_tool(
        "get_user",
        "Fetch user",
        callback=lambda p, s=None: f'{{"name": "user{p.get("id", 0)}"}}\n',
        schema={"type": "object", "properties": {"id": {"type": "integer"}}},
    )
    r = tool.execute_sync("""
        for uid in 1 2 3; do
            get_user --id $uid | jq -r '.name'
        done
    """)
    assert r.exit_code == 0
    assert r.stdout.strip() == "user1\nuser2\nuser3"


def test_scripted_tool_conditional():
    tool = ScriptedTool("api")
    tool.add_tool(
        "check",
        "Check status",
        callback=lambda p, s=None: '{"ok": true}\n',
    )
    r = tool.execute_sync("""
        status=$(check | jq -r '.ok')
        if [ "$status" = "true" ]; then
            echo "healthy"
        else
            echo "unhealthy"
        fi
    """)
    assert r.exit_code == 0
    assert r.stdout.strip() == "healthy"


def test_scripted_tool_multiple_execute():
    """Multiple execute calls on the same tool work (stateless between calls)."""
    tool = _make_echo_tool()
    r1 = tool.execute_sync("greet --name Alice")
    assert r1.stdout.strip() == "hello Alice"
    r2 = tool.execute_sync("greet --name Bob")
    assert r2.stdout.strip() == "hello Bob"


def test_scripted_tool_empty_script():
    tool = _make_echo_tool()
    r = tool.execute_sync("")
    assert r.exit_code == 0
    assert r.stdout == ""


def test_scripted_tool_boolean_flag():
    tool = ScriptedTool("api")
    tool.add_tool(
        "search",
        "Search",
        callback=lambda p, s=None: f"verbose={p.get('verbose', False)}\n",
        schema={"type": "object", "properties": {"verbose": {"type": "boolean"}}},
    )
    r = tool.execute_sync("search --verbose")
    assert r.exit_code == 0
    assert r.stdout.strip() == "verbose=True"


def test_scripted_tool_integer_coercion():
    tool = ScriptedTool("api")
    tool.add_tool(
        "get",
        "Get by ID",
        callback=lambda p, s=None: f"id={p.get('id')} type={type(p.get('id')).__name__}\n",
        schema={"type": "object", "properties": {"id": {"type": "integer"}}},
    )
    r = tool.execute_sync("get --id 42")
    assert r.exit_code == 0
    assert r.stdout.strip() == "id=42 type=int"


# -- ScriptedTool: Async execution -----------------------------------------


@pytest.mark.asyncio
async def test_scripted_tool_async_execute():
    tool = _make_echo_tool()
    r = await tool.execute("greet --name Async")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hello Async"


# -- ScriptedTool: Introspection -------------------------------------------


def test_scripted_tool_system_prompt():
    tool = _make_echo_tool()
    sp = tool.system_prompt()
    assert "# test_api" in sp
    assert "greet" in sp
    assert "--name" in sp


def test_scripted_tool_description():
    tool = _make_echo_tool()
    desc = tool.description()
    assert "greet" in desc


def test_scripted_tool_help():
    tool = _make_echo_tool()
    h = tool.help()
    assert "TOOL COMMANDS" in h
    assert "greet" in h


def test_scripted_tool_schemas():
    tool = _make_echo_tool()
    inp = json.loads(tool.input_schema())
    assert "properties" in inp
    out = json.loads(tool.output_schema())
    assert "properties" in out


def test_scripted_tool_version():
    tool = _make_echo_tool()
    assert isinstance(tool.version, str)
    assert len(tool.version) > 0


# -- ScriptedTool: Many tools (12) -----------------------------------------


def test_scripted_tool_dozen_tools():
    """Register 12 tools and execute a multi-tool script."""
    tool = ScriptedTool("big_api", short_description="API with 12 commands")
    for i in range(12):
        name = f"cmd{i}"
        tool.add_tool(
            name,
            f"Command {i}",
            callback=lambda p, s=None, idx=i: f"result-{idx}\n",
        )
    assert tool.tool_count() == 12
    # Call all 12
    r = tool.execute_sync("; ".join(f"cmd{i}" for i in range(12)))
    assert r.exit_code == 0
    lines = r.stdout.strip().splitlines()
    assert lines == [f"result-{i}" for i in range(12)]


# ===========================================================================
# BashTool: Resource limit enforcement
# ===========================================================================


def test_max_loop_iterations_prevents_infinite_loop():
    """max_loop_iterations stops infinite loops."""
    tool = BashTool(max_loop_iterations=10)
    r = tool.execute_sync("i=0; while true; do i=$((i+1)); done; echo $i")
    # Should stop before completing — either error or truncated output
    assert r.exit_code != 0 or int(r.stdout.strip() or "0") <= 100


def test_max_commands_limits_execution():
    """max_commands stops after N commands."""
    tool = BashTool(max_commands=5)
    r = tool.execute_sync("echo 1; echo 2; echo 3; echo 4; echo 5; echo 6; echo 7; echo 8; echo 9; echo 10")
    # Should stop before all 10 commands complete
    lines = [l for l in r.stdout.strip().splitlines() if l]
    assert len(lines) < 10 or r.exit_code != 0


# ===========================================================================
# BashTool: Error conditions
# ===========================================================================


def test_malformed_bash_syntax():
    """Unclosed quotes produce an error."""
    tool = BashTool()
    r = tool.execute_sync('echo "unclosed')
    # Should fail with parse error
    assert r.exit_code != 0 or r.error is not None


def test_nonexistent_command():
    """Unknown commands return exit code 127."""
    tool = BashTool()
    r = tool.execute_sync("nonexistent_xyz_cmd_12345")
    assert r.exit_code == 127


def test_large_output():
    """Large output is handled without crash."""
    tool = BashTool()
    r = tool.execute_sync("for i in $(seq 1 1000); do echo line$i; done")
    assert r.exit_code == 0
    lines = r.stdout.strip().splitlines()
    assert len(lines) == 1000


def test_empty_input():
    """Empty script returns success."""
    tool = BashTool()
    r = tool.execute_sync("")
    assert r.exit_code == 0
    assert r.stdout == ""


# ===========================================================================
# ScriptedTool: Edge cases
# ===========================================================================


def test_scripted_tool_callback_runtime_error():
    """RuntimeError in callback is caught."""
    tool = ScriptedTool("api")
    tool.add_tool(
        "fail",
        "Fails with RuntimeError",
        callback=lambda p, s=None: (_ for _ in ()).throw(RuntimeError("runtime fail")),
    )
    r = tool.execute_sync("fail")
    assert r.exit_code != 0
    assert "runtime fail" in r.stderr


def test_scripted_tool_callback_type_error():
    """TypeError in callback is caught."""
    tool = ScriptedTool("api")
    tool.add_tool(
        "bad",
        "Fails with TypeError",
        callback=lambda p, s=None: (_ for _ in ()).throw(TypeError("bad type")),
    )
    r = tool.execute_sync("bad")
    assert r.exit_code != 0


def test_scripted_tool_large_callback_output():
    """Callbacks returning large output work."""
    tool = ScriptedTool("api")
    tool.add_tool(
        "big",
        "Returns large output",
        callback=lambda p, s=None: "x" * 10000 + "\n",
    )
    r = tool.execute_sync("big")
    assert r.exit_code == 0
    assert len(r.stdout.strip()) == 10000


def test_scripted_tool_callback_returns_empty():
    """Callback returning empty string is ok."""
    tool = ScriptedTool("api")
    tool.add_tool(
        "empty",
        "Returns nothing",
        callback=lambda p, s=None: "",
    )
    r = tool.execute_sync("empty")
    assert r.exit_code == 0


@pytest.mark.asyncio
async def test_async_multiple_tools():
    """Multiple async calls to different tools work."""
    tool = ScriptedTool("api")
    tool.add_tool("a", "Tool A", callback=lambda p, s=None: "A\n")
    tool.add_tool("b", "Tool B", callback=lambda p, s=None: "B\n")
    r = await tool.execute("a; b")
    assert r.exit_code == 0
    assert "A" in r.stdout
    assert "B" in r.stdout
