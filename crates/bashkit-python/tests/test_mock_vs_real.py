"""Compare MockFileSystem behavior against bashkit's native FileSystem."""

import sys
from pathlib import Path

import pytest

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

from mock_filesystem import MockFileSystem  # noqa: E402

from bashkit import FileSystem  # noqa: E402


@pytest.fixture(params=["mock", "native"])
def fs(request):
    if request.param == "mock":
        return MockFileSystem()
    return FileSystem()


def _read(fs, path):
    return fs.read_file(path)


class TestParity:
    def test_write_read_roundtrip(self, fs):
        fs.write_file("/hello.txt", b"world")
        assert _read(fs, "/hello.txt") == b"world"

    def test_mkdir_recursive_and_read_dir(self, fs):
        fs.mkdir("/a/b/c", recursive=True)
        fs.write_file("/a/top.txt", b"t")
        names = sorted(e["name"] for e in fs.read_dir("/a"))
        assert names == ["b", "top.txt"]

    def test_append(self, fs):
        fs.write_file("/f.txt", b"hello")
        fs.append_file("/f.txt", b" world")
        assert _read(fs, "/f.txt") == b"hello world"

    def test_remove_recursive(self, fs):
        fs.mkdir("/d/sub", recursive=True)
        fs.write_file("/d/sub/f.txt", b"x")
        fs.remove("/d", recursive=True)
        assert not fs.exists("/d")

    def test_copy(self, fs):
        fs.write_file("/src.txt", b"data")
        fs.copy("/src.txt", "/dst.txt")
        assert _read(fs, "/dst.txt") == b"data"

    def test_rename(self, fs):
        fs.write_file("/old.txt", b"data")
        fs.rename("/old.txt", "/new.txt")
        assert _read(fs, "/new.txt") == b"data"
        assert not fs.exists("/old.txt")

    def test_symlink_and_read_link(self, fs):
        fs.write_file("/target.txt", b"data")
        fs.symlink("/target.txt", "/link.txt")
        assert fs.read_link("/link.txt") == "/target.txt"

    def test_chmod(self, fs):
        fs.write_file("/f.txt", b"data")
        fs.chmod("/f.txt", 0o600)
        assert fs.stat("/f.txt")["mode"] == 0o600

    def test_stat_file(self, fs):
        fs.write_file("/f.txt", b"hello")
        s = fs.stat("/f.txt")
        assert s["file_type"] == "file"
        assert s["size"] == 5
