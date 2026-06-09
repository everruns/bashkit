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
