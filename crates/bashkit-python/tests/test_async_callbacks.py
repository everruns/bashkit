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
import gc
import threading
import time
import weakref

import pytest

from bashkit import ScriptedTool

# ---------------------------------------------------------------------------
# ContextVar used across tests
# ---------------------------------------------------------------------------

request_id: contextvars.ContextVar[str] = contextvars.ContextVar("request_id")
trace_writer: contextvars.ContextVar[list] = contextvars.ContextVar("trace_writer")


@pytest.fixture(autouse=True)
def _collect_between_tests():
    """Drop Rust-backed callback runtimes outside async test bodies on Python 3.12."""
    yield
    gc.collect()


# ===========================================================================
# Async callback basics
# ===========================================================================


def test_async_callback_execute_sync_honors_timeout():
    """execute_sync() timeout preempts slow async callbacks on private loop."""

    callback_done = threading.Event()

    async def slow(params, stdin=None):
        await asyncio.sleep(0.25)
        callback_done.set()
        return "late\n"

    tool = ScriptedTool("api", timeout_seconds=0.05)
    tool.add_tool("slow", "Slow", callback=slow)

    start = time.monotonic()
    r = tool.execute_sync("slow")
    elapsed = time.monotonic() - start

    assert elapsed < 0.2
    assert r.exit_code == 1
    assert "timeout" in (r.stderr + (r.error or "")).lower()
    # Wait until the abandoned private-loop worker finishes before proceeding,
    # so interpreter state is clean. Avoids the fixed sleep(0.3) that was both
    # slow and flaky (threading.Event gives exact synchronisation).
    assert callback_done.wait(timeout=2.0), "slow callback did not finish within 2 s"


def test_dealloc_during_inflight_callback_does_not_deadlock():
    """Dropping the tool while a timed-out callback still runs must not hang.

    Regression for TM-PY-030 (2): pyclass dealloc runs with the GIL held and
    joins the tool's tokio runtime; the abandoned callback task needs the GIL
    to finish, so a join while attached deadlocked the whole interpreter.
    Teardown must release the GIL around the join and bound it by cancelling
    the in-flight callback, so dealloc returns promptly either way.
    """

    settled = threading.Event()

    async def slow(params, stdin=None):
        try:
            await asyncio.sleep(0.25)
        finally:
            # Runs on completion AND on cancellation (deterministic teardown
            # cancels abandoned callbacks rather than awaiting them).
            settled.set()
        return "late\n"

    tool = ScriptedTool("api", timeout_seconds=0.05)
    tool.add_tool("slow", "Slow", callback=slow)

    r = tool.execute_sync("slow")
    assert r.exit_code == 1

    # Dealloc the tool (and its runtime) while the abandoned callback is
    # still sleeping on the private-loop worker thread. This must neither
    # deadlock nor wait out the sleep.
    begin = time.monotonic()
    del tool
    gc.collect()
    assert time.monotonic() - begin < 5.0, "teardown blocked on abandoned callback"

    assert settled.wait(timeout=2.0), "slow callback neither finished nor cancelled"


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


@pytest.mark.asyncio
async def test_async_callback_async_execute_uses_caller_loop():
    """Async execute() runs callbacks on the caller's active event loop."""

    caller_loop = asyncio.get_running_loop()

    async def inspect_loop(params, stdin=None):
        same_loop = asyncio.get_running_loop() is caller_loop
        return f"same_loop={same_loop}\n"

    tool = ScriptedTool("api")
    tool.add_tool("inspect_loop", "Inspect loop", callback=inspect_loop)
    r = await tool.execute("inspect_loop")

    assert r.exit_code == 0
    assert r.stdout.strip() == "same_loop=True"


@pytest.mark.asyncio
async def test_async_callback_async_execute_cancels_callback_task():
    """Cancelling execute() also cancels the underlying callback task."""

    started = asyncio.Event()
    released = asyncio.Event()
    cancelled = asyncio.Event()
    completed = []

    async def block(params, stdin=None):
        started.set()
        try:
            await released.wait()
            completed.append("completed")
            return "done\n"
        except asyncio.CancelledError:
            cancelled.set()
            raise

    tool = ScriptedTool("api")
    tool.add_tool("block", "Block", callback=block)
    future = tool.execute("block")

    await started.wait()
    future.cancel()
    with pytest.raises(asyncio.CancelledError):
        await future

    released.set()
    await asyncio.sleep(0.05)

    assert cancelled.is_set()
    assert completed == []


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


def test_async_callback_sync_execute_reuses_private_loop_within_script():
    """execute_sync() reuses one private loop across async callback invocations."""

    first_loop = None

    async def inspect_loop(params, stdin=None):
        nonlocal first_loop
        current_loop = asyncio.get_running_loop()
        same_loop = first_loop is None or first_loop is current_loop
        first_loop = current_loop
        return f"same_loop={same_loop}\n"

    tool = ScriptedTool("api")
    tool.add_tool("inspect_loop", "Inspect loop", callback=inspect_loop)
    r = tool.execute_sync("inspect_loop; inspect_loop")

    assert r.exit_code == 0
    assert r.stdout.splitlines() == ["same_loop=True", "same_loop=True"]


def test_async_callback_sync_execute_isolates_private_loops_per_threaded_call():
    """Concurrent execute_sync() calls on one ScriptedTool do not share a private loop."""

    first_started = threading.Event()
    results = {}
    errors = []

    async def inspect_loop(params, stdin=None):
        name = params.get("name", "world")
        if name == "slow":
            first_started.set()
            await asyncio.sleep(0.1)
        return f"{name}\n"

    tool = ScriptedTool("api")
    tool.add_tool(
        "inspect_loop",
        "Inspect loop",
        callback=inspect_loop,
        schema={"type": "object", "properties": {"name": {"type": "string"}}},
    )

    def run(name: str):
        try:
            results[name] = tool.execute_sync(f"inspect_loop --name {name}")
        except BaseException as exc:  # pragma: no cover - exercised only on failure.
            errors.append(exc)

    slow_thread = threading.Thread(target=run, args=("slow",))
    fast_thread = threading.Thread(target=run, args=("fast",))
    slow_thread.start()
    assert first_started.wait(timeout=5)
    fast_thread.start()
    slow_thread.join(timeout=5)
    fast_thread.join(timeout=5)

    assert not slow_thread.is_alive()
    assert not fast_thread.is_alive()
    assert errors == []
    assert results["slow"].exit_code == 0
    assert results["slow"].stdout.strip() == "slow"
    assert results["fast"].exit_code == 0
    assert results["fast"].stdout.strip() == "fast"


@pytest.mark.asyncio
async def test_async_execute_releases_finished_callback_tasks_before_completion():
    """Completed caller-loop callback tasks are released before execute() returns."""

    finalized = []
    blocker_started = asyncio.Event()
    blocker_released = asyncio.Event()

    async def emit(params, stdin=None):
        weakref.finalize(asyncio.current_task(), finalized.append, params["name"])
        return f"{params['name']}\n"

    async def block(params, stdin=None):
        blocker_started.set()
        await blocker_released.wait()
        return "released\n"

    tool = ScriptedTool("api")
    tool.add_tool(
        "emit",
        "Emit name",
        callback=emit,
        schema={"type": "object", "properties": {"name": {"type": "string"}}},
    )
    tool.add_tool("block", "Block", callback=block)

    future = tool.execute("emit --name one; emit --name two; emit --name three; block")

    await blocker_started.wait()
    for _ in range(50):
        gc.collect()
        await asyncio.sleep(0)
        if sorted(finalized) == ["one", "three", "two"]:
            break

    assert sorted(finalized) == ["one", "three", "two"]

    blocker_released.set()
    result = await future

    assert result.exit_code == 0
    assert result.stdout.splitlines() == ["one", "two", "three", "released"]


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


def test_async_callback_execute_sync_first_private_loop_call_does_not_deadlock():
    """First execute_sync call must not deadlock when the private-loop worker
    needs the GIL to create its asyncio event loop.

    Regression for TM-PY-030 variant (1): the work-dispatch channel was a
    rendezvous (sync_channel(0)), so the dispatcher blocked on send() while the
    worker blocked on the GIL — both waiting on each other. The fix switches the
    dispatch channel to unbounded so send() never blocks.

    Runs in a fresh subprocess with a 10 s timeout to reliably detect a deadlock
    without hanging the whole test suite.
    """
    import subprocess
    import sys
    import textwrap

    script = textwrap.dedent("""
        import asyncio
        from bashkit import ScriptedTool

        async def greet(params, stdin=None):
            return "hello\\n"

        tool = ScriptedTool("api")
        tool.add_tool("greet", "Greet", callback=greet)
        r = tool.execute_sync("greet")
        assert r.exit_code == 0, f"exit_code={r.exit_code} stderr={r.stderr!r}"
        assert r.stdout.strip() == "hello"
        print("ok")
    """)

    result = subprocess.run(
        [sys.executable, "-c", script],
        timeout=10,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, (
        f"subprocess failed (exit {result.returncode}):\n"
        f"stdout: {result.stdout}\nstderr: {result.stderr}"
    )
