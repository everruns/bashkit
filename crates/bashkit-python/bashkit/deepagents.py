"""
Deep Agents backend for BashKit.

Provides a sandboxed backend that implements SandboxBackendProtocol
using BashKit's virtual filesystem for all file and shell operations.

Example:
    >>> from bashkit.deepagents import BashKitBackend, create_bashkit_backend
    >>> from deepagents import create_deep_agent
    >>>
    >>> backend = create_bashkit_backend()
    >>> agent = create_deep_agent(
    ...     model="anthropic:claude-sonnet-4-20250514",
    ...     backend=backend
    ... )
"""

from __future__ import annotations

import uuid
from datetime import datetime, timezone
from typing import Optional

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

    DEEPAGENTS_AVAILABLE = True
except ImportError:
    DEEPAGENTS_AVAILABLE = False
    SandboxBackendProtocol = object


from bashkit import BashTool as NativeBashTool


def _now_iso() -> str:
    """Return current time in ISO format."""
    return datetime.now(timezone.utc).isoformat()


if DEEPAGENTS_AVAILABLE:

    class BashKitBackend(SandboxBackendProtocol):
        """Deep Agents backend using BashKit's virtual filesystem.

        Implements SandboxBackendProtocol to provide:
        - Sandboxed shell execution via BashKit interpreter
        - Virtual filesystem for all file operations (read, write, edit, ls, glob, grep)
        - Complete isolation - no real filesystem access

        The backend maintains state between operations, so files created
        in shell commands are available via file operations and vice versa.

        Example:
            >>> from bashkit.deepagents import BashKitBackend
            >>> from deepagents import create_deep_agent
            >>>
            >>> backend = BashKitBackend()
            >>> agent = create_deep_agent(
            ...     model="anthropic:claude-sonnet-4-20250514",
            ...     backend=backend
            ... )

        Attributes:
            id: Unique identifier for this backend instance
        """

        def __init__(
            self,
            username: Optional[str] = None,
            hostname: Optional[str] = None,
            max_commands: Optional[int] = None,
            max_loop_iterations: Optional[int] = None,
        ):
            """Initialize BashKitBackend.

            Args:
                username: Custom username for sandbox (default: "user")
                hostname: Custom hostname for sandbox (default: "sandbox")
                max_commands: Maximum commands to execute per session
                max_loop_iterations: Maximum loop iterations allowed
            """
            self._bash = NativeBashTool(
                username=username,
                hostname=hostname,
                max_commands=max_commands,
                max_loop_iterations=max_loop_iterations,
            )
            self._id = f"bashkit-{uuid.uuid4().hex[:8]}"

        @property
        def id(self) -> str:
            """Unique identifier for this backend instance."""
            return self._id

        # ==================== Shell Execution ====================

        def execute(self, command: str) -> ExecuteResponse:
            """Execute a command in the BashKit sandbox.

            Args:
                command: Full shell command string to execute.

            Returns:
                ExecuteResponse with output, exit code, and truncation flag.
            """
            result = self._bash.execute_sync(command)

            output = result.stdout
            if result.stderr:
                output += result.stderr

            return ExecuteResponse(
                output=output,
                exit_code=result.exit_code,
                signal=None,
                truncated=False,
            )

        async def aexecute(self, command: str) -> ExecuteResponse:
            """Async version of execute."""
            return self.execute(command)

        # ==================== File Operations ====================

        def read(self, file_path: str, offset: int = 0, limit: int = 2000) -> str:
            """Read file content from the virtual filesystem.

            Args:
                file_path: Absolute path to the file.
                offset: Line number to start from (0-indexed).
                limit: Maximum lines to return.

            Returns:
                File content as string with line numbers.
            """
            result = self._bash.execute_sync(f"cat {file_path}")
            if result.exit_code != 0:
                return f"Error: {result.stderr or 'File not found'}"

            lines = result.stdout.splitlines()
            selected = lines[offset : offset + limit]

            # Format with line numbers
            numbered = []
            for i, line in enumerate(selected, start=offset + 1):
                numbered.append(f"{i:6d}\t{line}")

            return "\n".join(numbered)

        async def aread(self, file_path: str, offset: int = 0, limit: int = 2000) -> str:
            """Async version of read."""
            return self.read(file_path, offset, limit)

        def write(self, file_path: str, content: str) -> WriteResult:
            """Write content to a file in the virtual filesystem.

            Args:
                file_path: Absolute path to the file.
                content: Content to write.

            Returns:
                WriteResult indicating success/failure.
            """
            # Escape content for heredoc
            escaped = content.replace("'", "'\\''")
            cmd = f"cat > {file_path} << 'BASHKIT_EOF'\n{content}\nBASHKIT_EOF"
            result = self._bash.execute_sync(cmd)

            if result.exit_code != 0:
                return WriteResult(success=False, error=result.stderr)
            return WriteResult(success=True, error=None)

        async def awrite(self, file_path: str, content: str) -> WriteResult:
            """Async version of write."""
            return self.write(file_path, content)

        def edit(
            self, file_path: str, old_string: str, new_string: str, replace_all: bool = False
        ) -> EditResult:
            """Edit a file by replacing exact strings.

            Args:
                file_path: Path to the file.
                old_string: String to find and replace.
                new_string: Replacement string.
                replace_all: If True, replace all occurrences.

            Returns:
                EditResult indicating success/failure.
            """
            # Read current content
            result = self._bash.execute_sync(f"cat {file_path}")
            if result.exit_code != 0:
                return EditResult(success=False, error=f"File not found: {file_path}")

            content = result.stdout

            # Check occurrences
            count = content.count(old_string)
            if count == 0:
                return EditResult(success=False, error="old_string not found in file")
            if count > 1 and not replace_all:
                return EditResult(
                    success=False,
                    error=f"old_string found {count} times. Use replace_all=True or provide more context.",
                )

            # Perform replacement
            if replace_all:
                new_content = content.replace(old_string, new_string)
            else:
                new_content = content.replace(old_string, new_string, 1)

            # Write back
            write_result = self.write(file_path, new_content)
            if not write_result.success:
                return EditResult(success=False, error=write_result.error)

            return EditResult(success=True, error=None)

        async def aedit(
            self, file_path: str, old_string: str, new_string: str, replace_all: bool = False
        ) -> EditResult:
            """Async version of edit."""
            return self.edit(file_path, old_string, new_string, replace_all)

        def ls_info(self, path: str) -> list[FileInfo]:
            """List directory contents.

            Args:
                path: Directory path to list.

            Returns:
                List of FileInfo objects.
            """
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

                is_dir = parts[0].startswith("d")
                size = int(parts[4]) if parts[4].isdigit() else 0

                full_path = f"{path.rstrip('/')}/{name}"
                files.append(
                    FileInfo(
                        path=full_path,
                        name=name,
                        is_dir=is_dir,
                        size=size,
                        created_at=_now_iso(),
                        modified_at=_now_iso(),
                    )
                )

            return files

        async def als_info(self, path: str) -> list[FileInfo]:
            """Async version of ls_info."""
            return self.ls_info(path)

        def glob_info(self, pattern: str, path: str = "/") -> list[FileInfo]:
            """Find files matching a glob pattern.

            Args:
                pattern: Glob pattern (e.g., "*.py", "**/*.txt").
                path: Base path to search from.

            Returns:
                List of matching FileInfo objects.
            """
            # Use find with -name pattern
            if "**" in pattern:
                # Recursive search
                name_pattern = pattern.replace("**/", "").replace("**", "*")
                result = self._bash.execute_sync(f"find {path} -name '{name_pattern}' -type f")
            else:
                result = self._bash.execute_sync(f"find {path} -name '{pattern}' -type f")

            if result.exit_code != 0:
                return []

            files = []
            for line in result.stdout.splitlines():
                if not line.strip():
                    continue
                full_path = line.strip()
                name = full_path.split("/")[-1]
                files.append(
                    FileInfo(
                        path=full_path,
                        name=name,
                        is_dir=False,
                        size=0,
                        created_at=_now_iso(),
                        modified_at=_now_iso(),
                    )
                )

            return files

        async def aglob_info(self, pattern: str, path: str = "/") -> list[FileInfo]:
            """Async version of glob_info."""
            return self.glob_info(pattern, path)

        def grep_raw(
            self, pattern: str, path: str | None = None, glob: str | None = None
        ) -> list[GrepMatch] | str:
            """Search for pattern in files.

            Args:
                pattern: Regex pattern to search for.
                path: Specific file or directory to search.
                glob: Glob pattern to filter files.

            Returns:
                List of GrepMatch objects or error string.
            """
            if path:
                cmd = f"grep -rn '{pattern}' {path}"
            elif glob:
                cmd = f"find / -name '{glob}' -exec grep -Hn '{pattern}' {{}} \\;"
            else:
                cmd = f"grep -rn '{pattern}' /home"

            result = self._bash.execute_sync(cmd)

            matches = []
            for line in result.stdout.splitlines():
                if ":" not in line:
                    continue
                parts = line.split(":", 2)
                if len(parts) >= 3:
                    file_path, line_num, content = parts[0], parts[1], parts[2]
                    try:
                        matches.append(
                            GrepMatch(
                                path=file_path,
                                line_number=int(line_num),
                                content=content,
                            )
                        )
                    except ValueError:
                        continue

            return matches

        async def agrep_raw(
            self, pattern: str, path: str | None = None, glob: str | None = None
        ) -> list[GrepMatch] | str:
            """Async version of grep_raw."""
            return self.grep_raw(pattern, path, glob)

        def download_files(self, paths: list[str]) -> list[FileDownloadResponse]:
            """Download files from the virtual filesystem.

            Args:
                paths: List of file paths to download.

            Returns:
                List of FileDownloadResponse objects.
            """
            responses = []
            for path in paths:
                result = self._bash.execute_sync(f"cat {path}")
                if result.exit_code == 0:
                    responses.append(
                        FileDownloadResponse(
                            path=path,
                            content=result.stdout.encode(),
                            error=None,
                        )
                    )
                else:
                    responses.append(
                        FileDownloadResponse(
                            path=path,
                            content=None,
                            error=result.stderr or "File not found",
                        )
                    )
            return responses

        async def adownload_files(self, paths: list[str]) -> list[FileDownloadResponse]:
            """Async version of download_files."""
            return self.download_files(paths)

        def upload_files(self, files: list[tuple[str, bytes]]) -> list[FileUploadResponse]:
            """Upload files to the virtual filesystem.

            Args:
                files: List of (path, content) tuples.

            Returns:
                List of FileUploadResponse objects.
            """
            responses = []
            for path, content in files:
                try:
                    text = content.decode("utf-8")
                    write_result = self.write(path, text)
                    if write_result.success:
                        responses.append(FileUploadResponse(path=path, error=None))
                    else:
                        responses.append(FileUploadResponse(path=path, error=write_result.error))
                except UnicodeDecodeError:
                    responses.append(
                        FileUploadResponse(path=path, error="Binary files not supported")
                    )
            return responses

        async def aupload_files(
            self, files: list[tuple[str, bytes]]
        ) -> list[FileUploadResponse]:
            """Async version of upload_files."""
            return self.upload_files(files)

        # ==================== Utility Methods ====================

        def setup(self, script: str) -> str:
            """Execute a setup script and return output.

            Useful for initializing the virtual filesystem with
            directories, files, and configuration.

            Args:
                script: Bash script to execute.

            Returns:
                Combined stdout/stderr output.
            """
            result = self._bash.execute_sync(script)
            output = result.stdout
            if result.stderr:
                output += "\n" + result.stderr
            return output

        def reset(self) -> None:
            """Reset the virtual filesystem to initial state."""
            self._bash.reset()


def create_bashkit_backend(
    username: Optional[str] = None,
    hostname: Optional[str] = None,
    max_commands: Optional[int] = None,
    max_loop_iterations: Optional[int] = None,
) -> "BashKitBackend":
    """Create a Deep Agents backend using BashKit.

    The backend provides a fully sandboxed environment where:
    - All file operations use BashKit's virtual filesystem
    - Shell commands execute in the BashKit interpreter
    - No real filesystem access occurs

    Args:
        username: Custom username for sandbox (default: "user")
        hostname: Custom hostname for sandbox (default: "sandbox")
        max_commands: Maximum commands to execute
        max_loop_iterations: Maximum loop iterations

    Returns:
        BashKitBackend instance for use with create_deep_agent

    Raises:
        ImportError: If deepagents is not installed

    Example:
        >>> from bashkit.deepagents import create_bashkit_backend
        >>> from deepagents import create_deep_agent
        >>>
        >>> backend = create_bashkit_backend(username="dev", hostname="sandbox")
        >>> agent = create_deep_agent(backend=backend)
    """
    if not DEEPAGENTS_AVAILABLE:
        raise ImportError(
            "deepagents is required for Deep Agents integration. "
            "Install with: pip install 'bashkit[deepagents]'"
        )

    return BashKitBackend(
        username=username,
        hostname=hostname,
        max_commands=max_commands,
        max_loop_iterations=max_loop_iterations,
    )


# Keep middleware exports for backward compatibility
# but the backend is the recommended approach
try:
    from langchain.agents.middleware.types import AgentMiddleware
    from langchain_core.tools import tool as langchain_tool

    def _create_bash_tools(
        username: Optional[str] = None,
        hostname: Optional[str] = None,
        max_commands: Optional[int] = None,
        max_loop_iterations: Optional[int] = None,
    ):
        """Create bash tools with a shared BashTool instance."""
        bash_instance = NativeBashTool(
            username=username,
            hostname=hostname,
            max_commands=max_commands,
            max_loop_iterations=max_loop_iterations,
        )

        @langchain_tool
        def execute(commands: str) -> str:
            """Execute bash commands in a sandboxed virtual filesystem."""
            result = bash_instance.execute_sync(commands)
            if result.error:
                return f"Error: {result.error}"
            output = result.stdout
            if result.stderr:
                output += f"\nSTDERR: {result.stderr}"
            if result.exit_code != 0:
                output += f"\n[Exit code: {result.exit_code}]"
            return output

        @langchain_tool
        def reset_filesystem() -> str:
            """Reset the virtual filesystem to its initial state."""
            bash_instance.reset()
            return "Virtual filesystem has been reset."

        return execute, reset_filesystem, bash_instance

    class BashKitMiddleware(AgentMiddleware):
        """Middleware wrapper (deprecated - use BashKitBackend instead)."""

        def __init__(self, **kwargs):
            execute, reset, self._bash_instance = _create_bash_tools(**kwargs)
            self._tools = [execute, reset]

        @property
        def tools(self):
            return self._tools

        def execute_sync(self, commands: str) -> str:
            result = self._bash_instance.execute_sync(commands)
            return result.stdout + (result.stderr or "")

    def create_bash_middleware(**kwargs) -> BashKitMiddleware:
        """Create middleware (deprecated - use create_bashkit_backend instead)."""
        return BashKitMiddleware(**kwargs)

except ImportError:
    BashKitMiddleware = None
    create_bash_middleware = None


__all__ = [
    "BashKitBackend",
    "create_bashkit_backend",
    "BashKitMiddleware",
    "create_bash_middleware",
]
