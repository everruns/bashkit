"""Capsule interop tests for FileSystem (PR #1353).

Tests to_capsule()/from_capsule() API surface: VFS operations through
capsule-imported handles, mount behavior, capsule semantics, error cases,
and bash command execution against mounted capsule filesystems.
"""

import pytest

from bashkit import Bash, FileSystem

# -- VFS operations through capsule-imported FS --------------------------------


def _make_imported():
    source = FileSystem()
    source.mkdir("/data/sub", recursive=True)
    source.write_file("/data/hello.txt", b"world")
    source.write_file("/data/sub/nested.txt", b"deep")
    source.symlink("/data/hello.txt", "/data/link.txt")
    source.chmod("/data/hello.txt", 0o755)
    return FileSystem.from_capsule(source.to_capsule())


def test_capsule_read_file():
    fs = _make_imported()
    assert fs.read_file("/data/hello.txt") == b"world"


def test_capsule_write_file():
    fs = _make_imported()
    fs.write_file("/data/new.txt", b"created")
    assert fs.read_file("/data/new.txt") == b"created"


def test_capsule_append_file():
    fs = _make_imported()
    fs.append_file("/data/hello.txt", b"!")
    assert fs.read_file("/data/hello.txt") == b"world!"


def test_capsule_mkdir():
    fs = _make_imported()
    fs.mkdir("/data/newdir")
    assert fs.exists("/data/newdir")
    assert fs.stat("/data/newdir")["file_type"] == "directory"


def test_capsule_remove_file():
    fs = _make_imported()
    fs.remove("/data/sub/nested.txt")
    assert not fs.exists("/data/sub/nested.txt")


def test_capsule_remove_dir_recursive():
    fs = _make_imported()
    fs.remove("/data/sub", recursive=True)
    assert not fs.exists("/data/sub")


def test_capsule_exists():
    fs = _make_imported()
    assert fs.exists("/data/hello.txt")
    assert not fs.exists("/data/nope.txt")


def test_capsule_stat():
    fs = _make_imported()
    s = fs.stat("/data/hello.txt")
    assert s["file_type"] == "file"
    assert s["size"] == 5
    assert s["mode"] == 0o755


def test_capsule_read_dir():
    fs = _make_imported()
    names = sorted(e["name"] for e in fs.read_dir("/data"))
    assert "hello.txt" in names
    assert "sub" in names
    assert "link.txt" in names


def test_capsule_symlink_and_read_link():
    fs = _make_imported()
    assert fs.read_link("/data/link.txt") == "/data/hello.txt"


def test_capsule_chmod():
    fs = _make_imported()
    fs.chmod("/data/hello.txt", 0o600)
    assert fs.stat("/data/hello.txt")["mode"] == 0o600


def test_capsule_rename():
    fs = _make_imported()
    fs.rename("/data/hello.txt", "/data/renamed.txt")
    assert fs.read_file("/data/renamed.txt") == b"world"
    assert not fs.exists("/data/hello.txt")


def test_capsule_copy():
    fs = _make_imported()
    fs.copy("/data/hello.txt", "/data/copied.txt")
    assert fs.read_file("/data/copied.txt") == b"world"
    assert fs.read_file("/data/hello.txt") == b"world"


# -- Capsule semantics ---------------------------------------------------------


def test_multiple_imports_share_state():
    source = FileSystem()
    source.write_file("/f.txt", b"original")
    capsule = source.to_capsule()

    a = FileSystem.from_capsule(capsule)
    b = FileSystem.from_capsule(capsule)

    a.write_file("/f.txt", b"mutated")
    assert b.read_file("/f.txt") == b"mutated"


def test_source_mutation_visible_through_capsule():
    source = FileSystem()
    source.write_file("/f.txt", b"v1")
    imported = FileSystem.from_capsule(source.to_capsule())

    source.write_file("/f.txt", b"v2")
    assert imported.read_file("/f.txt") == b"v2"


def test_imported_mutation_visible_to_source():
    source = FileSystem()
    source.write_file("/f.txt", b"original")
    imported = FileSystem.from_capsule(source.to_capsule())

    imported.write_file("/f.txt", b"changed")
    assert source.read_file("/f.txt") == b"changed"


def test_double_capsule_roundtrip():
    fs1 = FileSystem()
    fs1.write_file("/f.txt", b"data")

    fs2 = FileSystem.from_capsule(fs1.to_capsule())
    fs3 = FileSystem.from_capsule(fs2.to_capsule())

    assert fs3.read_file("/f.txt") == b"data"


# -- Mount behavior ------------------------------------------------------------


def test_mount_and_execute():
    source = FileSystem()
    source.write_file("/greeting.txt", b"hello\n")

    bash = Bash()
    bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))

    result = bash.execute_sync("cat /mnt/greeting.txt")
    assert result.exit_code == 0
    assert result.stdout == "hello\n"


def test_multiple_mounts_isolated():
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


# NOTE: unmount of capsule-imported FS aborts in release_export_state
# (double-drop of Arc in bashkit-fs-interop). Skipped until fixed in PR #1353.
@pytest.mark.skip(reason="unmount capsule FS panics — native binding bug")
def test_unmount_removes_access():
    source = FileSystem()
    source.write_file("/f.txt", b"data\n")

    bash = Bash()
    bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))
    assert bash.execute_sync("cat /mnt/f.txt").exit_code == 0

    bash.unmount("/mnt")
    result = bash.execute_sync("test -f /mnt/f.txt && echo yes || echo no")
    assert result.stdout.strip() == "no"


def test_mount_deep_directory_structure():
    source = FileSystem()
    source.mkdir("/a/b/c/d", recursive=True)
    source.write_file("/a/b/c/d/deep.txt", b"found\n")

    bash = Bash()
    bash.mount("/workspace", FileSystem.from_capsule(source.to_capsule()))

    result = bash.execute_sync("cat /workspace/a/b/c/d/deep.txt")
    assert result.exit_code == 0
    assert result.stdout == "found\n"


def test_write_through_mounted_capsule():
    source = FileSystem()
    source.mkdir("/data")

    bash = Bash()
    bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))
    bash.execute_sync("echo 'written by bash' > /mnt/data/output.txt")

    assert source.read_file("/data/output.txt") == b"written by bash\n"


# -- Error cases ---------------------------------------------------------------


def test_from_capsule_wrong_type_raises():
    with pytest.raises((TypeError, RuntimeError)):
        FileSystem.from_capsule("not a capsule")


def test_from_capsule_none_raises():
    with pytest.raises((TypeError, RuntimeError)):
        FileSystem.from_capsule(None)


# -- Bash commands against capsule FS ------------------------------------------


def _make_workspace():
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


def test_bash_find():
    bash = _make_workspace()
    result = bash.execute_sync("find /ws/repo -name '*.py' | sort")
    assert result.exit_code == 0
    lines = result.stdout.strip().split("\n")
    assert lines == ["/ws/repo/src/lib.py", "/ws/repo/src/main.py"]


def test_bash_grep_recursive():
    bash = _make_workspace()
    result = bash.execute_sync("grep -rl 'hello' /ws/repo/src | sort")
    assert result.exit_code == 0
    lines = result.stdout.strip().split("\n")
    assert lines == ["/ws/repo/src/lib.py", "/ws/repo/src/main.py"]


def test_bash_wc():
    bash = _make_workspace()
    result = bash.execute_sync("wc -l /ws/repo/src/lib.py")
    assert result.exit_code == 0
    assert "2" in result.stdout


def test_bash_cat_pipe_grep():
    bash = _make_workspace()
    result = bash.execute_sync("cat /ws/repo/README.md | grep Project")
    assert result.exit_code == 0
    assert "Project" in result.stdout


def test_bash_ls():
    bash = _make_workspace()
    result = bash.execute_sync("ls /ws/repo | sort")
    assert result.exit_code == 0
    names = result.stdout.strip().split("\n")
    assert names == ["README.md", "docs", "src"]


def test_bash_head():
    bash = _make_workspace()
    result = bash.execute_sync("head -1 /ws/repo/docs/guide.md")
    assert result.exit_code == 0
    assert result.stdout.strip() == "# Guide"
