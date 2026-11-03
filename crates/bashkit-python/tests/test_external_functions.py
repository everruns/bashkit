"""Host Python functions use execution-local callback loops and contexts.

Decision: exercise sync, async, worker-thread and framework entry points with
real interpreters. A constructor-forwarding mock cannot detect loop affinity
errors, deadlocks or request-context leakage.
"""

import asyncio
import contextvars
import gc
import subprocess
import sys
import textwrap
import threading
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass

import pytest

from bashkit import BashTool

request_id = contextvars.ContextVar("external_request_id", default="missing")
PYTHON_ENV = {"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"}


@pytest.fixture(autouse=True)
def collect_runtimes():
    yield
    gc.collect()


@dataclass(frozen=True)
class Runtime:
    name: str

    @property
    def script(self):
        if self.name == "python":
            return "python -c 'print(lookup(21, multiplier=2)[\"value\"])'"
        return "ts -c 'const r = await lookup(21, {multiplier: 2}); r.value'"

    @property
    def print_call(self):
        if self.name == "python":
            return "python -c 'print(lookup())'"
        return "ts -c 'await lookup()'"

    @property
    def unknown_script(self):
        return "python -c 'unregistered()'" if self.name == "python" else "ts -c 'await unregistered()'"

    def options(self, handler):
        if self.name == "python":
            return dict(python=True, env=PYTHON_ENV, external_functions=["lookup"], external_handler=handler)

        async def dispatch(name, args):
            return await handler(name, args[:1], args[1] if len(args) > 1 else {})

        return dict(typescript=True, typescript_external_functions=["lookup"], typescript_external_handler=dispatch)

    def make_tool(self, handler, **kwargs):
        return BashTool(**self.options(handler), **kwargs)


@pytest.fixture(params=["python", "typescript"])
def runtime(request):
    return Runtime(request.param)


async def lookup(name, args, kwargs):
    assert name == "lookup"
    await asyncio.sleep(0)
    return {"value": args[0] * kwargs["multiplier"]}


def test_sync_external_functions(runtime):
    tool = runtime.make_tool(lookup)
    result = tool.execute_sync(runtime.script)
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"


@pytest.mark.asyncio
async def test_async_external_functions_use_caller_loop_and_context(runtime):
    loop = asyncio.get_running_loop()
    thread = threading.get_ident()
    gate = loop.create_future()

    async def handler(name, args, kwargs):
        assert asyncio.get_running_loop() is loop
        assert threading.get_ident() == thread
        assert request_id.get() == "caller"
        return await gate

    tool = runtime.make_tool(handler)
    token = request_id.set("caller")
    try:
        pending = tool.execute(runtime.script)
        loop.call_soon(gate.set_result, {"value": 42})
        result = await asyncio.wait_for(pending, 5)
    finally:
        request_id.reset(token)
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"


def test_sync_callbacks_from_multiple_threads_preserve_private_loop(runtime):
    seen = []

    async def handler(name, args, kwargs):
        seen.append((asyncio.get_running_loop(), threading.get_ident(), request_id.get()))
        await asyncio.sleep(0)
        return {"value": 42}

    tool = runtime.make_tool(handler)

    def run(index):
        request_id.set(str(index))
        result = tool.execute_sync(runtime.script)
        assert result.exit_code == 0, result.stderr
        return result.stdout

    with ThreadPoolExecutor(max_workers=4) as workers:
        assert list(workers.map(run, range(12))) == ["42\n"] * 12
    assert len({id(loop) for loop, _, _ in seen}) == 1
    assert len({thread for _, thread, _ in seen}) == 1
    assert {context for _, _, context in seen} == {str(i) for i in range(12)}


def test_tool_reused_across_event_loops_and_threads(runtime):
    async def handler(name, args, kwargs):
        assert request_id.get() == str(threading.get_ident())
        await asyncio.sleep(0)
        return {"value": 42}

    tool = runtime.make_tool(handler)

    async def run():
        request_id.set(str(threading.get_ident()))
        result = await asyncio.wait_for(tool.execute(runtime.script), 5)
        assert result.exit_code == 0, result.stderr
        assert result.stdout == "42\n"

    for _ in range(2):
        asyncio.run(run())
    with ThreadPoolExecutor(max_workers=3) as workers:
        list(workers.map(lambda _: asyncio.run(run()), range(6)))


@pytest.mark.parametrize("loop_kind", ["selector", "uvloop"])
def test_alternative_event_loop(runtime, loop_kind):
    if loop_kind == "uvloop":
        loop = pytest.importorskip("uvloop").new_event_loop()
    else:
        loop = asyncio.SelectorEventLoop()

    async def handler(name, args, kwargs):
        assert asyncio.get_running_loop() is loop
        assert request_id.get() == "alternate-loop"
        await asyncio.sleep(0)
        return {"value": 42}

    tool = runtime.make_tool(handler)

    async def run():
        token = request_id.set("alternate-loop")
        try:
            result = await asyncio.wait_for(tool.execute(runtime.script), 5)
            assert result.stdout == "42\n", result.stderr
        finally:
            request_id.reset(token)

    try:
        loop.run_until_complete(run())
    finally:
        loop.close()


def test_sync_access_cannot_block_an_active_handler_loop(runtime):
    # A separate process bounds native lock waits even if the calling Python
    # loop deadlocks. Async requests must still be allowed to queue on it.
    code = textwrap.dedent("""
        import asyncio
        import sys
        from bashkit import BashTool

        async def main():
            started, released = asyncio.Event(), asyncio.Event()
            async def handler(*args):
                started.set()
                await released.wait()
                return 42
            if sys.argv[1] == "python":
                options = dict(python=True, env={"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"},
                               external_functions=["lookup"], external_handler=handler)
                script = "python -c 'lookup()'"
            else:
                options = dict(typescript=True, typescript_external_functions=["lookup"],
                               typescript_external_handler=handler)
                script = "ts -c 'await lookup()'"
            tool = BashTool(**options, files={"/tmp/state": "data"})
            fs = tool.fs()
            active = tool.execute(script)
            await asyncio.wait_for(started.wait(), 2)
            queued = tool.execute("echo queued")
            for operation in (lambda: tool.execute_sync("echo unsafe"),
                              lambda: tool.read_file("/tmp/state"), tool.snapshot,
                              lambda: fs.read_file("/tmp/state")):
                try:
                    operation()
                except RuntimeError as exc:
                    assert "would block" in str(exc), str(exc)
                else:
                    raise AssertionError("synchronous access blocked an active handler loop")
            released.set()
            assert (await active).exit_code == 0
            assert (await queued).stdout == "queued\\n"
        asyncio.run(main())
    """)
    # Wheel CI runs from the Python package directory. Match the parent's
    # import paths so the source checkout cannot shadow the installed wheel.
    preamble = "import sys; sys.path[:0] = %r\n" % [p for p in sys.path if p]
    result = subprocess.run(
        [sys.executable, "-c", preamble + code, runtime.name], capture_output=True, text=True, timeout=8
    )
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.asyncio
async def test_sync_external_functions_inside_running_notebook_loop(runtime):
    caller = asyncio.get_running_loop()

    async def handler(name, args, kwargs):
        assert asyncio.get_running_loop() is not caller
        assert request_id.get() == "cell"
        await asyncio.sleep(0)
        return {"value": 42}

    token = request_id.set("cell")
    try:
        result = runtime.make_tool(handler).execute_sync(runtime.script)
    finally:
        request_id.reset(token)
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"


@pytest.mark.parametrize("keyed", [False, True])
def test_external_configuration_survives_reset_and_snapshot(runtime, keyed):
    tool = runtime.make_tool(lookup)
    tool.execute_sync("echo persisted > /tmp/state")
    key = b"k" * 32
    snapshot = tool.snapshot_keyed(key) if keyed else tool.snapshot()
    factory = BashTool.from_snapshot_keyed if keyed else BashTool.from_snapshot
    args = (snapshot, key) if keyed else (snapshot,)
    restored = factory(*args, **runtime.options(lookup))
    assert restored.read_file("/tmp/state") == "persisted\n"
    for _ in range(2):
        result = restored.execute_sync(runtime.script)
        assert result.exit_code == 0, result.stderr
        assert result.stdout == "42\n"
        restored.reset()


@pytest.mark.parametrize(
    "options, message",
    [
        ({"external_functions": ["lookup"]}, "external_handler"),
        ({"external_handler": lookup}, "python=True"),
        ({"python": True, "external_handler": 42}, "callable"),
        ({"python": True, "external_handler": lambda *args: 42}, "async"),
    ],
)
def test_invalid_external_configuration(options, message):
    with pytest.raises(ValueError, match=message):
        BashTool(**options)


def test_external_functions_preserve_host_python_gate():
    tool = BashTool(python=True, external_functions=["lookup"], external_handler=lookup)
    result = tool.execute_sync("export BASHKIT_ALLOW_INPROCESS_PYTHON=1; python -c 'print(lookup())'")
    assert result.exit_code != 0
    assert "in-process Python disabled" in result.stderr


@pytest.mark.asyncio
async def test_cancellation_cancels_only_this_execution_and_allows_reuse(runtime):
    started, cancelled = asyncio.Event(), asyncio.Event()

    async def handler(name, args, kwargs):
        started.set()
        try:
            await asyncio.Event().wait()
        finally:
            cancelled.set()

    tool = runtime.make_tool(handler)
    pending = tool.execute(runtime.script)
    await asyncio.wait_for(started.wait(), 5)
    pending.cancel()
    with pytest.raises(asyncio.CancelledError):
        await pending
    await asyncio.wait_for(cancelled.wait(), 5)
    result = await asyncio.wait_for(tool.execute("echo reusable"), 5)
    assert result.stdout == "reusable\n"


@pytest.mark.asyncio
async def test_timeout_cancels_handler(runtime):
    cancelled = asyncio.Event()

    async def handler(name, args, kwargs):
        try:
            await asyncio.Event().wait()
        finally:
            cancelled.set()

    tool = runtime.make_tool(handler, timeout_seconds=0.2)
    result = await asyncio.wait_for(tool.execute(runtime.script), 5)
    assert result.exit_code != 0
    await asyncio.wait_for(cancelled.wait(), 5)
    assert (await tool.execute("echo reusable")).stdout == "reusable\n"


def test_sync_timeout_cancels_private_loop_callback_and_allows_reuse(runtime):
    cancelled = threading.Event()
    calls = 0

    async def handler(name, args, kwargs):
        nonlocal calls
        calls += 1
        if calls == 1:
            try:
                await asyncio.Event().wait()
            finally:
                cancelled.set()
        return {"value": 42}

    tool = runtime.make_tool(handler, timeout_seconds=0.2)
    result = tool.execute_sync(runtime.script)
    assert result.exit_code != 0
    assert cancelled.wait(2), "timed-out callback kept running on the private loop"
    result = tool.execute_sync(runtime.script)
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"


@pytest.mark.asyncio
async def test_unrelated_calls_queue_while_handler_is_running(runtime):
    started, released = asyncio.Event(), asyncio.Event()

    async def handler(name, args, kwargs):
        started.set()
        await released.wait()
        return {"value": 42}

    tool = runtime.make_tool(handler)
    first = tool.execute(runtime.script)
    await asyncio.wait_for(started.wait(), 5)
    second = tool.execute("echo queued")
    released.set()
    results = await asyncio.wait_for(asyncio.gather(first, second), 5)
    assert [r.stdout for r in results] == ["42\n", "queued\n"]


@pytest.mark.asyncio
@pytest.mark.parametrize("capture", [None, 64])
async def test_cancelling_queued_request_preserves_active_handler(runtime, capture):
    started, released = asyncio.Event(), asyncio.Event()

    async def handler(name, args, kwargs):
        started.set()
        await released.wait()
        return {"value": 42}

    tool = runtime.make_tool(handler)
    active = tool.execute(runtime.script, capture=capture)
    await asyncio.wait_for(started.wait(), 5)
    queued = tool.execute("echo must-not-run > /tmp/cancelled-request", capture=capture)
    queued.cancel()
    with pytest.raises(asyncio.CancelledError):
        await queued
    released.set()
    result = await asyncio.wait_for(active, 5)
    assert result.stdout == "42\n", result.stderr
    assert not tool.exists("/tmp/cancelled-request")


@pytest.mark.asyncio
async def test_cancelling_active_request_preserves_queued_callback(runtime):
    started, cancelled = asyncio.Event(), asyncio.Event()

    async def handler(name, args, kwargs):
        if not args:
            return "next-request"
        started.set()
        try:
            await asyncio.Event().wait()
        finally:
            cancelled.set()

    tool = runtime.make_tool(handler)
    active = tool.execute(runtime.script)
    await asyncio.wait_for(started.wait(), 5)
    queued = tool.execute(runtime.print_call)
    active.cancel()
    with pytest.raises(asyncio.CancelledError):
        await active
    await asyncio.wait_for(cancelled.wait(), 5)
    result = await asyncio.wait_for(queued, 5)
    assert result.stdout == "next-request\n", result.stderr


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "operation",
    ["read_file", "fs_handle", "execute", "execute_sync", "reset", "snapshot", "child_task", "worker_thread"],
)
async def test_handler_cannot_reenter_own_interpreter(runtime, operation):
    tool = None
    handle = None

    async def handler(name, args, kwargs):
        if operation == "read_file":
            return tool.read_file("/tmp/state")
        if operation == "fs_handle":
            return handle.read_file("/tmp/state")
        if operation == "execute":
            return await tool.execute("echo reentry")
        if operation == "execute_sync":
            return tool.execute_sync("echo reentry")
        if operation == "worker_thread":
            return await asyncio.to_thread(tool.read_file, "/tmp/state")
        if operation == "child_task":

            async def reenter():
                return await tool.execute("echo reentry")

            return await asyncio.create_task(reenter())
        return getattr(tool, operation)()

    tool = runtime.make_tool(handler, files={"/tmp/state": "data"})
    handle = tool.fs()
    result = await asyncio.wait_for(tool.execute(runtime.script), 5)
    assert result.exit_code != 0
    assert "cannot re-enter" in result.stderr + (result.error or "")
    assert tool.read_file("/tmp/state") == "data"
    tool = handle = None  # Break the deliberately recursive callback closure.


def test_handler_exception_and_conversion_error(runtime):
    async def handler(name, args, kwargs):
        raise ValueError("lookup failed")

    result = runtime.make_tool(handler).execute_sync(runtime.script)
    assert result.exit_code != 0

    assert "lookup failed" in result.stderr + (result.error or "")

    async def invalid_result(name, args, kwargs):
        return object()

    result = runtime.make_tool(invalid_result).execute_sync(runtime.script)
    assert result.exit_code != 0


@pytest.mark.asyncio
async def test_external_functions_and_custom_builtins_share_execution_context(runtime):
    loop = asyncio.get_running_loop()
    seen = []

    async def handler(name, args, kwargs):
        seen.append((request_id.get(), asyncio.get_running_loop() is loop))
        return {"value": 42}

    async def builtin(ctx):
        seen.append((request_id.get(), asyncio.get_running_loop() is loop))
        return "builtin\n"

    tool = runtime.make_tool(handler, custom_builtins={"host-builtin": builtin})
    token = request_id.set("mixed")
    try:
        result = await tool.execute_or_throw("host-builtin; " + runtime.script)
    finally:
        request_id.reset(token)
    assert result.stdout == "builtin\n42\n"
    assert seen == [("mixed", True), ("mixed", True)]


def test_sync_or_throw_and_unknown_function(runtime):
    from bashkit import BashError

    tool = runtime.make_tool(lookup)
    assert tool.execute_sync_or_throw(runtime.script).stdout == "42\n"
    with pytest.raises(BashError):
        tool.execute_sync_or_throw(runtime.unknown_script)


@pytest.mark.asyncio
async def test_handler_can_call_another_interpreter(runtime):
    other = runtime.make_tool(lookup)

    async def handler(name, args, kwargs):
        result = await other.execute(runtime.script)
        assert result.exit_code == 0, result.stderr
        return {"value": int(result.stdout)}

    tool = runtime.make_tool(handler)
    result = await asyncio.wait_for(tool.execute(runtime.script), 5)
    assert result.exit_code == 0, result.stderr
    assert result.stdout == "42\n"


@pytest.mark.asyncio
async def test_langchain_sync_and_async_external_functions(runtime):
    pytest.importorskip("langchain_core")
    from bashkit.langchain import BashkitTool, create_bash_tool

    loop = asyncio.get_running_loop()

    async def handler(name, args, kwargs):
        return {"value": 42 if asyncio.get_running_loop() is loop else 21}

    for factory in (BashkitTool, create_bash_tool):
        tool = factory(**runtime.options(handler))
        assert await tool.ainvoke({"commands": runtime.script}) == "42\n"
        assert await asyncio.to_thread(tool.invoke, {"commands": runtime.script}) == "21\n"


@pytest.mark.asyncio
async def test_deepagents_sync_and_async_external_functions(runtime):
    pytest.importorskip("deepagents")
    from bashkit.deepagents import BashkitBackend, BashkitMiddleware

    loop = asyncio.get_running_loop()

    async def handler(name, args, kwargs):
        return {"value": 42 if asyncio.get_running_loop() is loop else 21}

    options = runtime.options(handler)
    backend = BashkitBackend(**options)
    assert (await backend.aexecute(runtime.script)).output == "42\n"
    assert (await asyncio.to_thread(backend.execute, runtime.script)).output == "21\n"
    for middleware in (BashkitMiddleware(**options), backend.create_middleware()):
        assert await middleware.tools[0].ainvoke({"command": runtime.script}) == "42"
        assert await asyncio.to_thread(middleware.tools[0].invoke, {"command": runtime.script}) == "21"


@pytest.mark.asyncio
async def test_fastapi_async_requests_share_tool_without_context_leakage(runtime):
    fastapi = pytest.importorskip("fastapi")
    httpx = pytest.importorskip("httpx")
    app = fastapi.FastAPI()
    expected_loop = asyncio.get_running_loop()

    async def handler(name, args, kwargs):
        assert asyncio.get_running_loop() is expected_loop
        before = request_id.get()
        await asyncio.sleep(0.01)
        assert request_id.get() == before
        return before

    tool = runtime.make_tool(handler)

    @app.get("/request/{value}")
    async def endpoint(value: str):
        token = request_id.set(value)
        try:
            result = await tool.execute(runtime.print_call)
            assert result.exit_code == 0, result.stderr
            return {"request_id": result.stdout.strip()}
        finally:
            request_id.reset(token)

    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app), base_url="http://test") as client:
        responses = await asyncio.wait_for(asyncio.gather(*(client.get(f"/request/{i}") for i in range(8))), 10)
    assert [r.json()["request_id"] for r in responses] == [str(i) for i in range(8)]


def test_fastapi_sync_endpoint_uses_private_loop_and_request_context(runtime):
    fastapi = pytest.importorskip("fastapi")
    from fastapi.testclient import TestClient

    async def handler(name, args, kwargs):
        await asyncio.sleep(0)
        return request_id.get()

    tool = runtime.make_tool(handler)
    app = fastapi.FastAPI()

    @app.get("/request/{value}")
    def endpoint(value: str):
        token = request_id.set(value)
        try:
            result = tool.execute_sync(runtime.print_call)
            assert result.exit_code == 0, result.stderr
            return {"request_id": result.stdout.strip()}
        finally:
            request_id.reset(token)

    with TestClient(app) as client:
        for value in ("first", "second"):
            assert client.get(f"/request/{value}").json() == {"request_id": value}
