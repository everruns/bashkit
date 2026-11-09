"""Custom-builtin cancellation contract: execution-scoped signal parity.

Mirrors crates/bashkit-js/__test__/custom-builtin-cancel.spec.ts:
- BuiltinContext exposes live cancellation state (is_cancelled,
  throw_if_cancelled).
- cancel() aborts a hanging builtin -> exit 130, state preserved, the next
  command runs normally after clear_cancel (sticky-until-clear contract).
- Execution timeout aborts the builtin signal -> exit 124.
"""

import threading
import time

from bashkit import Bash


def test_builtin_context_exposes_cancellation_api():
    seen = {}

    def probe(ctx):
        seen["is_cancelled"] = ctx.is_cancelled()
        seen["has_throw"] = callable(getattr(ctx, "throw_if_cancelled", None))
        return "ok"

    shell = Bash(custom_builtins={"probe": probe})
    result = shell.execute_sync("probe")
    assert result.exit_code == 0
    assert seen["is_cancelled"] is False
    assert seen["has_throw"] is True


def test_cancel_aborts_hanging_builtin_with_130():
    aborted = []

    def hold(ctx):
        while not ctx.is_cancelled():
            time.sleep(0.01)
        aborted.append(True)
        # Graceful resolve after observing cancellation: the budget
        # post-check still reports the interruption.
        return "released"

    shell = Bash(custom_builtins={"hold": hold})
    setup = shell.execute_sync("MARKER=kept")
    assert setup.exit_code == 0

    outcome = {}

    def run():
        outcome["result"] = shell.execute_sync("hold")

    worker = threading.Thread(target=run)
    worker.start()
    time.sleep(0.5)
    shell.cancel()
    worker.join(timeout=10)
    assert not worker.is_alive()

    result = outcome["result"]
    assert result.exit_code == 130
    assert aborted == [True]

    shell.clear_cancel()
    nxt = shell.execute_sync("echo next:$MARKER")
    assert nxt.exit_code == 0
    assert "next:kept" in nxt.stdout


def test_timeout_aborts_builtin_signal_with_124():
    aborted = []

    def hold(ctx):
        while not ctx.is_cancelled():
            time.sleep(0.01)
        aborted.append(True)
        return "released"

    shell = Bash(custom_builtins={"hold": hold}, timeout_seconds=0.5)
    start = time.time()
    result = shell.execute_sync("hold")
    elapsed = time.time() - start
    assert aborted == [True]
    assert result.exit_code == 124
    assert elapsed < 10
