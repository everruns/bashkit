"""Teardown determinism tests (TM-PY-030).

While the interpreter is alive, dropping a tool must deterministically
release everything it owns BEFORE the drop returns: the private-loop worker
thread is joined, its asyncio loop is closed (fds freed), and the tokio
blocking pool is joined. Abandoned timed-out callbacks are cancelled
cooperatively rather than awaited to completion. At interpreter exit the
machinery goes hands-off instead (covered by the subprocess tests).
"""

import asyncio
import gc
import os
import subprocess
import sys
import threading
import time

import pytest

from bashkit import Bash
from bashkit.scripted import ScriptedTool

PROC_AVAILABLE = os.path.exists("/proc/self/task")


def _native_thread_count() -> int:
    return len(os.listdir("/proc/self/task"))


def _fd_count() -> int:
    return len(os.listdir("/proc/self/fd"))


async def _ok(params, stdin=None):
    await asyncio.sleep(0)
    return "ok\n"


def _make_tool() -> ScriptedTool:
    t = ScriptedTool("api")
    t.add_tool("hit", "Hit", callback=_ok)
    return t


def _assert_no_thread_growth(baseline: int) -> None:
    """Assert teardown left nothing behind, relative to `baseline`.

    One-directional by design. TM-PY-030 is about what a drop *leaves behind*,
    and the process-wide count is not this tool's to own: tokio reaps idle
    blocking-pool threads on a 10 s timer, so a pytest session that ran async
    work before this module sheds an unrelated thread part-way through the
    churn loop. Measured directly — after an async workload the count holds at
    6 and drops to 5 at exactly t=10.0 s. Asserting exact equality read that
    as a failure (`assert 13 == 14`) though nothing leaked; the count moved
    *down*. A real leak still fails: it compounds across iterations and pushes
    the count above the baseline.
    """
    count = _native_thread_count()
    assert count <= baseline, f"thread count grew: {baseline} -> {count}"


@pytest.mark.skipif(not PROC_AVAILABLE, reason="requires /proc")
def test_thread_check_tolerates_shrink_from_unrelated_pools():
    """An inflated baseline (a thread reaped after sampling) is not a defect."""
    _assert_no_thread_growth(_native_thread_count() + 1)


@pytest.mark.skipif(not PROC_AVAILABLE, reason="requires /proc")
def test_thread_check_rejects_growth():
    """The check still catches a leak — the direction that means a defect."""
    with pytest.raises(AssertionError, match="thread count grew"):
        _assert_no_thread_growth(_native_thread_count() - 1)


@pytest.mark.skipif(not PROC_AVAILABLE, reason="requires /proc")
def test_threads_joined_deterministically_after_drop():
    """del tool returns only after worker + runtime threads are joined."""
    # Warm up imports/allocators so the baseline is stable.
    t = _make_tool()
    assert t.execute_sync("hit").exit_code == 0
    del t
    gc.collect()

    baseline = _native_thread_count()
    for _ in range(5):
        t = _make_tool()
        assert t.execute_sync("hit").exit_code == 0
        del t
        gc.collect()
        # Immediately: joins are synchronous in drop, so a tool's threads are
        # gone before `del` returns and repeated churn cannot accumulate.
        _assert_no_thread_growth(baseline)


@pytest.mark.skipif(not PROC_AVAILABLE, reason="requires /proc")
def test_fds_stable_across_tool_churn():
    """Asyncio loop fds are released by drop, not by a later gc pass."""
    t = _make_tool()
    assert t.execute_sync("hit").exit_code == 0
    del t
    gc.collect()

    baseline = _fd_count()
    for _ in range(20):
        t = _make_tool()
        assert t.execute_sync("hit").exit_code == 0
        del t
        gc.collect()
        assert _fd_count() <= baseline


@pytest.mark.asyncio
async def test_drop_bash_while_async_execute_in_flight_does_not_panic():
    """A `Bash` dropped while `await execute()` is in flight must not panic.

    The in-flight future holds the last `Arc<Mutex<Bash>>`; when it completes on
    a pyo3-async-runtimes worker thread, the `Bash` — and its custom-builtin
    adapters, which each hold a `PyRuntime` clone — drop *there*, so the last
    `PyRuntime` clone drops inside a tokio context. `PyRuntime::drop` must hand
    off to `shutdown_background()` rather than a blocking runtime join, which in
    that context panics ("Cannot drop a runtime in a context where blocking is
    not allowed") and surfaces as `pyo3_async_runtimes.RustPanic` to the
    awaiting caller. Regression for the #2009/#2010 interaction.
    """

    async def slow(ctx):
        await asyncio.sleep(0.3)
        return "done\n"

    b = Bash(custom_builtins={"slow": slow})
    fut = asyncio.ensure_future(b.execute("slow"))
    await asyncio.sleep(0.05)  # let execution start; callback is in flight
    del b
    gc.collect()
    result = await fut  # RustPanic here before the drop-path fix
    assert result.exit_code == 0
    assert result.stdout == "done\n"


def test_dropped_tool_cancels_abandoned_callback():
    """Teardown is bounded by cancellation, not callback duration."""
    started = threading.Event()
    cancelled = threading.Event()

    async def slow(params, stdin=None):
        started.set()
        try:
            await asyncio.sleep(30)
        except asyncio.CancelledError:
            cancelled.set()
            raise
        return "late\n"

    t = ScriptedTool("api", timeout_seconds=0.05)
    t.add_tool("slow", "Slow", callback=slow)
    r = t.execute_sync("slow")
    assert r.exit_code == 1
    assert started.wait(timeout=2.0), "callback never started"

    begin = time.monotonic()
    del t
    gc.collect()
    elapsed = time.monotonic() - begin

    # Without cancellation this would take the full 30 s sleep.
    assert elapsed < 5.0, f"teardown took {elapsed:.1f}s — callback not cancelled"
    assert cancelled.wait(timeout=2.0), "callback did not observe CancelledError"


_EXIT_SCRIPT_CLEAN = """
import asyncio
from bashkit.scripted import ScriptedTool

async def cb(params, stdin=None):
    await asyncio.sleep(0)
    return "ok\\n"

# Module-level: never deleted, torn down by interpreter finalization.
t = ScriptedTool("api")
t.add_tool("hit", "Hit", callback=cb)
assert t.execute_sync("hit").exit_code == 0
"""

_EXIT_SCRIPT_ABANDONED = """
import asyncio
from bashkit.scripted import ScriptedTool

async def slow(params, stdin=None):
    await asyncio.sleep(30)
    return "late\\n"

# Exit immediately with the abandoned callback still in flight.
t = ScriptedTool("api", timeout_seconds=0.05)
t.add_tool("slow", "Slow", callback=slow)
assert t.execute_sync("slow").exit_code == 1
"""


@pytest.mark.parametrize("script", [_EXIT_SCRIPT_CLEAN, _EXIT_SCRIPT_ABANDONED], ids=["clean", "abandoned"])
def test_interpreter_exit_does_not_crash(script):
    """Module-level tools torn down at Py_Finalize must not abort (SIGABRT)."""
    # Replicate this (parent) interpreter's exact import surface in the child.
    # The parent imports `bashkit` (and its compiled `_bashkit`) successfully —
    # every test in this process proves it — so whatever path entry makes that
    # work is on the parent's `sys.path`. A bare `python -c` child starts from
    # a clean slate and, under CI's in-place source-tree build, finds
    # `__init__.py` without `_bashkit` (ModuleNotFoundError). Prepending the
    # parent's `sys.path` to the child guarantees identical resolution,
    # independent of where `__init__.py` vs `_bashkit` happen to live.
    preamble = "import sys; sys.path[:0] = %r\n" % [p for p in sys.path if p]
    child = preamble + script
    for _ in range(10):
        proc = subprocess.run(
            [sys.executable, "-c", child],
            capture_output=True,
            timeout=60,
        )
        assert proc.returncode == 0, proc.stderr.decode()


@pytest.mark.parametrize("kind", ["Bash", "BashTool", "ScriptedTool"])
def test_exit_waits_for_native_result_delivery_to_return(kind):
    # Publishing a result wakes Python before call_soon_threadsafe() returns
    # on the native worker. Force that window open: atexit must drain the
    # delivery's Python references before CPython destroys its thread state.
    script = """
import atexit
import asyncio
import os
import sys
import threading
import time

returned = threading.Event()
def check_delivery_finished():
    if not returned.is_set():
        os._exit(5)
atexit.register(check_delivery_finished)

from bashkit import Bash, BashTool
from bashkit.scripted import ScriptedTool

class SlowDeliveryLoop(asyncio.SelectorEventLoop):
    def call_soon_threadsafe(self, callback, *args, context=None):
        native = threading.current_thread() is not threading.main_thread()
        if native:
            returned.clear()
        handle = super().call_soon_threadsafe(callback, *args, context=context)
        if native:
            time.sleep(0.2)
            returned.set()
        return handle

loop = SlowDeliveryLoop()
asyncio.set_event_loop(loop)
tool = {"Bash": Bash, "BashTool": BashTool, "ScriptedTool": lambda: ScriptedTool("api")}[sys.argv[1]]()
async def run():
    assert (await tool.execute("echo ok")).stdout == "ok\\n"
loop.run_until_complete(run())
loop.close()
"""
    preamble = "import sys; sys.path[:0] = %r\n" % [p for p in sys.path if p]
    proc = subprocess.run([sys.executable, "-c", preamble + script, kind], capture_output=True, timeout=10)
    assert proc.returncode == 0, proc.stderr.decode()


@pytest.mark.parametrize("kind", ["builtin", "python", "typescript"])
def test_exit_drains_private_callback_cancellation(kind):
    script = """
import atexit
import asyncio
import os
import sys
import threading
import time

cancelling, finished = threading.Event(), threading.Event()
def check_callback_finished():
    if not finished.is_set():
        os._exit(5)
atexit.register(check_callback_finished)

from bashkit import BashTool
async def slow(*args):
    try:
        await asyncio.sleep(30)
    finally:
        cancelling.set()
        time.sleep(0.2)
        finished.set()

kind = sys.argv[1]
if kind == "builtin":
    options = dict(custom_builtins={"slow": slow})
    command = "slow"
elif kind == "python":
    options = dict(python=True, env={"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"},
                   external_functions=["slow"], external_handler=slow)
    command = "python -c 'slow()'"
else:
    options = dict(typescript=True, typescript_external_functions=["slow"],
                   typescript_external_handler=slow)
    command = "ts -c 'await slow()'"
tool = BashTool(timeout_seconds=0.1, **options)
assert tool.execute_sync(command).exit_code != 0
assert cancelling.wait(2)
"""
    preamble = "import sys; sys.path[:0] = %r\n" % [p for p in sys.path if p]
    proc = subprocess.run([sys.executable, "-c", preamble + script, kind], capture_output=True, timeout=10)
    assert proc.returncode == 0, proc.stderr.decode()
