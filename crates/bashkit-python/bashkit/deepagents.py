"""
Deep Agents integration for Bashkit.

Provides middleware and backend for Deep Agents using Bashkit's VFS:

- ``BashkitMiddleware``: Adds ``bash`` tool via ``AgentMiddleware.tools``
- ``BashkitBackend``: ``SandboxBackendProtocol`` for execute/read/write/ls/glob/grep.

Standalone middleware (creates its own VFS)::

    >>> from bashkit.deepagents import create_bash_middleware
    >>> middleware = create_bash_middleware(timeout_seconds=30)
    >>> agent = create_deep_agent(middleware=[middleware])

Backend with shared VFS (recommended)::

    >>> from bashkit.deepagents import create_bashkit_backend
    >>> backend = create_bashkit_backend()
    >>> middleware = backend.create_middleware()  # shares VFS with backend
    >>> agent = create_deep_agent(backend=backend, middleware=[middleware])

The backend exposes file operations on the same VFS used by bash::

    >>> backend = create_bashkit_backend()
    >>> backend.execute("echo hello > /greeting.txt")
    >>> content = backend.read("/greeting.txt")
"""

from __future__ import annotations

import posixpath
import uuid
from typing import TYPE_CHECKING

from bashkit import BashTool as NativeBashTool

# Check for deepagents availability
try:
    from deepagents.backends.protocol import (
        DeleteResult,
        EditResult,
        ExecuteResponse,
        FileDownloadResponse,
        FileInfo,
        FileUploadResponse,
        GlobResult,
        GrepMatch,
        GrepResult,
        LsResult,
        ReadResult,
        SandboxBackendProtocol,
        WriteResult,
    )
    from deepagents.backends.utils import compile_grep_include_glob
    from langchain.agents.middleware.types import AgentMiddleware
    from langchain_core.tools import tool as langchain_tool

    DEEPAGENTS_AVAILABLE = True
except ImportError:
    DEEPAGENTS_AVAILABLE = False
    if not TYPE_CHECKING:
        SandboxBackendProtocol = object
        AgentMiddleware = object


def _make_bash_tool(bash_instance: NativeBashTool, max_output_length: int = 100_000):
    """Create a bash tool function from a BashTool instance."""
    # Use name and description from bashkit lib
    tool_name = bash_instance.name
    tool_description = bash_instance.description()

    @langchain_tool(tool_name, description=tool_description)
    def bashkit(command: str) -> str:
        result = bash_instance.execute_sync(command)
        output = result.stdout
        if result.error:
            output += f"\nError: {result.error}"
        if result.stderr:
            output += f"\n{result.stderr}"
        if result.exit_code != 0:
            output += f"\n[Exit code: {result.exit_code}]"
        output = output.strip() if output else "[No output]"
        if len(output) > max_output_length:
            output = output[:max_output_length] + "\n[truncated]"
        return output

    return bashkit


if DEEPAGENTS_AVAILABLE:

    class BashkitMiddleware(AgentMiddleware):
        """Middleware that adds `bash` tool for shell execution in VFS.

        Example standalone:
            >>> middleware = BashkitMiddleware()
            >>> agent = create_deep_agent(middleware=[middleware])

        Example with shared VFS (recommended):
            >>> backend = BashkitBackend()
            >>> middleware = backend.create_middleware()
            >>> agent = create_deep_agent(backend=backend, middleware=[middleware])
        """

        def __init__(
            self,
            bash_tool: NativeBashTool | None = None,
            username: str | None = None,
            hostname: str | None = None,
            max_commands: int | None = None,
            max_loop_iterations: int | None = None,
            timeout_seconds: float | None = None,
        ):
            """Initialize middleware.

            Args:
                bash_tool: Existing BashTool to use (for shared VFS)
                username: Username for new BashTool (ignored if bash_tool provided)
                hostname: Hostname for new BashTool (ignored if bash_tool provided)
                max_commands: Max commands (ignored if bash_tool provided)
                max_loop_iterations: Max iterations (ignored if bash_tool provided)
                timeout_seconds: Execution timeout in seconds (ignored if bash_tool provided)
            """
            if bash_tool is not None:
                self._bash = bash_tool
                self._owns_bash = False
            else:
                self._bash = NativeBashTool(
                    username=username,
                    hostname=hostname,
                    max_commands=max_commands,
                    max_loop_iterations=max_loop_iterations,
                    timeout_seconds=timeout_seconds,
                )
                self._owns_bash = True

            self._tools = [_make_bash_tool(self._bash)]

        @property
        def tools(self):
            """Tools provided by this middleware."""
            return self._tools

        def execute_sync(self, command: str) -> str:
            """Execute command synchronously (for setup scripts)."""
            result = self._bash.execute_sync(command)
            output = result.stdout + (result.stderr or "")
            if result.error and result.error not in output:
                output += f"\nError: {result.error}"
            return output

        def reset(self) -> None:
            """Reset VFS to initial state."""
            if self._owns_bash:
                self._bash.reset()

    class BashkitBackend(SandboxBackendProtocol):
        """Backend implementing SandboxBackendProtocol with Bashkit VFS.

        Provides execute, read, write, edit, delete, ls, glob, grep
        all operating on the same virtual filesystem.

        Example:
            >>> backend = BashkitBackend()
            >>> agent = create_deep_agent(backend=backend)

        With middleware for additional `bash` tool:
            >>> backend = BashkitBackend()
            >>> middleware = backend.create_middleware()
            >>> agent = create_deep_agent(backend=backend, middleware=[middleware])
        """

        def __init__(
            self,
            username: str | None = None,
            hostname: str | None = None,
            max_commands: int | None = None,
            max_loop_iterations: int | None = None,
            timeout_seconds: float | None = None,
        ):
            self._bash = NativeBashTool(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
                timeout_seconds=timeout_seconds,
            )
            self._id = f"bashkit-{uuid.uuid4().hex[:8]}"

        @property
        def id(self) -> str:
            return self._id

        def create_middleware(self) -> BashkitMiddleware:
            """Create middleware that shares this backend's VFS.

            Returns:
                BashkitMiddleware using same BashTool instance
            """
            return BashkitMiddleware(bash_tool=self._bash)

        # File helpers use the live VFS directly: shell output caps and delimiter
        # parsing must never silently corrupt file contents or discovered paths.
        # Async protocol defaults dispatch these sync methods off the event loop.

        # Deep Agents explicitly detects backends without per-call timeout support.
        # Keep the keyword absent so the framework uses our constructor timeout.
        def execute(self, command: str) -> ExecuteResponse:  # type: ignore[override]
            result = self._bash.execute_sync(command)
            output = result.stdout + (result.stderr or "")
            if result.error and result.error not in output:
                output += f"\nError: {result.error}"
            return ExecuteResponse(
                output=output,
                exit_code=result.exit_code,
                truncated=result.stdout_truncated or result.stderr_truncated,
            )

        def _path(self, path: str) -> str:
            return posixpath.normpath(posixpath.join(self._bash.shell_state().cwd, path))

        def read(self, file_path: str, offset: int = 0, limit: int = 2000) -> ReadResult:
            if limit <= 0:
                return ReadResult(file_data={"content": "", "encoding": "utf-8"}, no_lines_requested=True)
            try:
                lines = self._bash.read_file(self._path(file_path)).splitlines()
            except (RuntimeError, ValueError) as exc:
                return ReadResult(error=str(exc))
            offset = max(0, offset)
            selected = lines[offset : offset + limit]
            start_line = end_line = total_lines = next_offset = None
            if selected:
                end = offset + len(selected)
                start_line, end_line, total_lines = offset + 1, end, len(lines)
                if end < len(lines):
                    next_offset = end
            return ReadResult(
                file_data={"content": "\n".join(selected), "encoding": "utf-8"},
                start_line=start_line,
                end_line=end_line,
                total_lines=total_lines,
                next_offset=next_offset,
            )

        def write(self, file_path: str, content: str) -> WriteResult:
            path = self._path(file_path)
            try:
                if self._bash.exists(path):
                    return WriteResult(error=f"File already exists: {path}")
                self._bash.mkdir(posixpath.dirname(path), recursive=True)
                self._bash.write_file(path, content)
            except (RuntimeError, ValueError) as exc:
                return WriteResult(error=str(exc))
            return WriteResult(path=path)

        def edit(self, file_path: str, old_string: str, new_string: str, replace_all: bool = False) -> EditResult:
            path = self._path(file_path)
            try:
                content = self._bash.read_file(path)
                count = content.count(old_string)
                if count == 0:
                    return EditResult(error="old_string not found")
                if count > 1 and not replace_all:
                    return EditResult(error=f"Found {count} times. Use replace_all=True")
                self._bash.write_file(path, content.replace(old_string, new_string, -1 if replace_all else 1))
            except (RuntimeError, ValueError) as exc:
                return EditResult(error=str(exc))
            return EditResult(path=path, occurrences=count if replace_all else 1)

        def delete(self, file_path: str) -> DeleteResult:
            path = self._path(file_path)
            try:
                if self._bash.stat(path)["file_type"] == "directory":
                    return DeleteResult(error="Cannot delete a directory")
                self._bash.remove(path)
            except (RuntimeError, ValueError) as exc:
                return DeleteResult(error=str(exc))
            return DeleteResult(path=path)

        def ls(self, path: str) -> LsResult:
            path = self._path(path)
            try:
                entries = [
                    FileInfo(
                        path=posixpath.join(path, entry["name"]),
                        is_dir=entry["metadata"]["file_type"] == "directory",
                        size=entry["metadata"]["size"],
                    )
                    for entry in self._bash.read_dir(path)
                ]
            except (RuntimeError, ValueError) as exc:
                return LsResult(error=str(exc))
            return LsResult(entries=sorted(entries, key=lambda entry: entry["path"]))

        def _files(self, root: str):
            pending = [root]
            while pending:
                path = pending.pop()
                metadata = self._bash.stat(path)
                if metadata["file_type"] == "directory":
                    # Do not traverse symlink directories: cycles must not create
                    # an unbounded host-side walk outside shell execution limits.
                    entries = self._bash.read_dir(path)
                    pending.extend(
                        posixpath.join(path, entry["name"])
                        for entry in reversed(sorted(entries, key=lambda entry: entry["name"]))
                        if entry["metadata"]["file_type"] in ("file", "directory")
                    )
                elif metadata["file_type"] == "file":
                    yield FileInfo(path=path, is_dir=False, size=metadata["size"])

        def glob(self, pattern: str, path: str | None = None) -> GlobResult:
            root = self._path(path or ".")
            try:
                matches = compile_grep_include_glob(pattern)
                files = [info for info in self._files(root) if matches(posixpath.relpath(info["path"], root))]
            except (RuntimeError, ValueError) as exc:
                return GlobResult(error=str(exc))
            return GlobResult(matches=files)

        def grep(
            self, pattern: str, path: str | None = None, glob: str | None = None, *, max_count: int | None = None
        ) -> GrepResult:
            root = self._path(path or ".")
            matches: list[GrepMatch] = []
            try:
                include = compile_grep_include_glob(glob) if glob is not None else lambda _: True
                for info in self._files(root):
                    relative = posixpath.relpath(info["path"], root)
                    if not include(posixpath.basename(root) if relative == "." else relative):
                        continue
                    for number, line in enumerate(self._bash.read_file(info["path"]).splitlines(), 1):
                        if pattern in line:
                            if max_count is not None and len(matches) >= max(0, max_count):
                                return GrepResult(matches=matches, truncated=True)
                            matches.append(GrepMatch(path=info["path"], line=number, text=line))
            except (RuntimeError, ValueError) as exc:
                return GrepResult(error=str(exc), matches=matches or None, truncated=bool(matches))
            return GrepResult(matches=matches)

        def download_files(self, paths: list[str]) -> list[FileDownloadResponse]:
            responses = []
            for path in paths:
                try:
                    content = self._bash.fs().read_file(self._path(path))
                    responses.append(FileDownloadResponse(path=path, content=content))
                except (RuntimeError, ValueError) as exc:
                    responses.append(FileDownloadResponse(path=path, error=str(exc)))
            return responses

        def upload_files(self, files: list[tuple[str, bytes]]) -> list[FileUploadResponse]:
            responses = []
            for path, content in files:
                try:
                    absolute = self._path(path)
                    self._bash.mkdir(posixpath.dirname(absolute), recursive=True)
                    self._bash.fs().write_file(absolute, content)
                    responses.append(FileUploadResponse(path=path))
                except (RuntimeError, ValueError) as exc:
                    responses.append(FileUploadResponse(path=path, error=str(exc)))
            return responses

        # === Utility ===

        def setup(self, script: str) -> str:
            """Execute setup script."""
            result = self._bash.execute_sync(script)
            output = result.stdout + (result.stderr or "")
            if result.error and result.error not in output:
                output += f"\nError: {result.error}"
            return output

        def reset(self) -> None:
            """Reset VFS."""
            self._bash.reset()


def create_bash_middleware(**kwargs) -> BashkitMiddleware:
    """Create BashkitMiddleware for Deep Agents."""
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError("deepagents required. Install: pip install 'bashkit[deepagents]'")
    return BashkitMiddleware(**kwargs)


def create_bashkit_backend(**kwargs) -> BashkitBackend:
    """Create BashkitBackend for Deep Agents."""
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError("deepagents required. Install: pip install 'bashkit[deepagents]'")
    return BashkitBackend(**kwargs)


__all__ = [
    "BashkitMiddleware",
    "BashkitBackend",
    "create_bash_middleware",
    "create_bashkit_backend",
]
