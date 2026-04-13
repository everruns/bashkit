#!/usr/bin/env python3
"""FastAPI integration with async ScriptedTool callbacks.

Demonstrates:
- Async ``def`` callbacks in a FastAPI application
- Request-scoped ``ContextVar`` propagation into tool callbacks
- Sync endpoints with ``execute_sync()``
- Async endpoints with ``await execute()``

Usage:
    python examples/fastapi_async_tool.py
"""

import asyncio
import contextvars

from bashkit import ScriptedTool

# ---------------------------------------------------------------------------
# ContextVar for request-scoped state
# ---------------------------------------------------------------------------

request_id: contextvars.ContextVar[str] = contextvars.ContextVar("request_id", default="none")


# ---------------------------------------------------------------------------
# Tool callbacks
# ---------------------------------------------------------------------------


async def get_user(params, stdin=None):
    """Fetch a user by ID (simulated async I/O)."""
    uid = params.get("id", 0)
    rid = request_id.get()
    await asyncio.sleep(0)  # Simulate DB query
    return f'{{"id": {uid}, "name": "Alice", "request_id": "{rid}"}}\n'


async def get_orders(params, stdin=None):
    """Fetch orders for a user (simulated async I/O)."""
    user_id = params.get("user_id", 0)
    rid = request_id.get()
    await asyncio.sleep(0)
    return f'[{{"order_id": 1, "user_id": {user_id}, "total": 99.99, "request_id": "{rid}"}}]\n'


def build_tool() -> ScriptedTool:
    tool = ScriptedTool("user_api", short_description="User API with async callbacks")
    tool.add_tool(
        "get_user",
        "Fetch user by ID",
        callback=get_user,
        schema={"type": "object", "properties": {"id": {"type": "integer"}}},
    )
    tool.add_tool(
        "get_orders",
        "Fetch orders for user",
        callback=get_orders,
        schema={"type": "object", "properties": {"user_id": {"type": "integer"}}},
    )
    return tool


# ---------------------------------------------------------------------------
# Simulate FastAPI-like request handling (no server needed for this example)
# ---------------------------------------------------------------------------


def simulate_sync_endpoint(uid: int, rid: str):
    """Simulate a sync FastAPI endpoint using execute_sync()."""
    request_id.set(rid)
    tool = build_tool()

    script = f"""
        user=$(get_user --id {uid})
        orders=$(get_orders --user_id {uid})
        echo "$user" | jq -r '.name'
        echo "$orders" | jq -r '.[0].total'
        echo "$user" | jq -r '.request_id'
    """
    return tool.execute_sync(script)


async def simulate_async_endpoint(uid: int, rid: str):
    """Simulate an async FastAPI endpoint using await execute()."""
    request_id.set(rid)
    tool = build_tool()
    return await tool.execute(f"get_user --id {uid}")


# ---------------------------------------------------------------------------
# Run
# ---------------------------------------------------------------------------


def main():
    print("=== Sync endpoint simulation ===")
    r = simulate_sync_endpoint(42, "req-abc-123")
    print(f"exit_code: {r.exit_code}")
    lines = r.stdout.strip().split("\n")
    print(f"user name: {lines[0]}")
    print(f"order total: {lines[1]}")
    print(f"request_id: {lines[2]}")
    assert lines[0] == "Alice"
    assert lines[1] == "99.99"
    assert lines[2] == "req-abc-123", f"Expected req-abc-123, got {lines[2]}"
    print()

    print("=== Async endpoint simulation ===")
    r = asyncio.run(simulate_async_endpoint(42, "req-def-456"))
    print(f"exit_code: {r.exit_code}")
    print(f"stdout: {r.stdout.strip()}")
    assert r.exit_code == 0
    assert "req-def-456" in r.stdout
    print()

    print("All assertions passed!")


if __name__ == "__main__":
    main()
