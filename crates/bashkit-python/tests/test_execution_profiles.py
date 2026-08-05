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
