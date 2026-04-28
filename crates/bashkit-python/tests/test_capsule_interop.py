"""Integration tests for FileSystem capsule interop (PR #1353)."""

from bashkit import Bash, FileSystem


def test_capsule_roundtrip_basic():
    source = FileSystem()
    source.mkdir("/project/src", recursive=True)
    source.write_file("/project/src/main.py", b'print("hello")\n')
    source.write_file("/project/README.md", b"# My Project\n")

    capsule = source.to_capsule()
    imported = FileSystem.from_capsule(capsule)

    bash = Bash()
    bash.mount("/workspace", imported)

    result = bash.execute_sync("cat /workspace/project/README.md")
    assert result.exit_code == 0
    assert result.stdout == "# My Project\n"

    result = bash.execute_sync("ls /workspace/project/src")
    assert result.exit_code == 0
    assert "main.py" in result.stdout


def test_capsule_roundtrip_read_dir():
    source = FileSystem()
    source.mkdir("/data", recursive=True)
    source.write_file("/data/a.txt", b"aaa")
    source.write_file("/data/b.txt", b"bbb")

    imported = FileSystem.from_capsule(source.to_capsule())

    bash = Bash()
    bash.mount("/mnt", imported)

    result = bash.execute_sync("ls /mnt/data | sort")
    assert result.exit_code == 0
    assert result.stdout.strip() == "a.txt\nb.txt"


def test_capsule_roundtrip_stat_and_permissions():
    source = FileSystem()
    source.write_file("/script.sh", b"#!/bin/bash\necho hi\n")
    source.chmod("/script.sh", 0o755)

    imported = FileSystem.from_capsule(source.to_capsule())
    stat = imported.stat("/script.sh")
    assert stat["mode"] == 0o755
    assert stat["file_type"] == "file"
    assert stat["size"] == len(b"#!/bin/bash\necho hi\n")


def test_capsule_roundtrip_symlinks():
    source = FileSystem()
    source.write_file("/real.txt", b"content")
    source.symlink("/real.txt", "/link.txt")

    imported = FileSystem.from_capsule(source.to_capsule())
    assert imported.read_link("/link.txt") == "/real.txt"


def test_capsule_multiple_mounts():
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


def test_capsule_with_bash_commands():
    source = FileSystem()
    source.mkdir("/repo/src", recursive=True)
    source.write_file("/repo/src/lib.py", b"def hello():\n    return 'hello'\n")
    source.write_file("/repo/src/main.py", b"from lib import hello\nprint(hello())\n")
    source.write_file("/repo/README.md", b"# README\n")

    bash = Bash()
    bash.mount("/workspace", FileSystem.from_capsule(source.to_capsule()))

    result = bash.execute_sync("grep -r 'hello' /workspace/repo/src | wc -l")
    assert result.exit_code == 0
    assert result.stdout.strip() == "4"

    result = bash.execute_sync("find /workspace/repo -name '*.py' | sort")
    assert result.exit_code == 0
    lines = result.stdout.strip().split("\n")
    assert lines == ["/workspace/repo/src/lib.py", "/workspace/repo/src/main.py"]
