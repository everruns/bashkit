#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "deepagents>=0.3.11",
#     "langchain-anthropic>=0.3",
# ]
# [tool.uv]
# exclude-newer = "2026-02-06"
# ///
#
# Build bashkit first:
#   uv venv && source .venv/bin/activate
#   uv pip install maturin && cd crates/bashkit-python && maturin develop
#
"""
Deep Agent with BashKit Virtual Filesystem

Demonstrates BashKit as a sandboxed backend for Deep Agents.
The agent gets access to:
- `execute` tool: Run shell commands (via SandboxBackendProtocol)
- `read_file`, `write_file`, `edit_file`: File operations
- `ls`, `glob`, `grep`: File discovery

All operations use BashKit's virtual filesystem - completely isolated.

Usage:
    export ANTHROPIC_API_KEY=your_key
    python examples/deepagent_sandbox.py
"""

import asyncio
import os
import sys

from deepagents import create_deep_agent

try:
    from bashkit.deepagents import BashKitBackend
except ImportError:
    print("Error: bashkit not installed")
    print("Run: uv pip install maturin && cd crates/bashkit-python && maturin develop")
    sys.exit(1)


SYSTEM_PROMPT = """You are a coding assistant with a sandboxed bash environment.

You have access to:
- `execute`: Run shell commands (cat, grep, find, sed, awk, jq, echo, etc.)
- `read_file`, `write_file`, `edit_file`: File operations
- `ls`, `glob`, `grep`: Find and search files

Everything runs in a virtual filesystem - completely sandboxed, no real files affected.
Use shell commands for data processing and file tools for precise edits."""


async def main():
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Set ANTHROPIC_API_KEY environment variable")
        sys.exit(1)

    print("=" * 60)
    print("  Deep Agent + BashKit Virtual Filesystem")
    print("=" * 60)

    # Create BashKit backend - provides execute + file tools
    backend = BashKitBackend(username="dev", hostname="sandbox")

    # Setup: Create project files
    print("\n[Setup] Creating virtual filesystem...")
    backend.setup('''
mkdir -p /home/user/project
echo '{"name": "myapp", "version": "1.0", "debug": true}' > /home/user/project/config.json
cat > /home/user/project/app.py << 'PYEOF'
"""Simple application module."""

def calculate(a, b, op="+"):
    """Perform calculation."""
    if op == "+":
        return a + b
    elif op == "-":
        return a - b
    elif op == "*":
        return a * b
    return None

def main():
    result = calculate(10, 5, "+")
    print(f"Result: {result}")

if __name__ == "__main__":
    main()
PYEOF
''')
    print("  Created /home/user/project/config.json")
    print("  Created /home/user/project/app.py")

    # Create agent with BashKit backend
    agent = create_deep_agent(
        model="anthropic:claude-sonnet-4-20250514",
        backend=backend,
        system_prompt=SYSTEM_PROMPT,
    )

    # Task that demonstrates both shell commands and file operations
    task = """Please do these tasks:

1. Use `execute` to run: ls -la /home/user/project
2. Use `execute` to run: cat /home/user/project/config.json | jq '.name'
3. Use `read_file` to read /home/user/project/app.py
4. Use `execute` to run: grep -n "def" /home/user/project/app.py
5. Use `execute` to create a README: echo "# My App" > /home/user/project/README.md
6. Use `ls` to verify the README was created"""

    print(f"\n[Task]\n{task}")
    print("-" * 60)

    async for event in agent.astream_events(
        {"messages": [{"role": "user", "content": task}]},
        version="v2",
        config={"recursion_limit": 50},
    ):
        kind = event["event"]

        if kind == "on_tool_start":
            name = event.get("name", "")
            data = event["data"].get("input", {})
            if name == "execute":
                print(f"\n$ {data.get('command', '')}")
            elif name in ("read_file", "write_file", "edit_file", "ls", "glob", "grep"):
                arg = data.get("file_path") or data.get("path") or data.get("pattern") or str(data)[:50]
                print(f"\n[{name}] {arg}")

        elif kind == "on_tool_end":
            output = event["data"].get("output", "")
            if hasattr(output, "content"):
                output = output.content
            if output:
                lines = str(output).strip().split("\n")
                for line in lines[:12]:
                    print(f"  {line}")
                if len(lines) > 12:
                    print(f"  ... ({len(lines) - 12} more)")

        elif kind == "on_chat_model_stream":
            chunk = event["data"].get("chunk")
            if chunk and hasattr(chunk, "content"):
                content = chunk.content
                if isinstance(content, str):
                    print(content, end="", flush=True)

    print("\n" + "=" * 60)


if __name__ == "__main__":
    asyncio.run(main())
