"""Pure-Python mock of bashkit's FileSystem for local testing."""

import posixpath
import time


class _Entry:
    __slots__ = ("file_type", "content", "mode", "target", "created", "modified")

    def __init__(self, file_type, content=b"", mode=0o644, target=None):
        self.file_type = file_type
        self.content = bytearray(content) if file_type == "file" else bytearray()
        self.mode = mode
        self.target = target
        now = time.time()
        self.created = now
        self.modified = now


class MockFileSystem:
    """In-memory filesystem matching bashkit.FileSystem's Python API."""

    def __init__(self):
        self._entries: dict[str, _Entry] = {}
        self._entries["/"] = _Entry("directory", mode=0o755)

    def _normalize(self, path: str) -> str:
        if not path.startswith("/"):
            raise ValueError(f"path must be absolute: {path}")
        return posixpath.normpath(path)

    def _parent(self, path: str) -> str:
        return posixpath.dirname(path)

    def read_file(self, path: str) -> bytes:
        path = self._normalize(path)
        entry = self._entries.get(path)
        if entry is None:
            raise FileNotFoundError(f"No such file: {path}")
        if entry.file_type == "directory":
            raise IsADirectoryError(f"Is a directory: {path}")
        return bytes(entry.content)

    def write_file(self, path: str, content: bytes) -> None:
        path = self._normalize(path)
        parent = self._parent(path)
        if parent not in self._entries:
            raise FileNotFoundError(f"Parent directory does not exist: {parent}")
        entry = self._entries.get(path)
        if entry and entry.file_type == "directory":
            raise IsADirectoryError(f"Is a directory: {path}")
        if entry:
            entry.content = bytearray(content)
            entry.modified = time.time()
        else:
            self._entries[path] = _Entry("file", content)

    def append_file(self, path: str, content: bytes) -> None:
        path = self._normalize(path)
        entry = self._entries.get(path)
        if entry is None:
            self.write_file(path, content)
            return
        if entry.file_type == "directory":
            raise IsADirectoryError(f"Is a directory: {path}")
        entry.content.extend(content)
        entry.modified = time.time()

    def mkdir(self, path: str, recursive: bool = False) -> None:
        path = self._normalize(path)
        if path in self._entries:
            entry = self._entries[path]
            if entry.file_type == "directory" and recursive:
                return
            if entry.file_type == "directory":
                raise FileExistsError(f"Directory exists: {path}")
            raise FileExistsError(f"Path exists and is not a directory: {path}")
        parent = self._parent(path)
        if parent not in self._entries:
            if recursive:
                self.mkdir(parent, recursive=True)
            else:
                raise FileNotFoundError(f"Parent does not exist: {parent}")
        self._entries[path] = _Entry("directory", mode=0o755)

    def remove(self, path: str, recursive: bool = False) -> None:
        path = self._normalize(path)
        if path == "/":
            raise OSError("Cannot remove root directory")
        entry = self._entries.get(path)
        if entry is None:
            raise FileNotFoundError(f"No such file or directory: {path}")
        if entry.file_type == "directory":
            children = [k for k in self._entries if k != path and k.startswith(path + "/")]
            if children and not recursive:
                raise OSError(f"Directory not empty: {path}")
            for child in children:
                del self._entries[child]
        del self._entries[path]

    def exists(self, path: str) -> bool:
        path = self._normalize(path)
        return path in self._entries

    def stat(self, path: str) -> dict:
        path = self._normalize(path)
        entry = self._entries.get(path)
        if entry is None:
            raise FileNotFoundError(f"No such file or directory: {path}")
        return {
            "file_type": entry.file_type,
            "size": len(entry.content),
            "mode": entry.mode,
            "modified": entry.modified,
            "created": entry.created,
        }

    def read_dir(self, path: str) -> list[dict]:
        path = self._normalize(path)
        entry = self._entries.get(path)
        if entry is None:
            raise FileNotFoundError(f"No such directory: {path}")
        if entry.file_type != "directory":
            raise NotADirectoryError(f"Not a directory: {path}")
        prefix = path.rstrip("/") + "/"
        result = []
        seen = set()
        for k, v in self._entries.items():
            if not k.startswith(prefix):
                continue
            remainder = k[len(prefix):]
            if "/" in remainder:
                continue
            name = remainder
            if name and name not in seen:
                seen.add(name)
                result.append({
                    "name": name,
                    "metadata": {
                        "file_type": v.file_type,
                        "size": len(v.content),
                        "mode": v.mode,
                        "modified": v.modified,
                        "created": v.created,
                    },
                })
        return result

    def symlink(self, target: str, link: str) -> None:
        link = self._normalize(link)
        parent = self._parent(link)
        if parent not in self._entries:
            raise FileNotFoundError(f"Parent directory does not exist: {parent}")
        if link in self._entries:
            raise FileExistsError(f"Path already exists: {link}")
        entry = _Entry("symlink", target=target)
        self._entries[link] = entry

    def read_link(self, path: str) -> str:
        path = self._normalize(path)
        entry = self._entries.get(path)
        if entry is None:
            raise FileNotFoundError(f"No such symlink: {path}")
        if entry.file_type != "symlink" or entry.target is None:
            raise OSError(f"Not a symlink: {path}")
        return entry.target

    def chmod(self, path: str, mode: int) -> None:
        path = self._normalize(path)
        entry = self._entries.get(path)
        if entry is None:
            raise FileNotFoundError(f"No such file or directory: {path}")
        entry.mode = mode

    def rename(self, from_path: str, to_path: str) -> None:
        from_path = self._normalize(from_path)
        to_path = self._normalize(to_path)
        entry = self._entries.get(from_path)
        if entry is None:
            raise FileNotFoundError(f"No such file or directory: {from_path}")
        to_parent = self._parent(to_path)
        if to_parent not in self._entries:
            raise FileNotFoundError(f"Destination parent does not exist: {to_parent}")
        self._entries[to_path] = entry
        del self._entries[from_path]
        if entry.file_type == "directory":
            prefix = from_path + "/"
            moves = [(k, to_path + k[len(from_path):]) for k in list(self._entries) if k.startswith(prefix)]
            for old, new in moves:
                self._entries[new] = self._entries.pop(old)

    def copy(self, from_path: str, to_path: str) -> None:
        from_path = self._normalize(from_path)
        to_path = self._normalize(to_path)
        entry = self._entries.get(from_path)
        if entry is None:
            raise FileNotFoundError(f"No such file: {from_path}")
        if entry.file_type == "directory":
            raise IsADirectoryError(f"Cannot copy directory: {from_path}")
        self.write_file(to_path, bytes(entry.content))
        self._entries[to_path].mode = entry.mode
