#!/usr/bin/env python3
"""LangGraph-style async tool with ContextVar propagation.

Demonstrates:
- Async ``def`` callbacks registered with ``ScriptedTool.add_tool()``
- ``contextvars.ContextVar`` state propagating from caller into callbacks
- Multi-tool bash pipelines with async I/O
- The "stream writer" pattern used by LangGraph's ``get_stream_writer()``

Usage:
    python examples/langgraph_async_tool.py
"""

import asyncio
import contextvars

from bashkit import ScriptedTool

# ---------------------------------------------------------------------------
# Simulate LangGraph's stream-writer pattern: a ContextVar that carries a
# callback for streaming intermediate results back to the caller.
# ---------------------------------------------------------------------------

stream_writer: contextvars.ContextVar[list] = contextvars.ContextVar("stream_writer")


# ---------------------------------------------------------------------------
# Async tool callbacks — these would typically call external APIs
# ---------------------------------------------------------------------------


async def search_web(params, stdin=None):
    """Simulate an async web search."""
    query = params.get("query", "")
    # Write intermediate progress via the stream writer
    writer = stream_writer.get()
    writer.append({"step": "search", "query": query, "status": "started"})
    # Simulate async I/O (network call, database query, etc.)
    await asyncio.sleep(0)
    results = [
        {"title": f"Result 1 for: {query}", "url": "https://example.com/1"},
        {"title": f"Result 2 for: {query}", "url": "https://example.com/2"},
    ]
    writer.append({"step": "search", "query": query, "status": "done", "count": len(results)})
    import json

    return json.dumps(results) + "\n"


async def summarize(params, stdin=None):
    """Summarize text from stdin."""
    writer = stream_writer.get()
    writer.append({"step": "summarize", "input_length": len(stdin or "")})
    await asyncio.sleep(0)
    # In reality, this would call an LLM API
    return f"Summary: processed {len(stdin or '')} chars of input\n"


def format_output(params, stdin=None):
    """Sync callback — formatting doesn't need async."""
    fmt = params.get("format", "text")
    if fmt == "json":
        import json

        return json.dumps({"formatted": (stdin or "").strip()}) + "\n"
    return f"[formatted] {(stdin or '').strip()}\n"


# ---------------------------------------------------------------------------
# Build the tool
# ---------------------------------------------------------------------------


def build_tool() -> ScriptedTool:
    tool = ScriptedTool("research_agent", short_description="Research assistant with async tools")
    tool.add_tool(
        "search",
        "Search the web for a query",
        callback=search_web,
        schema={"type": "object", "properties": {"query": {"type": "string"}}},
    )
    tool.add_tool(
        "summarize",
        "Summarize text from stdin",
        callback=summarize,
    )
    tool.add_tool(
        "format",
        "Format output",
        callback=format_output,
        schema={"type": "object", "properties": {"format": {"type": "string"}}},
    )
    return tool


# ---------------------------------------------------------------------------
# Run
# ---------------------------------------------------------------------------


def main():
    # Set up the stream writer (simulating LangGraph's get_stream_writer())
    events: list = []
    stream_writer.set(events)

    tool = build_tool()

    # Single async tool call
    print("=== Single async tool call ===")
    r = tool.execute_sync('search --query "Python async patterns"')
    print(f"exit_code: {r.exit_code}")
    print(f"stdout: {r.stdout.strip()}")
    print(f"stream events: {len(events)}")
    print()

    # Reset events for next demo
    events.clear()

    # Multi-tool pipeline: search → summarize → format
    print("=== Multi-tool pipeline (async + sync) ===")
    script = """
        search --query "ContextVar propagation" | summarize | format --format json
    """
    r = tool.execute_sync(script)
    print(f"exit_code: {r.exit_code}")
    print(f"stdout: {r.stdout.strip()}")
    print(f"stream events: {events}")
    print()

    # Verify all stream events were captured
    assert len(events) == 3, f"Expected 3 events, got {len(events)}: {events}"
    assert events[0]["step"] == "search"
    assert events[1]["step"] == "search"
    assert events[2]["step"] == "summarize"
    print("All assertions passed!")


if __name__ == "__main__":
    main()
