#!/usr/bin/env python3
"""Interactive playground for bashkit FileSystem capsule interop.

Run:  python crates/bashkit-python/tests/playground_capsule.py

Demonstrates:
1. Creating a standalone FileSystem with files/dirs
2. Exporting as capsule, importing back
3. Mounting into Bash and running commands
4. Using MockFileSystem for comparison
"""

import sys
from pathlib import Path

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

from mock_filesystem import MockFileSystem  # noqa: E402

from bashkit import Bash, FileSystem  # noqa: E402


def section(title):
    print(f"\n{'='*60}")
    print(f"  {title}")
    print(f"{'='*60}\n")


def main():
    section("1. Create native FileSystem with project structure")

    fs = FileSystem()
    fs.mkdir("/project/src", recursive=True)
    fs.mkdir("/project/tests", recursive=True)
    fs.write_file("/project/README.md", b"# Demo Project\n\nCapsule interop works!\n")
    fs.write_file("/project/src/main.py", b"def greet(name):\n    return f'Hello, {name}!'\n")
    fs.write_file("/project/src/__init__.py", b"")
    fs.write_file(
        "/project/tests/test_main.py",
        b"from src.main import greet\n\ndef test_greet():\n    assert greet('World') == 'Hello, World!'\n",
    )

    print("Created files:")
    for entry in fs.read_dir("/project"):
        print(f"  /project/{entry['name']}  ({entry['metadata']['file_type']})")

    section("2. Export to capsule and reimport")

    capsule = fs.to_capsule()
    print(f"Capsule type: {type(capsule)}")

    imported = FileSystem.from_capsule(capsule)
    print(f"Imported type: {type(imported)}")
    print(f"README content: {imported.read_file('/project/README.md')}")

    section("3. Mount into Bash and run commands")

    bash = Bash()
    bash.mount("/workspace", imported)

    commands = [
        "ls /workspace/project",
        "cat /workspace/project/README.md",
        "find /workspace -name '*.py' | sort",
        "wc -l /workspace/project/src/main.py",
        "grep -r 'def ' /workspace/project/src/",
    ]

    for cmd in commands:
        result = bash.execute_sync(cmd)
        print(f"$ {cmd}")
        if result.stdout:
            for line in result.stdout.rstrip().split("\n"):
                print(f"  {line}")
        if result.exit_code != 0:
            print(f"  [exit code: {result.exit_code}]")
            if result.stderr:
                print(f"  stderr: {result.stderr.rstrip()}")
        print()

    section("4. MockFileSystem comparison")

    mock = MockFileSystem()
    mock.mkdir("/project/src", recursive=True)
    mock.write_file("/project/src/main.py", b"def greet(name):\n    return f'Hello, {name}!'\n")

    print("Mock read_file:", mock.read_file("/project/src/main.py"))
    print("Mock stat:", mock.stat("/project/src/main.py"))
    print("Mock read_dir:", [e["name"] for e in mock.read_dir("/project")])

    section("5. Multiple mounts")

    fs_a = FileSystem()
    fs_a.write_file("/data.csv", b"name,age\nAlice,30\nBob,25\n")

    fs_b = FileSystem()
    fs_b.write_file("/config.json", b'{"debug": true}\n')

    bash2 = Bash()
    bash2.mount("/data", FileSystem.from_capsule(fs_a.to_capsule()))
    bash2.mount("/config", FileSystem.from_capsule(fs_b.to_capsule()))

    result = bash2.execute_sync("cat /data/data.csv")
    print(f"$ cat /data/data.csv\n  {result.stdout.rstrip()}\n")

    result = bash2.execute_sync("cat /config/config.json")
    print(f"$ cat /config/config.json\n  {result.stdout.rstrip()}\n")

    print("Done! All capsule interop operations successful.")


if __name__ == "__main__":
    main()
