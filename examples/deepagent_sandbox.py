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
Deep Agent with BashKit Middleware + Backend

Demonstrates BashKit integration with Deep Agents using:
- BashKitBackend: SandboxBackendProtocol for file operations
- BashKitMiddleware: Adds `bash` tool via AgentMiddleware.tools

Both share the same VFS - files created via `bash` are visible to
`read_file` and vice versa. Completely sandboxed.

Run with:
    export ANTHROPIC_API_KEY=your_key
    uv run examples/deepagent_sandbox.py
"""

import asyncio
import os
import sys

from deepagents import create_deep_agent

try:
    from bashkit.deepagents import BashKitBackend
except ImportError:
    print("bashkit not found. Install: cd crates/bashkit-python && maturin develop")
    sys.exit(1)


SYSTEM_PROMPT = """You are a coding assistant with a sandboxed environment.

You have access to:
- `bash` tool: Execute shell commands (cat, grep, sed, awk, jq, find, etc.)
- `read_file`, `write_file`, `edit_file`: File operations
- `ls`, `glob`, `grep`: File discovery

All tools share the same virtual filesystem - completely isolated.
Prefer `bash` for data processing pipelines, file tools for precise edits."""


async def main():
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Set ANTHROPIC_API_KEY environment variable")
        sys.exit(1)

    print("=" * 60)
    print("  Deep Agent + BashKit")
    print("  Backend (file ops) + Middleware (bash tool)")
    print("=" * 60)

    # Create backend for file operations
    backend = BashKitBackend(username="dev", hostname="sandbox")

    # Create middleware from backend - shares the same VFS!
    middleware = backend.create_middleware()

    # Setup files using the shared VFS
    print("\n[Setup] Creating files in VFS...")
    backend.setup('''
mkdir -p /home/user/project
echo '{"name": "myapp", "version": "1.0", "debug": true}' > /home/user/project/config.json
cat > /home/user/project/app.py << 'EOF'
"""Simple app module."""

def greet(name):
    return f"Hello, {name}!"

def main():
    print(greet("World"))

if __name__ == "__main__":
    main()
EOF
''')
    print("  Created /home/user/project/config.json")
    print("  Created /home/user/project/app.py")

    # Create agent with BOTH backend and middleware
    agent = create_deep_agent(
        model="anthropic:claude-sonnet-4-20250514",
        backend=backend,
        middleware=[middleware],  # Adds `bash` tool
        system_prompt=SYSTEM_PROMPT,
    )

    # Task using BOTH bash tool (middleware) and file tools (backend)
    task = """Do these tasks showing both bash and file tools:

1. Use `bash` to run: cat /home/user/project/config.json | jq '.name'
2. Use `read_file` to read /home/user/project/app.py
3. Use `bash` to run: grep -n "def" /home/user/project/app.py
4. Use `bash` to create: echo "# README" > /home/user/project/README.md
5. Use `ls` to list /home/user/project (should show README.md from bash)"""

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
            if name == "bash":
                print(f"\n[bash] $ {data.get('command', '')}")
            elif name == "execute":
                print(f"\n[execute] $ {data.get('command', '')}")
            elif name in ("read_file", "write_file", "edit_file", "ls", "glob", "grep"):
                arg = data.get("file_path") or data.get("path") or str(data)[:50]
                print(f"\n[{name}] {arg}")

        elif kind == "on_tool_end":
            output = event["data"].get("output", "")
            if hasattr(output, "content"):
                output = output.content
            if output:
                lines = str(output).strip().split("\n")
                for line in lines[:10]:
                    print(f"  {line}")
                if len(lines) > 10:
                    print(f"  ... ({len(lines) - 10} more)")

        elif kind == "on_chat_model_stream":
            chunk = event["data"].get("chunk")
            if chunk and hasattr(chunk, "content"):
                content = chunk.content
                if isinstance(content, str):
                    print(content, end="", flush=True)

    print("\n" + "=" * 60)


if __name__ == "__main__":
    asyncio.run(main())
