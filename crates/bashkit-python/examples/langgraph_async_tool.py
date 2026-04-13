#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "bashkit",
#     "langchain-core>=0.3",
#     "langgraph>=0.2",
# ]
# ///
"""LangGraph agent using ScriptedTool with async callbacks and ContextVar propagation.

Builds a real LangGraph ``StateGraph`` where the tool node invokes a
``ScriptedTool`` with async Python callbacks.  The example demonstrates:

- Registering ``async def`` callbacks with ``ScriptedTool.add_tool()``
- ``contextvars.ContextVar`` propagating from the LangGraph task into callbacks
- Multi-tool bash pipelines composed by LangGraph's tool-calling loop

Run:
    uv run crates/bashkit-python/examples/langgraph_async_tool.py
"""

from __future__ import annotations

import asyncio
import contextvars
import json
import operator
from typing import Annotated, Any, TypedDict

from langchain_core.messages import AIMessage, BaseMessage, HumanMessage, ToolMessage
from langchain_core.tools import tool
from langgraph.graph import END, StateGraph

from bashkit import ScriptedTool

# ---------------------------------------------------------------------------
# ContextVar — simulates LangGraph's get_stream_writer() pattern.
# In a real LangGraph app this would be provided by the framework; here we
# set it manually to prove propagation works end-to-end.
# ---------------------------------------------------------------------------

stream_events: contextvars.ContextVar[list[dict]] = contextvars.ContextVar("stream_events")

# ---------------------------------------------------------------------------
# Async tool callbacks — each becomes a bash builtin inside ScriptedTool
# ---------------------------------------------------------------------------


async def search_web(params: dict, stdin: str | None = None) -> str:
    """Search the web for a query (simulated async I/O)."""
    query = params.get("query", "")
    # Prove ContextVar is accessible inside the callback
    writer = stream_events.get()
    writer.append({"tool": "search", "query": query})
    await asyncio.sleep(0)  # simulate network
    results = [
        {"title": "Async Python best practices", "url": "https://example.com/1"},
        {"title": "ContextVar deep dive", "url": "https://example.com/2"},
    ]
    return json.dumps(results) + "\n"


async def fetch_page(params: dict, stdin: str | None = None) -> str:
    """Fetch a URL and return its content (simulated)."""
    url = params.get("url", "")
    writer = stream_events.get()
    writer.append({"tool": "fetch", "url": url})
    await asyncio.sleep(0)
    return json.dumps({"url": url, "body": f"Content of {url}", "length": 1234}) + "\n"


def summarize(params: dict, stdin: str | None = None) -> str:
    """Sync callback — summarise text from stdin."""
    text = (stdin or "").strip()
    writer = stream_events.get()
    writer.append({"tool": "summarize", "chars": len(text)})
    return f"Summary ({len(text)} chars): {text[:80]}...\n"


# ---------------------------------------------------------------------------
# Build the ScriptedTool
# ---------------------------------------------------------------------------


def build_scripted_tool() -> ScriptedTool:
    st = ScriptedTool("research", short_description="Web research toolkit")
    st.add_tool(
        "search",
        "Search the web",
        callback=search_web,
        schema={"type": "object", "properties": {"query": {"type": "string"}}},
    )
    st.add_tool(
        "fetch",
        "Fetch a URL",
        callback=fetch_page,
        schema={"type": "object", "properties": {"url": {"type": "string"}}},
    )
    st.add_tool("summarize", "Summarize stdin text", callback=summarize)
    return st


# ---------------------------------------------------------------------------
# LangGraph state + nodes
# ---------------------------------------------------------------------------


class AgentState(TypedDict):
    messages: Annotated[list[BaseMessage], operator.add]


# Wrap ScriptedTool as a LangChain tool so LangGraph can call it
scripted = build_scripted_tool()


@tool
def research_tool(commands: str) -> str:
    """Run a bash script that orchestrates search, fetch, and summarize tools."""
    r = scripted.execute_sync(commands)
    if r.exit_code != 0:
        return f"Error (exit {r.exit_code}): {r.stderr}"
    return r.stdout


def tool_node(state: AgentState) -> dict[str, Any]:
    """Execute tool calls from the last AI message."""
    last = state["messages"][-1]
    results = []
    for tc in last.tool_calls:
        output = research_tool.invoke(tc["args"])
        results.append(ToolMessage(content=str(output), tool_call_id=tc["id"]))
    return {"messages": results}


def fake_llm_node(state: AgentState) -> dict[str, Any]:
    """Simulated LLM that emits a single tool call, then stops."""
    if any(isinstance(m, ToolMessage) for m in state["messages"]):
        return {"messages": [AIMessage(content="Done! See above results.")]}

    # First turn: emit a tool call with a bash pipeline
    return {
        "messages": [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "id": "call_1",
                        "name": "research_tool",
                        "args": {"commands": 'search --query "async Python" | summarize'},
                    }
                ],
            )
        ]
    }


def should_continue(state: AgentState) -> str:
    last = state["messages"][-1]
    if hasattr(last, "tool_calls") and last.tool_calls:
        return "tools"
    return END


# ---------------------------------------------------------------------------
# Build and run the graph
# ---------------------------------------------------------------------------


def build_graph() -> StateGraph:
    g = StateGraph(AgentState)
    g.add_node("llm", fake_llm_node)
    g.add_node("tools", tool_node)
    g.set_entry_point("llm")
    g.add_conditional_edges("llm", should_continue, {"tools": "tools", END: END})
    g.add_edge("tools", "llm")
    return g.compile()


def main() -> None:
    events: list[dict] = []
    stream_events.set(events)

    graph = build_graph()
    result = graph.invoke(
        {"messages": [HumanMessage(content="Research async Python patterns")]},
    )

    print("=== Final messages ===")
    for msg in result["messages"]:
        role = msg.__class__.__name__
        text = msg.content[:120] if msg.content else "(tool call)"
        print(f"  {role}: {text}")

    print("\n=== Stream events (from ContextVar) ===")
    for ev in events:
        print(f"  {ev}")

    # Assertions
    assert len(events) >= 2, f"Expected >=2 stream events, got {len(events)}"
    assert events[0]["tool"] == "search"
    assert events[-1]["tool"] == "summarize"
    assert any(isinstance(m, ToolMessage) for m in result["messages"])
    print("\nAll assertions passed!")


if __name__ == "__main__":
    main()
