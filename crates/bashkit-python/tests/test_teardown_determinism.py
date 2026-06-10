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

from bashkit import ScriptedTool

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
        # Exact equality, immediately: joins are synchronous in drop.
        assert _native_thread_count() == baseline


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
from bashkit import ScriptedTool

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
from bashkit import ScriptedTool

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
