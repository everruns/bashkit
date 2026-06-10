"""`BuiltinContext.fs` exposes the live VFS to custom builtin callbacks.

Decision: the context's ``fs`` wraps the *same* ``Arc<dyn FileSystem>`` the
interpreter uses (mirroring how the embedded ``python3``/Monty builtin gets
``ctx.fs``), so reads see files created by earlier bash commands and writes are
visible to later ones. Dispatch is runtime-safe from both ``execute_sync`` (runs
inside the shared runtime's ``block_on``) and ``await execute``.
"""

import pytest

from bashkit import Bash, BashError, BashTool, FileSystem


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
    # Mutations persist on the live VFS after the callback returns.
    assert shell.fs().read_file("/d/a.txt").decode() == "a"


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
@pytest.mark.asyncio
async def test_ctx_fs_async_callback_writes_file(factory):
    async def awriter(ctx):
        ctx.fs.write_file("/async-out.txt", b"async-write\n")
        return "wrote\n"

    shell = factory(custom_builtins={"awriter": awriter})
    result = await shell.execute("awriter")

    assert result.exit_code == 0
    # The write is visible to later commands and via the instance handle.
    cat = await shell.execute("cat /async-out.txt")
    assert cat.stdout == "async-write\n"
    assert shell.fs().read_file("/async-out.txt").decode() == "async-write\n"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_read_missing_raises_clean_error(factory):
    """A missing read raises a clean ``RuntimeError`` at the ``ctx.fs`` boundary;
    an unhandled one surfaces as a sanitized ``BashError`` at the call site
    (no panic, no leaked host path)."""

    def reader(ctx):
        with pytest.raises(RuntimeError) as exc_info:
            ctx.fs.read_file("/does-not-exist.txt")
        message = str(exc_info.value)
        assert "/rustc/" not in message
        assert ".cargo" not in message
        return "handled\n"

    def boom(ctx):
        # No try/except: the ctx.fs error escapes the callback.
        return ctx.fs.read_file("/does-not-exist.txt").decode()

    shell = factory(custom_builtins={"reader": reader, "boom": boom})

    handled = shell.execute_sync("reader")
    assert handled.exit_code == 0
    assert handled.stdout == "handled\n"

    # An unhandled ctx.fs error surfaces at the call site as a failed command;
    # execute_sync_or_throw turns that into a (sanitized) BashError.
    with pytest.raises(BashError) as exc_info:
        shell.execute_sync_or_throw("boom")
    message = str(exc_info.value)
    assert "/rustc/" not in message
    assert ".cargo" not in message


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

    def writer(ctx):
        assert ctx.fs.read_file("/seed.txt").decode() == "seeded\n"
        with pytest.raises(RuntimeError):
            ctx.fs.write_file("/seed.txt", b"nope")
        return "handled\n"

    shell = factory(
        custom_builtins={"writer": writer},
        files={"/seed.txt": "seeded\n"},
        readonly_filesystem=True,
    )
    result = shell.execute_sync("writer")

    assert result.exit_code == 0
    assert result.stdout == "handled\n"
    # The denied write left the seed file unchanged.
    assert shell.fs().read_file("/seed.txt").decode() == "seeded\n"


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


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_supports_more_ops(factory):
    """The remaining ``FileSystem`` ops (append_file/stat/rename/copy/chmod/
    symlink/read_link/remove) round-trip through ``ctx.fs`` on the live VFS."""

    def ops(ctx):
        ctx.fs.mkdir("/m/src", recursive=True)
        ctx.fs.write_file("/m/src/file.txt", b"alpha")
        ctx.fs.append_file("/m/src/file.txt", b"beta")
        assert ctx.fs.read_file("/m/src/file.txt") == b"alphabeta"

        st = ctx.fs.stat("/m/src/file.txt")
        assert st["file_type"] == "file"
        assert st["size"] == 9

        ctx.fs.mkdir("/m/dst", recursive=True)
        ctx.fs.copy("/m/src/file.txt", "/m/dst/copied.txt")
        ctx.fs.rename("/m/dst/copied.txt", "/m/dst/renamed.txt")
        ctx.fs.symlink("/m/dst/renamed.txt", "/m/link.txt")
        ctx.fs.chmod("/m/dst/renamed.txt", 0o600)
        assert ctx.fs.read_link("/m/link.txt") == "/m/dst/renamed.txt"
        assert ctx.fs.stat("/m/dst/renamed.txt")["mode"] == 0o600

        ctx.fs.remove("/m/link.txt")
        assert ctx.fs.exists("/m/link.txt") is False
        return "ok\n"

    shell = factory(custom_builtins={"ops": ops})
    result = shell.execute_sync("ops")

    assert result.exit_code == 0
    assert result.stdout == "ok\n"
    # Mutations persist on the live VFS after the callback returns.
    assert shell.fs().read_file("/m/dst/renamed.txt") == b"alphabeta"
    assert shell.fs().exists("/m/link.txt") is False


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_ctx_fs_nested_reentry_via_lazy_provider(factory):
    """A lazy provider triggered *via* ``ctx.fs.read_file`` itself reads another
    file through a captured ``ctx.fs`` handle. The provider runs on the
    worker-thread runtime (so ``Handle::try_current()`` is already true there),
    which makes the inner read take the worker-thread dispatch branch again —
    exercising recursive ``ctx.fs`` re-entry without deadlocking."""

    holder = {}

    def provider():
        # Re-enters ctx.fs from inside the worker-thread runtime. /inner.txt is a
        # plain file (no further Python callback), so the nested read completes.
        return "nested:" + holder["fs"].read_file("/inner.txt").decode()

    def setup(ctx):
        holder["fs"] = ctx.fs
        return "ready\n"

    def trigger(ctx):
        return ctx.fs.read_file("/lazy.txt").decode()

    shell = factory(
        custom_builtins={"setup": setup, "trigger": trigger},
        files={"/lazy.txt": provider, "/inner.txt": "inner-data\n"},
    )

    assert shell.execute_sync("setup").exit_code == 0
    result = shell.execute_sync("trigger")

    assert result.exit_code == 0
    assert result.stdout == "nested:inner-data\n"
