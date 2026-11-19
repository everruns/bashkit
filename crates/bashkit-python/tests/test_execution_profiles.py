import pytest

from bashkit import Bash, ExecutionProfile


def test_hardened_profile_keeps_isolated_vfs_writable() -> None:
    bash = Bash(profile=ExecutionProfile.Hardened)
    result = bash.execute_sync("printf x > /tmp/profile-test; cat /tmp/profile-test")
    assert result.exit_code == 0
    assert result.stdout == "x"


def test_profile_argument_is_typed() -> None:
    with pytest.raises(TypeError):
        Bash(profile="hardened")  # type: ignore[arg-type]


def _run(bash, script):
    """Return (exit_code, stdout, error_text); session-limit failures may raise."""
    try:
        r = bash.execute_sync(script)
    except Exception as exc:  # noqa: BLE001 - surface the error text
        return None, "", str(exc)
    return r.exit_code, r.stdout, (r.error or "") + r.stderr


def test_max_exec_calls_caps_session() -> None:
    bash = Bash(max_exec_calls=2)
    assert _run(bash, "echo a")[0] == 0
    assert _run(bash, "echo b")[0] == 0
    code, _, err = _run(bash, "echo c")
    assert code != 0
    assert "session exec() call limit exceeded (2 calls)" in err


def test_max_exec_calls_raises_default_cap() -> None:
    # Default is 1,000 calls; a host running long agent sessions can raise it.
    bash = Bash(max_exec_calls=1_200)
    for _ in range(1_100):
        code, _, err = _run(bash, "true")
        assert code == 0, err


def test_max_exec_calls_survives_reset_config() -> None:
    bash = Bash(max_exec_calls=1)
    assert _run(bash, "true")[0] == 0
    assert _run(bash, "true")[0] != 0
    bash.reset()
    assert _run(bash, "true")[0] == 0
    assert _run(bash, "true")[0] != 0


def test_max_session_commands_caps_session() -> None:
    bash = Bash(max_session_commands=3)
    assert _run(bash, "true; true")[0] == 0
    code, _, err = _run(bash, "true; true")
    assert code != 0
    assert "session command limit exceeded" in err


def test_bash_tool_accepts_session_limits() -> None:
    from bashkit import BashTool

    tool = BashTool(max_exec_calls=1)
    assert _run(tool, "echo a")[0] == 0
    code, _, err = _run(tool, "echo b")
    assert code != 0
    assert "session exec() call limit exceeded (1 calls)" in err
