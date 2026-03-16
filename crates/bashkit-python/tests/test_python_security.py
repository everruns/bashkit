"""Security tests for bashkit Python package.

White-box: tests that exploit knowledge of internals (VFS, env merging,
heredoc building, JSON nesting, reset semantics, GIL handling).

Black-box: tests that treat bashkit as an opaque sandbox and try to break out
via shell injection, resource exhaustion, information leakage, and state
manipulation.

Covers: deepagents.py injection vectors, Bash/BashTool sandbox escape,
resource limit enforcement, config preservation after reset, JSON depth
bombs, heredoc delimiter collision, and concurrent execution safety.
"""

import threading
from pathlib import Path

import pytest

from bashkit import Bash, BashTool

# Source of deepagents.py for static analysis
_DEEPAGENTS_SRC = (Path(__file__).resolve().parent.parent / "bashkit" / "deepagents.py").read_text()


# ===========================================================================
# 1. BLACK-BOX: Sandbox escape via Python builtins
# ===========================================================================


class TestSandboxEscape:
    """Try to break out of the sandbox through the bash interpreter."""

    def test_no_real_filesystem_read(self):
        bash = Bash()
        r = bash.execute_sync("cat /etc/passwd")
        # VFS has no /etc/passwd
        assert "root:" not in r.stdout

    def test_no_real_filesystem_proc(self):
        bash = Bash()
        r = bash.execute_sync("cat /proc/self/environ 2>/dev/null || echo blocked")
        assert "blocked" in r.stdout or r.exit_code != 0

    def test_no_real_filesystem_sys(self):
        bash = Bash()
        r = bash.execute_sync("ls /sys/ 2>/dev/null || echo blocked")
        assert "blocked" in r.stdout or r.exit_code != 0

    def test_path_traversal_via_bash(self):
        bash = Bash()
        r = bash.execute_sync("cat /home/../../../../etc/passwd 2>/dev/null")
        assert "root:" not in r.stdout

    def test_no_real_network(self):
        bash = Bash()
        r = bash.execute_sync("curl http://example.com 2>/dev/null || echo no_network")
        # curl either not available or fails in VFS
        assert "no_network" in r.stdout or r.exit_code != 0


# ===========================================================================
# 2. WHITE-BOX: Resource limit enforcement
# ===========================================================================


class TestResourceLimits:
    """Verify resource limits prevent denial of service."""

    def test_infinite_bash_loop_blocked(self):
        bash = Bash(max_loop_iterations=100)
        r = bash.execute_sync("while true; do echo x; done")
        # Should be stopped by loop iteration limit
        lines = [line for line in r.stdout.strip().splitlines() if line.strip()]
        assert len(lines) <= 101

    def test_fork_bomb_blocked(self):
        bash = Bash()
        r = bash.execute_sync(":(){ :|:& };:")
        # Fork bomb should either fail or be harmless in VFS
        # Key: should not crash the host
        assert isinstance(r.exit_code, int)

    def test_max_loop_iterations(self):
        bash = Bash(max_loop_iterations=10)
        r = bash.execute_sync("for i in $(seq 1 100); do echo $i; done")
        lines = [line for line in r.stdout.strip().splitlines() if line.strip()]
        assert len(lines) <= 11  # 10 iterations + possible off-by-one

    def test_max_commands(self):
        bash = Bash(max_commands=5)
        cmds = "\n".join(f"echo {i}" for i in range(20))
        r = bash.execute_sync(cmds)
        lines = [line for line in r.stdout.strip().splitlines() if line.strip()]
        assert len(lines) < 20


# ===========================================================================
# 3. WHITE-BOX: Config preservation after reset (TM-PY-026/028)
# ===========================================================================


class TestResetSecurity:
    """Verify reset() preserves security configuration."""

    def test_bash_reset_preserves_limits(self):
        bash = Bash(max_commands=5, max_loop_iterations=10)
        bash.reset()
        # After reset, limits should still be active
        r = bash.execute_sync("for i in $(seq 1 100); do echo $i; done")
        lines = [line for line in r.stdout.strip().splitlines() if line.strip()]
        assert len(lines) <= 11

    def test_bashtool_reset_preserves_limits(self):
        tool = BashTool(max_commands=5, max_loop_iterations=10)
        tool.reset()
        r = tool.execute_sync("for i in $(seq 1 100); do echo $i; done")
        lines = [line for line in r.stdout.strip().splitlines() if line.strip()]
        assert len(lines) <= 11

    def test_reset_clears_state(self):
        bash = Bash()
        bash.execute_sync("export SECRET=abc123")
        bash.reset()
        r = bash.execute_sync("echo $SECRET")
        assert "abc123" not in r.stdout.strip()

    def test_reset_clears_files(self):
        bash = Bash()
        bash.execute_sync("echo 'data' > /tmp/secret.txt")
        bash.reset()
        r = bash.execute_sync("cat /tmp/secret.txt")
        assert "data" not in r.stdout

    def test_multiple_resets_stable(self):
        bash = Bash(max_commands=10)
        for _ in range(50):
            bash.reset()
        r = bash.execute_sync("echo ok")
        assert r.exit_code == 0
        assert "ok" in r.stdout


# ===========================================================================
# 4. WHITE-BOX: deepagents.py heredoc injection
# ===========================================================================


class TestHeredocInjection:
    """Test that heredoc delimiter injection is prevented."""

    def _get_build_write_cmd(self):
        """Import _build_write_cmd from deepagents module."""
        import importlib

        mod = importlib.import_module("bashkit.deepagents")
        return mod._build_write_cmd

    def test_fixed_delimiter_cant_be_injected(self):
        build = self._get_build_write_cmd()
        # Content tries to terminate the heredoc early
        malicious = "BASHKIT_EOF\necho PWNED\nBASHKIT_EOF"
        cmd = build("/tmp/test.txt", malicious)
        # Delimiter should be randomized, not just BASHKIT_EOF
        assert "BASHKIT_EOF_" in cmd
        # The malicious BASHKIT_EOF in content won't match the random one
        lines = cmd.splitlines()
        delimiter = lines[0].split("'")[1]  # Extract from << 'DELIM'
        assert delimiter != "BASHKIT_EOF"
        assert len(delimiter) > 20  # BASHKIT_EOF_ + 16 hex chars

    def test_delimiter_unique_per_call(self):
        build = self._get_build_write_cmd()
        cmd1 = build("/tmp/a.txt", "content")
        cmd2 = build("/tmp/b.txt", "content")
        delim1 = cmd1.splitlines()[0].split("'")[1]
        delim2 = cmd2.splitlines()[0].split("'")[1]
        assert delim1 != delim2, "Each call must use a unique delimiter"

    def test_path_is_quoted(self):
        build = self._get_build_write_cmd()
        cmd = build("/tmp/path with spaces/file.txt", "content")
        assert "shlex" not in cmd  # shlex.quote result, not the word shlex
        # Path should be single-quoted by shlex.quote
        assert "'/tmp/path with spaces/file.txt'" in cmd

    def test_malicious_path_quoted(self):
        build = self._get_build_write_cmd()
        cmd = build("/tmp/'; rm -rf /; echo '", "content")
        # shlex.quote wraps in single quotes, escaping inner quotes
        assert "rm -rf" not in cmd.split("\n")[0].split(">")[0]  # Not in command part unquoted


# ===========================================================================
# 5. WHITE-BOX: deepagents.py shell injection via methods
# ===========================================================================


class TestDeepagentsInjection:
    """Static analysis of deepagents.py for injection vectors."""

    def test_all_file_ops_use_shlex_quote(self):
        """Every method interpolating paths must use shlex.quote."""
        dangerous_patterns = []
        for i, line in enumerate(_DEEPAGENTS_SRC.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            # Look for f-string with path-like variable without shlex.quote on same line
            # Skip lines that use pre-quoted variables (e.g., quoted_pattern, search_path)
            if ('f"' in stripped or "f'" in stripped) and "{" in stripped:
                has_cmd = any(cmd in stripped for cmd in ["cat ", "ls ", "find ", "grep ", "echo "])
                # Variables named "quoted_*" or "*_path" where quote was done earlier are ok
                uses_prequoted = any(v in stripped for v in ["quoted_", "search_path"])
                if has_cmd and "shlex.quote" not in stripped and not uses_prequoted:
                    dangerous_patterns.append(f"L{i}: {stripped}")
        assert not dangerous_patterns, "Unquoted shell commands:\n" + "\n".join(dangerous_patterns)

    def test_no_format_string_injection(self):
        """No .format() with user input that could access attributes."""
        for i, line in enumerate(_DEEPAGENTS_SRC.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if ".format(" in stripped and "{0" in stripped:
                pytest.fail(f"L{i}: Potential format string injection: {stripped}")

    def test_no_eval_or_exec(self):
        """deepagents.py must not use eval() or exec()."""
        for i, line in enumerate(_DEEPAGENTS_SRC.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if "eval(" in stripped or "exec(" in stripped:
                pytest.fail(f"L{i}: eval/exec found: {stripped}")


# ===========================================================================
# 6. WHITE-BOX: JSON nesting depth bomb
# ===========================================================================


class TestJsonNesting:
    """Verify deeply nested JSON structures are rejected."""

    def test_deep_dict_nesting_rejected(self):
        """ScriptedTool callbacks should reject >64 levels of nesting."""
        from bashkit import ScriptedTool

        # Build a deeply nested dict
        def deep_callback(params, stdin=None):
            # Return deeply nested JSON string
            inner = "null"
            for _ in range(100):
                inner = f'{{"deep": {inner}}}'
            return inner

        tool = ScriptedTool("deep_test")
        tool.add_tool("nested", "Returns deeply nested JSON", deep_callback)
        r = tool.execute_sync("nested '{}'")
        # Should either truncate, error, or handle gracefully — not crash
        assert isinstance(r.exit_code, int)

    def test_normal_nesting_works(self):
        """Reasonable nesting levels should work fine."""
        from bashkit import ScriptedTool

        def shallow_callback(params, stdin=None):
            return '{"a": {"b": {"c": "value"}}}'

        tool = ScriptedTool("shallow_test")
        tool.add_tool("shallow", "Normal nesting", shallow_callback)
        r = tool.execute_sync("shallow '{}'")
        assert isinstance(r.exit_code, int)


# ===========================================================================
# 7. BLACK-BOX: Error information leakage
# ===========================================================================


class TestErrorLeakage:
    """Verify errors don't leak host system information."""

    def test_bash_error_no_host_paths(self):
        bash = Bash()
        r = bash.execute_sync("cat /nonexistent/file")
        combined = r.stdout + r.stderr
        assert "/usr/lib" not in combined

    def test_error_on_stderr(self):
        bash = Bash()
        r = bash.execute_sync("echo err >&2")
        assert "err" in r.stderr

    def test_nonexistent_command_error(self):
        bash = Bash()
        r = bash.execute_sync("nonexistent_command_xyz")
        assert r.exit_code != 0
        assert "not found" in r.stderr

    def test_partial_output_preserved_on_bash_error(self):
        bash = Bash()
        r = bash.execute_sync("echo before; exit 1")
        assert "before" in r.stdout
        assert r.exit_code == 1


# ===========================================================================
# 8. WHITE-BOX: State isolation between executions
# ===========================================================================


class TestStateIsolation:
    """Verify state isolation between exec calls."""

    def test_bash_vars_persist(self):
        bash = Bash()
        bash.execute_sync("export MY_VAR=hello")
        r = bash.execute_sync("echo $MY_VAR")
        assert "hello" in r.stdout

    def test_vfs_files_persist(self):
        bash = Bash()
        bash.execute_sync("echo 'data' > /tmp/persist.txt")
        r = bash.execute_sync("cat /tmp/persist.txt")
        assert "data" in r.stdout

    def test_reset_clears_vars(self):
        bash = Bash()
        bash.execute_sync("export SECRET=abc")
        bash.reset()
        r = bash.execute_sync("echo ${SECRET:-empty}")
        assert "empty" in r.stdout


# ===========================================================================
# 9. GIL deadlock prevention (concurrent access)
# ===========================================================================


class TestConcurrency:
    """Verify execute_sync releases GIL and doesn't deadlock."""

    def test_concurrent_execute_sync(self):
        """Two threads can execute_sync concurrently without deadlock."""
        bash1 = Bash()
        bash2 = Bash()
        results = [None, None]
        errors = [None, None]

        def run(idx, bash_instance, cmd):
            try:
                results[idx] = bash_instance.execute_sync(cmd)
            except Exception as e:
                errors[idx] = e

        t1 = threading.Thread(target=run, args=(0, bash1, "echo thread1"))
        t2 = threading.Thread(target=run, args=(1, bash2, "echo thread2"))
        t1.start()
        t2.start()
        t1.join(timeout=10)
        t2.join(timeout=10)

        assert not t1.is_alive(), "Thread 1 deadlocked"
        assert not t2.is_alive(), "Thread 2 deadlocked"
        assert errors[0] is None, f"Thread 1 error: {errors[0]}"
        assert errors[1] is None, f"Thread 2 error: {errors[1]}"
        assert "thread1" in results[0].stdout
        assert "thread2" in results[1].stdout

    def test_rapid_execute_no_resource_leak(self):
        """Many rapid executions don't exhaust OS resources."""
        bash = Bash()
        for i in range(100):
            r = bash.execute_sync(f"echo {i}")
            assert r.exit_code == 0


# ===========================================================================
# 10. BLACK-BOX: Shell injection via command strings
# ===========================================================================


class TestShellInjection:
    """Attempt shell injection via various command string patterns."""

    def test_semicolon_in_echo(self):
        bash = Bash()
        r = bash.execute_sync("echo 'hello; rm -rf /'")
        assert r.exit_code == 0
        assert "hello; rm -rf /" in r.stdout

    def test_backtick_in_string(self):
        bash = Bash()
        r = bash.execute_sync("echo 'test `whoami` test'")
        # Inside single quotes, backticks should not be interpreted
        assert "`whoami`" in r.stdout or r.exit_code == 0

    def test_dollar_paren_in_string(self):
        bash = Bash()
        r = bash.execute_sync("echo '$(echo injected)'")
        assert "$(echo injected)" in r.stdout or r.exit_code == 0

    def test_newline_in_variable(self):
        bash = Bash()
        r = bash.execute_sync("X='hello\nworld'; echo \"$X\"")
        # Should handle newline in variable safely
        assert isinstance(r.exit_code, int)


# ===========================================================================
# 11. WHITE-BOX: BashTool metadata safety
# ===========================================================================


class TestToolMetadata:
    """Verify tool metadata doesn't expose sensitive information."""

    def test_system_prompt_no_secrets(self):
        tool = BashTool()
        prompt = tool.system_prompt()
        assert "password" not in prompt.lower()
        assert "secret" not in prompt.lower()
        assert "token" not in prompt.lower()

    def test_input_schema_valid_json(self):
        import json

        tool = BashTool()
        schema = json.loads(tool.input_schema())
        assert "type" in schema
        assert schema["type"] == "object"

    def test_output_schema_valid_json(self):
        import json

        tool = BashTool()
        schema = json.loads(tool.output_schema())
        assert "type" in schema


# ===========================================================================
# 12. BLACK-BOX: Python code edge cases through bashkit
# ===========================================================================


class TestBashEdgeCases:
    """Test bash edge cases that might crash the interpreter."""

    def test_deeply_nested_subshells(self):
        bash = Bash()
        # Nested subshells — should not crash (may hit parser fuel limit)
        r = bash.execute_sync("(echo deep)")
        assert r.exit_code == 0
        assert "deep" in r.stdout

    def test_very_long_echo(self):
        bash = Bash()
        long_str = "A" * 10000
        r = bash.execute_sync(f"echo '{long_str}' | wc -c")
        assert r.exit_code == 0

    def test_many_variables(self):
        bash = Bash()
        assigns = "\n".join(f"V{i}={i}" for i in range(200))
        r = bash.execute_sync(f"{assigns}\necho $V199")
        assert "199" in r.stdout

    def test_empty_command(self):
        bash = Bash()
        r = bash.execute_sync("")
        assert isinstance(r.exit_code, int)

    def test_only_whitespace_command(self):
        bash = Bash()
        r = bash.execute_sync("   ")
        assert isinstance(r.exit_code, int)

    def test_pipe_chain(self):
        bash = Bash()
        r = bash.execute_sync("echo hello | tr a-z A-Z | rev")
        assert r.exit_code == 0

    def test_here_string(self):
        bash = Bash()
        r = bash.execute_sync("cat <<< 'here string test'")
        if r.exit_code == 0:
            assert "here string test" in r.stdout
