"""
Deep Agents integration for BashKit.

Provides both middleware and backend for Deep Agents:
- BashKitMiddleware: Adds `bash` tool for shell execution in VFS
- BashKitBackend: Full SandboxBackendProtocol implementation

Example with middleware:
    >>> from bashkit.deepagents import BashKitMiddleware
    >>> from deepagents import create_deep_agent
    >>>
    >>> middleware = BashKitMiddleware()
    >>> agent = create_deep_agent(middleware=[middleware])

Example with backend:
    >>> from bashkit.deepagents import BashKitBackend
    >>> from deepagents import create_deep_agent
    >>>
    >>> backend = BashKitBackend()
    >>> agent = create_deep_agent(backend=backend)
"""

from __future__ import annotations

import uuid
from datetime import datetime, timezone
from typing import Optional

from bashkit import BashTool as NativeBashTool

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
    """Return current time in ISO format."""
    return datetime.now(timezone.utc).isoformat()


if DEEPAGENTS_AVAILABLE:

    class BashKitMiddleware(AgentMiddleware):
        """Deep Agents middleware providing sandboxed bash execution.

        Adds a `bash` tool that executes commands in BashKit's virtual filesystem.
        The VFS is isolated - no real filesystem access occurs.

        Use this middleware alongside Deep Agents' built-in FilesystemMiddleware
        to provide both file operations and shell execution in the same VFS.

        Example:
            >>> from bashkit.deepagents import BashKitMiddleware
            >>> from deepagents import create_deep_agent
            >>>
            >>> middleware = BashKitMiddleware()
            >>> agent = create_deep_agent(
            ...     model="anthropic:claude-sonnet-4-20250514",
            ...     middleware=[middleware]
            ... )

        Attributes:
            tools: List containing the `bash` tool
        """

        def __init__(
            self,
            username: Optional[str] = None,
            hostname: Optional[str] = None,
            max_commands: Optional[int] = None,
            max_loop_iterations: Optional[int] = None,
        ):
            """Initialize BashKitMiddleware.

            Args:
                username: Custom username for sandbox (default: "user")
                hostname: Custom hostname for sandbox (default: "sandbox")
                max_commands: Maximum commands per session
                max_loop_iterations: Maximum loop iterations
            """
            self._bash = NativeBashTool(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
            )

            # Create the bash tool
            bash_instance = self._bash

            @langchain_tool
            def bash(command: str) -> str:
                """Execute bash commands in a sandboxed virtual filesystem.

                Provides a safe execution environment with:
                - Virtual filesystem (no real filesystem access)
                - 66+ commands: cat, grep, sed, awk, jq, find, curl, etc.
                - Full bash syntax: variables, pipes, redirects, loops, functions
                - Persistent state: files and variables retained between calls

                Args:
                    command: Bash command to execute (like `bash -c 'command'`)

                Returns:
                    Command output with exit code if non-zero

                Examples:
                    bash("echo 'hello' > /tmp/test.txt")
                    bash("cat /tmp/test.txt | grep hello")
                    bash("for i in 1 2 3; do echo $i; done")
                """
                result = bash_instance.execute_sync(command)

                output = result.stdout
                if result.stderr:
                    output += f"\n{result.stderr}"
                if result.exit_code != 0:
                    output += f"\n[Exit code: {result.exit_code}]"

                return output.strip() if output else "[No output]"

            self._tools = [bash]

        @property
        def tools(self):
            """Tools provided by this middleware."""
            return self._tools

        def execute_sync(self, command: str) -> str:
            """Execute a command synchronously (for setup).

            Args:
                command: Bash command to execute

            Returns:
                Command output
            """
            result = self._bash.execute_sync(command)
            output = result.stdout
            if result.stderr:
                output += result.stderr
            return output

        def reset(self) -> None:
            """Reset VFS to initial state."""
            self._bash.reset()


    class BashKitBackend(SandboxBackendProtocol):
        """Deep Agents backend using BashKit's virtual filesystem.

        Implements SandboxBackendProtocol for full integration with
        Deep Agents' built-in tools (read_file, write_file, execute, etc.).
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

        def execute(self, command: str) -> ExecuteResponse:
            result = self._bash.execute_sync(command)
            output = result.stdout
            if result.stderr:
                output += result.stderr
            return ExecuteResponse(
                output=output,
                exit_code=result.exit_code,
                truncated=False,
            )

        async def aexecute(self, command: str) -> ExecuteResponse:
            return self.execute(command)

        def read(self, file_path: str, offset: int = 0, limit: int = 2000) -> str:
            result = self._bash.execute_sync(f"cat {file_path}")
            if result.exit_code != 0:
                return f"Error: {result.stderr or 'File not found'}"
            lines = result.stdout.splitlines()
            selected = lines[offset : offset + limit]
            numbered = [f"{i:6d}\t{line}" for i, line in enumerate(selected, start=offset + 1)]
            return "\n".join(numbered)

        async def aread(self, file_path: str, offset: int = 0, limit: int = 2000) -> str:
            return self.read(file_path, offset, limit)

        def write(self, file_path: str, content: str) -> WriteResult:
            cmd = f"cat > {file_path} << 'BASHKIT_EOF'\n{content}\nBASHKIT_EOF"
            result = self._bash.execute_sync(cmd)
            if result.exit_code != 0:
                return WriteResult(success=False, error=result.stderr)
            return WriteResult(success=True, error=None)

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
            write_result = self.write(file_path, new_content)
            return EditResult(success=write_result.success, error=write_result.error)

        async def aedit(self, file_path: str, old_string: str, new_string: str, replace_all: bool = False) -> EditResult:
            return self.edit(file_path, old_string, new_string, replace_all)

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
                    path=f"{path.rstrip('/')}/{name}",
                    name=name,
                    is_dir=parts[0].startswith("d"),
                    size=int(parts[4]) if parts[4].isdigit() else 0,
                    created_at=_now_iso(),
                    modified_at=_now_iso(),
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

        def download_files(self, paths: list[str]) -> list[FileDownloadResponse]:
            responses = []
            for path in paths:
                result = self._bash.execute_sync(f"cat {path}")
                if result.exit_code == 0:
                    responses.append(FileDownloadResponse(path=path, content=result.stdout.encode(), error=None))
                else:
                    responses.append(FileDownloadResponse(path=path, content=None, error=result.stderr or "File not found"))
            return responses

        async def adownload_files(self, paths: list[str]) -> list[FileDownloadResponse]:
            return self.download_files(paths)

        def upload_files(self, files: list[tuple[str, bytes]]) -> list[FileUploadResponse]:
            responses = []
            for path, content in files:
                try:
                    write_result = self.write(path, content.decode("utf-8"))
                    responses.append(FileUploadResponse(path=path, error=None if write_result.success else write_result.error))
                except UnicodeDecodeError:
                    responses.append(FileUploadResponse(path=path, error="Binary files not supported"))
            return responses

        async def aupload_files(self, files: list[tuple[str, bytes]]) -> list[FileUploadResponse]:
            return self.upload_files(files)

        def setup(self, script: str) -> str:
            """Execute setup script."""
            result = self._bash.execute_sync(script)
            return result.stdout + (result.stderr or "")

        def reset(self) -> None:
            """Reset VFS."""
            self._bash.reset()


def create_bash_middleware(**kwargs) -> "BashKitMiddleware":
    """Create BashKitMiddleware for Deep Agents.

    Returns middleware with `bash` tool for shell execution in VFS.

    Raises:
        ImportError: If deepagents not installed
    """
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError("deepagents required. Install: pip install 'bashkit[deepagents]'")
    return BashKitMiddleware(**kwargs)


def create_bashkit_backend(**kwargs) -> "BashKitBackend":
    """Create BashKitBackend for Deep Agents.

    Returns backend implementing SandboxBackendProtocol.

    Raises:
        ImportError: If deepagents not installed
    """
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError("deepagents required. Install: pip install 'bashkit[deepagents]'")
    return BashKitBackend(**kwargs)


__all__ = [
    "BashKitMiddleware",
    "BashKitBackend",
    "create_bash_middleware",
    "create_bashkit_backend",
]
