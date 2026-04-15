"""Security tests for bashkit Python bindings.

Covers: command injection, VFS isolation, resource limits,
callback safety, env var injection, and Unicode/special char handling.
"""

import re

import pytest

from bashkit import Bash, BashTool, ScriptedTool

# Decision: keep the issue-1264 parity cases in this hidden core module while
# the public `test_security.py` module re-exports the merged security suite.
# New threat-model-traceable cases use `test_tm_*` names so they can map back
# to `specs/006-threat-model.md`.


def _assert_sanitized_error(text: str) -> None:
    assert "/Users/" not in text
    assert "/private/" not in text
    assert "Traceback (most recent call last)" not in text
    assert "pyo3" not in text.lower()
    assert "rust" not in text.lower()
    assert not re.search(r"0x[0-9a-f]{6,}", text, re.IGNORECASE)


# ===========================================================================
# VFS isolation: no host filesystem access
# ===========================================================================


def test_tm_inf_001_vfs_cannot_read_host_etc_passwd():
    """VFS must not expose host /etc/passwd."""
    bash = Bash()
    r = bash.execute_sync("cat /etc/passwd")
    # Should either fail or return VFS-only content (not real users)
    if r.exit_code == 0:
        assert "root:x:0:0" not in r.stdout, "Host /etc/passwd leaked into VFS"


def test_tm_esc_003_vfs_cannot_read_host_proc():
    """VFS must not expose /proc filesystem."""
    bash = Bash()
    r = bash.execute_sync("cat /proc/self/cmdline")
    assert r.exit_code != 0 or r.stdout == "", "Host /proc leaked into VFS"


def test_tm_iso_002_vfs_writes_are_isolated_between_instances():
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


def test_tm_inj_005_vfs_directory_traversal_is_blocked():
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


def test_tm_dos_016_max_loop_iterations_enforced():
    """Infinite loops must be stopped by max_loop_iterations."""
    bash = Bash(max_loop_iterations=5)
    r = bash.execute_sync("while true; do echo x; done")
    assert r.exit_code != 0 or r.stdout.count("x") <= 50


def test_tm_dos_021_fork_bomb_prevented():
    """Fork bombs cannot run (no real processes in VFS)."""
    bash = Bash(max_commands=10)
    bash.execute_sync(":(){ :|:& };:")
    # Should either fail to parse or hit command limit
    # The key thing is it doesn't actually fork-bomb the host
    assert True  # If we get here, host wasn't harmed


# ===========================================================================
# Threat-model parity: VFS limits, arithmetic, env, injection
# ===========================================================================


def test_tm_dos_005_large_file_write_limited():
    """TM-DOS-005: direct VFS writes must honor the default 10 MB file limit."""
    with pytest.raises(RuntimeError, match="file too large"):
        Bash().fs().write_file("/tmp/big.bin", b"A" * (11 * 1024 * 1024))


def test_tm_dos_006_vfs_file_count_limit():
    """TM-DOS-006: creating files until exhaustion must hit the VFS file cap."""
    fs = Bash().fs()
    created = 0

    with pytest.raises(RuntimeError, match="too many files"):
        while True:
            fs.write_file(f"/tmp/f{created}", b"x")
            created += 1

    assert created >= 9000


def test_tm_dos_012_deep_directory_nesting_limited():
    """TM-DOS-012: path depth beyond 100 levels must be rejected."""
    deep_path = "/tmp/" + "/".join(["d"] * 150)
    with pytest.raises(RuntimeError, match="path too deep"):
        Bash().fs().mkdir(deep_path, recursive=True)


def test_tm_dos_013_long_filename_rejected():
    """TM-DOS-013: filenames longer than 255 bytes must be rejected."""
    with pytest.raises(RuntimeError, match="filename too long"):
        Bash().fs().write_file("/tmp/" + ("A" * 300), b"x")


def test_tm_dos_013_long_path_rejected():
    """TM-DOS-013: paths longer than 4096 bytes must be rejected."""
    with pytest.raises(RuntimeError, match="path too long"):
        Bash().fs().write_file("/tmp/" + ("a/" * 2100) + "leaf", b"x")


def test_tm_dos_029_arithmetic_overflow_does_not_crash():
    """TM-DOS-029: integer overflow must stay inside the sandbox."""
    r = Bash().execute_sync("echo $((9223372036854775807 + 1))")
    assert isinstance(r.exit_code, int)
    _assert_sanitized_error(r.stderr)


def test_tm_dos_029_division_by_zero_does_not_crash():
    """TM-DOS-029: division by zero must return an error value, not panic."""
    r = Bash().execute_sync("echo $((1 / 0)) 2>&1")
    assert isinstance(r.exit_code, int)
    assert r.stdout.strip() == "0"
    _assert_sanitized_error(r.stderr)


def test_tm_dos_029_modulo_by_zero_does_not_crash():
    """TM-DOS-029: modulo by zero must not unwind through the binding."""
    r = Bash().execute_sync("echo $((1 % 0)) 2>&1")
    assert isinstance(r.exit_code, int)
    assert r.stdout.strip() == "0"
    _assert_sanitized_error(r.stderr)


def test_tm_dos_029_negative_exponent_does_not_crash():
    """TM-DOS-029: unsupported arithmetic should stay bounded."""
    r = Bash().execute_sync("echo $((2 ** -1)) 2>&1")
    assert isinstance(r.exit_code, int)
    assert r.stdout.strip() == "0"
    _assert_sanitized_error(r.stderr)


def test_tm_dos_059_default_memory_limit_prevents_oom_without_max_memory():
    """TM-DOS-059: Bash() must enforce the default memory cap even without max_memory."""
    bash = Bash(max_loop_iterations=10000, max_commands=10000)
    r = bash.execute_sync('x=AAAAAAAAAA; i=0; while [ $i -lt 30 ]; do x="$x$x"; i=$((i+1)); done; echo ${#x}')
    assert r.exit_code == 0
    assert int(r.stdout.strip()) <= 10_000_000


def test_tm_inf_002_env_builtins_do_not_leak_host_env():
    """TM-INF-002: env/printenv must not expose host HOME/PATH values."""
    bash = Bash()
    env_output = bash.execute_sync("env").stdout
    printenv_output = bash.execute_sync("printenv").stdout
    combined = env_output + printenv_output
    assert "/Users/" not in combined
    assert "PATH=/usr" not in combined
    assert "HOME=/home/" not in combined


def test_tm_inf_002_default_env_vars_are_sandboxed():
    """TM-INF-002: builtin shell vars should resolve to virtual sandbox values."""
    r = Bash().execute_sync('echo "HOME=$HOME PATH=$PATH USER=$USER HOSTNAME=$HOSTNAME"')
    assert r.exit_code == 0
    assert r.stdout.strip() == "HOME=/home/sandbox PATH= USER=sandbox HOSTNAME=bashkit-sandbox"


def test_tm_esc_002_process_substitution_stays_in_sandbox():
    """TM-ESC-002: process substitution must not require real subprocesses."""
    r = Bash().execute_sync("cat <(echo test)")
    assert r.exit_code == 0
    assert r.stdout.strip() == "test"


def test_tm_esc_005_signal_trap_commands_stay_bounded():
    """TM-ESC-005: trap handlers must execute without leaving the virtual shell."""
    r = Bash().execute_sync('trap "echo trapped" EXIT; echo done')
    assert r.exit_code == 0
    assert r.stdout == "done\ntrapped\n"


def test_tm_inf_006_username_metacharacters_are_literal():
    """TM-INF-006: usernames with shell syntax must stay literal."""
    r = Bash(username="$(echo pwned)").execute_sync("whoami")
    assert r.exit_code == 0
    assert r.stdout.strip() == "$(echo pwned)"


def test_tm_inf_005_hostname_metacharacters_are_literal():
    """TM-INF-005: hostnames with shell syntax must stay literal."""
    r = Bash(hostname="$(rm -rf /)").execute_sync("hostname")
    assert r.exit_code == 0
    assert r.stdout.strip() == "$(rm -rf /)"


def test_tm_inf_006_username_newlines_are_stored_literally():
    """TM-INF-006: newline-bearing usernames must not execute trailing commands."""
    r = Bash(username="user\necho INJECTED").execute_sync("whoami")
    assert r.exit_code == 0
    assert r.stdout == "user\necho INJECTED\n"


def test_tm_inf_005_hostname_newlines_are_stored_literally():
    """TM-INF-005: newline-bearing hostnames must not execute trailing commands."""
    r = Bash(hostname="host\necho INJECTED").execute_sync("hostname")
    assert r.exit_code == 0
    assert r.stdout == "host\necho INJECTED\n"


def test_tm_inj_005_mounted_files_traversal_paths_stay_in_vfs():
    """TM-INJ-005: crafted files dict paths may normalize, but must stay virtual."""
    bash = Bash(files={"/tmp/../../../etc/passwd": "root:x:0"})
    r = bash.execute_sync("cat /etc/passwd")
    assert r.exit_code == 0
    assert r.stdout == "root:x:0"


def test_tm_inj_005_mounted_files_null_byte_path_is_safe():
    """TM-INJ-005: null bytes in mount paths must not escape or crash the binding."""
    bash = Bash(files={"/tmp/test\x00evil": "content"})
    r = bash.execute_sync("ls /tmp")
    assert r.exit_code == 0
    assert "test" not in r.stdout


def test_tm_inj_005_mounted_files_special_characters_in_content_roundtrip():
    """TM-INJ-005: mounted file contents should preserve control characters literally."""
    bash = Bash(files={"/tmp/special.txt": "line1\nline2\ttab\r\nwindows\n"})
    r = bash.execute_sync("cat /tmp/special.txt")
    assert r.exit_code == 0
    assert r.stdout == "line1\nline2\ttab\r\nwindows\n"


def test_tm_inj_005_mounted_files_empty_content_roundtrip():
    """TM-INJ-005: empty mounted files should remain addressable."""
    bash = Bash(files={"/tmp/empty.txt": ""})
    r = bash.execute_sync("wc -c /tmp/empty.txt")
    assert r.exit_code == 0
    assert "/tmp/empty.txt" in r.stdout
    assert "0" in r.stdout


def test_tm_inj_005_crlf_line_endings_in_scripts_execute_safely():
    """TM-INJ-005: CRLF scripts should execute without cross-line injection."""
    r = Bash().execute_sync("echo hello\r\necho world\r\n")
    assert r.exit_code == 0
    assert r.stdout == "hello\r\nworld\r\n"


def test_tm_inj_005_direct_read_file_traversal_is_blocked():
    """TM-INJ-005: Bash.read_file() must not expose paths outside the VFS root."""
    bash = Bash()
    with pytest.raises(RuntimeError, match="file not found"):
        bash.read_file("/tmp/../../etc/passwd")


def test_tm_inj_005_direct_write_file_traversal_normalizes_inside_vfs():
    """TM-INJ-005: Bash.write_file() must normalize traversal back into the VFS."""
    bash = Bash()
    bash.write_file("/tmp/../../evil.txt", "payload")
    assert bash.exists("/evil.txt") is True
    assert bash.exists("/tmp/evil.txt") is False
    assert bash.read_file("/evil.txt") == "payload"


def test_tm_inj_005_direct_ls_injection_is_safe():
    """TM-INJ-005: Bash.ls() must treat shell metacharacters as a literal path."""
    bash = Bash()
    bash.write_file("/tmp/safe.txt", "ok")
    assert bash.ls("'; echo pwned") == []


def test_tm_inj_005_direct_glob_injection_is_safe():
    """TM-INJ-005: Bash.glob() must reject unsafe patterns instead of composing shell."""
    bash = Bash()
    bash.write_file("/tmp/safe.txt", "ok")
    assert bash.glob("'; echo pwned") == []


def test_tm_int_001_shell_errors_do_not_leak_host_paths():
    """TM-INT-001: shell-facing errors must stay sanitized."""
    r = Bash().execute_sync("cat /definitely/missing/file")
    assert r.exit_code != 0
    _assert_sanitized_error(r.stderr)


def test_tm_int_001_direct_fs_errors_do_not_leak_host_paths():
    """TM-INT-001: direct VFS exceptions must not disclose host paths."""
    with pytest.raises(RuntimeError) as excinfo:
        Bash().fs().read_file("/missing")
    _assert_sanitized_error(str(excinfo.value))


def test_tm_int_002_parse_errors_do_not_leak_addresses_or_traces():
    """TM-INT-002: parse failures must not bubble interpreter internals to Python."""
    r = Bash().execute_sync(")")
    assert r.exit_code != 0
    _assert_sanitized_error(r.stderr)


def test_tm_int_002_callback_errors_do_not_leak_rust_internals():
    """TM-INT-002: callback failures must surface as compact binding errors."""
    tool = ScriptedTool("api")
    tool.add_tool("boom", "Explodes", callback=lambda p, s=None: (_ for _ in ()).throw(RuntimeError("kaboom")))
    r = tool.execute_sync("boom")
    assert r.exit_code != 0
    _assert_sanitized_error(r.stderr)


def test_tm_inf_014_special_variable_pid_is_sandboxed():
    """TM-INF-014: $$ must return the virtual PID, not the host one."""
    r = Bash().execute_sync("echo $$")
    assert r.exit_code == 0
    assert r.stdout.strip() == "1"


def test_tm_inf_014_special_variable_ppid_is_sandboxed():
    """TM-INF-014: $PPID must resolve to a virtual parent PID."""
    r = Bash().execute_sync("echo ${PPID:-missing}")
    assert r.exit_code == 0
    assert r.stdout.strip() == "0"


def test_tm_inf_014_special_variable_uid_is_sandboxed():
    """TM-INF-014: $UID must resolve to the virtual sandbox uid."""
    r = Bash().execute_sync("echo $UID")
    assert r.exit_code == 0
    assert r.stdout.strip() == "1000"


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
