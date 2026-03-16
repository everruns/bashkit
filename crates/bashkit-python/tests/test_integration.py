"""Integration tests for bashkit Python bindings.

Covers: multi-step workflows, async/sync interleaving, ScriptedTool
complex orchestration, state management, concurrent usage, and
framework integration helpers.
"""

import asyncio
import json
import threading

import pytest

from bashkit import Bash, BashTool, ScriptedTool

# ===========================================================================
# Multi-step workflows
# ===========================================================================


def test_multi_step_file_workflow():
    """Create, read, modify, and verify a file across multiple calls."""
    bash = Bash()
    bash.execute_sync("echo 'initial content' > /tmp/workflow.txt")
    bash.execute_sync("echo 'appended line' >> /tmp/workflow.txt")
    r = bash.execute_sync("wc -l < /tmp/workflow.txt")
    assert r.exit_code == 0
    assert r.stdout.strip() == "2"
    r = bash.execute_sync("head -1 /tmp/workflow.txt")
    assert r.stdout.strip() == "initial content"


def test_directory_tree_workflow():
    """Build a directory tree, populate files, and verify structure."""
    bash = Bash()
    bash.execute_sync("mkdir -p /tmp/project/src /tmp/project/tests")
    bash.execute_sync("echo 'fn main() {}' > /tmp/project/src/main.rs")
    bash.execute_sync("echo '[test]' > /tmp/project/tests/test.rs")
    r = bash.execute_sync("find /tmp/project -type f | sort")
    assert r.exit_code == 0
    assert "/tmp/project/src/main.rs" in r.stdout
    assert "/tmp/project/tests/test.rs" in r.stdout


def test_pipeline_data_processing():
    """Multi-stage pipeline: generate, filter, sort, count."""
    bash = Bash()
    r = bash.execute_sync("""
        for i in $(seq 1 20); do echo "item-$i"; done \
        | grep -E 'item-[0-9]$' \
        | sort -t- -k2 -n \
        | wc -l
    """)
    assert r.exit_code == 0
    assert int(r.stdout.strip()) == 9  # item-1 through item-9


def test_variable_computation_workflow():
    """Use variables for multi-step computation."""
    bash = Bash()
    bash.execute_sync("SUM=0")
    bash.execute_sync("for i in 1 2 3 4 5; do SUM=$((SUM + i)); done")
    r = bash.execute_sync("echo $SUM")
    assert r.exit_code == 0
    assert r.stdout.strip() == "15"


# ===========================================================================
# Async/sync interleaving
# ===========================================================================


@pytest.mark.asyncio
async def test_async_then_sync_same_instance():
    """Async execute followed by sync execute on same Bash."""
    bash = Bash()
    r1 = await bash.execute("export PHASE=async")
    assert r1.exit_code == 0
    r2 = bash.execute_sync("echo $PHASE")
    assert r2.exit_code == 0
    assert r2.stdout.strip() == "async"


@pytest.mark.asyncio
async def test_sync_then_async_same_instance():
    """Sync execute followed by async execute on same Bash."""
    bash = Bash()
    r1 = bash.execute_sync("export MODE=sync")
    assert r1.exit_code == 0
    r2 = await bash.execute("echo $MODE")
    assert r2.exit_code == 0
    assert r2.stdout.strip() == "sync"


@pytest.mark.asyncio
async def test_interleaved_file_operations():
    """Interleave sync and async file operations."""
    bash = Bash()
    bash.execute_sync("echo line1 > /tmp/interleave.txt")
    await bash.execute("echo line2 >> /tmp/interleave.txt")
    bash.execute_sync("echo line3 >> /tmp/interleave.txt")
    r = await bash.execute("cat /tmp/interleave.txt")
    assert r.exit_code == 0
    lines = r.stdout.strip().splitlines()
    assert lines == ["line1", "line2", "line3"]


# ===========================================================================
# ScriptedTool: complex orchestration
# ===========================================================================


def _make_crud_tool():
    """Build a CRUD-like ScriptedTool for integration tests."""
    db = {}

    def create(params, stdin=None):
        key = params.get("key", "")
        value = params.get("value", "")
        db[key] = value
        return json.dumps({"created": key}) + "\n"

    def read(params, stdin=None):
        key = params.get("key", "")
        if key in db:
            return json.dumps({"key": key, "value": db[key]}) + "\n"
        return json.dumps({"error": "not found"}) + "\n"

    def list_all(params, stdin=None):
        return json.dumps(list(db.keys())) + "\n"

    def delete(params, stdin=None):
        key = params.get("key", "")
        if key in db:
            del db[key]
            return json.dumps({"deleted": key}) + "\n"
        return json.dumps({"error": "not found"}) + "\n"

    tool = ScriptedTool("crud_api", short_description="CRUD API")
    schema = {"type": "object", "properties": {"key": {"type": "string"}, "value": {"type": "string"}}}
    tool.add_tool("create", "Create a record", callback=create, schema=schema)
    tool.add_tool("read", "Read a record", callback=read, schema=schema)
    tool.add_tool("list_all", "List all keys", callback=list_all)
    tool.add_tool("delete", "Delete a record", callback=delete, schema=schema)
    return tool


def test_crud_workflow():
    """Full CRUD cycle via bash scripting."""
    tool = _make_crud_tool()
    r = tool.execute_sync("""
        create --key user1 --value Alice
        create --key user2 --value Bob
        list_all | jq -r '.[]' | sort
    """)
    assert r.exit_code == 0
    assert "user1" in r.stdout
    assert "user2" in r.stdout


def test_crud_with_conditional_logic():
    """CRUD operations with bash conditionals."""
    tool = _make_crud_tool()
    r = tool.execute_sync("""
        create --key config --value enabled
        status=$(read --key config | jq -r '.value')
        if [ "$status" = "enabled" ]; then
            echo "CONFIG_ACTIVE"
        else
            echo "CONFIG_INACTIVE"
        fi
    """)
    assert r.exit_code == 0
    assert "CONFIG_ACTIVE" in r.stdout


def test_crud_error_handling():
    """Error handling in CRUD workflows."""
    tool = _make_crud_tool()
    r = tool.execute_sync("""
        result=$(read --key nonexistent | jq -r '.error')
        if [ "$result" = "not found" ]; then
            echo "HANDLED"
        else
            echo "MISSED"
        fi
    """)
    assert r.exit_code == 0
    assert "HANDLED" in r.stdout


def test_scripted_tool_chained_pipes():
    """Chain multiple tools via pipes."""
    tool = ScriptedTool("transform")
    tool.add_tool("upper", "Uppercase", callback=lambda p, s=None: (s or "").upper())
    tool.add_tool("prefix", "Add prefix", callback=lambda p, s=None: "PREFIX:" + (s or ""))
    r = tool.execute_sync("echo hello | upper | prefix")
    assert r.exit_code == 0
    assert "PREFIX:HELLO" in r.stdout


def test_scripted_tool_loop_with_accumulation():
    """Loop over tools and accumulate results in a variable."""
    tool = ScriptedTool("calc")
    tool.add_tool(
        "double",
        "Double a number",
        callback=lambda p, s=None: str(int(p.get("n", 0)) * 2) + "\n",
        schema={"type": "object", "properties": {"n": {"type": "integer"}}},
    )
    r = tool.execute_sync("""
        result=""
        for i in 1 2 3 4 5; do
            val=$(double --n $i)
            result="$result $val"
        done
        echo $result
    """)
    assert r.exit_code == 0
    nums = r.stdout.strip().split()
    assert nums == ["2", "4", "6", "8", "10"]


# ===========================================================================
# State management
# ===========================================================================


def test_reset_clears_files_and_vars():
    """reset() must clear both files and environment variables."""
    bash = Bash()
    bash.execute_sync("export MYVAR=hello")
    bash.execute_sync("echo data > /tmp/resettest.txt")
    bash.reset()
    r1 = bash.execute_sync("echo ${MYVAR:-cleared}")
    assert r1.stdout.strip() == "cleared"
    r2 = bash.execute_sync("cat /tmp/resettest.txt")
    assert r2.exit_code != 0


def test_bashtool_reset_clears_state():
    """BashTool reset() clears state but preserves config."""
    tool = BashTool(username="testuser")
    tool.execute_sync("export SECRET=123")
    tool.execute_sync("echo data > /tmp/toolreset.txt")
    tool.reset()
    r1 = tool.execute_sync("echo ${SECRET:-cleared}")
    assert r1.stdout.strip() == "cleared"
    r2 = tool.execute_sync("whoami")
    assert r2.stdout.strip() == "testuser"


def test_multiple_resets_stable():
    """Multiple consecutive resets don't break the interpreter."""
    bash = Bash()
    for i in range(10):
        bash.execute_sync(f"export V{i}=val{i}")
        bash.reset()
    r = bash.execute_sync("echo ok")
    assert r.exit_code == 0
    assert r.stdout.strip() == "ok"


# ===========================================================================
# Concurrent usage
# ===========================================================================


def test_concurrent_bash_instances():
    """Multiple Bash instances used from different threads."""
    results = [None] * 4
    errors = [None] * 4

    def worker(idx):
        try:
            bash = Bash()
            r = bash.execute_sync(f"echo thread-{idx}; echo done-{idx}")
            results[idx] = r
        except Exception as e:
            errors[idx] = e

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=15)

    for i in range(4):
        assert errors[i] is None, f"Thread {i} failed: {errors[i]}"
        assert results[i] is not None, f"Thread {i} returned None"
        assert results[i].exit_code == 0
        assert f"thread-{i}" in results[i].stdout


def test_concurrent_scripted_tool_instances():
    """Multiple ScriptedTool instances from different threads."""
    results = [None] * 3
    errors = [None] * 3

    def worker(idx):
        try:
            tool = ScriptedTool(f"api_{idx}")
            tool.add_tool("id", "Return ID", callback=lambda p, s=None, i=idx: f"tool-{i}\n")
            r = tool.execute_sync("id")
            results[idx] = r
        except Exception as e:
            errors[idx] = e

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(3)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=15)

    for i in range(3):
        assert errors[i] is None, f"Thread {i} failed: {errors[i]}"
        assert results[i].exit_code == 0
        assert f"tool-{i}" in results[i].stdout


# ===========================================================================
# Async concurrent operations
# ===========================================================================


@pytest.mark.asyncio
async def test_async_concurrent_executions():
    """Multiple async executions on separate instances concurrently."""
    instances = [Bash() for _ in range(3)]
    tasks = [inst.execute(f"echo async-{i}") for i, inst in enumerate(instances)]
    results = await asyncio.gather(*tasks)
    for i, r in enumerate(results):
        assert r.exit_code == 0
        assert f"async-{i}" in r.stdout


@pytest.mark.asyncio
async def test_async_scripted_tool_concurrent():
    """Concurrent async ScriptedTool executions."""
    tools = []
    for i in range(3):
        t = ScriptedTool(f"api_{i}")
        t.add_tool("ping", "Ping", callback=lambda p, s=None, idx=i: f"pong-{idx}\n")
        tools.append(t)
    tasks = [t.execute("ping") for t in tools]
    results = await asyncio.gather(*tasks)
    for i, r in enumerate(results):
        assert r.exit_code == 0
        assert f"pong-{i}" in r.stdout


# ===========================================================================
# ExecResult contract
# ===========================================================================


def test_exec_result_to_dict_contract():
    """ExecResult.to_dict() returns all expected fields."""
    bash = Bash()
    r = bash.execute_sync("echo hello")
    d = r.to_dict()
    assert set(d.keys()) == {"stdout", "stderr", "exit_code", "error", "stdout_truncated", "stderr_truncated", "final_env"}
    assert isinstance(d["stdout"], str)
    assert isinstance(d["stderr"], str)
    assert isinstance(d["exit_code"], int)
    assert isinstance(d["stdout_truncated"], bool)
    assert isinstance(d["stderr_truncated"], bool)


def test_exec_result_success_property():
    """ExecResult.success matches exit_code == 0."""
    bash = Bash()
    r_ok = bash.execute_sync("true")
    assert r_ok.success is True
    assert r_ok.exit_code == 0
    r_fail = bash.execute_sync("false")
    assert r_fail.success is False
    assert r_fail.exit_code != 0


def test_exec_result_error_on_parse_failure():
    """Parse failures set both error and stderr fields."""
    bash = Bash()
    r = bash.execute_sync("echo $(")
    assert r.exit_code != 0
    assert r.error is not None
    assert r.stderr != ""


# ===========================================================================
# BashTool metadata consistency
# ===========================================================================


def test_bashtool_schemas_are_valid_json():
    """input_schema and output_schema return valid JSON."""
    tool = BashTool()
    inp = json.loads(tool.input_schema())
    assert "properties" in inp or "type" in inp
    out = json.loads(tool.output_schema())
    assert "properties" in out or "type" in out


def test_bashtool_system_prompt_includes_username():
    """system_prompt reflects configured username."""
    tool = BashTool(username="agent007")
    sp = tool.system_prompt()
    assert "agent007" in sp


def test_bashtool_version_format():
    """Version string looks like a semver."""
    tool = BashTool()
    parts = tool.version.split(".")
    assert len(parts) >= 2
    assert all(p.isdigit() for p in parts[:2])


# ===========================================================================
# ScriptedTool metadata consistency
# ===========================================================================


def test_scripted_tool_system_prompt_lists_all_tools():
    """System prompt includes all registered tool names."""
    tool = ScriptedTool("multi")
    for name in ["alpha", "beta", "gamma"]:
        tool.add_tool(name, f"Tool {name}", callback=lambda p, s=None: "ok\n")
    sp = tool.system_prompt()
    for name in ["alpha", "beta", "gamma"]:
        assert name in sp


def test_scripted_tool_help_markdown():
    """Help output is valid markdown with tool name header."""
    tool = ScriptedTool("myapi")
    tool.add_tool("cmd", "A command", callback=lambda p, s=None: "ok\n")
    h = tool.help()
    assert "# myapi" in h
    assert "cmd" in h


# ===========================================================================
# Edge cases
# ===========================================================================


def test_very_long_command():
    """Very long command strings don't crash."""
    bash = Bash()
    # 10K character command
    long_val = "x" * 10000
    r = bash.execute_sync(f"echo '{long_val}'")
    assert r.exit_code == 0
    assert len(r.stdout.strip()) == 10000


def test_many_sequential_commands():
    """50 sequential commands in one script."""
    bash = Bash()
    cmds = "; ".join(f"echo line{i}" for i in range(50))
    r = bash.execute_sync(cmds)
    assert r.exit_code == 0
    lines = r.stdout.strip().splitlines()
    assert len(lines) == 50


def test_empty_pipeline_stages():
    """Commands with empty output in pipeline don't crash."""
    bash = Bash()
    r = bash.execute_sync("echo '' | cat | cat")
    assert r.exit_code == 0


@pytest.mark.asyncio
async def test_async_error_propagation():
    """Async execution properly propagates errors."""
    bash = Bash()
    r = await bash.execute("exit 42")
    assert r.exit_code == 42
    assert r.success is False
