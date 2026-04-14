#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "bashkit",
# ]
# ///
"""BashTool as an LLM tool — shows how to wire bashkit into any AI framework.

Demonstrates:
- Creating a BashTool instance
- Extracting tool metadata (description, input/output schemas)
- Simulating an LLM tool-call loop (no API key needed)
- Feeding results back as tool responses
- Using system_prompt() for LLM context
- Generic tool adapter pattern

Run:
    uv run crates/bashkit-python/examples/llm_tool.py

uv automatically installs bashkit from PyPI (pre-built wheels, no Rust needed).
"""

from __future__ import annotations

import json

from bashkit import BashTool


def demo_tool_definition():
    """Show how to extract tool metadata for any LLM framework."""
    print("=== Tool Definition ===\n")

    tool = BashTool(username="agent", hostname="sandbox")

    # These fields are what you'd send to any LLM as a tool definition
    tool_def = {
        "name": tool.name,
        "description": tool.short_description,
        "input_schema": json.loads(tool.input_schema()),
        "output_schema": json.loads(tool.output_schema()),
    }

    print(f"Name:        {tool_def['name']}")
    print(f"Description: {tool_def['description']}")
    input_props = tool_def["input_schema"].get("properties", {})
    print(f"Input keys:  {', '.join(input_props.keys())}")
    print(f"Version:     {tool.version}")
    assert tool_def["name"] == "bashkit"
    assert "commands" in input_props

    print()


def demo_system_prompt():
    """Show the token-efficient system prompt for LLM orchestration."""
    print("=== System Prompt (first 200 chars) ===\n")

    tool = BashTool()
    prompt = tool.system_prompt()
    print(prompt[:200] + "...\n")

    # The system prompt contains instructions for the LLM
    assert len(prompt) > 100

    # help() returns a longer markdown document
    help_text = tool.help()
    print(f"Help length: {len(help_text)} chars (vs system_prompt: {len(prompt)} chars)")
    assert len(help_text) > len(prompt)

    print()


def demo_tool_call_loop():
    """Simulate an LLM tool-call loop — the core pattern for any framework."""
    print("=== Simulated Tool-Call Loop ===\n")

    tool = BashTool(username="agent", hostname="sandbox")

    # Pretend the LLM decided to call our tool with these commands
    llm_tool_calls = [
        'echo "Setting up project..."',
        "mkdir -p /tmp/project/src",
        "echo 'def main(): print(\"hello\")' > /tmp/project/src/app.py",
        "cat /tmp/project/src/app.py",
        "ls -la /tmp/project/src/",
        "wc -l /tmp/project/src/app.py",
    ]

    for commands in llm_tool_calls:
        print(f"LLM calls: {commands}")
        result = tool.execute_sync(commands)

        # Build the response you'd send back to the LLM
        tool_response = {  # noqa: F841 (illustrative — shows the shape)
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
        }

        if result.exit_code == 0:
            print(f"  -> stdout: {result.stdout.strip() or '(empty)'}")
        else:
            print(f"  -> error ({result.exit_code}): {result.stderr.strip()}")
        print()

    # Verify state persisted across calls
    r = tool.execute_sync("test -f /tmp/project/src/app.py && echo exists")
    assert r.stdout.strip() == "exists"

    print()


def demo_error_handling():
    """Show how errors are reported back to the LLM."""
    print("=== Error Handling ===\n")

    tool = BashTool()

    # Non-zero exit
    r = tool.execute_sync("exit 42")
    print(f"exit 42: code={r.exit_code}, success={r.success}")
    assert r.exit_code == 42
    assert not r.success

    # Command not found
    r = tool.execute_sync("nonexistent_command")
    print(f"not found: code={r.exit_code}, stderr={r.stderr.strip()!r}")
    assert r.exit_code != 0

    # The tool keeps working after errors
    r = tool.execute_sync("echo 'recovered'")
    print(f"recovered: {r.stdout.strip()}")
    assert r.success

    print()


def demo_generic_adapter():
    """Create a generic tool adapter that works with any LLM framework."""
    print("=== Generic Tool Adapter ===\n")

    tool = BashTool()

    # This function converts BashTool into a format any framework can use
    adapter = create_tool_adapter(tool)
    print(f"Adapter name:   {adapter['name']}")
    schema_str = json.dumps(adapter["schema"])
    print(f"Adapter schema: {schema_str[:80]}...")

    # Execute through adapter
    result = adapter["execute"]({"commands": "echo 'via adapter'"})
    print(f"Adapter result: {result['stdout'].strip()}")
    assert result["exit_code"] == 0

    # Anthropic tool-use format
    anthropic_tool = {
        "name": adapter["name"],
        "description": adapter["description"],
        "input_schema": adapter["schema"],
    }
    print(f"\nAnthropic tool format: name={anthropic_tool['name']!r}")
    print(f"  input_schema keys: {list(anthropic_tool['input_schema'].get('properties', {}).keys())}")

    # OpenAI function-calling format
    openai_tool = {
        "type": "function",
        "function": {
            "name": adapter["name"],
            "description": adapter["description"],
            "parameters": adapter["schema"],
        },
    }
    print(f"OpenAI tool format:   type={openai_tool['type']!r}, function.name={openai_tool['function']['name']!r}")

    print()


def create_tool_adapter(bash_tool: BashTool) -> dict:
    """Create a generic tool adapter from BashTool.

    Returns a dict with:
    - name, description, schema (for tool registration)
    - execute(params) (for tool invocation)
    """

    def execute(params: dict) -> dict:
        r = bash_tool.execute_sync(params.get("commands", ""))
        return {"stdout": r.stdout, "stderr": r.stderr, "exit_code": r.exit_code}

    return {
        "name": bash_tool.name,
        "description": bash_tool.description(),
        "schema": json.loads(bash_tool.input_schema()),
        "execute": execute,
    }


# =============================================================================
# Main
# =============================================================================


def main():
    print("Bashkit — LLM Tool Examples\n")
    demo_tool_definition()
    demo_system_prompt()
    demo_tool_call_loop()
    demo_error_handling()
    demo_generic_adapter()
    print("All examples passed.")


if __name__ == "__main__":
    main()
