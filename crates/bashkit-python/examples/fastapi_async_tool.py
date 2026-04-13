#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "bashkit",
#     "fastapi>=0.110",
#     "httpx>=0.27",
#     "uvicorn>=0.29",
# ]
# ///
"""FastAPI app exposing ScriptedTool endpoints with async callbacks.

Starts a real FastAPI server, exercises it via ``httpx``, then shuts down.
Demonstrates:

- Async ``def`` tool callbacks in FastAPI endpoint handlers
- Request-scoped ``contextvars.ContextVar`` propagation into callbacks
- Sync endpoints using ``execute_sync()``
- Async endpoints using ``await execute()``
- Multi-tool bash pipelines from HTTP requests

Run:
    uv run crates/bashkit-python/examples/fastapi_async_tool.py
"""

from __future__ import annotations

import asyncio
import contextvars
import json
import threading

import httpx
import uvicorn
from fastapi import FastAPI, Request

from bashkit import ScriptedTool

# ---------------------------------------------------------------------------
# Request-scoped ContextVar (like Flask's g or Starlette's request state)
# ---------------------------------------------------------------------------

current_request_id: contextvars.ContextVar[str] = contextvars.ContextVar("current_request_id", default="none")

# ---------------------------------------------------------------------------
# Async tool callbacks
# ---------------------------------------------------------------------------


async def get_user(params: dict, stdin: str | None = None) -> str:
    """Fetch user by ID — reads the request-scoped ContextVar."""
    uid = params.get("id", 0)
    rid = current_request_id.get()
    await asyncio.sleep(0)  # simulate DB query
    return json.dumps({"id": uid, "name": "Alice", "request_id": rid}) + "\n"


async def get_orders(params: dict, stdin: str | None = None) -> str:
    """Fetch orders for a user."""
    user_id = params.get("user_id", 0)
    rid = current_request_id.get()
    await asyncio.sleep(0)
    orders = [{"order_id": 1, "user_id": user_id, "total": 99.99, "request_id": rid}]
    return json.dumps(orders) + "\n"


def build_tool() -> ScriptedTool:
    tool = ScriptedTool("user_api", short_description="User API")
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
# FastAPI app
# ---------------------------------------------------------------------------

app = FastAPI(title="Bashkit ScriptedTool API")


@app.get("/user/{uid}")
def get_user_endpoint(uid: int, request: Request):
    """Sync endpoint — uses execute_sync(). ContextVar propagates into callback."""
    current_request_id.set(request.headers.get("x-request-id", "unknown"))
    tool = build_tool()
    r = tool.execute_sync(f"get_user --id {uid}")
    return {"stdout": r.stdout.strip(), "exit_code": r.exit_code}


@app.get("/user/{uid}/orders")
async def get_user_orders_endpoint(uid: int, request: Request):
    """Async endpoint — uses await execute(). Must not call execute_sync()."""
    current_request_id.set(request.headers.get("x-request-id", "unknown"))
    tool = build_tool()
    r = await tool.execute(f"get_orders --user_id {uid}")
    return {"stdout": r.stdout.strip(), "exit_code": r.exit_code}


@app.get("/user/{uid}/summary")
def get_user_summary_endpoint(uid: int, request: Request):
    """Sync endpoint with a multi-tool bash pipeline."""
    current_request_id.set(request.headers.get("x-request-id", "unknown"))
    tool = build_tool()
    script = f"""
        user=$(get_user --id {uid})
        orders=$(get_orders --user_id {uid})
        echo "$user" | jq -r '.name'
        echo "$orders" | jq -r '.[0].total'
        echo "$user" | jq -r '.request_id'
    """
    r = tool.execute_sync(script)
    lines = r.stdout.strip().split("\n")
    return {
        "name": lines[0] if len(lines) > 0 else "",
        "total": lines[1] if len(lines) > 1 else "",
        "request_id": lines[2] if len(lines) > 2 else "",
    }


# ---------------------------------------------------------------------------
# Self-test: start the server, hit endpoints, verify, shut down
# ---------------------------------------------------------------------------


def run_server(ready: threading.Event) -> None:
    config = uvicorn.Config(app, host="127.0.0.1", port=9876, log_level="warning")
    server = uvicorn.Server(config)

    # Signal the main thread once the server is accepting connections
    original_startup = server.startup

    async def startup_with_signal(*a, **kw):
        await original_startup(*a, **kw)
        ready.set()

    server.startup = startup_with_signal  # type: ignore[assignment]
    server.run()


def main() -> None:
    ready = threading.Event()
    t = threading.Thread(target=run_server, args=(ready,), daemon=True)
    t.start()
    ready.wait(timeout=10)

    base = "http://127.0.0.1:9876"
    headers = {"x-request-id": "req-example-42"}

    with httpx.Client(base_url=base, headers=headers) as client:
        # 1. Sync endpoint: single tool call
        print("=== GET /user/1 (sync endpoint, async callback) ===")
        r = client.get("/user/1")
        assert r.status_code == 200
        data = r.json()
        print(f"  {data}")
        assert data["exit_code"] == 0
        body = json.loads(data["stdout"])
        assert body["name"] == "Alice"
        assert body["request_id"] == "req-example-42"

        # 2. Async endpoint: single tool call
        print("=== GET /user/1/orders (async endpoint, async callback) ===")
        r = client.get("/user/1/orders")
        assert r.status_code == 200
        data = r.json()
        print(f"  {data}")
        assert data["exit_code"] == 0
        body = json.loads(data["stdout"])
        assert body[0]["user_id"] == 1
        assert body[0]["request_id"] == "req-example-42"

        # 3. Pipeline endpoint
        print("=== GET /user/42/summary (pipeline, ContextVar) ===")
        r = client.get("/user/42/summary")
        assert r.status_code == 200
        data = r.json()
        print(f"  {data}")
        assert data["name"] == "Alice"
        assert data["total"] == "99.99"
        assert data["request_id"] == "req-example-42"

    print("\nAll assertions passed!")


if __name__ == "__main__":
    main()
