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
Deep Agent Sandbox - BashKit Virtual Filesystem with Deep Agents

This example demonstrates using BashKit's virtual filesystem as a
middleware for deep agents. The agent can:
1. Execute bash commands in an isolated sandbox
2. Create, read, and modify files
3. Use pipelines, loops, and other bash features
4. Process data with grep, sed, awk, jq

The virtual filesystem is completely isolated - no real files are affected.

Run with:
    export ANTHROPIC_API_KEY=your_key
    uv run examples/deepagent_sandbox.py
"""

import asyncio
import os
import sys

from deepagents import create_deep_agent

# Try to import from installed package
try:
    from bashkit.deepagents import create_bash_middleware
except ImportError:
    print("bashkit not found. Install with: cd crates/bashkit-python && maturin develop")
    sys.exit(1)


# Setup script to create an initial project structure
PROJECT_SETUP = '''
# Create a sample project structure
mkdir -p /home/user/project/src
mkdir -p /home/user/project/tests
mkdir -p /home/user/project/docs

# Create a Python module
cat > /home/user/project/src/calculator.py << 'EOF'
"""Simple calculator module."""

def add(a: int, b: int) -> int:
    """Add two numbers."""
    return a + b

def subtract(a: int, b: int) -> int:
    """Subtract b from a."""
    return a - b

def multiply(a: int, b: int) -> int:
    """Multiply two numbers."""
    return a * b

def divide(a: int, b: int) -> float:
    """Divide a by b."""
    if b == 0:
        raise ValueError("Cannot divide by zero")
    return a / b
EOF

# Create a test file with a bug
cat > /home/user/project/tests/test_calculator.py << 'EOF'
"""Tests for calculator module."""
import sys
sys.path.insert(0, "/home/user/project/src")
from calculator import add, subtract, multiply, divide

def test_add():
    assert add(2, 3) == 5
    assert add(-1, 1) == 0
    assert add(0, 0) == 0

def test_subtract():
    assert subtract(5, 3) == 2
    assert subtract(1, 1) == 0
    # BUG: This assertion is wrong!
    assert subtract(0, 5) == 5  # Should be -5

def test_multiply():
    assert multiply(3, 4) == 12
    assert multiply(0, 100) == 0
    assert multiply(-2, 3) == -6

def test_divide():
    assert divide(10, 2) == 5.0
    assert divide(7, 2) == 3.5
EOF

# Create a README
cat > /home/user/project/README.md << 'EOF'
# Calculator Project

A simple calculator module for demonstration.

## Usage

```python
from calculator import add, subtract, multiply, divide

result = add(2, 3)  # Returns 5
```

## Testing

Run tests with: python -m pytest tests/
EOF

# Create a config file
cat > /home/user/project/config.json << 'EOF'
{
    "name": "calculator",
    "version": "1.0.0",
    "author": "Demo User",
    "features": {
        "basic_ops": true,
        "advanced_ops": false,
        "logging": true
    }
}
EOF

echo "Project structure created at /home/user/project"
ls -la /home/user/project/
'''

SYSTEM_PROMPT = """You are a helpful coding assistant with access to a sandboxed bash environment.

You have a virtual filesystem where you can create, read, and modify files safely.
All file operations are isolated - nothing affects real files on disk.

Available bash commands include:
- File navigation: ls, cd, pwd, find
- File operations: cat, head, tail, touch, mkdir, rm, cp, mv
- Text processing: grep, sed, awk, jq, sort, uniq, cut, tr, wc
- Utilities: echo, printf, date, sleep

A sample project has been set up at /home/user/project with:
- src/calculator.py - A simple calculator module
- tests/test_calculator.py - Test file (contains a bug!)
- config.json - Project configuration
- README.md - Documentation

Help the user explore and work with this codebase. Be concise and practical."""


async def run_agent():
    """Run the deep agent with bash middleware."""
    # Check for API key
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Please set ANTHROPIC_API_KEY environment variable")
        print("  export ANTHROPIC_API_KEY=your_key_here")
        sys.exit(1)

    print("=" * 60)
    print("  DEEP AGENT SANDBOX")
    print("  BashKit Virtual Filesystem Demo")
    print("=" * 60)
    print()

    # Create the bash middleware
    bash_middleware = create_bash_middleware(
        username="developer",
        hostname="sandbox",
        max_commands=500,
    )

    # Set up the project structure
    print("Setting up virtual filesystem...")
    setup_output = bash_middleware.execute_sync(PROJECT_SETUP)
    print(setup_output)
    print()

    # Create the deep agent with our middleware
    agent = create_deep_agent(
        model="anthropic:claude-sonnet-4-20250514",
        middleware=[bash_middleware],
        system_prompt=SYSTEM_PROMPT,
    )

    print("-" * 60)
    print("INTERACTIVE SESSION")
    print("Type 'quit' or 'exit' to end the session")
    print("-" * 60)
    print()

    # Example tasks the user might try
    print("Try these example prompts:")
    print("  1. 'Show me the project structure'")
    print("  2. 'Find the bug in the test file'")
    print("  3. 'Extract feature flags from config.json using jq'")
    print("  4. 'Create a new utility module with a helper function'")
    print()

    while True:
        try:
            user_input = input("\nYou: ").strip()
        except (EOFError, KeyboardInterrupt):
            print("\nGoodbye!")
            break

        if not user_input:
            continue

        if user_input.lower() in ("quit", "exit", "q"):
            print("Goodbye!")
            break

        print("\nAgent: ", end="", flush=True)

        # Stream the agent response
        async for event in agent.astream_events(
            {
                "messages": [{"role": "user", "content": user_input}]
            },
            version="v2",
            config={"recursion_limit": 30},
        ):
            kind = event["event"]

            # Tool invocation
            if kind == "on_tool_start":
                tool_name = event.get("name", "")
                tool_input = event["data"].get("input", {})
                if tool_name == "execute":
                    cmd = tool_input.get("commands", "")[:80]
                    print(f"\n  [bash] {cmd}{'...' if len(tool_input.get('commands', '')) > 80 else ''}")
                elif tool_name == "reset_filesystem":
                    print("\n  [reset filesystem]")

            # Tool result (abbreviated)
            elif kind == "on_tool_end":
                output = event["data"].get("output", "")
                if hasattr(output, "content"):
                    output = output.content
                if output:
                    lines = str(output).strip().split("\n")
                    for line in lines[:8]:
                        print(f"    {line}")
                    if len(lines) > 8:
                        print(f"    ... ({len(lines) - 8} more lines)")

            # Agent text (streaming)
            elif kind == "on_chat_model_stream":
                chunk = event["data"].get("chunk")
                if chunk and hasattr(chunk, "content") and chunk.content:
                    content = chunk.content
                    if isinstance(content, str):
                        print(content, end="", flush=True)
                    elif isinstance(content, list):
                        for block in content:
                            if isinstance(block, dict) and block.get("type") == "text":
                                print(block.get("text", ""), end="", flush=True)

        print()  # Newline after response

    print()
    print("=" * 60)
    print("  SESSION ENDED")
    print("=" * 60)


async def run_demo():
    """Run a non-interactive demo."""
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Please set ANTHROPIC_API_KEY environment variable")
        sys.exit(1)

    print("=" * 60)
    print("  DEEP AGENT SANDBOX DEMO")
    print("=" * 60)
    print()

    bash_middleware = create_bash_middleware(
        username="developer",
        hostname="sandbox",
        max_commands=500,
    )

    print("Setting up virtual filesystem...")
    bash_middleware.execute_sync(PROJECT_SETUP)
    print()

    agent = create_deep_agent(
        model="anthropic:claude-sonnet-4-20250514",
        middleware=[bash_middleware],
        system_prompt=SYSTEM_PROMPT,
    )

    # Run a demo task
    demo_task = """
    Please do the following:
    1. Show the project structure
    2. Find the bug in test_calculator.py
    3. Fix the bug and show the corrected test
    """

    print(f"Demo task: {demo_task.strip()}")
    print("-" * 60)

    async for event in agent.astream_events(
        {"messages": [{"role": "user", "content": demo_task}]},
        version="v2",
        config={"recursion_limit": 30},
    ):
        kind = event["event"]

        if kind == "on_tool_start":
            tool_name = event.get("name", "")
            if tool_name == "execute":
                cmd = event["data"].get("input", {}).get("commands", "")
                print(f"\n> {cmd}")

        elif kind == "on_tool_end":
            output = event["data"].get("output", "")
            if hasattr(output, "content"):
                output = output.content
            if output:
                for line in str(output).strip().split("\n")[:15]:
                    print(f"  {line}")

        elif kind == "on_chat_model_stream":
            chunk = event["data"].get("chunk")
            if chunk and hasattr(chunk, "content"):
                content = chunk.content
                if isinstance(content, str):
                    print(content, end="", flush=True)

    print()
    print("=" * 60)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Deep Agent Sandbox Demo")
    parser.add_argument(
        "--demo",
        action="store_true",
        help="Run non-interactive demo instead of interactive session",
    )
    args = parser.parse_args()

    if args.demo:
        asyncio.run(run_demo())
    else:
        asyncio.run(run_agent())
