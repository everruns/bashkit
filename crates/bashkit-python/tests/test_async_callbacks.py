"""Tests for async callback support and ContextVar propagation in ScriptedTool.

Covers:
- Async def callbacks registered via add_tool()
- ContextVar propagation into sync callbacks
- ContextVar propagation into async callbacks
- Mixed sync/async callbacks in a single ScriptedTool
- Concurrent async executions with isolated contexts
"""

import asyncio
import contextvars

import pytest

from bashkit import ScriptedTool

# ---------------------------------------------------------------------------
# ContextVar used across tests
# ---------------------------------------------------------------------------

request_id: contextvars.ContextVar[str] = contextvars.ContextVar("request_id")
trace_writer: contextvars.ContextVar[list] = contextvars.ContextVar("trace_writer")


# ===========================================================================
# Async callback basics
# ===========================================================================


def test_async_callback_sync_execute():
    """Async callback works via execute_sync()."""

    async def greet(params, stdin=None):
        name = params.get("name", "world")
        return f"hello {name}\n"

    tool = ScriptedTool("api")
    tool.add_tool(
        "greet",
        "Greet",
        callback=greet,
        schema={"type": "object", "properties": {"name": {"type": "string"}}},
    )
    r = tool.execute_sync("greet --name Async")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hello Async"


@pytest.mark.asyncio
async def test_async_callback_async_execute():
    """Async callback works via await execute()."""

    async def greet(params, stdin=None):
        name = params.get("name", "world")
        return f"hello {name}\n"

    tool = ScriptedTool("api")
    tool.add_tool(
        "greet",
        "Greet",
        callback=greet,
        schema={"type": "object", "properties": {"name": {"type": "string"}}},
    )
    r = await tool.execute("greet --name Awaited")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hello Awaited"


def test_async_callback_with_await():
    """Async callback that internally awaits (simulated async I/O)."""

    async def fetch_user(params, stdin=None):
        # Simulate async I/O with asyncio.sleep
        await asyncio.sleep(0)
        uid = params.get("id", "0")
        return f'{{"id": {uid}, "name": "Alice"}}\n'

    tool = ScriptedTool("api")
    tool.add_tool(
        "get_user",
        "Fetch user",
        callback=fetch_user,
        schema={"type": "object", "properties": {"id": {"type": "integer"}}},
    )
    r = tool.execute_sync("get_user --id 42 | jq -r '.name'")
    assert r.exit_code == 0
    assert r.stdout.strip() == "Alice"


def test_async_callback_error_propagates():
    """Errors from async callbacks propagate correctly."""

    async def failing(params, stdin=None):
        raise ValueError("async boom")

    tool = ScriptedTool("api")
    tool.add_tool("fail", "Fails", callback=failing)
    r = tool.execute_sync("fail")
    assert r.exit_code != 0


def test_async_callback_stdin_pipe():
    """Async callback receives stdin from pipe."""

    async def upper(params, stdin=None):
        return (stdin or "").upper()

    tool = ScriptedTool("api")
    tool.add_tool("upper", "Uppercase stdin", callback=upper)
    r = tool.execute_sync("echo hello | upper")
    assert r.exit_code == 0
    assert "HELLO" in r.stdout


# ===========================================================================
# Mixed sync + async callbacks
# ===========================================================================


def test_mixed_sync_async_callbacks():
    """ScriptedTool with both sync and async callbacks in one tool."""

    def sync_greet(params, stdin=None):
        return f"sync-hello {params.get('name', '?')}\n"

    async def async_greet(params, stdin=None):
        return f"async-hello {params.get('name', '?')}\n"

    tool = ScriptedTool("api")
    tool.add_tool("sync_greet", "Sync greet", callback=sync_greet)
    tool.add_tool("async_greet", "Async greet", callback=async_greet)
    r = tool.execute_sync('echo "$(sync_greet --name A) $(async_greet --name B)"')
    assert r.exit_code == 0
    assert "sync-hello A" in r.stdout
    assert "async-hello B" in r.stdout


# ===========================================================================
# ContextVar propagation — sync callbacks
# ===========================================================================


def test_contextvar_propagation_sync():
    """ContextVar set before execute_sync() is visible in sync callback."""

    def check_ctx(params, stdin=None):
        return f"req={request_id.get('MISSING')}\n"

    request_id.set("abc-123")
    tool = ScriptedTool("api")
    tool.add_tool("check", "Check ctx", callback=check_ctx)
    r = tool.execute_sync("check")
    assert r.exit_code == 0
    assert r.stdout.strip() == "req=abc-123"


@pytest.mark.asyncio
async def test_contextvar_propagation_sync_via_async_execute():
    """ContextVar set before await execute() is visible in sync callback."""

    def check_ctx(params, stdin=None):
        return f"req={request_id.get('MISSING')}\n"

    request_id.set("def-456")
    tool = ScriptedTool("api")
    tool.add_tool("check", "Check ctx", callback=check_ctx)
    r = await tool.execute("check")
    assert r.exit_code == 0
    assert r.stdout.strip() == "req=def-456"


# ===========================================================================
# ContextVar propagation — async callbacks
# ===========================================================================


def test_contextvar_propagation_async():
    """ContextVar set before execute_sync() is visible in async callback."""

    async def check_ctx(params, stdin=None):
        return f"req={request_id.get('MISSING')}\n"

    request_id.set("ghi-789")
    tool = ScriptedTool("api")
    tool.add_tool("check", "Check ctx", callback=check_ctx)
    r = tool.execute_sync("check")
    assert r.exit_code == 0
    assert r.stdout.strip() == "req=ghi-789"


@pytest.mark.asyncio
async def test_contextvar_propagation_async_via_async_execute():
    """ContextVar set before await execute() is visible in async callback."""

    async def check_ctx(params, stdin=None):
        return f"req={request_id.get('MISSING')}\n"

    request_id.set("jkl-012")
    tool = ScriptedTool("api")
    tool.add_tool("check", "Check ctx", callback=check_ctx)
    r = await tool.execute("check")
    assert r.exit_code == 0
    assert r.stdout.strip() == "req=jkl-012"


# ===========================================================================
# ContextVar isolation between concurrent executions
# ===========================================================================


@pytest.mark.asyncio
async def test_contextvar_isolation_concurrent():
    """Concurrent executions each see their own ContextVar snapshot."""
    results = {}

    async def capture_ctx(params, stdin=None):
        rid = request_id.get("NONE")
        return f"{rid}\n"

    async def run_with_id(rid: str):
        request_id.set(rid)
        tool = ScriptedTool("api")
        tool.add_tool("capture", "Capture", callback=capture_ctx)
        r = await tool.execute("capture")
        results[rid] = r.stdout.strip()

    await asyncio.gather(
        run_with_id("req-A"),
        run_with_id("req-B"),
        run_with_id("req-C"),
    )
    assert results["req-A"] == "req-A"
    assert results["req-B"] == "req-B"
    assert results["req-C"] == "req-C"


# ===========================================================================
# ContextVar with trace_writer pattern (LangGraph-like)
# ===========================================================================


def test_contextvar_trace_writer_pattern():
    """Simulate LangGraph's get_stream_writer() pattern via ContextVar."""
    events = []
    trace_writer.set(events)

    def emit_event(params, stdin=None):
        writer = trace_writer.get()
        writer.append(f"event:{params.get('msg', '')}")
        return "ok\n"

    tool = ScriptedTool("api")
    tool.add_tool("emit", "Emit event", callback=emit_event)
    r = tool.execute_sync("emit --msg hello; emit --msg world")
    assert r.exit_code == 0
    assert events == ["event:hello", "event:world"]


def test_contextvar_trace_writer_pattern_async():
    """Async version of trace_writer pattern."""
    events = []
    trace_writer.set(events)

    async def emit_event(params, stdin=None):
        writer = trace_writer.get()
        writer.append(f"event:{params.get('msg', '')}")
        return "ok\n"

    tool = ScriptedTool("api")
    tool.add_tool("emit", "Emit event", callback=emit_event)
    r = tool.execute_sync("emit --msg ping; emit --msg pong")
    assert r.exit_code == 0
    assert events == ["event:ping", "event:pong"]


# ===========================================================================
# Callable objects with async __call__
# ===========================================================================


def test_async_callable_object():
    """Object with async __call__ works as async callback."""

    class AsyncGreeter:
        async def __call__(self, params, stdin=None):
            return f"hello {params.get('name', '?')}\n"

    tool = ScriptedTool("api")
    tool.add_tool("greet", "Greet", callback=AsyncGreeter())
    r = tool.execute_sync("greet --name Object")
    assert r.exit_code == 0
    assert r.stdout.strip() == "hello Object"
