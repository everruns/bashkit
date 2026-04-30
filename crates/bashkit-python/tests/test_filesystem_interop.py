"""Native filesystem interop tests using a downstream-style PyO3 fixture."""

import pytest
from bashkit_random_fs import create_random_filesystem_capsule, expected_random_text

from bashkit import Bash, FileSystem


def random_filesystem(seed: int = 7) -> FileSystem:
    return FileSystem.from_capsule(create_random_filesystem_capsule(seed))


def test_random_filesystem_capsule_mounts_into_bash():
    fs = random_filesystem(2026)
    bash = Bash()
    bash.mount("/remote", fs)

    result = bash.execute_sync("cat /remote/random.txt")

    assert result.exit_code == 0
    assert result.stdout == expected_random_text(2026, "/random.txt")


def test_random_filesystem_capsule_supports_directory_listing():
    fs = random_filesystem(99)
    bash = Bash()
    bash.mount("/remote", fs)

    result = bash.execute_sync("find /remote -type f | sort")

    assert result.exit_code == 0
    assert result.stdout.splitlines() == [
        "/remote/README.md",
        "/remote/nested/data.txt",
        "/remote/random.txt",
    ]


def test_random_filesystem_capsule_is_readonly():
    fs = random_filesystem(42)
    bash = Bash()
    bash.mount("/remote", fs)

    result = bash.execute_sync("echo nope > /remote/random.txt")

    assert result.exit_code != 0
    assert "read-only" in result.stderr


@pytest.mark.asyncio
async def test_random_filesystem_capsule_works_in_async_bash_execution():
    fs = random_filesystem(123)
    bash = Bash()
    bash.mount("/remote", fs)

    result = await bash.execute("cat /remote/random.txt")

    assert result.exit_code == 0
    assert result.stdout == expected_random_text(123, "/random.txt")
