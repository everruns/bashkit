"""`BuiltinContext.fs` exposes the live VFS to custom builtin callbacks.

Decision: the context's ``fs`` wraps the *same* ``Arc<dyn FileSystem>`` the
interpreter uses (mirroring how the embedded ``python3``/Monty builtin gets
``ctx.fs``), so reads see files created by earlier bash commands and writes are
visible to later ones. Dispatch is runtime-safe from both ``execute_sync`` (runs
inside the shared runtime's ``block_on``) and ``await execute``.
"""

import pytest

from bashkit import Bash, BashTool, FileSystem


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_is_a_filesystem_handle(factory):
    captured = {}

    def grab(ctx):
        captured["fs"] = ctx.fs
        return "ok\n"

    shell = factory(custom_builtins={"grab": grab})
    result = shell.execute_sync("grab")

    assert result.exit_code == 0
    assert isinstance(captured["fs"], FileSystem)


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_reads_files_created_by_bash(factory):
    def reader(ctx):
        return ctx.fs.read_file("/scratch/data.txt").decode()

    shell = factory(custom_builtins={"reader": reader})
    first = shell.execute_sync("mkdir -p /scratch && printf 'hello\\n' > /scratch/data.txt")
    second = shell.execute_sync("reader")

    assert first.exit_code == 0
    assert second.exit_code == 0
    assert second.stdout == "hello\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_writes_visible_to_later_commands(factory):
    def writer(ctx):
        ctx.fs.write_file("/out.txt", b"from-callback\n")
        return "wrote\n"

    shell = factory(custom_builtins={"writer": writer})
    first = shell.execute_sync("writer")
    second = shell.execute_sync("cat /out.txt")

    assert first.exit_code == 0
    assert second.stdout == "from-callback\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_supports_common_ops(factory):
    def ops(ctx):
        ctx.fs.mkdir("/d", recursive=True)
        ctx.fs.write_file("/d/a.txt", b"a")
        assert ctx.fs.exists("/d/a.txt")
        names = [e["name"] for e in ctx.fs.read_dir("/d")]
        return ",".join(names) + "\n"

    shell = factory(custom_builtins={"ops": ops})
    result = shell.execute_sync("ops")

    assert result.exit_code == 0
    assert result.stdout == "a.txt\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
@pytest.mark.asyncio
async def test_ctx_fs_works_from_async_callback(factory):
    async def areader(ctx):
        return ctx.fs.read_file("/async.txt").decode()

    shell = factory(custom_builtins={"areader": areader})
    await shell.execute("printf 'async-data\\n' > /async.txt")
    result = await shell.execute("areader")

    assert result.exit_code == 0
    assert result.stdout == "async-data\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_read_missing_raises_clean_error(factory):
    """Reading a missing path surfaces a clean ``RuntimeError`` (not a panic or a
    leaked host path)."""
    seen = {}

    def reader(ctx):
        try:
            ctx.fs.read_file("/does-not-exist.txt")
        except Exception as exc:  # inspect then re-raise
            seen["type"] = type(exc).__name__
            seen["msg"] = str(exc)
            raise
        return ""

    shell = factory(custom_builtins={"reader": reader})
    result = shell.execute_sync("reader")

    assert result.exit_code != 0
    assert seen["type"] == "RuntimeError"
    # No host filesystem path leaks through the error.
    assert "/rustc/" not in seen["msg"]
    assert ".cargo" not in seen["msg"]


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_write_then_read_within_one_callback(factory):
    """A write and a subsequent read inside a single callback observe the same
    live filesystem."""

    def rw(ctx):
        ctx.fs.write_file("/inline.txt", b"inline-bytes\n")
        return ctx.fs.read_file("/inline.txt").decode()

    shell = factory(custom_builtins={"rw": rw})
    result = shell.execute_sync("rw")

    assert result.exit_code == 0
    assert result.stdout == "inline-bytes\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_respects_readonly_filesystem(factory):
    """Under ``readonly_filesystem=True`` reads succeed but writes are denied —
    ``ctx.fs`` does not bypass the read-only wrapper."""
    seen = {}

    def writer(ctx):
        seen["read"] = ctx.fs.read_file("/seed.txt").decode()
        try:
            ctx.fs.write_file("/seed.txt", b"nope")
        except Exception as exc:  # inspect then re-raise
            seen["write_err"] = type(exc).__name__
            raise
        return ""

    shell = factory(
        custom_builtins={"writer": writer},
        files={"/seed.txt": "seeded\n"},
        readonly_filesystem=True,
    )
    result = shell.execute_sync("writer")

    assert seen["read"] == "seeded\n"
    assert seen.get("write_err") == "RuntimeError"
    assert result.exit_code != 0


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_reads_lazy_provider_file(factory):
    """Reading a lazy (callable-backed) file through ``ctx.fs`` materializes it by
    calling back into Python."""
    calls = {"n": 0}

    def provider():
        calls["n"] += 1
        return "lazy-content\n"

    def reader(ctx):
        return ctx.fs.read_file("/lazy.txt").decode()

    shell = factory(custom_builtins={"reader": reader}, files={"/lazy.txt": provider})
    result = shell.execute_sync("reader")

    assert result.exit_code == 0
    assert result.stdout == "lazy-content\n"
    assert calls["n"] >= 1


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_cross_handle_consistency(factory):
    """A write via ``ctx.fs`` is observable via the instance's own ``fs()``
    handle — both wrap the same filesystem."""

    def writer(ctx):
        ctx.fs.write_file("/shared.txt", b"shared\n")
        return ""

    shell = factory(custom_builtins={"writer": writer})

    assert shell.execute_sync("writer").exit_code == 0
    assert shell.fs().read_file("/shared.txt").decode() == "shared\n"
