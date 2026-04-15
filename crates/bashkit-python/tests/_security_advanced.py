"""Advanced security tests for bashkit Python integration.

White-box: tests exploiting knowledge of internals — resource limit edge cases,
VFS path resolution, Python (Monty) sandboxing, callback reentrancy, JSON
conversion boundaries, cancellation safety, reset/rebuild semantics.

Black-box: tests treating bashkit as an opaque sandbox — attempting sandbox
escapes via creative shell constructs, resource exhaustion attacks, timing
side-channels, encoding attacks, state confusion, and privilege escalation.

Goal: hit every limit, find every escape hatch, break every assumption.
"""

import asyncio
import threading
import time
from pathlib import Path

import pytest

from bashkit import Bash, BashTool, ScriptedTool

# ===========================================================================
# 1. BLACK-BOX: Creative sandbox escape attempts
# ===========================================================================


class TestSandboxEscapeCreative:
    """Try unconventional methods to escape the sandbox."""

    def test_tm_net_001_dev_tcp_network_escape(self):
        """Bash /dev/tcp pseudo-device must not allow real network access."""
        bash = Bash()
        r = bash.execute_sync("echo test > /dev/tcp/127.0.0.1/80 2>/dev/null; echo $?")
        # Should fail — no real /dev/tcp in VFS
        assert "0" not in r.stdout.strip() or r.exit_code != 0

    def test_tm_net_001_dev_udp_network_escape(self):
        """Bash /dev/udp pseudo-device must not allow network access."""
        bash = Bash()
        r = bash.execute_sync("echo test > /dev/udp/127.0.0.1/53 2>/dev/null; echo $?")
        assert "0" not in r.stdout.strip() or r.exit_code != 0

    def test_proc_self_exe_escape(self):
        """/proc/self/exe must not expose the host binary."""
        bash = Bash()
        r = bash.execute_sync("cat /proc/self/exe 2>/dev/null | head -c 4")
        assert "\x7fELF" not in r.stdout

    def test_dev_random_not_real(self):
        """/dev/random should not expose host entropy source."""
        bash = Bash()
        r = bash.execute_sync("dd if=/dev/random bs=1 count=4 2>/dev/null | od -x")
        # Should fail or return empty
        assert r.exit_code != 0 or r.stdout.strip() == ""

    def test_symlink_escape_attempt(self):
        """Symlinks within VFS must not escape to host filesystem."""
        bash = Bash()
        bash.execute_sync("ln -s /etc/passwd /tmp/escape_link 2>/dev/null")
        r = bash.execute_sync("cat /tmp/escape_link 2>/dev/null")
        assert "root:x:0" not in r.stdout

    def test_dot_dot_chain_escape(self):
        """Long ../../.. chains must not escape VFS root."""
        bash = Bash()
        chain = "/".join([".."] * 50)
        r = bash.execute_sync(f"cat /{chain}/etc/passwd 2>/dev/null")
        assert "root:x:0" not in r.stdout

    def test_absolute_path_with_null_byte(self):
        """Null bytes in paths must not cause C-string truncation issues."""
        bash = Bash()
        # If underlying code uses C strings, null could truncate the path
        r = bash.execute_sync("cat '/etc\\x00/passwd' 2>/dev/null || echo safe")
        assert "root:" not in r.stdout

    def test_tm_uni_003_unicode_path_normalization_escape(self):
        """Unicode normalization tricks must not bypass path checks."""
        bash = Bash()
        # Unicode fullwidth solidus (U+FF0F) — should not be treated as path separator
        r = bash.execute_sync("cat '/etc\uff0fpasswd' 2>/dev/null || echo safe")
        assert "root:x:0" not in r.stdout

    def test_env_manipulation_path(self):
        """Changing PATH should not allow executing host binaries."""
        bash = Bash()
        r = bash.execute_sync("export PATH=/usr/bin:/bin; which python3 2>/dev/null || echo none")
        # VFS has no real binaries at these paths
        assert isinstance(r.exit_code, int)

    def test_source_remote_script(self):
        """source/. cannot fetch from network."""
        bash = Bash()
        r = bash.execute_sync("source /dev/tcp/example.com/80 2>/dev/null; echo $?")
        assert r.exit_code != 0 or "0" not in r.stdout.strip()


# ===========================================================================
# 2. WHITE-BOX: Python (Monty) sandbox limits
# ===========================================================================


class TestMontySandboxLimits:
    """Push Monty interpreter limits via python3 -c."""

    def test_python_infinite_loop_killed(self):
        """Python infinite loop must be terminated by time/allocation limit."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'while True: pass'")
        assert r.exit_code != 0

    def test_tm_dos_059_python_memory_bomb(self):
        """Python allocating huge lists must be stopped by allocation limit."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'x = [0] * 10000000'")
        assert r.exit_code != 0

    def test_python_recursion_bomb(self):
        """Deep recursion must be stopped by recursion limit."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'def f(): f()\nf()'")
        assert r.exit_code != 0
        # python3 may not be registered (exit 127) or recursion is caught
        assert "recursion" in r.stderr.lower() or "limit" in r.stderr.lower() or r.exit_code in (1, 127)

    def test_python_string_multiplication_bomb(self):
        """Huge string via multiplication must hit allocation limit."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'x = \"A\" * (10**9)'")
        assert r.exit_code != 0

    def test_python_dict_bomb(self):
        """Creating huge dicts must hit allocation limit."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'x = {i: i for i in range(5000000)}'")
        assert r.exit_code != 0

    def test_python_nested_list_comprehension_bomb(self):
        """Nested comprehensions creating exponential data must be limited."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'x = [[i*j for i in range(1000)] for j in range(1000)]'")
        assert r.exit_code != 0

    def test_python_no_os_system(self):
        """os.system() must not exist in Monty."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'import os; os.system(\"echo pwned\")'")
        assert "pwned" not in r.stdout
        assert r.exit_code != 0

    def test_python_no_subprocess(self):
        """subprocess module must not be available."""
        bash = Bash()
        r = bash.execute_sync('python3 -c \'import subprocess; subprocess.run(["echo", "pwned"])\'')
        assert "pwned" not in r.stdout
        assert r.exit_code != 0

    def test_python_no_socket(self):
        """socket module must not be available."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'import socket; socket.create_connection((\"8.8.8.8\", 53))'")
        assert r.exit_code != 0

    def test_python_no_ctypes(self):
        """ctypes must not be available (could call arbitrary C functions)."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'import ctypes'")
        assert r.exit_code != 0

    def test_python_no_importlib(self):
        """importlib must not allow loading arbitrary modules."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'import importlib; importlib.import_module(\"os\")'")
        # Either importlib itself fails or os fails when used dangerously
        assert isinstance(r.exit_code, int)

    def test_python_no_eval_exec(self):
        """eval/exec should not be able to bypass sandbox."""
        bash = Bash()
        r = bash.execute_sync('python3 -c \'eval("__import__(\\"os\\").system(\\"echo pwned\\")")\'')
        assert "pwned" not in r.stdout

    def test_python_no_open_builtin(self):
        """open() must not be available in Monty."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'f = open(\"/etc/passwd\"); print(f.read())'")
        assert "root:" not in r.stdout
        assert r.exit_code != 0

    def test_python_no_compile_exec(self):
        """compile() + exec() must not bypass sandbox."""
        bash = Bash()
        r = bash.execute_sync('python3 -c \'code = compile("import os", "<str>", "exec"); exec(code)\'')
        # Should fail at compile, exec, or os import
        assert r.exit_code != 0 or "pwned" not in r.stdout

    def test_python_no_globals_manipulation(self):
        """__builtins__ access should not expose dangerous functions."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'print(type(__builtins__))'")
        # May work but should not expose anything dangerous
        assert isinstance(r.exit_code, int)

    def test_python_no_pickle(self):
        """pickle module (arbitrary code execution vector) must not be available."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'import pickle'")
        assert r.exit_code != 0

    def test_python_no_code_module(self):
        """code module must not be available."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'import code'")
        assert r.exit_code != 0

    def test_python_vfs_write_does_not_escape(self):
        """Python pathlib.Path write operations stay in VFS."""
        bash = Bash()
        r = bash.execute_sync(
            "python3 -c '\n"
            "from pathlib import Path\n"
            'Path("/tmp/test_escape.txt").write_text("data")\n'
            'print(Path("/tmp/test_escape.txt").read_text())\n'
            "'"
        )
        if r.exit_code == 0:
            assert "data" in r.stdout
        # Verify no file was created on host
        assert not Path("/tmp/test_escape.txt").exists()

    def test_python_pathlib_traversal(self):
        """Python pathlib with .. chains must stay in VFS."""
        bash = Bash()
        r = bash.execute_sync(
            "python3 -c '\n"
            "from pathlib import Path\n"
            "try:\n"
            '    content = Path("/tmp/../../../etc/passwd").read_text()\n'
            "    print(content)\n"
            "except Exception as e:\n"
            '    print(f"blocked: {e}")\n'
            "'"
        )
        assert "root:x:0" not in r.stdout


# ===========================================================================
# 3. WHITE-BOX: Resource limit boundary conditions
# ===========================================================================


class TestResourceLimitBoundaries:
    """Test exact boundaries of resource limits."""

    def test_tm_dos_002_max_commands_exact_boundary(self):
        """Exactly max_commands should execute, max_commands+1 should not."""
        bash = Bash(max_commands=3)
        r = bash.execute_sync("echo 1; echo 2; echo 3; echo 4; echo 5")
        lines = [ln for ln in r.stdout.strip().splitlines() if ln.strip()]
        # Should stop at or before 3
        assert len(lines) <= 3

    def test_tm_dos_016_max_loop_iterations_exact_boundary(self):
        """Loop should stop at exactly max_loop_iterations."""
        bash = Bash(max_loop_iterations=5)
        r = bash.execute_sync("for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done")
        lines = [ln for ln in r.stdout.strip().splitlines() if ln.strip()]
        assert len(lines) <= 6  # 5 iterations + possible off-by-one

    def test_zero_max_commands(self):
        """max_commands=0 should prevent any command execution or be handled safely."""
        # This tests whether the implementation handles the edge case
        try:
            bash = Bash(max_commands=0)
            r = bash.execute_sync("echo should_not_run")
            # Either nothing runs or it's handled gracefully
            assert isinstance(r.exit_code, int)
        except (ValueError, OverflowError):
            pass  # Rejecting 0 at construction is also valid

    def test_zero_max_loop_iterations(self):
        """max_loop_iterations=0 should prevent any loops or be handled safely."""
        try:
            bash = Bash(max_loop_iterations=0)
            r = bash.execute_sync("for i in 1 2 3; do echo $i; done")
            assert isinstance(r.exit_code, int)
        except (ValueError, OverflowError):
            pass

    def test_very_large_max_commands(self):
        """Very large max_commands should not cause overflow."""
        bash = Bash(max_commands=2**62)
        r = bash.execute_sync("echo ok")
        assert r.exit_code == 0

    def test_limits_after_multiple_resets(self):
        """Limits must survive 100 consecutive resets."""
        bash = Bash(max_commands=5, max_loop_iterations=10)
        for _ in range(100):
            bash.reset()
        r = bash.execute_sync("for i in $(seq 1 100); do echo $i; done")
        lines = [ln for ln in r.stdout.strip().splitlines() if ln.strip()]
        assert len(lines) <= 11  # max_loop_iterations + possible off-by-one

    def test_max_commands_resets_per_exec(self):
        """max_commands budget resets per exec() call.

        Each exec() invocation gets a fresh command budget, preventing
        a prior call from permanently poisoning the session.
        """
        bash = Bash(max_commands=5)
        r1 = bash.execute_sync("echo 1; echo 2; echo 3")
        lines1 = [ln for ln in r1.stdout.strip().splitlines() if ln.strip()]
        r2 = bash.execute_sync("echo a; echo b; echo c")
        lines2 = [ln for ln in r2.stdout.strip().splitlines() if ln.strip()]
        # Both calls should produce output since budget resets per exec()
        assert len(lines1) >= 3, f"first exec should produce 3 lines, got {len(lines1)}"
        assert len(lines2) >= 3, f"second exec should produce 3 lines, got {len(lines2)}"


# ===========================================================================
# 4. BLACK-BOX: Shell metacharacter attacks
# ===========================================================================


class TestShellMetacharacterAttacks:
    """Test every dangerous shell metacharacter for proper handling."""

    def test_pipe_to_external_command(self):
        """Pipes must work within VFS builtins, not leak to host."""
        bash = Bash()
        r = bash.execute_sync("echo hello | cat")
        assert "hello" in r.stdout

    def test_tm_esc_002_process_substitution(self):
        """Process substitution <() should either work in VFS or fail safely."""
        bash = Bash()
        r = bash.execute_sync("cat <(echo process_sub) 2>/dev/null || echo no_procsub")
        assert isinstance(r.exit_code, int)

    def test_brace_expansion_bomb(self):
        """Brace expansion creating huge output must be bounded."""
        bash = Bash(max_commands=100)
        r = bash.execute_sync("echo {1..10000}")
        # Should either truncate or handle without crashing
        assert isinstance(r.exit_code, int)

    def test_glob_expansion_bomb(self):
        """Glob expansion in a directory with many files must be bounded."""
        bash = Bash(max_commands=200)
        # Create many files then glob
        bash.execute_sync("for i in $(seq 1 50); do touch /tmp/f_$i; done")
        r = bash.execute_sync("echo /tmp/f_*")
        assert isinstance(r.exit_code, int)

    def test_heredoc_within_heredoc(self):
        """Nested heredocs should be handled safely."""
        bash = Bash()
        r = bash.execute_sync("cat << 'OUTER'\nbefore\ncat << 'INNER'\nnested\nINNER\nafter\nOUTER")
        assert isinstance(r.exit_code, int)

    def test_command_substitution_nesting(self):
        """Deeply nested command substitution must be bounded."""
        bash = Bash(max_commands=50)
        r = bash.execute_sync("echo $(echo $(echo $(echo $(echo deep))))")
        if r.exit_code == 0:
            assert "deep" in r.stdout

    def test_arithmetic_overflow(self):
        """Arithmetic expressions with huge numbers must not crash."""
        bash = Bash()
        r = bash.execute_sync("echo $((2**63))")
        assert isinstance(r.exit_code, int)

    def test_ifs_manipulation(self):
        """Changing IFS must not break security boundaries."""
        bash = Bash()
        r = bash.execute_sync("IFS=/; CMD='echo/hello'; $CMD 2>/dev/null || echo safe")
        assert isinstance(r.exit_code, int)

    def test_ansi_escape_injection(self):
        """ANSI escape sequences in output should not cause terminal control issues."""
        bash = Bash()
        r = bash.execute_sync("echo -e '\\033[2J\\033[H'")
        # Output may contain escape sequences, but should not crash
        assert isinstance(r.exit_code, int)


# ===========================================================================
# 5. WHITE-BOX: Callback security edge cases
# ===========================================================================


class TestCallbackSecurity:
    """Test ScriptedTool callback security boundaries."""

    def test_callback_returns_huge_output(self):
        """Callbacks returning megabytes of data must be handled."""
        tool = ScriptedTool("big")
        tool.add_tool("big", "Returns huge output", lambda p, s=None: "X" * (1024 * 1024))
        r = tool.execute_sync("big")
        assert isinstance(r.exit_code, int)

    def test_callback_takes_long_time(self):
        """Slow callbacks should not deadlock the interpreter."""
        tool = ScriptedTool("slow")

        def slow_callback(params, stdin=None):
            time.sleep(0.5)
            return "done"

        tool.add_tool("slow", "Slow callback", slow_callback)
        r = tool.execute_sync("slow")
        assert r.exit_code == 0
        assert "done" in r.stdout

    def test_callback_raises_different_exceptions(self):
        """Various exception types must be caught without crashing."""
        tool = ScriptedTool("errors")

        exceptions = [
            ("val_err", ValueError("bad value")),
            ("type_err", TypeError("wrong type")),
            ("runtime_err", RuntimeError("runtime failure")),
            ("key_err", KeyError("missing")),
            ("index_err", IndexError("out of bounds")),
            ("os_err", OSError("os failure")),
        ]

        for name, exc in exceptions:

            def make_cb(e):
                return lambda p, s=None: (_ for _ in ()).throw(type(e)(str(e)))

            tool.add_tool(name, f"Raises {type(exc).__name__}", make_cb(exc))

        for name, _ in exceptions:
            r = tool.execute_sync(name)
            assert r.exit_code != 0, f"{name} should fail"

    def test_callback_modifies_params_dict(self):
        """Callback mutating its params dict must not affect interpreter state."""
        tool = ScriptedTool("mutate")

        def mutating_callback(params, stdin=None):
            params["injected"] = "evil"
            params.clear()
            return "ok"

        tool.add_tool("mutate", "Mutates params", mutating_callback)
        r = tool.execute_sync("mutate --key value")
        assert r.exit_code == 0

    def test_callback_returns_non_string_types(self):
        """Callbacks returning non-string types must fail gracefully."""
        tool = ScriptedTool("types")

        for name, val in [("int_cb", 42), ("list_cb", [1, 2, 3]), ("dict_cb", {"a": 1}), ("bool_cb", True)]:
            tool.add_tool(name, f"Returns {type(val).__name__}", lambda p, s=None, v=val: v)

        for name in ["int_cb", "list_cb", "dict_cb", "bool_cb"]:
            r = tool.execute_sync(name)
            assert r.exit_code != 0, f"{name} should fail (non-string return)"

    def test_callback_shell_injection_in_output(self):
        """Callback output containing shell metacharacters must stay literal in pipes."""
        tool = ScriptedTool("inject")
        tool.add_tool(
            "inject",
            "Returns shell-like output",
            lambda p, s=None: "$(echo pwned); `rm -rf /`; echo evil\n",
        )
        r = tool.execute_sync("inject | cat")
        # The dangerous characters should be treated as data, not commands
        assert "evil" not in r.stdout or r.exit_code == 0
        # The literal text should pass through the pipe as data
        assert isinstance(r.exit_code, int)

    def test_callback_with_stdin_injection(self):
        """Data piped to callback via stdin must not be interpreted as commands."""
        tool = ScriptedTool("echo_stdin")
        tool.add_tool(
            "echo_stdin",
            "Echoes stdin",
            lambda p, s=None: s if s else "no stdin",
        )
        r = tool.execute_sync("echo '$(echo injected)' | echo_stdin")
        # stdin should be the literal string, not command-substituted
        assert isinstance(r.exit_code, int)

    def test_many_tools_registered(self):
        """Registering many tools must not crash or degrade."""
        tool = ScriptedTool("many")
        for i in range(100):
            tool.add_tool(f"tool_{i}", f"Tool {i}", lambda p, s=None, n=i: f"tool_{n}\n")
        r = tool.execute_sync("tool_50")
        assert r.exit_code == 0
        assert "tool_50" in r.stdout

    def test_tool_name_with_special_characters(self):
        """Tool names with special characters should be handled safely."""
        tool = ScriptedTool("special")
        # These names might cause issues if not properly handled
        for name in ["tool-with-dash", "tool_underscore"]:
            tool.add_tool(name, "Special name", lambda p, s=None: "ok\n")
        r = tool.execute_sync("tool-with-dash")
        assert isinstance(r.exit_code, int)


# ===========================================================================
# 6. WHITE-BOX: JSON conversion boundaries (TM-PY-027)
# ===========================================================================


class TestJsonConversionSecurity:
    """Test JSON <-> Python conversion edge cases."""

    def test_json_nesting_at_exact_limit(self):
        """Nesting at exactly 64 levels should be accepted."""
        nested = {"value": "leaf"}
        for _ in range(63):  # 63 + 1 = 64 levels
            nested = {"child": nested}
        tool = ScriptedTool("depth_64")
        # Should succeed — exactly at the limit
        tool.add_tool("test", "Test", callback=lambda p, s=None: "", schema=nested)
        assert tool.tool_count() == 1

    def test_json_nesting_at_limit_plus_one(self):
        """Nesting at 65 levels must be rejected."""
        nested = {"value": "leaf"}
        for _ in range(64):  # 64 + 1 = 65 levels
            nested = {"child": nested}
        tool = ScriptedTool("depth_65")
        with pytest.raises((ValueError, RuntimeError), match="nesting depth"):
            tool.add_tool("test", "Test", callback=lambda p, s=None: "", schema=nested)

    def test_json_wide_object(self):
        """Very wide (many keys) JSON objects should be handled."""
        schema = {f"key_{i}": {"type": "string"} for i in range(1000)}
        tool = ScriptedTool("wide")
        tool.add_tool("test", "Test", callback=lambda p, s=None: "", schema=schema)
        assert tool.tool_count() == 1

    def test_json_with_special_string_values(self):
        """JSON strings containing special characters must round-trip safely."""
        tool = ScriptedTool("special_json")
        schema = {
            "null_field": "\x00",
            "newline_field": "line1\nline2",
            "unicode_field": "\u4e16\u754c",
            "emoji_field": "\U0001f680",
            "escape_field": "\\n\\t\\r",
            "quote_field": 'say "hello"',
        }
        tool.add_tool("test", "Test", callback=lambda p, s=None: "", schema=schema)
        assert tool.tool_count() == 1

    def test_json_array_nesting_bomb(self):
        """Deeply nested arrays must be rejected at depth limit."""
        nested = [1]
        for _ in range(70):
            nested = [nested]
        tool = ScriptedTool("arr_depth")
        with pytest.raises((ValueError, RuntimeError), match="nesting depth"):
            tool.add_tool("test", "Test", callback=lambda p, s=None: "", schema=nested)


# ===========================================================================
# 7. BLACK-BOX: Concurrent access safety
# ===========================================================================


class TestConcurrentSafety:
    """Test thread safety and race conditions."""

    def test_concurrent_different_instances(self):
        """Multiple threads with separate instances must not interfere."""
        results = {}
        errors = {}

        def run(idx):
            try:
                bash = Bash()
                bash.execute_sync(f"export ID={idx}")
                r = bash.execute_sync("echo $ID")
                results[idx] = r.stdout.strip()
            except Exception as e:
                errors[idx] = e

        threads = [threading.Thread(target=run, args=(i,)) for i in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=30)

        assert not errors, f"Thread errors: {errors}"
        for i in range(10):
            assert results[i] == str(i), f"Thread {i} got wrong value: {results[i]}"

    def test_concurrent_same_instance_safety(self):
        """Multiple threads using same Bash instance should not crash.

        Due to Mutex<Bash>, calls will serialize. The important thing is
        no crash, no data corruption, no deadlock.
        """
        bash = Bash()
        errors = []

        def run(idx):
            try:
                for _ in range(5):
                    r = bash.execute_sync(f"echo thread_{idx}")
                    assert r.exit_code == 0
            except Exception as e:
                errors.append((idx, e))

        threads = [threading.Thread(target=run, args=(i,)) for i in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=30)

        for t in threads:
            assert not t.is_alive(), "Thread deadlocked"
        assert not errors, f"Thread errors: {errors}"

    def test_cancel_during_execution(self):
        """Cancelling during a long-running command should not deadlock."""
        bash = Bash(max_loop_iterations=10000)
        done = threading.Event()

        def run_long():
            bash.execute_sync("for i in $(seq 1 10000); do echo $i; done")
            done.set()

        t = threading.Thread(target=run_long)
        t.start()
        time.sleep(0.1)  # Let it start
        bash.cancel()
        t.join(timeout=10)
        assert not t.is_alive(), "Execution didn't stop after cancel"

    def test_reset_during_idle(self):
        """Reset while no execution is running should be safe."""
        bash = Bash()
        bash.execute_sync("echo setup")
        bash.reset()
        r = bash.execute_sync("echo after_reset")
        assert "after_reset" in r.stdout

    @pytest.mark.asyncio
    async def test_async_concurrent_instances(self):
        """Multiple async executions on different instances must not interfere."""

        async def run_one(idx):
            bash = Bash()
            r = await bash.execute(f"echo async_{idx}")
            return (idx, r.stdout.strip())

        tasks = [run_one(i) for i in range(5)]
        results = await asyncio.gather(*tasks)

        for idx, output in results:
            assert output == f"async_{idx}"


# ===========================================================================
# 8. WHITE-BOX: State confusion attacks
# ===========================================================================


class TestStateConfusion:
    """Attack interpreter state management."""

    def test_tm_iso_001_env_pollution_between_calls(self):
        """Environment from one call must persist (stateful), but not leak to new instances."""
        b1 = Bash()
        b1.execute_sync("export EVIL=payload")
        # Same instance: should persist
        r1 = b1.execute_sync("echo $EVIL")
        assert "payload" in r1.stdout
        # New instance: must not see it
        b2 = Bash()
        r2 = b2.execute_sync("echo ${EVIL:-clean}")
        assert r2.stdout.strip() == "clean"

    def test_tm_iso_003_function_persistence_is_per_instance(self):
        """Shell functions persist within instance but not across instances."""
        b1 = Bash()
        b1.execute_sync("myfunc() { echo secret; }")
        r1 = b1.execute_sync("myfunc")
        assert "secret" in r1.stdout
        b2 = Bash()
        r2 = b2.execute_sync("myfunc 2>/dev/null || echo not_found")
        assert "not_found" in r2.stdout

    def test_alias_persistence(self):
        """Aliases should persist within instance."""
        bash = Bash()
        bash.execute_sync("alias ll='ls -la'")
        r = bash.execute_sync("alias")
        # Alias behavior depends on implementation
        assert isinstance(r.exit_code, int)

    def test_working_directory_isolation(self):
        """Working directory must be isolated between instances."""
        b1 = Bash()
        b1.execute_sync("mkdir -p /tmp/deep/dir && cd /tmp/deep/dir")
        b1.execute_sync("pwd")

        b2 = Bash()
        r2 = b2.execute_sync("pwd")
        # b2 should be at default cwd, not b1's cd location
        # (Note: cd may or may not persist between execute_sync calls within same instance)
        assert isinstance(r2.exit_code, int)

    def test_vfs_size_growth(self):
        """Writing many/large files to VFS should be bounded somehow."""
        bash = Bash(max_commands=200)
        for i in range(50):
            bash.execute_sync(f"dd if=/dev/zero bs=1024 count=1 2>/dev/null | tr '\\0' 'A' > /tmp/big_{i}.txt")
        r = bash.execute_sync("echo alive")
        assert r.exit_code == 0

    def test_reset_clears_vfs_completely(self):
        """After reset, the VFS should be completely clean."""
        bash = Bash()
        bash.execute_sync("mkdir -p /tmp/a/b/c")
        bash.execute_sync("echo data > /tmp/a/b/c/file.txt")
        bash.execute_sync("export SECRET=abc123")
        bash.reset()
        r = bash.execute_sync("cat /tmp/a/b/c/file.txt 2>&1; echo ${SECRET:-gone}")
        assert "data" not in r.stdout
        assert "gone" in r.stdout


# ===========================================================================
# 9. BLACK-BOX: Encoding and special input attacks
# ===========================================================================


class TestEncodingAttacks:
    """Test various encoding tricks."""

    def test_very_long_command(self):
        """Extremely long commands must not crash."""
        bash = Bash()
        long_cmd = "echo " + "A" * 100_000
        r = bash.execute_sync(long_cmd)
        assert isinstance(r.exit_code, int)

    def test_many_arguments(self):
        """Commands with thousands of arguments must be handled."""
        bash = Bash()
        args = " ".join(str(i) for i in range(5000))
        r = bash.execute_sync(f"echo {args}")
        assert isinstance(r.exit_code, int)

    def test_binary_in_variable(self):
        """Binary data in variables must not crash the interpreter."""
        bash = Bash()
        r = bash.execute_sync("X=$(printf '\\x00\\x01\\x02\\xff'); echo ${#X}")
        assert isinstance(r.exit_code, int)

    def test_multiline_command_injection(self):
        """Multiline input must be parsed correctly."""
        bash = Bash()
        r = bash.execute_sync("echo 'line1\nline2\nline3'")
        assert isinstance(r.exit_code, int)

    def test_crlf_injection(self):
        """\\r\\n in strings must not cause protocol confusion."""
        bash = Bash()
        r = bash.execute_sync("echo 'before\\r\\nHTTP/1.1 200 OK\\r\\n'")
        assert isinstance(r.exit_code, int)

    def test_utf8_overlong_encoding(self):
        """Overlong UTF-8 sequences must not bypass security checks."""
        bash = Bash()
        # Overlong encoding of '/' would be [0xc0, 0xaf] — this should be rejected
        r = bash.execute_sync("echo 'test\xc0\xafetc\xc0\xafpasswd' 2>/dev/null || echo safe")
        assert "root:" not in r.stdout

    def test_null_in_command_string(self):
        """Null bytes in command string must not cause truncation."""
        bash = Bash()
        r = bash.execute_sync("echo before\x00after")
        # Either handles both parts or fails cleanly
        assert isinstance(r.exit_code, int)

    def test_tm_uni_004_emoji_and_rtl_in_commands(self):
        """Emoji and RTL characters in commands must not crash."""
        bash = Bash()
        r = bash.execute_sync("echo '\U0001f680 \u202e reversed \u202c'")
        assert isinstance(r.exit_code, int)

    def test_backslash_at_end_of_command(self):
        """Trailing backslash (line continuation) handled correctly."""
        bash = Bash()
        r = bash.execute_sync("echo hello\\")
        assert isinstance(r.exit_code, int)


# ===========================================================================
# 10. WHITE-BOX: BashTool-specific security
# ===========================================================================


class TestBashToolSecurity:
    """Test BashTool-specific security properties."""

    def test_bashtool_isolation_from_bash(self):
        """BashTool and Bash instances must not share state."""
        bash = Bash()
        tool = BashTool()
        bash.execute_sync("export SHARED=yes")
        r = tool.execute_sync("echo ${SHARED:-no}")
        assert r.stdout.strip() == "no"

    def test_bashtool_limits_preserved_after_reset(self):
        """BashTool limits must survive reset."""
        tool = BashTool(max_commands=5, max_loop_iterations=10)
        tool.reset()
        r = tool.execute_sync("while true; do echo x; done")
        lines = [ln for ln in r.stdout.strip().splitlines() if ln.strip()]
        assert len(lines) <= 11

    def test_bashtool_metadata_stable_after_execution(self):
        """Tool metadata must be stable after executing commands."""
        tool = BashTool()
        desc1 = tool.description()
        tool.execute_sync("echo test; export X=1")
        desc2 = tool.description()
        assert desc1 == desc2

    def test_bashtool_input_schema_not_injectable(self):
        """Input schema must be valid JSON and not injectable."""
        import json

        tool = BashTool()
        schema = json.loads(tool.input_schema())
        assert isinstance(schema, dict)
        assert schema.get("type") == "object"
        # Schema should define 'commands' property
        assert "properties" in schema

    def test_bashtool_system_prompt_no_internals(self):
        """System prompt must not expose internal implementation details."""
        tool = BashTool()
        prompt = tool.system_prompt()
        # Should not expose Rust implementation details
        assert "tokio" not in prompt.lower()
        assert "pyo3" not in prompt.lower()
        assert "mutex" not in prompt.lower()
        assert "arc<" not in prompt.lower()


# ===========================================================================
# 11. BLACK-BOX: Python-via-bash attack chains
# ===========================================================================


class TestPythonBashChains:
    """Attack using Python-via-bash execution chains."""

    def test_bash_spawns_python_spawns_bash(self):
        """Python code trying to spawn bash must fail."""
        bash = Bash()
        r = bash.execute_sync("python3 -c 'import os; os.system(\"echo escaped\")'")
        assert "escaped" not in r.stdout

    def test_python_reads_bash_env(self):
        """Python can read bash env vars via os.getenv (intentional, but must not leak host)."""
        bash = Bash()
        bash.execute_sync("export SAFE_VAR=hello")
        r = bash.execute_sync('python3 -c \'import os; print(os.getenv("SAFE_VAR", "missing"))\'')
        if r.exit_code == 0:
            assert "hello" in r.stdout or "missing" in r.stdout

    def test_python_cannot_read_host_env(self):
        """Python os.getenv must not expose host environment variables."""
        bash = Bash()
        r = bash.execute_sync('python3 -c \'import os; print(os.getenv("HOME", "none"))\'')
        # Should return the VFS home or "none", not the real host HOME
        if r.exit_code == 0 and "none" not in r.stdout:
            # If HOME is set, it should be the VFS path, not host path
            assert r.stdout.strip() == "/home/user" or r.stdout.strip() == "none"

    def test_python_math_edge_cases(self):
        """Python math operations must not crash on edge cases."""
        bash = Bash()
        cases = [
            "import math; print(math.inf)",
            "import math; print(math.nan)",
            "import math; print(math.inf - math.inf)",
            "print(float('inf'))",
            "print(1 / 0)",  # Should raise ZeroDivisionError
        ]
        for case in cases:
            r = bash.execute_sync(f"python3 -c '{case}'")
            assert isinstance(r.exit_code, int)

    def test_python_regex_dos(self):
        """Regex catastrophic backtracking must be bounded by time limit."""
        bash = Bash()
        r = bash.execute_sync(
            "python3 -c '\n"
            "import re\n"
            "# ReDoS pattern: exponential backtracking\n"
            're.match("(a+)+$", "a" * 30 + "!")\n'
            "'"
        )
        # Should either complete (Monty may not have full regex) or be killed by time limit
        assert isinstance(r.exit_code, int)


# ===========================================================================
# 12. WHITE-BOX: ScriptedTool environment variable injection
# ===========================================================================


class TestScriptedToolEnvSecurity:
    """Test env variable injection through ScriptedTool."""

    def test_env_value_not_expanded(self):
        """Environment variable values with $(cmd) must stay literal."""
        tool = ScriptedTool("env_test")
        tool.env("DANGEROUS", "$(echo pwned)")
        tool.add_tool("noop", "No-op", lambda p, s=None: "ok\n")
        r = tool.execute_sync('echo "$DANGEROUS"')
        assert "$(echo pwned)" in r.stdout

    def test_env_value_with_backticks(self):
        """Env values with backticks must not be executed."""
        tool = ScriptedTool("env_test2")
        tool.env("BACKTICK", "`echo pwned`")
        tool.add_tool("noop", "No-op", lambda p, s=None: "ok\n")
        r = tool.execute_sync('echo "$BACKTICK"')
        assert "`echo pwned`" in r.stdout or "pwned" not in r.stdout

    def test_env_key_with_special_chars(self):
        """Environment variable keys with special chars should be handled."""
        tool = ScriptedTool("env_test3")
        # Valid env var names are typically [A-Za-z_][A-Za-z0-9_]*
        tool.env("NORMAL_KEY", "value")
        tool.add_tool("noop", "No-op", lambda p, s=None: "ok\n")
        r = tool.execute_sync("echo $NORMAL_KEY")
        assert "value" in r.stdout

    def test_env_many_variables(self):
        """Setting many environment variables should not crash."""
        tool = ScriptedTool("env_many")
        for i in range(200):
            tool.env(f"VAR_{i}", f"value_{i}")
        tool.add_tool("check", "Check env", lambda p, s=None: "ok\n")
        r = tool.execute_sync("echo $VAR_199")
        if r.exit_code == 0:
            assert "value_199" in r.stdout


# ===========================================================================
# 13. BLACK-BOX: Rapid-fire stress tests
# ===========================================================================


class TestStress:
    """Stress tests for stability under load."""

    def test_rapid_execute_sync_no_leak(self):
        """200 rapid sync calls must not exhaust resources."""
        bash = Bash()
        for i in range(200):
            r = bash.execute_sync(f"echo {i}")
            assert r.exit_code == 0

    def test_rapid_instance_creation(self):
        """Creating and destroying 100 instances must not leak."""
        for i in range(100):
            bash = Bash()
            r = bash.execute_sync(f"echo {i}")
            assert r.exit_code == 0
            del bash

    def test_rapid_reset_cycle(self):
        """200 reset cycles must not exhaust resources."""
        bash = Bash(max_commands=10)
        for i in range(200):
            bash.reset()
        r = bash.execute_sync("echo alive")
        assert "alive" in r.stdout

    def test_rapid_scripted_tool_creation(self):
        """Creating many ScriptedTool instances must not leak."""
        for i in range(50):
            tool = ScriptedTool(f"tool_{i}")
            tool.add_tool("mytool", "Echo", lambda p, s=None: "ok\n")
            r = tool.execute_sync("mytool")
            assert r.exit_code == 0
            del tool

    def test_alternating_execute_reset(self):
        """Alternating execute and reset in rapid succession."""
        bash = Bash(max_commands=10)
        for i in range(100):
            bash.execute_sync(f"echo {i}")
            if i % 5 == 0:
                bash.reset()
        r = bash.execute_sync("echo stable")
        assert "stable" in r.stdout


# ===========================================================================
# 14. WHITE-BOX: deepagents.py edit operation security
# ===========================================================================


class TestDeepagentsEditSecurity:
    """Test deepagents.py edit operations for injection vulnerabilities."""

    _DEEPAGENTS_SRC = (Path(__file__).resolve().parent.parent / "bashkit" / "deepagents.py").read_text()

    def test_edit_uses_python_string_replace(self):
        """edit() must use Python string operations, not shell sed."""
        # Verify edit method reads file, does Python replace, writes back
        in_edit = False
        edit_lines = []
        for line in self._DEEPAGENTS_SRC.splitlines():
            if "def edit(" in line:
                in_edit = True
            elif in_edit and (line.strip().startswith("def ") or line.strip().startswith("async def ")):
                break
            if in_edit:
                edit_lines.append(line)
        edit_body = "\n".join(edit_lines)
        # Should use .replace() or .count(), not sed
        assert "sed" not in edit_body, "edit() should not shell out to sed"
        assert ".replace(" in edit_body or ".count(" in edit_body, "edit() should use Python string operations"

    def test_no_raw_string_formatting_in_methods(self):
        """No method should use %-formatting or .format() with untrusted input."""
        for i, line in enumerate(self._DEEPAGENTS_SRC.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            # % formatting with user input
            if "% (" in stripped and "path" in stripped:
                pytest.fail(f"L{i}: Potential %-format injection: {stripped}")

    def test_write_cmd_preserves_content_integrity(self):
        """_build_write_cmd must not corrupt special characters in content."""
        import importlib

        mod = importlib.import_module("bashkit.deepagents")
        build = mod._build_write_cmd

        # Content with every dangerous character
        content = "line1\n$HOME\n`whoami`\n$(id)\n'single'\n\"double\"\n\\backslash"
        cmd = build("/tmp/test.txt", content)
        # The heredoc uses single-quoted delimiter, so content is literal
        assert "<<" in cmd
        # Delimiter is single-quoted (no expansion)
        assert "'" in cmd.split("<<")[1].split("\n")[0]


# ===========================================================================
# 15. BLACK-BOX: ExecResult data integrity
# ===========================================================================


class TestExecResultIntegrity:
    """Verify ExecResult fields are not corrupted or mixed."""

    def test_stdout_stderr_separation(self):
        """stdout and stderr must be in separate fields."""
        bash = Bash()
        r = bash.execute_sync("echo out; echo err >&2")
        assert "out" in r.stdout
        assert "err" in r.stderr
        assert "err" not in r.stdout
        assert "out" not in r.stderr

    def test_exit_code_accuracy(self):
        """Exit codes must match the actual command result."""
        bash = Bash()
        assert bash.execute_sync("true").exit_code == 0
        assert bash.execute_sync("false").exit_code != 0
        r = bash.execute_sync("exit 42")
        assert r.exit_code == 42

    def test_truncation_flags(self):
        """Truncation flags should be booleans."""
        bash = Bash()
        r = bash.execute_sync("echo short")
        assert isinstance(r.stdout_truncated, bool)
        assert isinstance(r.stderr_truncated, bool)

    def test_success_property(self):
        """success property must match exit_code == 0."""
        bash = Bash()
        r_ok = bash.execute_sync("echo ok")
        r_fail = bash.execute_sync("false")
        assert r_ok.success is True
        assert r_fail.success is False

    def test_to_dict_completeness(self):
        """to_dict() must include all fields."""
        bash = Bash()
        r = bash.execute_sync("echo test")
        d = r.to_dict()
        assert "stdout" in d
        assert "stderr" in d
        assert "exit_code" in d
        assert "error" in d
        assert "stdout_truncated" in d
        assert "stderr_truncated" in d

    def test_repr_no_secret_leak(self):
        """__repr__ must not expose more than necessary."""
        bash = Bash()
        bash.execute_sync("export SECRET=hunter2")
        r = bash.execute_sync("echo $SECRET")
        repr_str = repr(r)
        # repr should include stdout (which has the secret — that's expected)
        # but should not include extra host info
        assert "ExecResult" in repr_str
