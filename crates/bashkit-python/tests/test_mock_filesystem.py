import sys
from pathlib import Path
import pytest

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

from mock_filesystem import MockFileSystem

def test_write_and_read_file():
    fs = MockFileSystem()
    fs.write_file("/hello.txt", b"world")
    assert fs.read_file("/hello.txt") == b"world"

def test_read_nonexistent_raises():
    fs = MockFileSystem()
    with pytest.raises(FileNotFoundError):
        fs.read_file("/nope.txt")
