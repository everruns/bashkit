"""
Deep Agents integration for BashKit.

Provides middleware and backend for Deep Agents using BashKit's VFS:
- BashKitMiddleware: Adds `bash` tool via AgentMiddleware.tools
- BashKitBackend: SandboxBackendProtocol for execute/read_file/write_file/etc.

Use together for shared VFS:
    >>> backend = BashKitBackend()
    >>> middleware = backend.create_middleware()  # shares VFS
    >>> agent = create_deep_agent(backend=backend, middleware=[middleware])
"""

from __future__ import annotations

import uuid
from datetime import datetime, timezone
from typing import Optional, TYPE_CHECKING

from bashkit import BashTool as NativeBashTool

if TYPE_CHECKING:
    pass

# Check for deepagents availability
try:
    from deepagents.backends.protocol import (
        SandboxBackendProtocol,
        ExecuteResponse,
        FileInfo,
        GrepMatch,
        EditResult,
        WriteResult,
        FileDownloadResponse,
        FileUploadResponse,
    )
    from langchain.agents.middleware.types import AgentMiddleware
    from langchain_core.tools import tool as langchain_tool

    DEEPAGENTS_AVAILABLE = True
except ImportError:
    DEEPAGENTS_AVAILABLE = False
    SandboxBackendProtocol = object
    AgentMiddleware = object


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _make_bash_tool(bash_instance: NativeBashTool):
    """Create a bash tool function from a BashTool instance."""
    @langchain_tool
    def bash(command: str) -> str:
        """Execute bash commands in a sandboxed virtual filesystem.

        Provides isolated execution with 66+ commands (cat, grep, sed, awk,
        jq, find, curl, etc.) and full bash syntax (pipes, redirects, loops).
        State persists between calls.

        Args:
            command: Bash command to execute

        Returns:
            Command output with exit code if non-zero
        """
        result = bash_instance.execute_sync(command)
        output = result.stdout
        if result.stderr:
            output += f"\n{result.stderr}"
        if result.exit_code != 0:
            output += f"\n[Exit code: {result.exit_code}]"
        return output.strip() if output else "[No output]"

    return bash


if DEEPAGENTS_AVAILABLE:

    class BashKitMiddleware(AgentMiddleware):
        """Middleware that adds `bash` tool for shell execution in VFS.

        Example standalone:
            >>> middleware = BashKitMiddleware()
            >>> agent = create_deep_agent(middleware=[middleware])

        Example with shared VFS (recommended):
            >>> backend = BashKitBackend()
            >>> middleware = backend.create_middleware()
            >>> agent = create_deep_agent(backend=backend, middleware=[middleware])
        """

        def __init__(
            self,
            bash_tool: Optional[NativeBashTool] = None,
            username: Optional[str] = None,
            hostname: Optional[str] = None,
            max_commands: Optional[int] = None,
            max_loop_iterations: Optional[int] = None,
        ):
            """Initialize middleware.

            Args:
                bash_tool: Existing BashTool to use (for shared VFS)
                username: Username for new BashTool (ignored if bash_tool provided)
                hostname: Hostname for new BashTool (ignored if bash_tool provided)
                max_commands: Max commands (ignored if bash_tool provided)
                max_loop_iterations: Max iterations (ignored if bash_tool provided)
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
            return result.stdout + (result.stderr or "")

        def reset(self) -> None:
            """Reset VFS to initial state."""
            if self._owns_bash:
                self._bash.reset()


    class BashKitBackend(SandboxBackendProtocol):
        """Backend implementing SandboxBackendProtocol with BashKit VFS.

        Provides execute, read_file, write_file, edit_file, ls, glob, grep
        all operating on the same virtual filesystem.

        Example:
            >>> backend = BashKitBackend()
            >>> agent = create_deep_agent(backend=backend)

        With middleware for additional `bash` tool:
            >>> backend = BashKitBackend()
            >>> middleware = backend.create_middleware()
            >>> agent = create_deep_agent(backend=backend, middleware=[middleware])
        """

        def __init__(
            self,
            username: Optional[str] = None,
            hostname: Optional[str] = None,
            max_commands: Optional[int] = None,
            max_loop_iterations: Optional[int] = None,
        ):
            self._bash = NativeBashTool(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
            )
            self._id = f"bashkit-{uuid.uuid4().hex[:8]}"

        @property
        def id(self) -> str:
            return self._id

        def create_middleware(self) -> BashKitMiddleware:
            """Create middleware that shares this backend's VFS.

            Returns:
                BashKitMiddleware using same BashTool instance
            """
            return BashKitMiddleware(bash_tool=self._bash)

        # === Shell Execution ===

        def execute(self, command: str) -> ExecuteResponse:
            result = self._bash.execute_sync(command)
            output = result.stdout + (result.stderr or "")
            return ExecuteResponse(output=output, exit_code=result.exit_code, truncated=False)

        async def aexecute(self, command: str) -> ExecuteResponse:
            return self.execute(command)

        # === File Operations ===

        def read(self, file_path: str, offset: int = 0, limit: int = 2000) -> str:
            result = self._bash.execute_sync(f"cat {file_path}")
            if result.exit_code != 0:
                return f"Error: {result.stderr or 'File not found'}"
            lines = result.stdout.splitlines()
            selected = lines[offset:offset + limit]
            return "\n".join(f"{i:6d}\t{line}" for i, line in enumerate(selected, start=offset + 1))

        async def aread(self, file_path: str, offset: int = 0, limit: int = 2000) -> str:
            return self.read(file_path, offset, limit)

        def write(self, file_path: str, content: str) -> WriteResult:
            cmd = f"cat > {file_path} << 'BASHKIT_EOF'\n{content}\nBASHKIT_EOF"
            result = self._bash.execute_sync(cmd)
            return WriteResult(success=result.exit_code == 0, error=result.stderr if result.exit_code != 0 else None)

        async def awrite(self, file_path: str, content: str) -> WriteResult:
            return self.write(file_path, content)

        def edit(self, file_path: str, old_string: str, new_string: str, replace_all: bool = False) -> EditResult:
            result = self._bash.execute_sync(f"cat {file_path}")
            if result.exit_code != 0:
                return EditResult(success=False, error=f"File not found: {file_path}")
            content = result.stdout
            count = content.count(old_string)
            if count == 0:
                return EditResult(success=False, error="old_string not found")
            if count > 1 and not replace_all:
                return EditResult(success=False, error=f"Found {count} times. Use replace_all=True")
            new_content = content.replace(old_string, new_string) if replace_all else content.replace(old_string, new_string, 1)
            wr = self.write(file_path, new_content)
            return EditResult(success=wr.success, error=wr.error)

        async def aedit(self, file_path: str, old_string: str, new_string: str, replace_all: bool = False) -> EditResult:
            return self.edit(file_path, old_string, new_string, replace_all)

        # === File Discovery ===

        def ls_info(self, path: str) -> list[FileInfo]:
            result = self._bash.execute_sync(f"ls -la {path}")
            if result.exit_code != 0:
                return []
            files = []
            for line in result.stdout.splitlines():
                parts = line.split()
                if len(parts) < 9 or parts[0].startswith("total"):
                    continue
                name = " ".join(parts[8:])
                if name in (".", ".."):
                    continue
                files.append(FileInfo(
                    path=f"{path.rstrip('/')}/{name}", name=name,
                    is_dir=parts[0].startswith("d"),
                    size=int(parts[4]) if parts[4].isdigit() else 0,
                    created_at=_now_iso(), modified_at=_now_iso(),
                ))
            return files

        async def als_info(self, path: str) -> list[FileInfo]:
            return self.ls_info(path)

        def glob_info(self, pattern: str, path: str = "/") -> list[FileInfo]:
            name_pattern = pattern.replace("**/", "").replace("**", "*") if "**" in pattern else pattern
            result = self._bash.execute_sync(f"find {path} -name '{name_pattern}' -type f")
            if result.exit_code != 0:
                return []
            return [
                FileInfo(path=p.strip(), name=p.strip().split("/")[-1], is_dir=False, size=0, created_at=_now_iso(), modified_at=_now_iso())
                for p in result.stdout.splitlines() if p.strip()
            ]

        async def aglob_info(self, pattern: str, path: str = "/") -> list[FileInfo]:
            return self.glob_info(pattern, path)

        def grep_raw(self, pattern: str, path: str | None = None, glob: str | None = None) -> list[GrepMatch] | str:
            cmd = f"grep -rn '{pattern}' {path}" if path else f"grep -rn '{pattern}' /home"
            result = self._bash.execute_sync(cmd)
            matches = []
            for line in result.stdout.splitlines():
                if ":" not in line:
                    continue
                parts = line.split(":", 2)
                if len(parts) >= 3:
                    try:
                        matches.append(GrepMatch(path=parts[0], line_number=int(parts[1]), content=parts[2]))
                    except ValueError:
                        continue
            return matches

        async def agrep_raw(self, pattern: str, path: str | None = None, glob: str | None = None) -> list[GrepMatch] | str:
            return self.grep_raw(pattern, path, glob)

        # === File Transfer ===

        def download_files(self, paths: list[str]) -> list[FileDownloadResponse]:
            responses = []
            for p in paths:
                result = self._bash.execute_sync(f"cat {p}")
                if result.exit_code == 0:
                    responses.append(FileDownloadResponse(path=p, content=result.stdout.encode(), error=None))
                else:
                    responses.append(FileDownloadResponse(path=p, content=None, error=result.stderr or "File not found"))
            return responses

        async def adownload_files(self, paths: list[str]) -> list[FileDownloadResponse]:
            return self.download_files(paths)

        def upload_files(self, files: list[tuple[str, bytes]]) -> list[FileUploadResponse]:
            responses = []
            for p, content in files:
                try:
                    wr = self.write(p, content.decode("utf-8"))
                    responses.append(FileUploadResponse(path=p, error=None if wr.success else wr.error))
                except UnicodeDecodeError:
                    responses.append(FileUploadResponse(path=p, error="Binary files not supported"))
            return responses

        async def aupload_files(self, files: list[tuple[str, bytes]]) -> list[FileUploadResponse]:
            return self.upload_files(files)

        # === Utility ===

        def setup(self, script: str) -> str:
            """Execute setup script."""
            result = self._bash.execute_sync(script)
            return result.stdout + (result.stderr or "")

        def reset(self) -> None:
            """Reset VFS."""
            self._bash.reset()


def create_bash_middleware(**kwargs) -> "BashKitMiddleware":
    """Create BashKitMiddleware for Deep Agents."""
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError("deepagents required. Install: pip install 'bashkit[deepagents]'")
    return BashKitMiddleware(**kwargs)


def create_bashkit_backend(**kwargs) -> "BashKitBackend":
    """Create BashKitBackend for Deep Agents."""
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError("deepagents required. Install: pip install 'bashkit[deepagents]'")
    return BashKitBackend(**kwargs)


__all__ = [
    "BashKitMiddleware",
    "BashKitBackend",
    "create_bash_middleware",
    "create_bashkit_backend",
]
