#!/usr/bin/env python3
"""Mount a custom native filesystem into Bashkit through the interop capsule ABI."""

from bashkit_random_fs import create_random_filesystem_capsule, expected_random_text

from bashkit import Bash, FileSystem


def create_filesystem(seed: int) -> FileSystem:
    """Wrap the downstream extension capsule as a bashkit-owned FileSystem."""
    return FileSystem.from_capsule(create_random_filesystem_capsule(seed))


def main() -> None:
    seed = 2026
    fs = create_filesystem(seed)

    bash = Bash()
    bash.mount("/remote", fs)

    random_file = bash.execute_sync("cat /remote/random.txt")
    assert random_file.exit_code == 0
    assert random_file.stdout == expected_random_text(seed, "/random.txt")

    listing = bash.execute_sync("find /remote -type f | sort")
    assert listing.exit_code == 0
    assert listing.stdout.splitlines() == [
        "/remote/README.md",
        "/remote/nested/data.txt",
        "/remote/random.txt",
    ]

    print("Custom filesystem interop example passed.")


if __name__ == "__main__":
    main()
