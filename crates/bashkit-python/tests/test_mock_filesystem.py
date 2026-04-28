import sys
from pathlib import Path

import pytest

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

from mock_filesystem import MockFileSystem  # noqa: E402


class TestReadWrite:
    def test_write_and_read(self):
        fs = MockFileSystem()
        fs.write_file("/hello.txt", b"world")
        assert fs.read_file("/hello.txt") == b"world"

    def test_read_nonexistent_raises(self):
        fs = MockFileSystem()
        with pytest.raises(FileNotFoundError):
            fs.read_file("/nope.txt")

    def test_write_overwrites(self):
        fs = MockFileSystem()
        fs.write_file("/f.txt", b"old")
        fs.write_file("/f.txt", b"new")
        assert fs.read_file("/f.txt") == b"new"

    def test_append(self):
        fs = MockFileSystem()
        fs.write_file("/f.txt", b"hello")
        fs.append_file("/f.txt", b" world")
        assert fs.read_file("/f.txt") == b"hello world"

    def test_append_creates_file(self):
        fs = MockFileSystem()
        fs.append_file("/new.txt", b"data")
        assert fs.read_file("/new.txt") == b"data"

    def test_write_to_directory_raises(self):
        fs = MockFileSystem()
        fs.mkdir("/dir")
        with pytest.raises(IsADirectoryError):
            fs.write_file("/dir", b"data")

    def test_read_directory_raises(self):
        fs = MockFileSystem()
        fs.mkdir("/dir")
        with pytest.raises(IsADirectoryError):
            fs.read_file("/dir")

    def test_write_missing_parent_raises(self):
        fs = MockFileSystem()
        with pytest.raises(FileNotFoundError):
            fs.write_file("/no/such/file.txt", b"data")

    def test_binary_content(self):
        fs = MockFileSystem()
        data = bytes(range(256))
        fs.write_file("/bin.dat", data)
        assert fs.read_file("/bin.dat") == data

    def test_empty_file(self):
        fs = MockFileSystem()
        fs.write_file("/empty.txt", b"")
        assert fs.read_file("/empty.txt") == b""


class TestDirectories:
    def test_mkdir(self):
        fs = MockFileSystem()
        fs.mkdir("/data")
        assert fs.exists("/data")
        assert fs.stat("/data")["file_type"] == "directory"

    def test_mkdir_recursive(self):
        fs = MockFileSystem()
        fs.mkdir("/a/b/c", recursive=True)
        assert fs.exists("/a")
        assert fs.exists("/a/b")
        assert fs.exists("/a/b/c")

    def test_mkdir_existing_raises(self):
        fs = MockFileSystem()
        fs.mkdir("/data")
        with pytest.raises(FileExistsError):
            fs.mkdir("/data")

    def test_mkdir_existing_recursive_ok(self):
        fs = MockFileSystem()
        fs.mkdir("/data")
        fs.mkdir("/data", recursive=True)

    def test_mkdir_no_parent_raises(self):
        fs = MockFileSystem()
        with pytest.raises(FileNotFoundError):
            fs.mkdir("/a/b")

    def test_read_dir(self):
        fs = MockFileSystem()
        fs.mkdir("/data")
        fs.write_file("/data/a.txt", b"a")
        fs.write_file("/data/b.txt", b"b")
        names = sorted(e["name"] for e in fs.read_dir("/data"))
        assert names == ["a.txt", "b.txt"]

    def test_read_dir_excludes_nested(self):
        fs = MockFileSystem()
        fs.mkdir("/a/b", recursive=True)
        fs.write_file("/a/b/deep.txt", b"deep")
        fs.write_file("/a/top.txt", b"top")
        names = [e["name"] for e in fs.read_dir("/a")]
        assert sorted(names) == ["b", "top.txt"]


class TestRemove:
    def test_remove_file(self):
        fs = MockFileSystem()
        fs.write_file("/f.txt", b"data")
        fs.remove("/f.txt")
        assert not fs.exists("/f.txt")

    def test_remove_nonexistent_raises(self):
        fs = MockFileSystem()
        with pytest.raises(FileNotFoundError):
            fs.remove("/nope")

    def test_remove_nonempty_dir_raises(self):
        fs = MockFileSystem()
        fs.mkdir("/dir")
        fs.write_file("/dir/f.txt", b"data")
        with pytest.raises(OSError):
            fs.remove("/dir")

    def test_remove_recursive(self):
        fs = MockFileSystem()
        fs.mkdir("/dir/sub", recursive=True)
        fs.write_file("/dir/sub/f.txt", b"data")
        fs.remove("/dir", recursive=True)
        assert not fs.exists("/dir")
        assert not fs.exists("/dir/sub")
        assert not fs.exists("/dir/sub/f.txt")

    def test_remove_root_raises(self):
        fs = MockFileSystem()
        with pytest.raises(OSError):
            fs.remove("/")


class TestSymlinks:
    def test_symlink_and_read_link(self):
        fs = MockFileSystem()
        fs.write_file("/target.txt", b"data")
        fs.symlink("/target.txt", "/link.txt")
        assert fs.read_link("/link.txt") == "/target.txt"
        assert fs.stat("/link.txt")["file_type"] == "symlink"

    def test_read_link_nonexistent_raises(self):
        fs = MockFileSystem()
        with pytest.raises(FileNotFoundError):
            fs.read_link("/nope")

    def test_read_link_not_symlink_raises(self):
        fs = MockFileSystem()
        fs.write_file("/f.txt", b"data")
        with pytest.raises(OSError):
            fs.read_link("/f.txt")


class TestMetadata:
    def test_stat_file(self):
        fs = MockFileSystem()
        fs.write_file("/f.txt", b"hello")
        s = fs.stat("/f.txt")
        assert s["file_type"] == "file"
        assert s["size"] == 5
        assert s["mode"] == 0o644

    def test_stat_nonexistent_raises(self):
        fs = MockFileSystem()
        with pytest.raises(FileNotFoundError):
            fs.stat("/nope")

    def test_chmod(self):
        fs = MockFileSystem()
        fs.write_file("/f.txt", b"data")
        fs.chmod("/f.txt", 0o755)
        assert fs.stat("/f.txt")["mode"] == 0o755


class TestCopyRename:
    def test_copy(self):
        fs = MockFileSystem()
        fs.write_file("/src.txt", b"data")
        fs.copy("/src.txt", "/dst.txt")
        assert fs.read_file("/dst.txt") == b"data"
        assert fs.read_file("/src.txt") == b"data"

    def test_copy_preserves_mode(self):
        fs = MockFileSystem()
        fs.write_file("/src.txt", b"data")
        fs.chmod("/src.txt", 0o755)
        fs.copy("/src.txt", "/dst.txt")
        assert fs.stat("/dst.txt")["mode"] == 0o755

    def test_copy_directory_raises(self):
        fs = MockFileSystem()
        fs.mkdir("/dir")
        with pytest.raises(IsADirectoryError):
            fs.copy("/dir", "/dir2")

    def test_rename(self):
        fs = MockFileSystem()
        fs.write_file("/old.txt", b"data")
        fs.rename("/old.txt", "/new.txt")
        assert fs.read_file("/new.txt") == b"data"
        assert not fs.exists("/old.txt")

    def test_rename_directory_moves_children(self):
        fs = MockFileSystem()
        fs.mkdir("/a/b", recursive=True)
        fs.write_file("/a/b/f.txt", b"data")
        fs.rename("/a", "/z")
        assert fs.read_file("/z/b/f.txt") == b"data"
        assert not fs.exists("/a")

    def test_exists(self):
        fs = MockFileSystem()
        assert not fs.exists("/nope")
        fs.write_file("/yes.txt", b"")
        assert fs.exists("/yes.txt")


class TestPathNormalization:
    def test_dotdot_resolves(self):
        fs = MockFileSystem()
        fs.mkdir("/a")
        fs.write_file("/a/f.txt", b"data")
        assert fs.read_file("/a/../a/f.txt") == b"data"

    def test_relative_path_raises(self):
        fs = MockFileSystem()
        with pytest.raises(ValueError):
            fs.write_file("relative.txt", b"data")
