"""LangGraph integration tests for async callbacks and ContextVar propagation.

Tests the pattern where LangGraph passes a stream writer via ContextVar to tool
callbacks. Requires ``langchain-core`` to be installed.
"""

import asyncio
import contextvars

import pytest

langchain_core = pytest.importorskip("langchain_core")

from langchain_core.tools import StructuredTool  # noqa: E402

from bashkit import ScriptedTool  # noqa: E402

# ---------------------------------------------------------------------------
# Simulate LangGraph's get_stream_writer() pattern
# ---------------------------------------------------------------------------

_stream_writer: contextvars.ContextVar[list] = contextvars.ContextVar("_stream_writer")


# ===========================================================================
# Tests
# ===========================================================================


def test_sync_callback_reads_contextvar():
    """Sync callback reads ContextVar (LangGraph stream-writer pattern)."""
    events = []
    _stream_writer.set(events)

    def search(params, stdin=None):
        writer = _stream_writer.get()
        query = params.get("query", "")
        writer.append({"type": "search", "query": query})
        return f'{{"results": ["result for {query}"]}}\n'

    tool = ScriptedTool("agent")
    tool.add_tool(
        "search",
        "Search the web",
        callback=search,
        schema={"type": "object", "properties": {"query": {"type": "string"}}},
    )
    r = tool.execute_sync('search --query "python async"')
    assert r.exit_code == 0
    assert "result for python async" in r.stdout
    assert len(events) == 1
    assert events[0]["type"] == "search"


def test_async_callback_reads_contextvar():
    """Async callback reads ContextVar (LangGraph stream-writer pattern)."""
    events = []
    _stream_writer.set(events)

    async def search(params, stdin=None):
        writer = _stream_writer.get()
        query = params.get("query", "")
        writer.append({"type": "search", "query": query})
        # Simulate async I/O
        await asyncio.sleep(0)
        return f'{{"results": ["result for {query}"]}}\n'

    tool = ScriptedTool("agent")
    tool.add_tool(
        "search",
        "Search the web",
        callback=search,
        schema={"type": "object", "properties": {"query": {"type": "string"}}},
    )
    r = tool.execute_sync('search --query "langchain async"')
    assert r.exit_code == 0
    assert "result for langchain async" in r.stdout
    assert len(events) == 1
    assert events[0]["type"] == "search"


@pytest.mark.asyncio
async def test_async_execute_with_contextvar():
    """Async callback + async execute preserves ContextVar."""
    events = []
    _stream_writer.set(events)

    async def fetch(params, stdin=None):
        writer = _stream_writer.get()
        url = params.get("url", "")
        writer.append({"type": "fetch", "url": url})
        await asyncio.sleep(0)
        return f'{{"status": 200, "url": "{url}"}}\n'

    tool = ScriptedTool("agent")
    tool.add_tool(
        "fetch",
        "Fetch URL",
        callback=fetch,
        schema={"type": "object", "properties": {"url": {"type": "string"}}},
    )
    r = await tool.execute("fetch --url https://example.com")
    assert r.exit_code == 0
    assert "https://example.com" in r.stdout
    assert len(events) == 1
    assert events[0]["url"] == "https://example.com"


def test_multi_tool_pipeline_with_contextvar():
    """Multiple tools in a pipeline all see the same ContextVar."""
    events = []
    _stream_writer.set(events)

    async def search(params, stdin=None):
        writer = _stream_writer.get()
        query = params.get("query", "")
        writer.append({"step": "search", "query": query})
        return f'{{"id": 1, "title": "Result: {query}"}}\n'

    async def summarize(params, stdin=None):
        writer = _stream_writer.get()
        writer.append({"step": "summarize", "input_len": len(stdin or "")})
        return '{"summary": "Summary of input"}\n'

    tool = ScriptedTool("agent")
    tool.add_tool(
        "search",
        "Search",
        callback=search,
        schema={"type": "object", "properties": {"query": {"type": "string"}}},
    )
    tool.add_tool("summarize", "Summarize", callback=summarize)

    r = tool.execute_sync('search --query "AI" | summarize')
    assert r.exit_code == 0
    assert len(events) == 2
    assert events[0]["step"] == "search"
    assert events[1]["step"] == "summarize"


def test_langchain_structured_tool_wrapper():
    """ScriptedTool wrapping a LangChain StructuredTool callback."""
    events = []
    _stream_writer.set(events)

    # Create a LangChain StructuredTool
    def calculator(expression: str) -> str:
        """Evaluate a math expression."""
        # Read ContextVar to verify propagation
        writer = _stream_writer.get()
        writer.append({"tool": "calculator", "expr": expression})
        result = eval(expression)  # noqa: S307 — demo only
        return str(result)

    lc_tool = StructuredTool.from_function(
        func=calculator,
        name="calculator",
        description="Evaluate math",
    )

    # Wrap LangChain tool in ScriptedTool
    def calc_callback(params, stdin=None):
        expr = params.get("expression", "0")
        return lc_tool.invoke({"expression": expr}) + "\n"

    st = ScriptedTool("math_agent")
    st.add_tool(
        "calc",
        "Calculate",
        callback=calc_callback,
        schema={
            "type": "object",
            "properties": {"expression": {"type": "string"}},
        },
    )

    r = st.execute_sync('calc --expression "2 + 3"')
    assert r.exit_code == 0
    assert r.stdout.strip() == "5"
    assert len(events) == 1
    assert events[0]["tool"] == "calculator"
