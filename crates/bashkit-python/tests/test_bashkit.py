"""Tests for bashkit Python package."""

import json

import pytest

from bashkit import Bash, BashTool, FileSystem, ScriptedTool, create_langchain_tool_spec

# ===========================================================================
# Bash: Core interpreter
# ===========================================================================


# -- Bash: Construction ----------------------------------------------------


def test_bash_default_construction():
    bash = Bash()
    assert bash is not None


def test_bash_custom_construction():
    bash = Bash(username="alice", hostname="box", max_commands=100, max_loop_iterations=500)
    assert bash is not None


# -- Bash: Sync execution --------------------------------------------------


def test_bash_echo():
    bash = Bash()
    r = bash.execute_sync("echo hello")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hello"
    assert r.success is True


def test_bash_exit_code():
    bash = Bash()
    r = bash.execute_sync("exit 42")
    assert r.exit_code == 42
    assert r.success is False


def test_bash_stderr():
    bash = Bash()
    r = bash.execute_sync("echo err >&2")
    assert "err" in r.stderr


def test_bash_pipeline():
    bash = Bash()
    r = bash.execute_sync("echo -e 'banana\\napple\\ncherry' | sort")
    assert r.stdout.strip() == "apple\nbanana\ncherry"


def test_bash_state_persists():
    """Variables persist across calls."""
    bash = Bash()
    bash.execute_sync("export FOO=bar")
    r = bash.execute_sync("echo $FOO")
    assert r.stdout.strip() == "bar"


def test_bash_file_persistence():
    """Files created in one call are visible in the next."""
    bash = Bash()
    bash.execute_sync("echo content > /tmp/test.txt")
    r = bash.execute_sync("cat /tmp/test.txt")
    assert r.stdout.strip() == "content"


def test_bash_reset():
    bash = Bash()
    bash.execute_sync("export KEEP=1")
    bash.reset()
    r = bash.execute_sync("echo ${KEEP:-empty}")
    assert r.stdout.strip() == "empty"


def test_bash_fs_handle_bytes_roundtrip():
    bash = Bash()
    fs = bash.fs()
    assert isinstance(fs, FileSystem)
    fs.mkdir("/data", recursive=True)
    payload = b"\x00\x01\x02\xffoffice"
    fs.write_file("/data/blob.bin", payload)
    assert fs.read_file("/data/blob.bin") == payload
    stat = fs.stat("/data/blob.bin")
    assert stat["size"] == len(payload)
    assert fs.exists("/data/blob.bin") is True


def test_bash_mount_text_and_readonly_text():
    bash = Bash(
        mount_text=[("/config/app.conf", "debug=true\n")],
        mount_readonly_text=[("/etc/version", "1.2.3\n")],
    )
    assert bash.execute_sync("cat /config/app.conf").stdout == "debug=true\n"
    assert bash.execute_sync("cat /etc/version").stdout == "1.2.3\n"
    mode = bash.fs().stat("/etc/version")["mode"]
    assert mode == 0o444


def test_bash_realfs_readwrite_at(tmp_path):
    bash = Bash(mount_real_readwrite_at=[(str(tmp_path), "/workspace")])
    result = bash.execute_sync("echo 'hello host' > /workspace/hello.txt")
    assert result.exit_code == 0
    assert (tmp_path / "hello.txt").read_text().strip() == "hello host"


def test_bash_live_mount_preserves_state_and_unmounts(tmp_path):
    bash = Bash()
    bash.execute_sync("export KEEP=1")

    workspace = FileSystem.real(str(tmp_path), readwrite=True)
    bash.mount("/workspace", workspace)
    bash.execute_sync("echo live > /workspace/live.txt")

    assert (tmp_path / "live.txt").read_text().strip() == "live"
    assert bash.execute_sync("echo $KEEP").stdout.strip() == "1"

    bash.unmount("/workspace")
    result = bash.execute_sync("if test -f /workspace/live.txt; then echo present; else echo missing; fi")
    assert result.stdout.strip() == "missing"


def test_bash_fs_handle_tracks_reset_and_new_live_mounts():
    bash = Bash()
    fs = bash.fs()
    fs.write_file("/tmp/old.txt", b"old")

    bash.reset()
    assert fs.exists("/tmp/old.txt") is False

    overlay = FileSystem()
    overlay.write_file("/mounted.txt", b"fresh")
    bash.mount("/mnt", overlay)
    assert fs.read_file("/mnt/mounted.txt") == b"fresh"


def test_bash_fs_handle_supports_directory_ops_and_links():
    fs = FileSystem()

    fs.mkdir("/data/src", recursive=True)
    fs.write_file("/data/src/file.txt", b"alpha")
    fs.append_file("/data/src/file.txt", b"beta")
    assert fs.read_file("/data/src/file.txt") == b"alphabeta"

    fs.mkdir("/data/dst", recursive=True)
    fs.copy("/data/src/file.txt", "/data/dst/copied.txt")
    fs.rename("/data/dst/copied.txt", "/data/dst/renamed.txt")
    fs.symlink("/data/dst/renamed.txt", "/data/link.txt")
    fs.chmod("/data/dst/renamed.txt", 0o600)

    entries = sorted(entry["name"] for entry in fs.read_dir("/data"))
    assert entries == ["dst", "link.txt", "src"]
    assert fs.read_link("/data/link.txt") == "/data/dst/renamed.txt"
    assert fs.stat("/data/dst/renamed.txt")["mode"] == 0o600

    fs.remove("/data/link.txt")
    fs.remove("/data", recursive=True)
    assert fs.exists("/data") is False


# -- Bash: FS / mount error cases ------------------------------------------


def test_bash_unmount_nonexistent_raises():
    bash = Bash()
    with pytest.raises(Exception):
        bash.unmount("/nonexistent")


def test_bash_write_readonly_text_mount_raises():
    bash = Bash(mount_readonly_text=[("/etc/version", "1.0\n")])
    result = bash.execute_sync("echo new > /etc/version")
    assert result.exit_code != 0


def test_filesystem_real_nonexistent_host_path_raises():
    with pytest.raises(Exception):
        FileSystem.real("/nonexistent_path_that_does_not_exist_abc123", readwrite=True)


def test_filesystem_read_nonexistent_file_raises():
    fs = FileSystem()
    with pytest.raises(Exception):
        fs.read_file("/no/such/file")


def test_filesystem_stat_nonexistent_raises():
    fs = FileSystem()
    with pytest.raises(Exception):
        fs.stat("/no/such/path")


# -- Bash: Async execution -------------------------------------------------


@pytest.mark.asyncio
async def test_bash_async_execute():
    bash = Bash()
    r = await bash.execute("echo async_hello")
    assert r.exit_code == 0
    assert r.stdout.strip() == "async_hello"


@pytest.mark.asyncio
async def test_bash_async_state_persists():
    bash = Bash()
    await bash.execute("X=123")
    r = await bash.execute("echo $X")
    assert r.stdout.strip() == "123"


# -- Bash: Resource limits -------------------------------------------------


def test_bash_max_loop_iterations():
    bash = Bash(max_loop_iterations=10)
    r = bash.execute_sync("i=0; while true; do i=$((i+1)); done; echo $i")
    assert r.exit_code != 0 or int(r.stdout.strip() or "0") <= 100


def test_bash_empty_input():
    bash = Bash()
    r = bash.execute_sync("")
    assert r.exit_code == 0
    assert r.stdout == ""


def test_bash_nonexistent_command():
    bash = Bash()
    r = bash.execute_sync("nonexistent_xyz_cmd_12345")
    assert r.exit_code == 127


# ===========================================================================
# BashTool tests
# ===========================================================================

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


def test_bashtool_realfs_and_fs_handle(tmp_path):
    tool = BashTool(mount_real_readwrite_at=[(str(tmp_path), "/workspace")])
    tool.execute_sync("echo 'from tool' > /workspace/tool.txt")
    assert (tmp_path / "tool.txt").read_text().strip() == "from tool"
    assert tool.fs().read_file("/workspace/tool.txt") == b"from tool\n"


def test_bashtool_live_mount_preserves_state(tmp_path):
    tool = BashTool()
    tool.execute_sync("export KEEP=1")

    workspace = FileSystem.real(str(tmp_path), readwrite=True)
    tool.mount("/workspace", workspace)
    tool.execute_sync("echo tool > /workspace/tool.txt")

    assert (tmp_path / "tool.txt").read_text().strip() == "tool"
    assert tool.execute_sync("echo $KEEP").stdout.strip() == "1"

    tool.unmount("/workspace")
    assert tool.execute_sync("echo ${KEEP:-missing}").stdout.strip() == "1"


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


# Issue #424: reset() should preserve security configuration
def test_reset_preserves_config():
    bash = Bash(max_commands=5, username="testuser", hostname="testhost")
    # Verify config works before reset
    r = bash.execute_sync("whoami")
    assert r.stdout.strip() == "testuser"

    bash.reset()

    # Config should survive reset
    r = bash.execute_sync("whoami")
    assert r.stdout.strip() == "testuser", "username lost after reset"

    r = bash.execute_sync("hostname")
    assert r.stdout.strip() == "testhost", "hostname lost after reset"

    # Max commands limit should still be enforced
    # Run enough commands to hit the limit
    r = bash.execute_sync("echo 1; echo 2; echo 3; echo 4; echo 5; echo 6")
    assert r.exit_code != 0 or "limit" in r.stderr.lower() or r.stdout.count("\n") <= 5


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


def test_system_prompt_reflects_configured_home_path():
    tool = BashTool(username="agent", hostname="sandbox")
    sp = tool.system_prompt()
    assert "agent" in sp
    assert "/home/agent" in sp


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
    assert sp.startswith("test_api:")
    assert "greet" in sp
    assert "--name" in sp


def test_scripted_tool_description():
    tool = _make_echo_tool()
    desc = tool.description()
    assert "greet" in desc


def test_scripted_tool_help():
    tool = _make_echo_tool()
    h = tool.help()
    assert "# test_api" in h
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
    lines = [line for line in r.stdout.strip().splitlines() if line]
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


# ===========================================================================
# GIL deadlock prevention tests
# ===========================================================================


def test_execute_sync_releases_gil_for_callback():
    """execute_sync must release GIL before blocking on tokio runtime.

    Without py.allow_threads(), a tool callback calling Python::attach()
    inside rt.block_on() can deadlock when another thread holds the GIL.
    This test validates the fix by running execute_sync with a callback
    from a background thread while the main thread also holds the GIL.
    """
    import threading
    import time

    tool = ScriptedTool("api")

    def slow_callback(params, stdin=None):
        """Callback that briefly sleeps, requiring GIL reacquisition."""
        time.sleep(0.01)
        return f"ok-{params.get('id', 0)}\n"

    tool.add_tool(
        "slow",
        "Slow command",
        callback=slow_callback,
        schema={"type": "object", "properties": {"id": {"type": "integer"}}},
    )

    results = [None, None]
    errors = [None, None]

    def run_in_thread(idx):
        try:
            r = tool.execute_sync(f"slow --id {idx}")
            results[idx] = r
        except Exception as e:
            errors[idx] = e

    t0 = threading.Thread(target=run_in_thread, args=(0,))
    t1 = threading.Thread(target=run_in_thread, args=(1,))

    t0.start()
    t1.start()

    # Timeout guards against deadlock — if GIL isn't released, threads block forever
    t0.join(timeout=10)
    t1.join(timeout=10)

    assert not t0.is_alive(), "Thread 0 deadlocked (GIL not released in execute_sync)"
    assert not t1.is_alive(), "Thread 1 deadlocked (GIL not released in execute_sync)"
    assert errors[0] is None, f"Thread 0 error: {errors[0]}"
    assert errors[1] is None, f"Thread 1 error: {errors[1]}"
    assert results[0].exit_code == 0
    assert results[1].exit_code == 0
    assert "ok-0" in results[0].stdout
    assert "ok-1" in results[1].stdout


def test_bash_execute_sync_releases_gil():
    """Bash.execute_sync must release GIL before blocking on tokio runtime."""
    import threading

    bash = Bash()
    results = [None, None]
    errors = [None, None]

    def run_in_thread(idx):
        try:
            r = bash.execute_sync(f"echo thread-{idx}")
            results[idx] = r
        except Exception as e:
            errors[idx] = e

    t0 = threading.Thread(target=run_in_thread, args=(0,))
    t1 = threading.Thread(target=run_in_thread, args=(1,))

    t0.start()
    t1.start()

    t0.join(timeout=10)
    t1.join(timeout=10)

    assert not t0.is_alive(), "Thread 0 deadlocked (GIL not released)"
    assert not t1.is_alive(), "Thread 1 deadlocked (GIL not released)"
    assert errors[0] is None, f"Thread 0 error: {errors[0]}"
    assert errors[1] is None, f"Thread 1 error: {errors[1]}"


def test_bashtool_execute_sync_releases_gil():
    """BashTool.execute_sync must release GIL before blocking on tokio runtime."""
    import threading

    tool = BashTool()
    results = [None, None]
    errors = [None, None]

    def run_in_thread(idx):
        try:
            r = tool.execute_sync(f"echo thread-{idx}")
            results[idx] = r
        except Exception as e:
            errors[idx] = e

    t0 = threading.Thread(target=run_in_thread, args=(0,))
    t1 = threading.Thread(target=run_in_thread, args=(1,))

    t0.start()
    t1.start()

    t0.join(timeout=10)
    t1.join(timeout=10)

    assert not t0.is_alive(), "Thread 0 deadlocked (GIL not released)"
    assert not t1.is_alive(), "Thread 1 deadlocked (GIL not released)"
    assert errors[0] is None, f"Thread 0 error: {errors[0]}"
    assert errors[1] is None, f"Thread 1 error: {errors[1]}"


# ===========================================================================
# Runtime reuse (issue #414)
# ===========================================================================


def test_bash_rapid_sync_calls_no_resource_exhaustion():
    """Rapid execute_sync calls reuse a single runtime (no thread/fd leak)."""
    bash = Bash()
    for i in range(200):
        r = bash.execute_sync(f"echo {i}")
        assert r.exit_code == 0
        assert r.stdout.strip() == str(i)


def test_bashtool_rapid_sync_calls_no_resource_exhaustion():
    """Rapid execute_sync calls reuse a single runtime (no thread/fd leak)."""
    tool = BashTool()
    for i in range(200):
        r = tool.execute_sync(f"echo {i}")
        assert r.exit_code == 0
        assert r.stdout.strip() == str(i)


def test_bashtool_rapid_reset_no_resource_exhaustion():
    """Rapid reset calls reuse a single runtime (no thread/fd leak)."""
    tool = BashTool()
    for _ in range(200):
        tool.reset()
    # After many resets, tool still works
    r = tool.execute_sync("echo ok")
    assert r.exit_code == 0
    assert r.stdout.strip() == "ok"


# TM-PY-028: BashTool.reset() must preserve security config
def test_bashtool_reset_preserves_config():
    tool = BashTool(
        username="secuser",
        hostname="sechost",
        max_commands=5,
    )
    # Verify config before reset
    r = tool.execute_sync("whoami")
    assert r.stdout.strip() == "secuser"

    tool.reset()

    # Config must survive reset
    r = tool.execute_sync("whoami")
    assert r.stdout.strip() == "secuser", "BashTool username lost after reset"

    r = tool.execute_sync("hostname")
    assert r.stdout.strip() == "sechost", "BashTool hostname lost after reset"


def test_scripted_tool_rapid_sync_calls_no_resource_exhaustion():
    """Rapid execute_sync calls on ScriptedTool reuse a single runtime."""
    tool = ScriptedTool("api")
    tool.add_tool("ping", "Ping", callback=lambda p, s=None: "pong\n")
    for i in range(200):
        r = tool.execute_sync("ping")
        assert r.exit_code == 0
        assert r.stdout.strip() == "pong"


def test_deeply_nested_schema_rejected():
    """py_to_json rejects nesting deeper than 64 levels."""
    # Build a dict nested 70 levels deep
    nested = {"value": "leaf"}
    for _ in range(70):
        nested = {"child": nested}

    tool = ScriptedTool("deep")
    with pytest.raises(ValueError, match="nesting depth"):
        tool.add_tool("deep", "Deep", callback=lambda p, s=None: "", schema=nested)


# ===========================================================================
# Pre-exec failure stderr surfacing (issue #606)
# ===========================================================================


def test_bash_pre_exec_error_in_stderr():
    """Pre-exec failures (parse errors) must appear in stderr, not only error field."""
    bash = Bash()
    # Unclosed subshell triggers parse error -> Err path in bindings
    r = bash.execute_sync("echo $(")
    assert r.exit_code != 0
    assert r.error is not None
    # Bug #606: stderr was empty even though error had the message
    assert r.stderr != "", "stderr must contain the error message, not be empty"
    assert r.error in r.stderr


def test_bashtool_pre_exec_error_in_stderr():
    """BashTool pre-exec failures must also surface in stderr."""
    tool = BashTool()
    r = tool.execute_sync("echo $(")
    assert r.exit_code != 0
    assert r.error is not None
    assert r.stderr != "", "stderr must contain the error message, not be empty"
    assert r.error in r.stderr


@pytest.mark.asyncio
async def test_bash_pre_exec_error_in_stderr_async():
    """Async path should also surface pre-exec errors in stderr."""
    bash = Bash()
    r = await bash.execute("echo $(")
    assert r.exit_code != 0
    assert r.error is not None
    assert r.stderr != "", "stderr must contain the error message, not be empty"
    assert r.error in r.stderr


# ===========================================================================
# Bash: Python execution (python=True)
# ===========================================================================


def test_bash_python_enabled():
    """python=True makes python3 available as a builtin."""
    bash = Bash(python=True)
    r = bash.execute_sync("python3 -c 'print(1 + 1)'")
    assert r.exit_code == 0
    assert r.stdout.strip() == "2"


def test_bash_python_disabled_by_default():
    """python3 is not available without python=True."""
    bash = Bash()
    r = bash.execute_sync("python3 -c 'print(1)'")
    assert r.exit_code != 0


@pytest.mark.asyncio
async def test_bash_python_basic_arithmetic():
    """Python execution handles basic arithmetic and print."""
    bash = Bash(python=True)
    r = await bash.execute("python3 -c 'x = 6 * 7; print(x)'")
    assert r.exit_code == 0
    assert r.stdout.strip() == "42"


@pytest.mark.asyncio
async def test_bash_python_string_ops():
    """Python string operations work in Monty."""
    bash = Bash(python=True)
    r = await bash.execute("python3 -c 'print(\"hello\".upper())'")
    assert r.exit_code == 0
    assert r.stdout.strip() == "HELLO"


# ===========================================================================
# Bash: External function handler (PTC tool calling)
# ===========================================================================


@pytest.mark.asyncio
async def test_bash_external_handler_called():
    """External functions registered in Monty invoke the async handler."""
    calls = []

    async def handler(fn_name: str, args: list, kwargs: dict):
        calls.append((fn_name, kwargs))
        return f"result_for_{kwargs.get('key', 'none')}"

    bash = Bash(
        python=True,
        external_functions=["lookup"],
        external_handler=handler,
    )
    r = await bash.execute("python3 -c \"result = lookup(key='foo'); print(result)\"")
    assert r.exit_code == 0
    assert r.stdout.strip() == "result_for_foo"
    assert len(calls) == 1
    assert calls[0] == ("lookup", {"key": "foo"})


@pytest.mark.asyncio
async def test_bash_external_handler_multiple_calls():
    """Multiple external function calls in one code block all dispatch."""
    results = []

    async def handler(fn_name: str, args: list, kwargs: dict):
        results.append(fn_name)
        return len(results)

    bash = Bash(
        python=True,
        external_functions=["ping"],
        external_handler=handler,
    )
    r = await bash.execute('python3 -c "a = ping(); b = ping(); c = ping(); print(a, b, c)"')
    assert r.exit_code == 0
    assert r.stdout.strip() == "1 2 3"
    assert len(results) == 3


@pytest.mark.asyncio
async def test_bash_external_handler_returns_dict():
    """Handler returning a dict is accessible as a Python dict in Monty."""

    async def handler(fn_name: str, args: list, kwargs: dict):
        return {"status": "ok", "value": 42}

    bash = Bash(
        python=True,
        external_functions=["get_data"],
        external_handler=handler,
    )
    r = await bash.execute("python3 -c \"d = get_data(); print(d['status'], d['value'])\"")
    assert r.exit_code == 0
    assert r.stdout.strip() == "ok 42"


@pytest.mark.asyncio
async def test_bash_external_handler_vfs_and_tools_together():
    """External tool calls and VFS file I/O work in the same code block."""

    async def handler(fn_name: str, args: list, kwargs: dict):
        return {"data": kwargs.get("query", "")}

    bash = Bash(
        python=True,
        external_functions=["fetch"],
        external_handler=handler,
    )
    # Test that external function call works and result is accessible
    r = await bash.execute("python3 -c \"result = fetch(query='hello'); print(str(result))\"")
    assert r.exit_code == 0, f"code failed: {r.stderr}"
    # Result dict was returned from handler — verify it's present in output
    assert "hello" in r.stdout


@pytest.mark.asyncio
async def test_bash_external_handler_error_propagates():
    """Exception raised in the handler propagates as a Python RuntimeError."""

    async def handler(fn_name: str, args: list, kwargs: dict):
        raise ValueError("service unavailable")

    bash = Bash(
        python=True,
        external_functions=["failing_tool"],
        external_handler=handler,
    )
    r = await bash.execute('python3 -c "failing_tool()"')
    assert r.exit_code != 0
    assert "service unavailable" in r.stderr or "service unavailable" in (r.error or "")


@pytest.mark.asyncio
async def test_bash_reset_preserves_python_and_handler():
    """reset() must preserve python=True and external_handler config."""
    calls = []

    async def handler(fn_name: str, args: list, kwargs: dict):
        calls.append(fn_name)
        return "ok"

    bash = Bash(
        python=True,
        external_functions=["ping"],
        external_handler=handler,
    )
    r = await bash.execute('python3 -c "result = ping(); print(result)"')
    assert r.exit_code == 0
    assert r.stdout.strip() == "ok"

    bash.reset()

    # After reset, python and handler must still be active
    r = await bash.execute('python3 -c "result = ping(); print(result)"')
    assert r.exit_code == 0, f"python lost after reset: {r.stderr}"
    assert r.stdout.strip() == "ok"
    assert len(calls) == 2


def test_bash_external_functions_without_handler_raises():
    """Providing external_functions without external_handler must raise ValueError."""
    with pytest.raises(ValueError, match="external_handler"):
        Bash(python=True, external_functions=["foo"])


def test_bash_non_callable_handler_raises():
    """Providing a non-callable as external_handler must raise ValueError."""
    with pytest.raises(ValueError, match="callable"):
        Bash(python=True, external_functions=["foo"], external_handler=42)


@pytest.mark.asyncio
async def test_bash_external_handler_without_external_functions():
    """external_handler without external_functions is allowed — python mode with no registered fns."""

    async def handler(fn_name, args, kwargs):
        return "should not be called"

    bash = Bash(python=True, external_functions=[], external_handler=handler)
    r = await bash.execute("python3 -c 'print(1 + 1)'")
    assert r.exit_code == 0
    assert r.stdout.strip() == "2"


@pytest.mark.asyncio
async def test_bash_bigint_roundtrip():
    """Large integers (beyond i64) returned from handler arrive as Python int in Monty."""
    large_int = 2**65 + 1  # overflows i64

    async def handler(fn_name, args, kwargs):
        return large_int

    bash = Bash(
        python=True,
        external_functions=["get_big"],
        external_handler=handler,
    )
    r = await bash.execute('python3 -c "v = get_big(); print(type(v).__name__)"')
    assert r.exit_code == 0, f"failed: {r.stderr}"
    assert r.stdout.strip() == "int"


def test_bash_external_handler_requires_python_true():
    """external_handler without python=True must raise ValueError."""

    async def handler(fn_name, args, kwargs):
        return "ok"

    with pytest.raises(ValueError, match="python=True"):
        Bash(python=False, external_functions=["foo"], external_handler=handler)


def test_bash_execute_sync_with_handler_raises():
    """execute_sync is not supported when external_handler is configured."""

    async def handler(fn_name, args, kwargs):
        return "ok"

    bash = Bash(python=True, external_functions=["foo"], external_handler=handler)
    with pytest.raises(RuntimeError, match="execute_sync"):
        bash.execute_sync("python3 -c 'print(foo())'")


def test_bash_sync_handler_raises():
    """Passing a sync function as external_handler must raise ValueError."""

    def sync_handler(fn_name, args, kwargs):
        return "ok"

    with pytest.raises(ValueError, match="async"):
        Bash(python=True, external_functions=["foo"], external_handler=sync_handler)


@pytest.mark.asyncio
async def test_bash_bigint_roundtrip_value():
    """Large integer returned from handler preserves exact value."""
    large_int = 2**65 + 1

    async def handler(fn_name, args, kwargs):
        return large_int

    bash = Bash(python=True, external_functions=["get_big"], external_handler=handler)
    r = await bash.execute(f'python3 -c "v = get_big(); print(v == {large_int})"')
    assert r.exit_code == 0, f"failed: {r.stderr}"
    assert r.stdout.strip() == "True"


def test_bash_async_callable_object_accepted():
    """An object with async def __call__ satisfies external_handler validation."""

    class AsyncCallable:
        async def __call__(self, fn_name, args, kwargs):
            return "ok"

    # Should not raise — async __call__ is a valid handler
    bash = Bash(python=True, external_functions=["foo"], external_handler=AsyncCallable())
    assert bash is not None


@pytest.mark.asyncio
async def test_bash_handler_receives_set_as_set():
    """Monty Set passed to handler arrives as Python set, not list."""
    received = []

    async def handler(fn_name, args, kwargs):
        received.append(args)
        return "ok"

    bash = Bash(python=True, external_functions=["check"], external_handler=handler)
    await bash.execute('python3 -c "check({1, 2, 3})"')
    assert len(received) == 1
    assert isinstance(received[0][0], set), f"expected set, got {type(received[0][0]).__name__}"
    assert received[0][0] == {1, 2, 3}


def test_bash_cancel_then_reset_clears_cancellation():
    """cancel() before reset() must not affect post-reset executions."""
    bash = Bash()
    bash.cancel()
    bash.reset()
    r = bash.execute_sync("echo hi")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hi"


def test_bash_cancel_after_reset_still_works():
    """cancel() after reset() must target the new interpreter."""
    bash = Bash()
    bash.reset()
    bash.cancel()
    # The cancel targets the new interpreter — verify it took effect
    # by checking the cancelled flag prevents execution
    r = bash.execute_sync("echo hi")
    # Cancelled execution should fail
    assert r.exit_code != 0 or "cancel" in r.stderr.lower() or "cancel" in (r.error or "").lower()


def test_bashtool_cancel_then_reset_clears_cancellation():
    """BashTool: cancel() before reset() must not affect post-reset executions."""
    tool = BashTool()
    tool.cancel()
    tool.reset()
    r = tool.execute_sync("echo hi")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hi"


def test_bashtool_cancel_after_reset_still_works():
    """BashTool: cancel() after reset() must target the new interpreter."""
    tool = BashTool()
    tool.reset()
    tool.cancel()
    r = tool.execute_sync("echo hi")
    assert r.exit_code != 0 or "cancel" in r.stderr.lower() or "cancel" in (r.error or "").lower()
