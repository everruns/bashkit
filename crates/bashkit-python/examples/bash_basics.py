#!/usr/bin/env python3
"""Basic usage of the Bash interface.

Demonstrates core Bash features: command execution, pipelines, variables,
loops, virtual filesystem persistence, and resource limits.

Run directly:
    cd crates/bashkit-python && maturin develop && python examples/bash_basics.py
"""

from __future__ import annotations

import asyncio

from bashkit import Bash


def demo_sync():
    """Synchronous API basics."""
    print("=== Sync API ===\n")

    bash = Bash()

    # Simple command
    r = bash.execute_sync("echo 'Hello from Bash!'")
    print(f"echo: {r.stdout.strip()}")
    assert r.success

    # Pipeline
    r = bash.execute_sync("echo -e 'banana\\napple\\ncherry' | sort")
    print(f"sort: {r.stdout.strip()}")
    assert r.stdout.strip() == "apple\nbanana\ncherry"

    # Variables persist across calls
    bash.execute_sync("MY_VAR='persistent'")
    r = bash.execute_sync("echo $MY_VAR")
    print(f"var:  {r.stdout.strip()}")
    assert r.stdout.strip() == "persistent"

    # Virtual filesystem persists
    bash.execute_sync("mkdir -p /tmp/demo && echo 'data' > /tmp/demo/file.txt")
    r = bash.execute_sync("cat /tmp/demo/file.txt")
    print(f"file: {r.stdout.strip()}")
    assert r.stdout.strip() == "data"

    # Loops and arithmetic
    r = bash.execute_sync("""
        total=0
        for i in 1 2 3 4 5; do
            total=$((total + i))
        done
        echo $total
    """)
    print(f"sum:  {r.stdout.strip()}")
    assert r.stdout.strip() == "15"

    # Error handling
    r = bash.execute_sync("exit 42")
    print(f"exit: code={r.exit_code}, success={r.success}")
    assert r.exit_code == 42
    assert not r.success

    # Text processing pipeline
    r = bash.execute_sync("""
        cat << 'EOF' | grep -c 'error'
info: all good
error: disk full
info: recovered
error: timeout
EOF
    """)
    print(f"grep: {r.stdout.strip()} errors found")
    assert r.stdout.strip() == "2"

    # Reset clears state
    bash.reset()
    r = bash.execute_sync("echo ${MY_VAR:-unset}")
    print(f"reset: {r.stdout.strip()}")
    assert r.stdout.strip() == "unset"

    print()


async def demo_async():
    """Async API basics."""
    print("=== Async API ===\n")

    bash = Bash()

    # Async execution
    r = await bash.execute("echo 'async hello'")
    print(f"async: {r.stdout.strip()}")
    assert r.success

    # Build a JSON report with jq
    await bash.execute("""
        cat > /tmp/users.json << 'EOF'
[
  {"name": "Alice", "role": "admin"},
  {"name": "Bob", "role": "user"},
  {"name": "Carol", "role": "admin"}
]
EOF
    """)
    r = await bash.execute("cat /tmp/users.json | jq '[.[] | select(.role == \"admin\")] | length'")
    print(f"admins: {r.stdout.strip()}")
    assert r.stdout.strip() == "2"

    # ExecResult as dict
    r = await bash.execute("echo ok")
    d = r.to_dict()
    print(f"dict: stdout={d['stdout'].strip()!r}, exit_code={d['exit_code']}")
    assert d["exit_code"] == 0

    print()


def demo_config():
    """Custom configuration."""
    print("=== Configuration ===\n")

    bash = Bash(username="agent", hostname="sandbox")
    r = bash.execute_sync("whoami")
    print(f"whoami:   {r.stdout.strip()}")
    assert r.stdout.strip() == "agent"

    r = bash.execute_sync("hostname")
    print(f"hostname: {r.stdout.strip()}")
    assert r.stdout.strip() == "sandbox"

    # Resource limits
    limited = Bash(max_loop_iterations=50)
    r = limited.execute_sync("i=0; while true; do i=$((i+1)); done; echo $i")
    print(f"limited:  stopped (exit_code={r.exit_code})")
    assert r.exit_code != 0 or int(r.stdout.strip() or "0") <= 100

    print()


def main():
    print("Bashkit — Bash interface examples\n")
    demo_sync()
    asyncio.run(demo_async())
    demo_config()
    print("All examples passed.")


if __name__ == "__main__":
    main()
