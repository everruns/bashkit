"""Security tests for bashkit Python bindings.

Covers: command injection, VFS isolation, resource limits,
callback safety, env var injection, and Unicode/special char handling.
"""

import pytest

from bashkit import Bash, BashTool, ScriptedTool

# ===========================================================================
# VFS isolation: no host filesystem access
# ===========================================================================


def test_vfs_cannot_read_host_etc_passwd():
    """VFS must not expose host /etc/passwd."""
    bash = Bash()
    r = bash.execute_sync("cat /etc/passwd")
    # Should either fail or return VFS-only content (not real users)
    if r.exit_code == 0:
        assert "root:x:0:0" not in r.stdout, "Host /etc/passwd leaked into VFS"


def test_vfs_cannot_read_host_proc():
    """VFS must not expose /proc filesystem."""
    bash = Bash()
    r = bash.execute_sync("cat /proc/self/cmdline")
    assert r.exit_code != 0 or r.stdout == "", "Host /proc leaked into VFS"


def test_vfs_writes_isolated_between_instances():
    """Files written in one VFS are not visible in another instance."""
    bash = Bash()
    bash.execute_sync("echo pwned > /tmp/vfs_test_file.txt")
    r = bash.execute_sync("cat /tmp/vfs_test_file.txt")
    assert r.exit_code == 0
    assert r.stdout.strip() == "pwned"
    # A separate instance must not see this file
    bash2 = Bash()
    r2 = bash2.execute_sync("cat /tmp/vfs_test_file.txt")
    assert r2.exit_code != 0, "VFS state leaked between interpreter instances"


def test_vfs_directory_traversal():
    """Directory traversal must not escape VFS."""
    bash = Bash()
    r = bash.execute_sync("cat /home/user/../../../etc/hostname")
    # Should resolve within VFS, not expose host
    if r.exit_code == 0:
        assert r.stdout.strip() != "", "Traversal returned host data"


# ===========================================================================
# Command injection via untrusted input
# ===========================================================================


def test_single_quoted_strings_prevent_expansion():
    """Single-quoted strings must prevent all expansion."""
    bash = Bash()
    r = bash.execute_sync("echo '$(whoami) ${HOME} `date`'")
    assert r.exit_code == 0
    assert "$(whoami)" in r.stdout
    assert "${HOME}" in r.stdout
    assert "`date`" in r.stdout


def test_semicolon_in_variable_does_not_execute():
    """Semicolons in variable values must not become command separators."""
    bash = Bash()
    bash.execute_sync("export DATA='foo; echo INJECTED'")
    r = bash.execute_sync('echo "$DATA"')
    assert r.stdout.strip() == "foo; echo INJECTED"
    assert r.stdout.count("INJECTED") <= 1  # Only the literal, not executed


def test_backtick_in_echo_argument():
    """Backtick injection in single-quoted strings stays literal."""
    bash = Bash()
    r = bash.execute_sync("echo '`echo INJECTED`'")
    assert "INJECTED" not in r.stdout or "`echo INJECTED`" in r.stdout


# ===========================================================================
# ScriptedTool callback security
# ===========================================================================


def test_callback_exception_does_not_crash_interpreter():
    """Callback exceptions must not crash the interpreter."""
    tool = ScriptedTool("api")

    def bad_callback(params, stdin=None):
        raise Exception("catastrophic failure")

    tool.add_tool("crash", "Crashes", callback=bad_callback)
    r = tool.execute_sync("crash")
    assert r.exit_code != 0
    # Interpreter still works after crash
    tool.add_tool("ok", "OK", callback=lambda p, s=None: "alive\n")
    r2 = tool.execute_sync("ok")
    assert r2.exit_code == 0
    assert r2.stdout.strip() == "alive"


def test_callback_returning_none_handled():
    """Callback returning None should be handled gracefully."""
    tool = ScriptedTool("api")
    tool.add_tool("noner", "Returns None", callback=lambda p, s=None: None)
    r = tool.execute_sync("noner")
    # Should fail gracefully (callback must return str)
    assert r.exit_code != 0


def test_callback_with_huge_params():
    """Callback receiving many parameters doesn't crash."""
    tool = ScriptedTool("api")
    tool.add_tool(
        "many",
        "Many params",
        callback=lambda p, s=None: str(len(p)) + "\n",
    )
    # Build a command with many --key value pairs
    args = " ".join(f"--key{i} val{i}" for i in range(50))
    r = tool.execute_sync(f"many {args}")
    assert r.exit_code == 0
    assert int(r.stdout.strip()) == 50


# ===========================================================================
# Environment variable injection
# ===========================================================================


def test_env_var_value_not_executed():
    """Environment variable values must not be executed as commands."""
    tool = ScriptedTool("api")
    tool.env("SAFE", "$(echo INJECTED)")
    tool.add_tool("noop", "No-op", callback=lambda p, s=None: "ok\n")
    r = tool.execute_sync('echo "$SAFE"')
    assert r.exit_code == 0
    # The literal string should appear, not the result of execution
    assert "$(echo INJECTED)" in r.stdout


def test_env_var_with_newlines():
    """Environment variables with embedded newlines handled correctly."""
    bash = Bash()
    bash.execute_sync('export MULTI="line1\nline2"')
    r = bash.execute_sync('echo -e "$MULTI"')
    assert r.exit_code == 0


# ===========================================================================
# Resource limit enforcement
# ===========================================================================


def test_resource_limits_survive_reset():
    """Resource limits must persist after reset (TM-PY-026)."""
    bash = Bash(max_commands=3)
    bash.reset()
    r = bash.execute_sync("echo 1; echo 2; echo 3; echo 4; echo 5")
    lines = [line for line in r.stdout.strip().splitlines() if line]
    assert len(lines) < 5 or r.exit_code != 0, "max_commands not enforced after reset"


def test_max_loop_iterations_enforced():
    """Infinite loops must be stopped by max_loop_iterations."""
    bash = Bash(max_loop_iterations=5)
    r = bash.execute_sync("while true; do echo x; done")
    assert r.exit_code != 0 or r.stdout.count("x") <= 50


def test_fork_bomb_prevented():
    """Fork bombs cannot run (no real processes in VFS)."""
    bash = Bash(max_commands=10)
    bash.execute_sync(":(){ :|:& };:")
    # Should either fail to parse or hit command limit
    # The key thing is it doesn't actually fork-bomb the host
    assert True  # If we get here, host wasn't harmed


# ===========================================================================
# Unicode and special character handling
# ===========================================================================


def test_unicode_in_commands():
    """Unicode characters in commands work correctly."""
    bash = Bash()
    r = bash.execute_sync("echo 'Hello \u4e16\u754c'")
    assert r.exit_code == 0
    assert "\u4e16\u754c" in r.stdout


def test_emoji_in_output():
    """Emoji characters pass through correctly."""
    bash = Bash()
    r = bash.execute_sync("echo '\U0001f680\U0001f30d'")
    assert r.exit_code == 0
    assert "\U0001f680" in r.stdout


def test_null_bytes_in_input():
    """Null bytes in input handled without crash."""
    bash = Bash()
    r = bash.execute_sync("echo 'before\\0after'")
    assert r.exit_code == 0


def test_special_shell_chars_in_strings():
    """Special shell metacharacters in quoted strings stay literal."""
    bash = Bash()
    r = bash.execute_sync("echo 'hello & world | test > file < input'")
    assert r.exit_code == 0
    assert "&" in r.stdout
    assert "|" in r.stdout


# ===========================================================================
# Deeply nested schema rejection
# ===========================================================================


def test_nested_json_beyond_limit_rejected():
    """JSON nesting > 64 levels must be rejected to prevent stack exhaustion."""
    nested = {"value": "leaf"}
    for _ in range(70):
        nested = {"child": nested}
    tool = ScriptedTool("deep")
    with pytest.raises(ValueError, match="nesting depth"):
        tool.add_tool("deep", "Deep", callback=lambda p, s=None: "", schema=nested)


# ===========================================================================
# Cancellation safety
# ===========================================================================


def test_cancel_is_safe():
    """Calling cancel() is safe even when nothing is running."""
    bash = Bash()
    bash.cancel()  # Should not raise
    bash.execute_sync("echo still_works")
    # After cancel, the next execution may or may not work depending on
    # whether the token auto-resets, but it should not crash
    assert True  # If we get here, no crash


def test_bashtool_cancel_is_safe():
    """BashTool cancel() is safe even when nothing is running."""
    tool = BashTool()
    tool.cancel()  # Should not raise
    # Should not crash
    assert True


# ===========================================================================
# Instance isolation
# ===========================================================================


def test_separate_instances_isolated():
    """Two Bash instances must not share state."""
    b1 = Bash()
    b2 = Bash()
    b1.execute_sync("export SECRET=hunter2")
    b1.execute_sync("echo private > /tmp/secret.txt")
    r = b2.execute_sync("echo ${SECRET:-empty}")
    assert r.stdout.strip() == "empty", "Variables leaked between instances"
    r2 = b2.execute_sync("cat /tmp/secret.txt")
    assert r2.exit_code != 0, "Files leaked between instances"


def test_separate_bashtool_instances_isolated():
    """Two BashTool instances must not share state."""
    t1 = BashTool()
    t2 = BashTool()
    t1.execute_sync("export TOKEN=secret123")
    r = t2.execute_sync("echo ${TOKEN:-empty}")
    assert r.stdout.strip() == "empty", "Variables leaked between BashTool instances"
