#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "deepagents>=0.3.11",
#     "langchain-anthropic>=0.3",
# ]
# ///
# Note: Install bashkit first: cd crates/bashkit-python && maturin develop
"""
Deep Agent with Bashkit Virtual Filesystem

Run:
    export ANTHROPIC_API_KEY=your_key
    uv run examples/deepagent_coding_agent.py
"""

import asyncio
import os
import sys

from deepagents import create_deep_agent

try:
    from bashkit.deepagents import BashkitBackend
except ImportError:
    print("bashkit not found. Install: cd crates/bashkit-python && maturin develop")
    sys.exit(1)


SYSTEM_PROMPT = """You are a coding assistant with a sandboxed bash environment.

Tools:
- `bash`: Shell commands (mkdir, cat, grep, sed, awk, jq, find, echo, etc.)
- `read_file`, `write_file`, `edit_file`: File operations
- `ls`, `glob`, `grep`: File discovery

All tools share the same virtual filesystem. Completely isolated."""


async def main():
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Set ANTHROPIC_API_KEY")
        sys.exit(1)

    print("=" * 60)
    print("  Deep Agent + Bashkit")
    print("=" * 60)

    backend = BashkitBackend()
    bashkit_middleware = backend.create_middleware()

    agent = create_deep_agent(
        model="anthropic:claude-sonnet-4-20250514",
        backend=backend,
        middleware=[bashkit_middleware],
        system_prompt=SYSTEM_PROMPT,
    )

    task = """Create a Python calculator project:
1. Create directory /home/user/calc
2. Create calculator.py with add, subtract, multiply, divide functions
3. Create test_calculator.py with a few tests
4. Show both files with cat"""

    print(f"\n{task}\n")
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
            if name == "bash":
                cmd = data.get("command", "")
                print(f"\n$ {cmd[:100]}{'...' if len(cmd) > 100 else ''}")
            elif name in ("read_file", "write_file", "edit_file", "ls"):
                print(f"\n[{name}] {data.get('file_path') or data.get('path', '')}")

        elif kind == "on_tool_end":
            output = event["data"].get("output", "")
            if hasattr(output, "content"):
                output = output.content
            if output:
                lines = str(output).strip().split("\n")
                for line in lines[:20]:
                    print(f"  {line}")
                if len(lines) > 20:
                    print(f"  ... ({len(lines) - 20} more)")

        elif kind == "on_chat_model_stream":
            chunk = event["data"].get("chunk")
            if chunk and hasattr(chunk, "content") and isinstance(chunk.content, str):
                print(chunk.content, end="", flush=True)

    print("\n" + "=" * 60)


if __name__ == "__main__":
    asyncio.run(main())
