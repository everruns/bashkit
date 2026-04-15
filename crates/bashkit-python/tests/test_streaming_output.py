"""Tests for live chunked stdout/stderr callbacks."""

import asyncio
import contextvars

import pytest

from bashkit import Bash, BashTool

SCRIPT = """
for i in 1 2 3; do
    echo "out-$i"
    echo "err-$i" >&2
done
"""

request_id: contextvars.ContextVar[str] = contextvars.ContextVar(
    "stream_request_id",
    default="MISSING",
)


def _assert_chunks_match_result(result, chunks):
    assert chunks
    assert "".join(stdout for stdout, _ in chunks) == result.stdout
    assert "".join(stderr for _, stderr in chunks) == result.stderr


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_execute_sync_on_output_matches_final_result(factory):
    shell = factory()
    chunks = []

    result = shell.execute_sync(
        SCRIPT,
        on_output=lambda stdout, stderr: chunks.append((stdout, stderr)),
    )

    assert result.exit_code == 0
    _assert_chunks_match_result(result, chunks)


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_execute_sync_on_output_requires_callable(factory):
    shell = factory()

    with pytest.raises(TypeError, match="on_output must be callable"):
        shell.execute_sync("echo hi", on_output=object())


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_execute_sync_on_output_rejects_async_callable(factory):
    shell = factory()

    async def on_output(stdout, stderr):
        del stdout, stderr

    with pytest.raises(
        TypeError,
        match="on_output must be a synchronous callable",
    ):
        shell.execute_sync("echo hi", on_output=on_output)


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_execute_on_output_matches_final_result(factory):
    shell = factory()
    chunks = []

    result = await shell.execute(
        SCRIPT,
        on_output=lambda stdout, stderr: chunks.append((stdout, stderr)),
    )

    assert result.exit_code == 0
    _assert_chunks_match_result(result, chunks)


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_execute_on_output_rejects_async_callable(factory):
    shell = factory()

    async def on_output(stdout, stderr):
        del stdout, stderr

    with pytest.raises(
        TypeError,
        match="on_output must be a synchronous callable",
    ):
        await shell.execute("echo hi", on_output=on_output)


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_execute_sync_on_output_error_propagates(factory):
    shell = factory()
    calls = []

    def on_output(stdout, stderr):
        calls.append((stdout, stderr))
        raise RuntimeError("on_output exploded")

    with pytest.raises(RuntimeError, match="on_output exploded"):
        shell.execute_sync(SCRIPT, on_output=on_output)

    assert calls


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_execute_sync_on_output_rejects_awaitable_return(factory):
    shell = factory()

    def on_output(stdout, stderr):
        del stdout, stderr
        return asyncio.sleep(0)

    with pytest.raises(
        TypeError,
        match="on_output must be synchronous and must not return an awaitable",
    ):
        shell.execute_sync("echo hi", on_output=on_output)


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_execute_sync_on_output_error_does_not_poison_future_calls(factory):
    shell = factory()

    with pytest.raises(RuntimeError, match="on_output exploded"):
        shell.execute_sync(
            SCRIPT,
            on_output=lambda *_: (_ for _ in ()).throw(RuntimeError("on_output exploded")),
        )

    result = shell.execute_sync("echo after-error")

    assert result.exit_code == 0
    assert result.stdout == "after-error\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_execute_sync_on_output_error_does_not_clear_future_explicit_cancel(factory):
    shell = factory()

    with pytest.raises(RuntimeError, match="on_output exploded"):
        shell.execute_sync(
            SCRIPT,
            on_output=lambda *_: (_ for _ in ()).throw(RuntimeError("on_output exploded")),
        )

    shell.cancel()
    result = shell.execute_sync("echo after-error")

    assert result.exit_code != 0 or "cancel" in result.stderr.lower() or "cancel" in (result.error or "").lower()


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_execute_on_output_error_propagates(factory):
    shell = factory()
    calls = []

    def on_output(stdout, stderr):
        calls.append((stdout, stderr))
        raise RuntimeError("on_output exploded")

    with pytest.raises(RuntimeError, match="on_output exploded"):
        await shell.execute(SCRIPT, on_output=on_output)

    assert calls


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_execute_on_output_preserves_contextvars(factory):
    shell = factory()
    seen_request_ids = []
    token = request_id.set(f"{factory.__name__}-req")

    try:
        result = await shell.execute(
            SCRIPT,
            on_output=lambda stdout, stderr: seen_request_ids.append((request_id.get(), stdout, stderr)),
        )
    finally:
        request_id.reset(token)

    assert result.exit_code == 0
    assert seen_request_ids
    assert {rid for rid, _, _ in seen_request_ids} == {f"{factory.__name__}-req"}


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_execute_on_output_error_does_not_poison_future_calls(factory):
    shell = factory()

    with pytest.raises(RuntimeError, match="on_output exploded"):
        await shell.execute(
            SCRIPT,
            on_output=lambda *_: (_ for _ in ()).throw(RuntimeError("on_output exploded")),
        )

    result = await shell.execute("echo after-error")

    assert result.exit_code == 0
    assert result.stdout == "after-error\n"
