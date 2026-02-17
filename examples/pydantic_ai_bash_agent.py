#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "bashkit[pydantic-ai]>=0.1.4",
# ]
# ///
"""
PydanticAI Agent with Bashkit Virtual Filesystem

A coding agent that writes and tests a bash script entirely
inside Bashkit's sandboxed virtual filesystem.

Run:
    export ANTHROPIC_API_KEY=your_key
    uv run examples/pydantic_ai_bash_agent.py

uv automatically installs bashkit from PyPI (pre-built wheels, no Rust needed).
"""

import asyncio
import os
import sys

from pydantic_ai import Agent

from bashkit.pydantic_ai import create_bash_tool


SYSTEM_PROMPT = """\
You are a coding assistant with a sandboxed bash environment.

You can run any bash command: echo, cat, grep, sed, awk, mkdir, etc.
All commands execute in a virtual filesystem — nothing touches the real host.

When asked to build something, create the files, then verify your work."""


async def main():
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Set ANTHROPIC_API_KEY")
        sys.exit(1)

    print("=" * 60)
    print("  PydanticAI + Bashkit")
    print("=" * 60)

    bash_tool = create_bash_tool(username="agent", hostname="sandbox")

    agent = Agent(
        "anthropic:claude-sonnet-4-20250514",
        system_prompt=SYSTEM_PROMPT,
        tools=[bash_tool],
    )

    task = (
        "Create a FizzBuzz bash script:\n"
        "1. mkdir -p /home/agent/project\n"
        "2. Write a bash script /home/agent/project/fizzbuzz.sh that prints "
        "FizzBuzz for numbers 1 to 20 using a for loop\n"
        "3. Show the file with cat\n"
        "4. Run it with: source /home/agent/project/fizzbuzz.sh\n"
    )

    print(f"\nTask: {task}")
    print("-" * 60)

    result = await agent.run(task)
    print("\n" + "-" * 60)
    print("Agent output:")
    print(result.data)
    print("=" * 60)


if __name__ == "__main__":
    asyncio.run(main())
