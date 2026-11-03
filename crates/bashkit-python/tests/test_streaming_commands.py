"""Streaming command I/O is live, byte-native and scoped to one execution."""

import asyncio

import pytest

from bashkit import Bash, BashTool, CommandStatus


@pytest.mark.parametrize("factory", [Bash, BashTool])
@pytest.mark.parametrize("capture", [None, 64])
async def test_cancelled_queued_command_does_not_write_files(factory, capture):
    shell = factory()
    started, released = asyncio.Event(), asyncio.Event()

    async def hold(ctx):
        started.set()
        await released.wait()
        await ctx.stdout.write(b"done")
        return CommandStatus()

    shell.add_command("hold", hold)
    active = shell.execute("hold", capture=capture)
    await asyncio.wait_for(started.wait(), 5)
    queued = shell.execute("echo forbidden > /tmp/cancelled-request", capture=capture)
    queued.cancel()
    with pytest.raises(asyncio.CancelledError):
        await queued
    released.set()
    result = await asyncio.wait_for(active, 5)
    assert result.stdout_bytes == b"done"
    assert not shell.exists("/tmp/cancelled-request")


@pytest.mark.parametrize("runtime", ["python", "typescript"])
async def test_host_functions_work_with_byte_sinks(runtime):
    loop = asyncio.get_running_loop()
    chunks = []

    async def lookup(name, args, *kwargs):
        assert asyncio.get_running_loop() is loop
        assert name == "lookup"
        await asyncio.sleep(0)
        return args[0] * 2

    async def sink(chunk):
        chunks.append(chunk)

    if runtime == "python":
        tool = BashTool(
            python=True,
            env={"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"},
            external_functions=["lookup"],
            external_handler=lookup,
        )
        script = "python -c 'print(lookup(21))'"
    else:
        tool = BashTool(
            typescript=True,
            typescript_external_functions=["lookup"],
            typescript_external_handler=lookup,
        )
        script = "ts -c 'await lookup(21)'"

    result = await tool.execute(script, capture=64, stdout=sink)
    assert result.error is None
    assert result.exit_code == 0
    assert result.stdout_bytes == b"42\n"
    assert b"".join(chunks) == result.stdout_bytes


@pytest.mark.parametrize("factory", [Bash, BashTool])
async def test_final_filter_delivers_while_producer_is_still_running(factory):
    shell = factory()
    release = asyncio.Event()
    first = asyncio.Event()
    chunks = []

    async def producer(ctx):
        await ctx.stdout.write(b"error: first\n")
        await release.wait()
        await ctx.stdout.write(b"error: last\n")
        return CommandStatus()

    async def copy(ctx):
        async for chunk in ctx.stdin:
            await ctx.stdout.write(chunk)
        return CommandStatus()

    async def sink(chunk):
        assert isinstance(chunk, bytes)
        chunks.append(chunk)
        first.set()

    shell.add_command("produce", producer)
    shell.add_command("copy", copy)
    execution = asyncio.ensure_future(shell.execute("produce | copy | grep error", capture=1024, stdout=sink))
    try:
        await asyncio.wait_for(first.wait(), timeout=2)
        assert not execution.done()
        assert b"".join(chunks) == b"error: first\n"
    finally:
        release.set()
    result = await asyncio.wait_for(execution, timeout=2)
    assert result.error is None
    assert result.stdout_bytes == b"error: first\nerror: last\n"
    assert b"".join(chunks) == result.stdout_bytes


async def test_arbitrary_bytes_capture_limits_and_revoked_command_io():
    shell = Bash()
    contexts = []

    async def binary(ctx):
        contexts.append(ctx)
        await ctx.stdout.write(b"\x00\xffabcdefgh")
        return CommandStatus(exit_code=7)

    shell.add_command("binary", binary)
    result = await shell.execute("binary", capture=3)
    assert result.stdout_bytes == b"\x00\xffa"
    assert result.stdout_truncated
    assert result.exit_code == 7
    with pytest.raises(RuntimeError, match="request closed"):
        await contexts[0].stdout.write(b"late")
    with pytest.raises(RuntimeError, match="request closed"):
        await contexts[0].stdin.read_chunk()
    result = await shell.execute("echo recovered", capture=64)
    assert result.stdout_bytes == b"recovered\n"


async def test_sinks_without_capture_do_not_retain_output():
    shell = Bash()
    chunks = []

    async def sink(chunk):
        chunks.append(chunk)

    result = await shell.execute("echo hello", stdout=sink)
    assert result.stdout_bytes == b""
    assert not result.stdout_truncated
    assert b"".join(chunks) == b"hello\n"


async def test_sync_sink_and_mixed_observer_options_are_rejected():
    shell = Bash()
    with pytest.raises(TypeError, match="async callable"):
        await shell.execute("echo hello", stdout=lambda chunk: None)
    with pytest.raises(TypeError, match="instead of on_output"):
        await shell.execute("echo hello", on_output=lambda a, b: None, capture=64)


async def test_sink_failure_and_cancellation_release_the_execution():
    shell = Bash()

    async def failed_sink(chunk):
        raise ValueError("sink failed")

    result = await shell.execute("echo hello", stdout=failed_sink)
    assert result.exit_code == 1
    assert result.error is not None
    assert (await shell.execute("echo recovered", capture=64)).stdout_bytes == b"recovered\n"

    async def pending_sink(chunk):
        shell.cancel()
        await asyncio.Event().wait()

    result = await asyncio.wait_for(shell.execute("echo hello", stdout=pending_sink), timeout=2)
    assert result.error is not None
    shell.clear_cancel()
    assert (await shell.execute("echo recovered", capture=64)).stdout_bytes == b"recovered\n"
