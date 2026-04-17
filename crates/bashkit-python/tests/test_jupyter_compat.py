"""Jupyter / IPython compatibility: execute_sync + async callbacks inside a running loop.

Jupyter runs each cell inside a persistent asyncio event loop. This means:
  - ``await execute()`` works natively (Jupyter accepts top-level await)
  - ``execute_sync()`` must work even though asyncio forbids a second
    ``run_until_complete`` on the same thread while a loop is running
  - async ``custom_builtins`` / ``add_tool`` callbacks must run to completion
    despite having no free loop slot on the calling thread

Two simulation patterns are used:
  ``asyncio.run(cell())`` — boots a loop, then calls synchronous ``execute_sync``
                            from inside it; the closest analogue of a live
                            Jupyter cell running on a persistent loop
  ``@pytest.mark.asyncio`` — runs the test coroutine on pytest's managed loop,
                             identical to what Jupyter does for async cells

Surfaces covered: Bash, BashTool (``custom_builtins``), ScriptedTool
(``add_tool``).
"""

import asyncio
import contextvars
import gc

import pytest

from bashkit import Bash, BashTool, BuiltinContext, ScriptedTool

trace_id: contextvars.ContextVar[str] = contextvars.ContextVar("trace_id")


@pytest.fixture(autouse=True)
def _gc():
    yield
    gc.collect()


# ===========================================================================
# asyncio.run() simulation — explicit Jupyter analogue
#
# asyncio.run() creates a new event loop, runs the coroutine on it, then
# closes the loop. While the coroutine runs, the thread has a *running* loop,
# reproducing the same condition that Jupyter maintains across all cells.
# ===========================================================================


def test_bash_execute_sync_async_builtin_inside_asyncio_run():
    """execute_sync() with async custom_builtin works inside asyncio.run()."""

    async def greet(ctx: BuiltinContext) -> str:
        await asyncio.sleep(0)
        return f"hello {ctx.argv[0] if ctx.argv else 'world'}\n"

    async def jupyter_cell():
        bash = Bash(custom_builtins={"greet": greet})
        return bash.execute_sync("greet Jupyter")

    result = asyncio.run(jupyter_cell())
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello Jupyter"


def test_bash_execute_sync_contextvar_inside_asyncio_run():
    """ContextVars set before execute_sync() reach async callbacks inside asyncio.run()."""

    async def report(ctx: BuiltinContext) -> str:
        return f"trace={trace_id.get('none')}\n"

    async def jupyter_cell():
        trace_id.set("cell-run-42")
        bash = Bash(custom_builtins={"report": report})
        return bash.execute_sync("report")

    result = asyncio.run(jupyter_cell())
    assert result.exit_code == 0
    assert result.stdout.strip() == "trace=cell-run-42"


def test_scripted_tool_execute_sync_async_callback_inside_asyncio_run():
    """ScriptedTool.execute_sync() with async callback works inside asyncio.run()."""

    async def fetch(params, stdin=None):
        await asyncio.sleep(0)
        return f"data:{params.get('key', '?')}\n"

    async def jupyter_cell():
        tool = ScriptedTool("demo")
        tool.add_tool(
            "fetch",
            "Fetch by key",
            callback=fetch,
            schema={"type": "object", "properties": {"key": {"type": "string"}}},
        )
        return tool.execute_sync("fetch --key mykey")

    result = asyncio.run(jupyter_cell())
    assert result.exit_code == 0
    assert result.stdout.strip() == "data:mykey"


def test_bash_execute_sync_multiple_async_calls_inside_asyncio_run():
    """Multiple execute_sync() calls from the same 'cell' all succeed."""

    call_count = 0

    async def counter(ctx: BuiltinContext) -> str:
        nonlocal call_count
        await asyncio.sleep(0)
        call_count += 1
        return f"call:{call_count}\n"

    async def jupyter_cell():
        bash = Bash(custom_builtins={"counter": counter})
        r1 = bash.execute_sync("counter")
        r2 = bash.execute_sync("counter")
        r3 = bash.execute_sync("counter")
        return r1, r2, r3

    r1, r2, r3 = asyncio.run(jupyter_cell())
    assert r1.stdout.strip() == "call:1"
    assert r2.stdout.strip() == "call:2"
    assert r3.stdout.strip() == "call:3"


# ===========================================================================
# @pytest.mark.asyncio — live running loop, exact Jupyter match
# ===========================================================================


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
@pytest.mark.asyncio
async def test_execute_sync_async_builtin_live_loop(factory):
    """execute_sync() with async builtin works while pytest's event loop is running."""

    async def greet(ctx: BuiltinContext) -> str:
        await asyncio.sleep(0)
        return f"hello {ctx.argv[0] if ctx.argv else 'world'}\n"

    shell = factory(custom_builtins={"greet": greet})
    result = shell.execute_sync("greet Jupyter")

    assert result.exit_code == 0
    assert result.stdout.strip() == "hello Jupyter"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
@pytest.mark.asyncio
async def test_execute_sync_contextvar_live_loop(factory):
    """ContextVars propagate into async builtin callbacks with a live loop present."""

    async def report(ctx: BuiltinContext) -> str:
        return f"trace={trace_id.get('none')}\n"

    trace_id.set("live-loop-req")
    shell = factory(custom_builtins={"report": report})
    result = shell.execute_sync("report")

    assert result.exit_code == 0
    assert result.stdout.strip() == "trace=live-loop-req"


@pytest.mark.asyncio
async def test_scripted_tool_execute_sync_async_callback_live_loop():
    """ScriptedTool.execute_sync() async callback works with a live loop."""

    async def fetch(params, stdin=None):
        await asyncio.sleep(0)
        return f"data:{params.get('key', '?')}\n"

    tool = ScriptedTool("demo")
    tool.add_tool(
        "fetch",
        "Fetch by key",
        callback=fetch,
        schema={"type": "object", "properties": {"key": {"type": "string"}}},
    )
    result = tool.execute_sync("fetch --key live")

    assert result.exit_code == 0
    assert result.stdout.strip() == "data:live"


# ===========================================================================
# await execute() — natural async Jupyter cell
# ===========================================================================


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
@pytest.mark.asyncio
async def test_await_execute_async_builtin_uses_caller_loop(factory):
    """async builtin via await execute() runs on the caller's event loop."""

    caller_loop = asyncio.get_running_loop()
    captured: list = []

    async def inspect(ctx: BuiltinContext) -> str:
        captured.append(asyncio.get_running_loop())
        return "ok\n"

    shell = factory(custom_builtins={"inspect": inspect})
    result = await shell.execute("inspect")

    assert result.exit_code == 0
    assert captured == [caller_loop]


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
@pytest.mark.asyncio
async def test_await_execute_contextvar(factory):
    """ContextVars propagate into async builtins via await execute()."""

    async def report(ctx: BuiltinContext) -> str:
        return f"trace={trace_id.get('none')}\n"

    trace_id.set("await-req")
    shell = factory(custom_builtins={"report": report})
    result = await shell.execute("report")

    assert result.exit_code == 0
    assert result.stdout.strip() == "trace=await-req"
