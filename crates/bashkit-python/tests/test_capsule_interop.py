"""Capsule interop tests for FileSystem (PR #1353).

Tests to_capsule()/from_capsule() API surface: VFS operations through
capsule-imported handles, mount behavior, capsule semantics, error cases,
and bash command execution against mounted capsule filesystems.

VFS operation tests are parametrized to run against both the native
FileSystem (via capsule roundtrip) and the pure-Python MockFileSystem.
"""

from __future__ import annotations

from typing import Any, Union

import pytest
from mock_filesystem import MockFileSystem

from bashkit import Bash, FileSystem

FS = Union[FileSystem, MockFileSystem]


def _populate(fs: Any) -> Any:
    fs.mkdir("/data/sub", recursive=True)
    fs.write_file("/data/hello.txt", b"world")
    fs.write_file("/data/sub/nested.txt", b"deep")
    fs.symlink("/data/hello.txt", "/data/link.txt")
    fs.chmod("/data/hello.txt", 0o755)
    return fs


@pytest.fixture(params=["capsule", "mock"])
def imported(request: pytest.FixtureRequest) -> FS:
    match request.param:
        case "capsule":
            source = FileSystem()
            return _populate(FileSystem.from_capsule(source.to_capsule()))
        case "mock":
            return _populate(MockFileSystem())
        case _:
            raise ValueError(request.param)


# -- VFS operations (parametrized: capsule + mock) -----------------------------


def test_read_file(imported: FS) -> None:
    assert imported.read_file("/data/hello.txt") == b"world"


def test_write_file(imported: FS) -> None:
    imported.write_file("/data/new.txt", b"created")
    assert imported.read_file("/data/new.txt") == b"created"


def test_append_file(imported: FS) -> None:
    imported.append_file("/data/hello.txt", b"!")
    assert imported.read_file("/data/hello.txt") == b"world!"


def test_mkdir(imported: FS) -> None:
    imported.mkdir("/data/newdir")
    assert imported.exists("/data/newdir")
    assert imported.stat("/data/newdir")["file_type"] == "directory"


def test_remove_file(imported: FS) -> None:
    imported.remove("/data/sub/nested.txt")
    assert not imported.exists("/data/sub/nested.txt")


def test_remove_dir_recursive(imported: FS) -> None:
    imported.remove("/data/sub", recursive=True)
    assert not imported.exists("/data/sub")


def test_exists(imported: FS) -> None:
    assert imported.exists("/data/hello.txt")
    assert not imported.exists("/data/nope.txt")


def test_stat(imported: FS) -> None:
    s = imported.stat("/data/hello.txt")
    assert s["file_type"] == "file"
    assert s["size"] == 5
    assert s["mode"] == 0o755


def test_read_dir(imported: FS) -> None:
    names = sorted(e["name"] for e in imported.read_dir("/data"))
    assert "hello.txt" in names
    assert "sub" in names
    assert "link.txt" in names


def test_symlink_and_read_link(imported: FS) -> None:
    assert imported.read_link("/data/link.txt") == "/data/hello.txt"


def test_chmod(imported: FS) -> None:
    imported.chmod("/data/hello.txt", 0o600)
    assert imported.stat("/data/hello.txt")["mode"] == 0o600


def test_rename(imported: FS) -> None:
    imported.rename("/data/hello.txt", "/data/renamed.txt")
    assert imported.read_file("/data/renamed.txt") == b"world"
    assert not imported.exists("/data/hello.txt")


def test_copy(imported: FS) -> None:
    imported.copy("/data/hello.txt", "/data/copied.txt")
    assert imported.read_file("/data/copied.txt") == b"world"
    assert imported.read_file("/data/hello.txt") == b"world"


# -- Capsule semantics (native only) ------------------------------------------


def test_multiple_imports_share_state() -> None:
    source = FileSystem()
    source.write_file("/f.txt", b"original")
    capsule = source.to_capsule()

    a = FileSystem.from_capsule(capsule)
    b = FileSystem.from_capsule(capsule)

    a.write_file("/f.txt", b"mutated")
    assert b.read_file("/f.txt") == b"mutated"


def test_source_mutation_visible_through_capsule() -> None:
    source = FileSystem()
    source.write_file("/f.txt", b"v1")
    imported = FileSystem.from_capsule(source.to_capsule())

    source.write_file("/f.txt", b"v2")
    assert imported.read_file("/f.txt") == b"v2"


def test_imported_mutation_visible_to_source() -> None:
    source = FileSystem()
    source.write_file("/f.txt", b"original")
    imported = FileSystem.from_capsule(source.to_capsule())

    imported.write_file("/f.txt", b"changed")
    assert source.read_file("/f.txt") == b"changed"


def test_double_capsule_roundtrip() -> None:
    fs1 = FileSystem()
    fs1.write_file("/f.txt", b"data")

    fs2 = FileSystem.from_capsule(fs1.to_capsule())
    fs3 = FileSystem.from_capsule(fs2.to_capsule())

    assert fs3.read_file("/f.txt") == b"data"


# -- Mount behavior ------------------------------------------------------------


def test_mount_and_execute() -> None:
    source = FileSystem()
    source.write_file("/greeting.txt", b"hello\n")

    bash = Bash()
    bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))

    result = bash.execute_sync("cat /mnt/greeting.txt")
    assert result.exit_code == 0
    assert result.stdout == "hello\n"


def test_multiple_mounts_isolated() -> None:
    fs1 = FileSystem()
    fs1.write_file("/data.txt", b"from fs1\n")

    fs2 = FileSystem()
    fs2.write_file("/data.txt", b"from fs2\n")

    bash = Bash()
    bash.mount("/mnt/one", FileSystem.from_capsule(fs1.to_capsule()))
    bash.mount("/mnt/two", FileSystem.from_capsule(fs2.to_capsule()))

    r1 = bash.execute_sync("cat /mnt/one/data.txt")
    r2 = bash.execute_sync("cat /mnt/two/data.txt")
    assert r1.stdout == "from fs1\n"
    assert r2.stdout == "from fs2\n"


@pytest.mark.skip(reason="unmount capsule FS aborts process — double-drop of Arc in bashkit-fs-interop")
def test_unmount_removes_access() -> None:
    source = FileSystem()
    source.write_file("/f.txt", b"data\n")

    bash = Bash()
    bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))
    assert bash.execute_sync("cat /mnt/f.txt").exit_code == 0

    bash.unmount("/mnt")
    result = bash.execute_sync("test -f /mnt/f.txt && echo yes || echo no")
    assert result.stdout.strip() == "no"


def test_mount_deep_directory_structure() -> None:
    source = FileSystem()
    source.mkdir("/a/b/c/d", recursive=True)
    source.write_file("/a/b/c/d/deep.txt", b"found\n")

    bash = Bash()
    bash.mount("/workspace", FileSystem.from_capsule(source.to_capsule()))

    result = bash.execute_sync("cat /workspace/a/b/c/d/deep.txt")
    assert result.exit_code == 0
    assert result.stdout == "found\n"


def test_write_through_mounted_capsule() -> None:
    source = FileSystem()
    source.mkdir("/data")

    bash = Bash()
    bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))
    bash.execute_sync("echo 'written by bash' > /mnt/data/output.txt")

    assert source.read_file("/data/output.txt") == b"written by bash\n"


# -- Error cases ---------------------------------------------------------------


def test_from_capsule_wrong_type_raises() -> None:
    with pytest.raises((TypeError, RuntimeError)):
        FileSystem.from_capsule("not a capsule")


def test_from_capsule_none_raises() -> None:
    with pytest.raises((TypeError, RuntimeError)):
        FileSystem.from_capsule(None)


# -- Bash commands against capsule FS ------------------------------------------


@pytest.fixture()
def workspace() -> Bash:
    source = FileSystem()
    source.mkdir("/repo/src", recursive=True)
    source.mkdir("/repo/docs")
    source.write_file("/repo/src/lib.py", b"def hello():\n    return 'hello'\n")
    source.write_file("/repo/src/main.py", b"from lib import hello\nprint(hello())\n")
    source.write_file("/repo/docs/guide.md", b"# Guide\n\nSee src/ for code.\n")
    source.write_file("/repo/README.md", b"# Project\n")
    bash = Bash()
    bash.mount("/ws", FileSystem.from_capsule(source.to_capsule()))
    return bash


def test_bash_find(workspace: Bash) -> None:
    result = workspace.execute_sync("find /ws/repo -name '*.py' | sort")
    assert result.exit_code == 0
    lines = result.stdout.strip().split("\n")
    assert lines == ["/ws/repo/src/lib.py", "/ws/repo/src/main.py"]


def test_bash_grep_recursive(workspace: Bash) -> None:
    result = workspace.execute_sync("grep -rl 'hello' /ws/repo/src | sort")
    assert result.exit_code == 0
    lines = result.stdout.strip().split("\n")
    assert lines == ["/ws/repo/src/lib.py", "/ws/repo/src/main.py"]


def test_bash_wc(workspace: Bash) -> None:
    result = workspace.execute_sync("wc -l /ws/repo/src/lib.py")
    assert result.exit_code == 0
    assert "2" in result.stdout


def test_bash_cat_pipe_grep(workspace: Bash) -> None:
    result = workspace.execute_sync("cat /ws/repo/README.md | grep Project")
    assert result.exit_code == 0
    assert "Project" in result.stdout


def test_bash_ls(workspace: Bash) -> None:
    result = workspace.execute_sync("ls /ws/repo | sort")
    assert result.exit_code == 0
    names = result.stdout.strip().split("\n")
    assert names == ["README.md", "docs", "src"]


def test_bash_head(workspace: Bash) -> None:
    result = workspace.execute_sync("head -1 /ws/repo/docs/guide.md")
    assert result.exit_code == 0
    assert result.stdout.strip() == "# Guide"
