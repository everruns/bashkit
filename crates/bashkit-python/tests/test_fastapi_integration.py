"""FastAPI integration tests for async callbacks and ContextVar propagation.

Tests the pattern where FastAPI endpoints use ScriptedTool with async callbacks,
verifying that request-scoped ContextVars propagate into tool callbacks.
Requires ``fastapi`` and ``httpx`` to be installed.
"""

import asyncio
import contextvars

import pytest

fastapi = pytest.importorskip("fastapi")
httpx = pytest.importorskip("httpx")

from fastapi import FastAPI, Request  # noqa: E402
from fastapi.testclient import TestClient  # noqa: E402

from bashkit import ScriptedTool  # noqa: E402

# ---------------------------------------------------------------------------
# ContextVar for request-scoped state
# ---------------------------------------------------------------------------

current_request_id: contextvars.ContextVar[str] = contextvars.ContextVar("current_request_id", default="none")

# ===========================================================================
# Tests
# ===========================================================================


def test_sync_endpoint_with_async_callback():
    """FastAPI sync endpoint uses ScriptedTool with async callback."""
    app = FastAPI()

    async def greet(params, stdin=None):
        rid = current_request_id.get()
        name = params.get("name", "world")
        return f'{{"greeting": "hello {name}", "request_id": "{rid}"}}\n'

    @app.get("/greet/{name}")
    def greet_endpoint(name: str, request: Request):
        current_request_id.set(request.headers.get("x-request-id", "unknown"))
        tool = ScriptedTool("api")
        tool.add_tool(
            "greet",
            "Greet",
            callback=greet,
            schema={"type": "object", "properties": {"name": {"type": "string"}}},
        )
        r = tool.execute_sync(f"greet --name {name}")
        return {"stdout": r.stdout, "exit_code": r.exit_code}

    client = TestClient(app)
    resp = client.get("/greet/Alice", headers={"x-request-id": "req-123"})
    assert resp.status_code == 200
    data = resp.json()
    assert data["exit_code"] == 0
    assert "hello Alice" in data["stdout"]
    assert "req-123" in data["stdout"]


def test_async_endpoint_with_async_callback():
    """FastAPI async endpoint uses ``await tool.execute()`` (not execute_sync).

    Async endpoints must use the async API to avoid blocking the event loop.
    """
    app = FastAPI()

    async def lookup(params, stdin=None):
        rid = current_request_id.get()
        uid = params.get("id", "0")
        await asyncio.sleep(0)  # Simulate async I/O
        return f'{{"id": {uid}, "name": "User-{uid}", "request_id": "{rid}"}}\n'

    @app.get("/user/{uid}")
    async def user_endpoint(uid: int, request: Request):
        current_request_id.set(request.headers.get("x-request-id", "unknown"))
        tool = ScriptedTool("api")
        tool.add_tool(
            "lookup",
            "Lookup user",
            callback=lookup,
            schema={"type": "object", "properties": {"id": {"type": "integer"}}},
        )
        r = await tool.execute(f"lookup --id {uid}")
        return {"stdout": r.stdout, "exit_code": r.exit_code}

    client = TestClient(app)
    resp = client.get("/user/42", headers={"x-request-id": "req-456"})
    assert resp.status_code == 200
    data = resp.json()
    assert data["exit_code"] == 0
    assert "User-42" in data["stdout"]
    assert "req-456" in data["stdout"]


def test_pipeline_in_endpoint():
    """FastAPI sync endpoint executes a multi-tool bash pipeline."""
    app = FastAPI()

    async def get_user(params, stdin=None):
        uid = params.get("id", "0")
        return f'{{"id": {uid}, "name": "Alice", "email": "alice@example.com"}}\n'

    async def get_orders(params, stdin=None):
        uid = params.get("user_id", "0")
        return f'[{{"order_id": 1, "user_id": {uid}, "total": 99.99}}]\n'

    @app.get("/user/{uid}/summary")
    def summary_endpoint(uid: int):
        tool = ScriptedTool("api")
        tool.add_tool(
            "get_user",
            "Fetch user",
            callback=get_user,
            schema={"type": "object", "properties": {"id": {"type": "integer"}}},
        )
        tool.add_tool(
            "get_orders",
            "Fetch orders",
            callback=get_orders,
            schema={
                "type": "object",
                "properties": {"user_id": {"type": "integer"}},
            },
        )
        script = f"""
            user=$(get_user --id {uid})
            orders=$(get_orders --user_id {uid})
            echo "$user" | jq -r '.name'
            echo "$orders" | jq -r '.[0].total'
        """
        r = tool.execute_sync(script)
        lines = r.stdout.strip().split("\n")
        return {"name": lines[0] if lines else "", "total": lines[1] if len(lines) > 1 else ""}

    client = TestClient(app)
    resp = client.get("/user/42/summary")
    assert resp.status_code == 200
    data = resp.json()
    assert data["name"] == "Alice"
    assert data["total"] == "99.99"


def test_concurrent_requests_context_isolation():
    """Concurrent FastAPI requests have isolated ContextVars."""
    app = FastAPI()

    def echo_request_id(params, stdin=None):
        rid = current_request_id.get()
        return f"{rid}\n"

    @app.get("/echo")
    def echo_endpoint(request: Request):
        current_request_id.set(request.headers.get("x-request-id", "none"))
        tool = ScriptedTool("api")
        tool.add_tool("echo_rid", "Echo request ID", callback=echo_request_id)
        r = tool.execute_sync("echo_rid")
        return {"request_id": r.stdout.strip()}

    client = TestClient(app)

    # Sequential requests should each see their own request ID
    r1 = client.get("/echo", headers={"x-request-id": "aaa"})
    r2 = client.get("/echo", headers={"x-request-id": "bbb"})
    r3 = client.get("/echo", headers={"x-request-id": "ccc"})

    assert r1.json()["request_id"] == "aaa"
    assert r2.json()["request_id"] == "bbb"
    assert r3.json()["request_id"] == "ccc"


def test_error_handling_in_endpoint():
    """FastAPI endpoint handles ScriptedTool callback errors gracefully."""
    app = FastAPI()

    async def failing_tool(params, stdin=None):
        raise ValueError("simulated failure")

    @app.get("/fail")
    async def fail_endpoint():
        tool = ScriptedTool("api")
        tool.add_tool("fail", "Always fails", callback=failing_tool)
        r = tool.execute_sync("fail")
        return {"exit_code": r.exit_code, "stderr": r.stderr}

    client = TestClient(app)
    resp = client.get("/fail")
    assert resp.status_code == 200
    data = resp.json()
    assert data["exit_code"] != 0
