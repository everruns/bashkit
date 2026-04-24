"""Parity tests for constructor-time callback-backed custom builtins."""

# Decision: parameterize Bash/BashTool here so custom builtin semantics stay
# identical across both Python surfaces.

import asyncio
import contextvars
import gc
import json

import pytest

from bashkit import Bash, BashTool, BuiltinResult

request_id: contextvars.ContextVar[str] = contextvars.ContextVar("request_id")


@pytest.fixture(autouse=True)
def _collect_between_tests():
    """Drop Rust-backed callback runtimes outside async test bodies on Python 3.12."""
    yield
    gc.collect()


def build_shell(factory, custom_builtins):
    return factory(custom_builtins=custom_builtins)


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_persist_vfs_across_calls(factory):
    shell = build_shell(
        factory,
        {
            "get-order": lambda ctx: (
                json.dumps(
                    {
                        "id": ctx.argv[1] if len(ctx.argv) >= 2 and ctx.argv[0] == "get" else "?",
                        "status": "shipped",
                        "items": ["widget"],
                    }
                )
                + "\n"
            )
        },
    )

    first = shell.execute_sync("mkdir -p /scratch && get-order get 42 > /scratch/order.json")
    second = shell.execute_sync("cat /scratch/order.json | jq -r '.items[]'")

    assert first.exit_code == 0
    assert second.exit_code == 0
    assert second.stdout.strip() == "widget"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_survive_reset(factory):
    shell = build_shell(
        factory,
        {"ping": lambda ctx: "pong\n"},
    )
    shell.execute_sync("mkdir -p /scratch && printf 'gone\\n' > /scratch/state.txt")

    shell.reset()
    result = shell.execute_sync("ping && test ! -e /scratch/state.txt")

    assert result.exit_code == 0
    assert result.stdout == "pong\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_from_snapshot_accepts_custom_builtins(factory):
    snapshot = factory().snapshot()

    restored = factory.from_snapshot(
        snapshot,
        custom_builtins={"ping": lambda ctx: "pong\n"},
    )
    result = restored.execute_sync("ping")

    assert result.exit_code == 0
    assert result.stdout == "pong\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_support_async_callbacks(factory):
    async def greet(ctx):
        await asyncio.sleep(0)
        return f"hello {ctx.argv[0] if ctx.argv else 'world'}\n"

    shell = build_shell(factory, {"greet": greet})
    result = shell.execute_sync("greet Async")

    assert result.exit_code == 0
    assert result.stdout.strip() == "hello Async"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_str_returns_still_work(factory):
    shell = build_shell(factory, {"ping": lambda ctx: "pong\n"})
    result = shell.execute_sync("ping")

    assert result.exit_code == 0
    assert result.stdout == "pong\n"
    assert result.stderr == ""


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_builtin_result_preserves_stdout_stderr_and_exit_code(factory):
    def report(ctx):
        return BuiltinResult(stdout="partial\n", stderr="problem\n", exit_code=7)

    shell = build_shell(factory, {"report": report})
    result = shell.execute_sync("report")

    assert result.exit_code == 7
    assert result.stdout == "partial\n"
    assert result.stderr == "problem\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_builtin_result_allows_nonzero_exit_without_stderr(factory):
    def fail_without_stderr(ctx):
        return BuiltinResult(exit_code=3)

    shell = build_shell(factory, {"fail-without-stderr": fail_without_stderr})
    result = shell.execute_sync("fail-without-stderr")

    assert result.exit_code == 3
    assert result.stdout == ""
    assert result.stderr == ""


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_custom_builtins_async_callbacks_accept_builtin_result(factory):
    async def greet(ctx):
        await asyncio.sleep(0)
        return BuiltinResult(stdout=f"hello {ctx.argv[0]}\n")

    shell = build_shell(factory, {"greet": greet})
    result = await shell.execute("greet Async")

    assert result.exit_code == 0
    assert result.stdout == "hello Async\n"
    assert result.stderr == ""


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_invalid_return_type_is_a_type_error(factory):
    shell = build_shell(factory, {"oops": lambda ctx: 123})
    result = shell.execute_sync("oops")

    assert result.exit_code == 1
    assert result.stdout == ""
    assert "oops: callback must return str or BuiltinResult" in result.stderr


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_custom_builtins_async_execute_preserves_contextvars(factory):
    def check_ctx(ctx):
        return f"req={request_id.get('missing')}\n"

    request_id.set("ctx-123")
    shell = build_shell(factory, {"check-ctx": check_ctx})

    result = await shell.execute("check-ctx")

    assert result.exit_code == 0
    assert result.stdout.strip() == "req=ctx-123"


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_custom_builtins_async_execute_uses_caller_loop(factory):
    caller_loop = asyncio.get_running_loop()

    async def inspect_loop(ctx):
        same_loop = asyncio.get_running_loop() is caller_loop
        return f"same_loop={same_loop}\n"

    shell = build_shell(factory, {"inspect-loop": inspect_loop})
    result = await shell.execute("inspect-loop")

    assert result.exit_code == 0
    assert result.stdout.strip() == "same_loop=True"


@pytest.mark.asyncio
@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
async def test_custom_builtins_async_execute_cancels_callback_task(factory):
    started = asyncio.Event()
    released = asyncio.Event()
    cancelled = asyncio.Event()
    completed = []

    async def block(ctx):
        started.set()
        try:
            await released.wait()
            completed.append("completed")
            return "done\n"
        except asyncio.CancelledError:
            cancelled.set()
            raise

    shell = build_shell(factory, {"block": block})
    future = shell.execute("block")

    await started.wait()
    future.cancel()
    with pytest.raises(asyncio.CancelledError):
        await future

    released.set()
    await asyncio.sleep(0.05)

    assert cancelled.is_set()
    assert completed == []


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
@pytest.mark.asyncio
async def test_custom_builtins_async_execute_cancellation_stays_per_call(factory):
    started = asyncio.Event()
    released = asyncio.Event()
    cancelled = asyncio.Event()
    completed = []

    async def block(ctx):
        started.set()
        try:
            await released.wait()
            completed.append("completed")
            return "done\n"
        except asyncio.CancelledError:
            cancelled.set()
            raise

    shell = build_shell(factory, {"block": block})
    first = shell.execute("block")

    await started.wait()

    second = shell.execute("echo second")
    await asyncio.sleep(0)

    second.cancel()
    with pytest.raises(asyncio.CancelledError):
        await second

    released.set()
    first_result = await first

    assert first_result.exit_code == 0
    assert first_result.stdout == "done\n"
    assert not cancelled.is_set()
    assert completed == ["completed"]


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_execute_sync_reuses_private_loop_across_calls(factory):
    first_loop = None

    async def inspect_loop(ctx):
        nonlocal first_loop
        current_loop = asyncio.get_running_loop()
        same_loop = first_loop is None or first_loop is current_loop
        first_loop = current_loop
        return f"same_loop={same_loop}\n"

    shell = build_shell(factory, {"inspect-loop": inspect_loop})
    first = shell.execute_sync("inspect-loop")
    second = shell.execute_sync("inspect-loop")

    assert first.exit_code == 0
    assert second.exit_code == 0
    assert first.stdout.strip() == "same_loop=True"
    assert second.stdout.strip() == "same_loop=True"


def test_bashtool_help_lists_custom_builtins():
    shell = BashTool(custom_builtins={"ping": lambda ctx: "pong\n"})

    help_text = shell.help()

    assert "ping" in help_text
    assert "Custom commands: `ping`" in help_text


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_receive_raw_argv_and_stdin(factory):
    def inspect(ctx):
        return json.dumps({"argv": ctx.argv, "stdin": ctx.stdin}) + "\n"

    shell = build_shell(factory, {"inspect": inspect})
    result = shell.execute_sync("printf 'payload' | inspect subcommand --flag value")

    assert result.exit_code == 0
    assert json.loads(result.stdout) == {
        "argv": ["subcommand", "--flag", "value"],
        "stdin": "payload",
    }


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_custom_builtins_support_subcommands(factory):
    def orders(ctx):
        if not ctx.argv:
            return "usage: orders <get|list>\n"

        subcmd, *rest = ctx.argv
        if subcmd == "get":
            return json.dumps({"id": rest[0], "status": "shipped"}) + "\n"
        if subcmd == "list":
            return json.dumps(["42", "7"]) + "\n"
        return f"unknown subcommand: {subcmd}\n"

    shell = build_shell(factory, {"orders": orders})
    get_result = shell.execute_sync("orders get 42 | jq -r '.status'")
    list_result = shell.execute_sync("orders list | jq -r '.[]'")

    assert get_result.exit_code == 0
    assert get_result.stdout.strip() == "shipped"
    assert list_result.exit_code == 0
    assert list_result.stdout.splitlines() == ["42", "7"]
